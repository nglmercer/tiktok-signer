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

| Engine | Acceptance | Prepare | Per signature | Notes |
|---|---|---|---|---|
| **QuickJS** (`rquickjs` 0.12) | **PASS**, byte-identical | 137 ms | **76 ms** | whole spike binary is 1.9 MB |
| V8 (Node/Deno, in-process) | PASS, byte-identical | 57 ms | **8 ms** | the engine the bundle was built for |
| Boa 0.21 | **FAIL** | — | — | see below |
| *Node subprocess (today)* | PASS | — | *118 ms* | includes process start and a full parse |

Measured 2026-08-18 on this machine with `spikes/embed-spike`, release builds, 20 iterations.

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

**QuickJS**, via `rquickjs`. It passes, it is 1.9 MB rather than tens of megabytes, it needs no JIT,
and 76 ms per signature is faster than today's 118 ms while removing the Node dependency entirely.
V8 is nine times faster per signature and remains the right choice where binary size does not matter
and the target is a desktop or a server — but it cannot target `wasm32`, which is one of this
project's stated targets.

For the browser target specifically, embed nothing: the page already has an engine. Compile the
Rust core to `wasm32` and run the same flattened bootstrap through `wasm-bindgen`.

## Reproducing

```sh
node scripts/headless/tools/build-bootstrap.mjs
cargo run --release --manifest-path spikes/embed-spike/Cargo.toml --features quickjs -- /tmp/webmssdk.js
cargo run --release --manifest-path spikes/embed-spike/Cargo.toml --features boa -- --probe
```

`spikes/embed-spike` is deliberately not a workspace member: these dependencies are heavy and must
not reach the lockfile of anything that ships. It is kept rather than deleted so the table above can
be re-derived when an engine releases a new version.
