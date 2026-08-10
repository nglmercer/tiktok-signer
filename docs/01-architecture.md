# 01 — Architecture

The system is split into four crates:

- `ttl-sign-core`: pure data types, ordered queries, cookies, presets, room lookup,
  and the minimal protobuf codec.
- `ttl-sign-webview`: Wry/WebKitGTK event loop, initialization bridge, session cookies,
  navigation, SDK signing, and HTTP replay.
- `ttl-live-ws`: signed WebSocket URI construction, handshake, room entry, heartbeats,
  acknowledgements, and frame decoding.
- `ttl-sign-server`: thin HTTP integration layer exposing the signer.

## Design decisions

The WebView initialization script is installed before TikTok page scripts so the SDK can
be observed without replacing its signing algorithm. Rust owns query construction.
JavaScript only invokes the patched page fetch and reports the resulting signed URL.

One WebView owns one cookie session. Sessions must not be shared between rooms because
that increases rate-limit and anti-bot risk.

The main WebView event loop owns the GUI thread. Tokio runs on a worker thread and sends
requests through an event-loop proxy. The WebSocket crate does not reconnect internally:
signed URLs expire and the orchestrator must start a fresh flow.

The server returns typed rejection responses instead of treating an empty HTTP 200 body
as a successful payload.

