# 08 — Headless migration audit

This file records current evidence against the architectural definition of done. A checked
item means the repository currently contains a direct implementation and an executable gate;
it does not mean the native signer is live-compatible.

| Requirement | State | Evidence |
|---|---|---|
| Normal CI runs without WebView | Complete | `.github/workflows/ci.yml`, workspace `default-members`, and the dependency-tree gate. |
| Normal integration tests run offline | Complete | `ttl-sign-server/tests/replay.rs` covers the sanitized success/error corpus. |
| Sign server is backend-agnostic | Complete | `AppState` owns `Arc<dyn SignerBackend>`; the server library contains no WebView type. |
| WebView implements the common contract | Complete | `impl SignerBackend for ttl_sign_webview::Signer`; optional `webview` feature compiles separately. |
| Replay implements the common contract | Complete | `ReplayBackend`, schema version 1 validation, and missing-case failure. |
| Native implements the common contract | Structural only | `NativeBackend` passes the shared contract with `StaticAlgorithm`; the live algorithm is explicitly unsupported. |
| Research observations serialize safely | Complete for backend and URL-signing outputs | Signed queries are never stored; cookie values, route values, declared signing inputs, cursors, internal metadata, and protobuf bytes serialize only as placeholders or SHA-256 digests and lengths. Repeated and same-identity paired traces are non-overwriting and path-safe. |
| Oracle/native comparisons are automated | Infrastructure complete | `DifferentialRunner` accepts any two `SignerBackend` implementations and classifies differences; `ttl-sign-oracle-replay` supplies the explicit live-oracle command. A controlled live corpus and compatible native algorithm are still missing. |
| Discovered behavior has regression tests | Partial | Query encoding/mutation, URI reconstruction, transport protobuf, preset consistency, event goldens, replay cases, trace stability, entropy-aware comparison, and the first sanitized SDK/VM profile are covered. A broad authorized oracle corpus is still missing. |
| Browser dependencies are optional | Complete for normal build/test/server library | Default members and replay server contain no Wry edge; the live server explicitly enables `webview`. |
| Production can operate headlessly | Complete for the transport path | `ttl-sign-headless` implements `SignerBackend` with no browser, and `ttl-sign-server --features headless` runs live with no `wry` in its dependency tree. Signing runs through an external signer process; an account session is required because `/webcast/im/fetch/` refuses guests. Behaviour matches the WebView case for case, including `EmptyBody` on the same rooms at the same time. |

## Reproducible gates

```sh
cargo +1.86.0 fmt --all -- --check
cargo +1.86.0 clippy --all-targets -- -D warnings
cargo +1.86.0 test
cargo check -p ttl-sign-webview --all-targets
cargo check -p ttl-sign-server --all-features --all-targets
cargo check -p ttl-sign-lab --all-features --all-targets
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo run -p ttl-sign-lab -- fixtures/signing fixtures/signing
cargo run -p ttl-sign-lab --bin ttl-sign-plan -- fixtures/research/plan.example.json
```

The next convergence work starts at the confirmed webmssdk VM entry points. It should recover
the VM instruction and call graph, expand the controlled corpus one variable at a time, and
convert every confirmed result into a sanitized fixture before enabling the native algorithm.
