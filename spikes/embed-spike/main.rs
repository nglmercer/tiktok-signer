//! Can this engine produce the signature Node produces, and how fast?
//!
//! ```sh
//! node scripts/headless/tools/build-bootstrap.mjs
//! cargo run --release --features quickjs -- /tmp/webmssdk.js
//! cargo run --release --features boa     -- /tmp/webmssdk.js
//! ```
//!
//! One acceptance test, the same for all three: sign the canonical URL under the pinned profile and
//! print the four parameters. They must match what Node prints byte for byte — the sandbox freezes
//! the clock, `performance`, `Math.random` and the entropy sequence precisely so that this is a
//! diff rather than an opinion.
//!
//! Everything else — load time, per-signature time, binary size — is secondary, and only worth
//! reading for an engine that passes.

use std::time::Instant;

/// The URL every engine signs. Same query as `scripts/headless/lib/sign.mjs`'s `DEFAULT_URL`, so
/// the Node reference and these runs are comparable.
const URL: &str = "https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&app_language=en\
&app_name=tiktok_web&browser_language=en-US&browser_name=Mozilla&browser_online=true\
&browser_platform=Linux%20x86_64\
&browser_version=5.0%20(X11%3B%20Linux%20x86_64)%20AppleWebKit%2F537.36%20(KHTML%2C%20like%20Gecko)\
%20Chrome%2F131.0.0.0%20Safari%2F537.36\
&cookie_enabled=true&cursor=&device_id=7300000000000000001&device_platform=web&did_rule=3\
&fetch_rule=1&identity=audience&internal_ext=&last_rtt=0&live_id=12&resp_content_type=protobuf\
&room_id=7300000000000000001&screen_height=1080&screen_width=1920&sup_ws_ds_opt=1\
&tz_name=America%2FNew_York&version_code=270000&webcast_language=en";

const ITERATIONS: usize = 20;

/// A minimal version of what the sandbox does, for engines that fail somewhere inside 235 KB of
/// obfuscated code. Isolates the two semantics the sandbox depends on: `with` over a `Proxy` whose
/// `has` trap always answers true, and assignment through that proxy landing on the target.
const PROBE: &str = r#"
(function () {
  var target = { seen: [] };
  var proxy = new Proxy(target, {
    has: function () { return true; },
    get: function (t, k) { return k === Symbol.unscopables ? undefined : t[k]; },
    set: function (t, k, v) { t[k] = v; t.seen.push(String(k)); return true; }
  });
  var out = [];
  new Function('__s', 'with(__s){ table = []; table[7] = function () { return 42; }; }')(proxy);
  out.push('assign-to-target=' + (typeof target.table === 'object'));
  out.push('leaked-to-global=' + (typeof globalThis.table !== 'undefined'));
  out.push('call-through-index=' + (target.table && typeof target.table[7] === 'function'
    ? target.table[7]() : 'not-callable'));
  new Function('__s', 'with(__s){ var declared = 1; fn = function () { return declared; }; }')(proxy);
  out.push('var-in-with=' + (target.fn ? target.fn() : 'missing'));
  return out.join('\n  ');
})()
"#;

fn main() {
    if std::env::args().any(|arg| arg == "--probe") {
        match probe(PROBE) {
            Ok(report) => println!("{ENGINE} sandbox semantics:\n  {report}"),
            Err(error) => println!("{ENGINE} probe failed: {error}"),
        }
        return;
    }

    let bundle_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/webmssdk.js".to_string());
    let bootstrap = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/bootstrap.js"))
        .expect("run: node scripts/headless/tools/build-bootstrap.mjs");
    let bundle = std::fs::read_to_string(&bundle_path).expect("signing bundle");

    println!("engine     {ENGINE}");
    println!("bundle     {} bytes", bundle.len());
    println!("bootstrap  {} bytes", bootstrap.len());

    let started = Instant::now();
    let first = match sign(&bootstrap, &bundle, URL) {
        Ok(signed) => signed,
        Err(error) => {
            println!("\nFAILED: {error}");
            std::process::exit(1);
        }
    };
    let cold = started.elapsed();
    println!("cold run   {} ms", cold.as_millis());

    for (name, value) in parameters(&first) {
        println!("  {name:<12} {} bytes", value.len());
    }
    println!("\nsigned URL for comparison:\n{first}");

    match warm(&bootstrap, &bundle, URL, ITERATIONS, &first) {
        Ok((prepare, per_signature, healthy)) => {
            println!("\nwarm context");
            println!("  prepare    {prepare} ms (bundle parsed and initialised once)");
            println!("  signature  {per_signature} ms each over {ITERATIONS}");
            println!(
                "  behaviour  {}",
                if healthy {
                    "first matches the cold run, and every signature after it differs"
                } else {
                    "UNEXPECTED: either the first differs from cold, or two signatures repeated"
                }
            );
        }
        Err(error) => println!("\nwarm context unavailable: {error}"),
    }
}

/// The parameters the SDK appended, in the order they appear. String surgery rather than a URL
/// crate: this spike has no dependencies beyond the engine under test.
fn parameters(signed: &str) -> Vec<(String, String)> {
    let query = signed.split_once('?').map(|(_, q)| q).unwrap_or_default();
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(key, _)| key.starts_with("X-") || *key == "msToken")
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[cfg(feature = "quickjs")]
/// Prepare once, then sign repeatedly against the same loaded sandbox.
fn warm(
    bootstrap: &str,
    bundle: &str,
    url: &str,
    iterations: usize,
    reference: &str,
) -> Result<(u128, u128, bool), String> {
    use rquickjs::{Context, Function, Runtime};

    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    context.with(|ctx| {
        ctx.eval::<(), _>(bootstrap).map_err(|e| e.to_string())?;
        let prepare: Function = ctx.globals().get("ttlPrepare").map_err(|e| e.to_string())?;
        let sign: Function = ctx.globals().get("ttlSignUrl").map_err(|e| e.to_string())?;

        let started = Instant::now();
        let out: String = prepare.call((bundle,)).map_err(|e| e.to_string())?;
        if out.contains("\"error\"") {
            return Err(out);
        }
        let prepared_in = started.elapsed().as_millis();

        // Signatures are *not* expected to repeat: the SDK carries per-call state, so a warm
        // context produces a fresh value each time — which is what a browser does too. What must
        // hold is that the first one off a warm context equals the one off a cold context.
        let mut first: Option<String> = None;
        let mut distinct = std::collections::HashSet::new();
        let signing = Instant::now();
        for _ in 0..iterations {
            let out: String = sign.call((url,)).map_err(|e| e.to_string())?;
            let signed = decode(&out)?;
            distinct.insert(signed.clone());
            if first.is_none() {
                first = Some(signed);
            }
        }
        let identical = first.as_deref() == Some(reference) && distinct.len() == iterations;
        Ok((
            prepared_in,
            signing.elapsed().as_micros() / iterations as u128 / 1000,
            identical,
        ))
    })
}

#[cfg(feature = "quickjs")]
const ENGINE: &str = "QuickJS (rquickjs)";

#[cfg(feature = "quickjs")]
fn probe(source: &str) -> Result<String, String> {
    use rquickjs::{Context, Runtime};
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    context.with(|ctx| {
        ctx.eval::<String, _>(source).map_err(|_| {
            let value = ctx.catch();
            match value.as_exception() {
                Some(exception) => exception.message().unwrap_or_else(|| "?".into()),
                None => format!("{value:?}"),
            }
        })
    })
}

#[cfg(feature = "quickjs")]
fn sign(bootstrap: &str, bundle: &str, url: &str) -> Result<String, String> {
    use rquickjs::{Context, Function, Runtime};

    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    context.with(|ctx| {
        // QuickJS reports "Exception generated by QuickJS" and keeps the real error on the context,
        // so every failure fetches it. A spike that cannot say *why* an engine failed answers
        // nothing.
        let describe = |stage: &str| {
            let value = ctx.catch();
            let detail = match value.as_exception() {
                Some(exception) => format!(
                    "{}{}",
                    exception.message().unwrap_or_else(|| "?".into()),
                    exception
                        .stack()
                        .map(|stack| format!("\n{stack}"))
                        .unwrap_or_default()
                ),
                None => format!("{value:?}"),
            };
            format!("{stage}: {detail}")
        };

        ctx.eval::<(), _>(bootstrap)
            .map_err(|_| describe("bootstrap"))?;
        let sign: Function = ctx
            .globals()
            .get("ttlSign")
            .map_err(|_| describe("ttlSign missing"))?;
        let out: String = sign
            .call((bundle, url))
            .map_err(|_| describe("ttlSign threw"))?;
        decode(&out)
    })
}

#[cfg(feature = "boa")]
fn warm(_: &str, _: &str, _: &str, _: usize, _: &str) -> Result<(u128, u128, bool), String> {
    Err("only measured for the engine that passes the acceptance test".into())
}

#[cfg(feature = "boa")]
const ENGINE: &str = "Boa";

#[cfg(feature = "boa")]
fn probe(source: &str) -> Result<String, String> {
    use boa_engine::{Context, Source};
    let mut context = Context::default();
    let value = context
        .eval(Source::from_bytes(source))
        .map_err(|e| e.to_string())?;
    value
        .as_string()
        .ok_or("probe did not return a string")?
        .to_std_string()
        .map_err(|e| e.to_string())
}

#[cfg(feature = "boa")]
fn sign(bootstrap: &str, bundle: &str, url: &str) -> Result<String, String> {
    use boa_engine::{js_string, Context, JsValue, Source};

    let mut context = Context::default();
    context
        .eval(Source::from_bytes(bootstrap))
        .map_err(|e| format!("bootstrap: {e}"))?;

    let sign = context
        .global_object()
        .get(js_string!("ttlSign"), &mut context)
        .map_err(|e| format!("ttlSign missing: {e}"))?;
    let sign = sign.as_callable().ok_or("ttlSign is not callable")?.clone();
    let out = sign
        .call(
            &JsValue::undefined(),
            &[
                js_string!(bundle).into(),
                js_string!(url).into(),
            ],
            &mut context,
        )
        .map_err(|e| format!("ttlSign threw: {e}"))?;
    let out = out
        .as_string()
        .ok_or("ttlSign did not return a string")?
        .to_std_string()
        .map_err(|e| e.to_string())?;
    decode(&out)
}

/// The driver returns `{"signed": …}` or `{"error": …}`. Parsed by hand for the same reason.
fn decode(out: &str) -> Result<String, String> {
    if let Some(at) = out.find("\"error\":\"") {
        let rest = &out[at + 9..];
        let end = rest.find('"').unwrap_or(rest.len());
        return Err(rest[..end].to_string());
    }
    let at = out
        .find("\"signed\":\"")
        .ok_or_else(|| format!("unexpected driver output: {out}"))?;
    let rest = &out[at + 10..];
    let end = rest.find('"').ok_or("unterminated signed URL")?;
    let signed = &rest[..end];
    if signed == "null" || signed.is_empty() {
        return Err("the patched fetch never signed the request".into());
    }
    Ok(signed.replace("\\u0026", "&").replace("\\/", "/"))
}

#[cfg(not(any(feature = "quickjs", feature = "boa")))]
compile_error!("pick an engine: --features quickjs | boa");
