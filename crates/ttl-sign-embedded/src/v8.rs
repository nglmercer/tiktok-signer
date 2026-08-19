//! The V8 engine, through `deno_core`: the engine the bundle was actually built for.
//!
//! Roughly nine times faster per signature than QuickJS and tens of megabytes larger, which is the
//! whole trade — see `docs/13-embedded-runtime.md`. It signs the same bytes: `tests/parity.rs`
//! compares the two engines directly and fails on any difference.
//!
//! # Entropy without an op layer
//!
//! `deno_core`'s bare runtime has no `crypto`, and the sandbox needs one. Rather than register an
//! op — whose macro surface moves between `deno_core` releases and would pin this crate to one —
//! the host hands V8 a **pool** of random bytes as a `Uint8Array` and a three-line JavaScript
//! function that draws from it. The pool is replaced before every signature, so no signature ever
//! reuses a byte from a previous one, and a draw past its end throws rather than repeating.
//!
//! Bundle 1.0.0.388 draws nothing at all — measured in `tests/entropy.rs`, which is also what sizes
//! the pool. Its variation comes from `Date.now` and `Math.random`. The pool is here for the
//! version that starts asking.

use std::sync::Once;

use deno_core::{v8, JsRuntime, RuntimeOptions};
use rand::RngCore;

use crate::{EmbeddedError, Engine, BOOTSTRAP, MAX_RANDOM_BYTES};

/// V8 must be initialised once per process, and `JsRuntime::new` will not do it twice.
static PLATFORM: Once = Once::new();

/// Draws from the pool the host refreshes, and refuses to wrap. Installed before the bootstrap,
/// which looks for exactly this name when it finds no `crypto`.
const POOL_DRAW: &str = r#"
globalThis.__ttl_pool_at = 0;
globalThis.__ttl_random_bytes = function (count) {
  var pool = globalThis.__ttl_pool;
  var at = globalThis.__ttl_pool_at;
  if (!pool || at + count > pool.length) {
    throw new Error('entropy pool exhausted: ' + count + ' bytes requested at ' + at);
  }
  globalThis.__ttl_pool_at = at + count;
  return pool.subarray(at, at + count);
};
"#;

/// A prepared V8 runtime. Lives on the signer's thread; never crosses one.
pub struct V8 {
    // Declared first so it is dropped first: the isolate must go while its tokio runtime is still
    // there to receive whatever V8 cancels on the way out.
    runtime: JsRuntime,
    // V8 posts delayed tasks and aborts the process if it is created outside a tokio runtime, so
    // the engine carries its own — single-threaded, and never given anything else to do.
    tokio: tokio::runtime::Runtime,
}

impl Engine for V8 {
    const NAME: &'static str = "V8";

    fn start(bundle: &str, options: &str) -> Result<Self, EmbeddedError> {
        PLATFORM.call_once(|| JsRuntime::init_platform(None));
        let tokio = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|error| EmbeddedError::Engine(error.to_string()))?;
        let runtime = {
            let _entered = tokio.enter();
            JsRuntime::new(RuntimeOptions::default())
        };
        let mut engine = Self { runtime, tokio };

        engine
            .run("ttl:pool.js", POOL_DRAW)
            .map_err(EmbeddedError::Engine)?;
        engine.refill_pool().map_err(EmbeddedError::Engine)?;
        engine
            .run("ttl:bootstrap.js", BOOTSTRAP)
            .map_err(EmbeddedError::Engine)?;

        let out = engine
            .call("ttlPrepare", bundle, options)
            .map_err(EmbeddedError::Bundle)?;
        crate::read_error(&out).map_err(EmbeddedError::Bundle)?;
        Ok(engine)
    }

    fn sign(&mut self, url: &str, product: &str) -> Result<String, String> {
        // A fresh pool per signature: entropy is never shared across two of them.
        self.refill_pool()?;
        let out = self.call("ttlSignUrl", url, product)?;
        crate::read(&out)
    }
}

impl V8 {
    /// Evaluate a script for its side effects, reporting the exception rather than a stack.
    fn run(&mut self, name: &'static str, source: &str) -> Result<(), String> {
        let _entered = self.tokio.enter();
        self.runtime
            .execute_script(name, source.to_string())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// Replace the entropy pool with fresh bytes and rewind its cursor.
    fn refill_pool(&mut self) -> Result<(), String> {
        let mut bytes = vec![0u8; MAX_RANDOM_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);

        let _entered = self.tokio.enter();
        deno_core::scope!(scope, &mut self.runtime);
        let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
        let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
        let pool = v8::Uint8Array::new(scope, buffer, 0, MAX_RANDOM_BYTES)
            .ok_or("could not build the entropy pool")?;
        let global = scope.get_current_context().global(scope);

        let key = v8::String::new(scope, "__ttl_pool").ok_or("out of V8 string space")?;
        global.set(scope, key.into(), pool.into());
        let cursor = v8::String::new(scope, "__ttl_pool_at").ok_or("out of V8 string space")?;
        let zero = v8::Integer::new(scope, 0);
        global.set(scope, cursor.into(), zero.into());
        Ok(())
    }

    /// Call one of the driver's two-argument globals and read back its JSON reply.
    ///
    /// Arguments are passed as V8 strings rather than interpolated into a script: the bundle is
    /// 235 KB, and escaping it into source would be both slower and one more thing to get wrong.
    fn call(&mut self, name: &str, first: &str, second: &str) -> Result<String, String> {
        let _entered = self.tokio.enter();
        deno_core::scope!(scope, &mut self.runtime);
        v8::tc_scope!(scope, scope);

        let global = scope.get_current_context().global(scope);
        let key = v8::String::new(scope, name).ok_or("out of V8 string space")?;
        let found = global
            .get(scope, key.into())
            .ok_or_else(|| format!("{name} is missing from the sandbox"))?;
        let function = v8::Local::<v8::Function>::try_from(found)
            .map_err(|_| format!("{name} is not callable"))?;

        let first = v8::String::new(scope, first).ok_or("out of V8 string space")?;
        let second = v8::String::new(scope, second).ok_or("out of V8 string space")?;
        let undefined = v8::undefined(scope);
        let out = function.call(scope, undefined.into(), &[first.into(), second.into()]);

        match out {
            Some(value) => Ok(value.to_rust_string_lossy(scope)),
            // What the sandbox threw, as a sentence. V8 reports nothing else once a call has
            // returned `None`.
            None => Err(match scope.exception() {
                Some(thrown) => format!("{name} threw: {}", thrown.to_rust_string_lossy(scope)),
                None => format!("{name} failed without an exception"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pool must reach the sandbox, and it must refuse to wrap: a draw that quietly restarted
    /// would repeat entropy inside one signature, which is the failure this design exists to avoid.
    #[test]
    fn the_entropy_pool_is_wired_up_and_refuses_to_wrap() {
        PLATFORM.call_once(|| JsRuntime::init_platform(None));
        let tokio = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let runtime = {
            let _entered = tokio.enter();
            JsRuntime::new(RuntimeOptions::default())
        };
        let mut engine = V8 { runtime, tokio };
        engine.run("ttl:pool.js", POOL_DRAW).expect("pool");
        engine.refill_pool().expect("fill");
        engine.run("ttl:bootstrap.js", BOOTSTRAP).expect("bootstrap");

        let report = engine
            .call(
                "eval",
                &format!(
                    "(function () {{ \
                       if (typeof globalThis.TTL_RANDOM_SOURCE !== 'function') return 'unwired'; \
                       var a = new Uint8Array(16); \
                       globalThis.TTL_RANDOM_SOURCE(a); \
                       if (!Array.prototype.some.call(a, function (b) {{ return b !== 0; }})) \
                         return 'all zero'; \
                       try {{ globalThis.__ttl_random_bytes({}); }} \
                       catch (error) {{ return 'ok'; }} \
                       return 'wrapped'; \
                     }})()",
                    MAX_RANDOM_BYTES + 1
                ),
                "",
            )
            .expect("probe the pool");
        assert_eq!(report, "ok");
    }
}
