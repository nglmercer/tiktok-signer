# 01 — Architecture

The system is split into four crates:

- `ttl-sign-core`: pure data types, ordered queries, cookies, presets, room lookup,
  and the minimal protobuf codec.
- `ttl-sign-webview`: Wry/WebKitGTK event loop, initialization bridge, session cookies,
  navigation, and page-owned WebSocket relay.
- `ttl-live-ws`: signed WebSocket URI construction, handshake, room entry, heartbeats,
  acknowledgements, and frame decoding.
- `ttl-sign-server`: thin HTTP integration layer exposing the signer.

## Design decisions

The WebView initialization script is installed before TikTok page scripts. It wraps the
native WebSocket constructor, leaves TikTok's connection behavior intact, and mirrors
open/frame/close events to Rust. TikTok's page owns signing, room entry, and heartbeats.

The older signed-fetch replay path remains available for diagnostics and compatibility,
but the primary transport does not depend on it.

One WebView owns one cookie session. Sessions must not be shared between rooms because
that increases rate-limit and anti-bot risk.

The main WebView event loop owns the GUI thread. Tokio runs on a worker thread and sends
requests through an event-loop proxy. The WebSocket crate does not reconnect internally:
signed URLs expire and the orchestrator must start a fresh flow.

The server returns typed rejection responses instead of treating an empty HTTP 200 body
as a successful payload.
