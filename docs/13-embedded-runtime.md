# 13 — Running the signer without Node

Question this answers: which JavaScript engine can run the real `webmssdk` bundle inside a Rust
process, and does it produce the same signature Node does?

## Why it matters

Every signature today costs a process. `CommandSigner` spawns `node scripts/headless/sign-url.mjs`,
which parses the 235 KB bundle from scratch, and the host must have Node — the Docker image exists
as a `node:22` runtime for that one reason. A packaged desktop app cannot do that, and neither can
anything that wants more than a handful of signatures a second.

## What the sandbox actually needs

The bundle itself is undemanding. It is fully ES5-transpiled: no `class`, no arrow functions, no
`async`, no spread, no `WebAssembly`, no `eval`. What it uses is `Symbol`, `Object.defineProperty`,
`Uint8Array`, one `Reflect`, `crypto.getRandomValues`, and `TextEncoder`.

The demanding part is ours. `scripts/headless/shim.mjs` evaluates the bundle as

```js
new Function('__sandbox', `with(__sandbox){ ${source} }`)(sandbox)
```

where `sandbox` is a `Proxy` whose `has` trap always returns `true` and which special-cases
`Symbol.unscopables`. Dynamic compilation, `with`, and Proxy traps together are the combination a
young engine is most likely to get wrong.

Two changes made the sandbox portable before any engine was chosen (see the commit "Take Node out of
the sandbox"): the canvas fingerprint PNG became a generated constant instead of being built with
`zlib`, and base64 became plain JavaScript instead of `Buffer`. What is left is filled in by
`scripts/headless/tools/build-bootstrap.mjs`, which flattens the shim into one classic script and
prepends only what a bare engine lacks: `console`, timers, `queueMicrotask`, `TextEncoder`,
`TextDecoder`, `URL`, `URLSearchParams`, `Intl`, and the Annex B `escape`/`unescape` pair.

## The measurement

One acceptance test for every engine: sign the canonical URL under the pinned profile — frozen
clock, frozen `performance`, `Math.random` fixed, deterministic entropy — and compare the four
parameters against what Node produces. Byte equality or nothing.

| Engine | Acceptance | Load the bundle | Per signature, warm | Binary |
|---|---|---|---|---|
| **QuickJS** (`rquickjs` 0.12) | **PASS**, byte-identical | 140 ms | **28 ms** | 3.1 MB |
| **V8** (`deno_core` 0.410) | **PASS**, byte-identical | 57 ms | **3 ms** | 69.9 MB |
| Boa 0.21 | **FAIL** — see below | — | — | — |
| *Node subprocess (today)* | PASS | *per signature* | *118 ms* | — |

Measured 2026-08-18 on this machine, release builds, 20 iterations. The two engine rows come from
`cargo test --release -p ttl-sign-embedded --features v8 --test latency -- --ignored --nocapture`,
which is the shipped code rather than a spike, and the binary column is the same test binary built
with each feature — so the 67 MB is what V8 actually costs a deliverable, not an estimate.

A warm context is the point of the exercise: the first signature off a warm QuickJS context is
byte-identical to a cold one, and every signature after it differs — the SDK carries per-call state,
exactly as it does in a browser. So warmth costs nothing in fidelity and removes both the process
start and the parse.

## Why Boa fails, as far as it was chased

Boa handles the sandbox. A probe of the exact semantics the shim depends on
(`spikes/embed-spike --probe`) passes on all four points: assignment through the `with` proxy lands
on the target, nothing leaks to the global object, an indexed function stored through the proxy is
callable, and `var` inside `with` behaves.

It fails later, inside the bundle's own bytecode interpreter, at its opcode dispatch:

```text
TypeError: not a callable function
    at L (unknown at :2:235263)
```

That offset is the bundle's dispatch loop, `b[c](u)` — an opcode handler that was never installed.
Something earlier in 235 KB of obfuscated code behaved differently under Boa and left a hole in the
table. It was not chased further: the decision rule for this work is that an engine which cannot
reproduce the signature is out, and finding which of Boa's builtins differs is open-ended.

Worth revisiting when Boa matures, because it is the only pure-Rust option and the only one that
targets `wasm32` without a C toolchain.

## Decision

**Both, behind features; QuickJS by default.**

QuickJS is the default because it passes, it costs 3.1 MB rather than 70, it needs no JIT, and
28 ms per signature is already four times better than today's 118 ms with the Node dependency gone.

V8 is built too, behind `--features v8`, because it is nine times faster again and this project's
targets include a server that may sign continuously. It wins the `EmbeddedSigner` alias whenever it
is enabled: `quickjs` is a default feature that arrives without anyone choosing it, `v8` only ever
arrives because someone did.

```sh
cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless      # QuickJS
cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless,v8   # V8
```

WASM no longer constrains the choice — it was dropped from the targets. For a browser, embed
nothing anyway: the page already has an engine. Compile the Rust core to `wasm32` and run the same
flattened bootstrap through `wasm-bindgen`.

## What was built on that decision

`crates/ttl-sign-embedded` holds a warm context on its own thread — neither engine's context is
`Send` — and implements the same `UrlSigner` trait as `CommandSigner`, so the sign server and
`live-check` swap between them with `TTL_SIGNER=embedded` and nothing else changes. It signs all
three products, and its `Profile` carries the user agent, cookie jar and stored token the sandbox
should report.

The engines sit behind one four-line trait:

```rust
pub trait Engine: Sized {
    const NAME: &'static str;
    fn start(bundle: &str, options: &str) -> Result<Self, EmbeddedError>;
    fn sign(&mut self, url: &str, product: &str) -> Result<String, String>;
}
```

Everything the two could differ about above that line — how the sandbox is built, which products
exist, how a reply is read, the thread and the channel — is shared, so a second engine cannot grow
a second behaviour where nobody is looking. `tests/parity.rs` then signs every product in both and
compares the bytes, with no Node involved, which is the check that keeps them honest offline.

### What V8 needed that QuickJS did not

- **A tokio runtime on its thread.** `deno_core` posts delayed tasks and *aborts the process* if
  the isolate was created outside one. The signer thread is a plain `std::thread`, so the V8 engine
  carries its own current-thread runtime and enters it around every use.
- **Entropy without an op layer.** A bare `deno_core` runtime has no `crypto`. Rather than register
  an `#[op2]` — whose macro surface moves between `deno_core` releases and would pin this crate to
  one — the host installs a pool of random bytes as a `Uint8Array` plus a three-line JavaScript
  draw function, and replaces the pool before every signature. A draw past its end throws instead
  of wrapping, because repeated entropy inside one signature is precisely the kind of quiet
  difference this project keeps getting bitten by.

  How big should the pool be? `tests/entropy.rs` counts the bytes at the host boundary and the
  answer is **zero**: bundle 1.0.0.388 never reaches `crypto.getRandomValues` on any of the three
  signing paths. Its per-signature variation comes from `Date.now` and `Math.random`, which is also
  why the pinned profile — which freezes both — reproduces exactly. The pool is 8 KB and exists for
  the bundle version that starts asking; the test is what notices when that happens.

Measured through the sign server against a live room, same machine, same rooms:

| Signer | Latency per signature |
|---|---|
| subprocess (`node`, current default) | 95–105 ms |
| **embedded (QuickJS, warm)** | **70–89 ms** |
| **embedded (V8, warm)** | **19 ms** |

The QuickJS gap is smaller than the raw engine numbers suggest — most of the subprocess cost is
Node's start-up and the 235 KB parse, and most of what is left is QuickJS being an interpreter,
which is exactly what V8 removes. Both rows are from the same live room through the same server
(`examples/node-connector`, one signature each); the V8 row was measured on 2026-08-19. What the
embedded path buys beyond the milliseconds is that there is no Node on the host at all.

### One thing the subprocess was hiding

`byted_acrawler.registerWsSigner` is a **one-shot**: it hands back the signer and removes itself, so
a second call finds nothing. A process that signs once and exits never notices. A warm context does,
and the second socket signature failed with "this bundle exposes no registerWsSigner" until the
driver started keeping what it was given — which is exactly what the player does with its own cached
copy. `crates/ttl-sign-embedded/tests/warm.rs` signs 25 times to keep it that way.

That is the general shape of the risk in this migration: not that an engine computes a different
signature, but that a warm context exercises the SDK in ways a fresh process never did.

## Reproducing

```sh
node scripts/headless/tools/build-bootstrap.mjs
cargo run --release --manifest-path spikes/embed-spike/Cargo.toml --features quickjs -- /tmp/webmssdk.js
cargo run --release --manifest-path spikes/embed-spike/Cargo.toml --features boa -- --probe
```

```sh
cargo test -p ttl-sign-embedded                     # QuickJS against Node, and the warm-context regressions
cargo test -p ttl-sign-embedded --features v8      # and the two embedded engines against each other
cargo test --release -p ttl-sign-embedded --features v8 --test latency -- --ignored --nocapture
TTL_SIGNER=embedded cargo run -p ttl-live-discovery --example live-check
TTL_SIGNER=embedded cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless
```

`spikes/embed-spike` is deliberately not a workspace member: these dependencies are heavy and must
not reach the lockfile of anything that ships. It is kept rather than deleted so the Boa row can be
re-derived when Boa releases a new version. Its V8 arm is gone — V8 ships now, so its numbers come
from `crates/ttl-sign-embedded/tests/latency.rs` and a spike copy could only drift.
