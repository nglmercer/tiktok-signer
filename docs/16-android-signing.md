# 16. Android: reuse the Rust core

**Verdict: the Rust signing core runs on Android as-is, so the app reuses it over
JNI. No separate Android signing SDK is needed.**

That sentence is the whole document; what follows is the evidence. The work behind it:

- `crates/ttl-sign-mobile` — the desktop signing stack reduced to what survives on a
  phone: QuickJS engine, the shared `bootstrap.js` sandbox, URL builders, C ABI, JNI.
- `android/` — an example app that resolves a room and signs the socket URL through
  the reused core.
- `scripts/android/build-native.sh` — NDK build for all four ABIs.

## What was reused, what was rewritten, and why

| Piece | Reused? | Form on Android |
|---|---|---|
| `ttl-sign-core` (query/URL construction, presets) | **Yes, as-is** | Compiled into `libttl_sign_mobile.so`; zero Android-specific changes |
| `bootstrap.js` (the sandbox) | **Yes, same file** | Included by the Rust crate |
| QuickJS engine (`rquickjs`) | **Yes, with one workaround** | `bindgen` feature on Android targets (below) |
| `ttl-live-discovery` (`reqwest` + `tokio`) | **No** | Rewritten in ~120 lines of Kotlin (`Discovery.kt`, `BundleFetch.kt`) over `HttpURLConnection` |
| `ttl-sign-embedded` (async signer, V8 engine) | **No** | Replaced by a blocking signer: JNI calls block, so `tokio` buys nothing; V8/`deno_core` does not cross-compile sanely |
| WebSocket + events (`ttl-live-ws`, `ttl-live-events`, `ttl-live-proto`) | **No** | Not ported. On Android this is OkHttp's WebSocket plus generated protobuf — standard app work, blocked on nothing |

The boundary is principled: **pure logic travels, runtimes stay home.** Anything that
does I/O or owns an async runtime gets rewritten against the platform's own stack;
everything else compiles unchanged.

A pure-Kotlin signing alternative (same sandbox evaluated in a WebView, i.e. the
device's own V8) was considered and dropped: it would be a second engine to keep in
parity for no capability the reused core lacks.

## The one Rust workaround: QuickJS bindings

`rquickjs-sys 0.12` ships pre-generated C bindings per host platform, and Android is
not among them — the stock build fails in the bindings include. The fix is the
crate's own `bindgen` feature, enabled for Android targets only:

```toml
[target.'cfg(target_os = "android")'.dependencies]
rquickjs = { version = "0.12", features = ["bindgen"] }
```

Each ABI build passes that ABI's `--target`/`--sysroot` to bindgen
(`scripts/android/build-native.sh`), so the 32-bit ABIs get ILP32 layouts rather than
the host's LP64. Host builds keep the fast prebuilt path and need no libclang.

Result: `libttl_sign_mobile.so` for `arm64-v8a`, `armeabi-v7a`, `x86_64`, `x86`
(~2.1–2.5 MB each, debug symbols included), exporting 6 JNI entry points and the
7-function C ABI. No `tokio`, no TLS stack, no HTTP client in the `.so`.

## What was verified, and what still needs a device

Verified on this machine (no KVM, no physical device — neither emulator CPU mode
could boot here):

- `cargo test -p ttl-sign-mobile`: 12 tests green, including **byte-parity between
  mobile QuickJS and the Node/V8 reference** (`tests/mobile_sign.rs`, same technique
  as `ttl-sign-embedded/tests/parity.rs`) and a C-ABI round trip.
- `cargo clippy -p ttl-sign-mobile --all-targets --features jni -- -D warnings`: clean.
- NDK builds for all four ABIs link; `llvm-nm` confirms all 13 exported symbols.
- **The real JNI entry points driven from a host JVM** (`java` + `javac` only, no
  Android): version, URL building, open/sign/close, pinned determinism across two
  JNI signers, and both error paths throwing `RuntimeException` with the Rust message.
  Host debug timings: open 323 ms (bundle parse, once), sign 88 ms.
- `./gradlew assembleDebug`: APK builds with all four natives.
- `./gradlew testDebugUnitTest`: JVM unit tests green (discovery parsing).

Not verifiable here — needs one device or KVM emulator run:

- `connectedDebugAndroidTest` (`OnDeviceSignTest`): asserts the reused core produces
  a browser-shaped `X-Gnarly` signature on a phone. Written; never executed.
- Tapping through `MainActivity`, and real-device latency/size numbers.
- First live connection from a phone (the signed URL against `webcast-ws`).

The honest gap is therefore narrow: everything except actual execution on Android
is proven, and the unproven part is one thin seam (ART's JNI dispatch of already
correct symbols).

## Recommendation

1. **Ship the Rust core in the app** (`ttl-sign-mobile` over JNI): byte-parity with
   the server, no WebView dependency, URL builders reused rather than ported.
2. **Do not port `reqwest`/`tokio`/protobuf to the phone.** HTTP is
   `HttpURLConnection`/OkHttp, the socket is OkHttp's WebSocket, events are the same
   `.proto` files through `protobuf-javalite`. That is ordinary app code, not research.
3. When the bundle version moves, app and server move together: the URL and SHA-256
   live in `BundleFetch.kt`, `packages/tiktok-live/src/signer.ts`, and the fixture
   profile — one version string in three places, all pinned to the measured bundle.
