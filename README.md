# tiktok-signer

Custom sign server for TikTok LIVE, written in Rust and using a WebView
(`wry`) as the signing engine.

**Single objective:** obtain a valid protobuf response from
`https://webcast.tiktok.com/webcast/im/fetch/` so the room WebSocket can be opened.
Protobuf parsing and event consumption already exist and are outside this project's scope.

## Status

Current phase: **F0 — Reconnaissance** (see [roadmap](docs/02-roadmap.md)).

The workspace contains four crates and the F1–F3 tools implemented against the
specifications. Real-session validation is available through `fetch-dump`.

| Crate | Status |
|---|---|
| `ttl-sign-core` | Presets, queries, cookie jar, `SignOutcome`, and minimal protobuf decoding. |
| `ttl-sign-webview` | Wry engine, JS bridge, readiness gate, navigation, channel discovery, signing, and replay. |
| `ttl-live-ws` | WebSocket client with heartbeat, acknowledgements, and typed rejection handling. |
| `ttl-sign-server` | `GET /webcast/fetch` and `GET /healthz` endpoints. |

> Protobuf field numbers are confirmed against a real response
> (`2 = cursor`, `5 = internal_ext`, `7 = route_params`, `10 = push_server`).
> Capture with:
>
> `cargo run -p ttl-sign-webview --example fetch-dump`

### Verified flow (2026-08-10)

| Step | Status |
|---|---|
| Discover live channels | Works through the rendered `/live` DOM. |
| `unique_id` → `room_id` | Works without signing or a display. |
| Signed `/webcast/im/fetch/` | Works with an authenticated session. |
| Decode protobuf | Works; field numbers are confirmed. |
| WebSocket handshake | Accepted with status 101 and `handshake-msg: OK`. |
| Receive frames | Requires continued live-session validation. |

Anonymous sessions return an empty `/webcast/im/fetch/` body. Use the login example
to install an authenticated session before signing.

The WebSocket URI is signed independently by the SDK. `Signer::sign_ws_uri` performs
that signing; avoid repeated attempts from one IP because TikTok rate-limits signing.

## Tools

```sh
cargo run -p ttl-sign-webview --example endpoint-probe -- <user>
cargo run -p ttl-sign-webview --example fetch-dump -- <user>
cargo run -p ttl-sign-webview --example ws-probe -- <user>
cargo run -p ttl-sign-webview --example page-probe -- <user> "<js>"
```

### Log in

```sh
cargo run -p ttl-sign-webview --example login
cargo run -p ttl-sign-webview --example login -- --timeout 600
cargo run -p ttl-sign-webview --example login -- --logout
```

The login example opens a visible TikTok window, polls the `sessionid` cookie, and
stores the session at `$XDG_CONFIG_HOME/ttl-signer/session` with mode `0600`.
`TTL_SESSION_ID` takes precedence; `TTL_SESSION_FILE` changes the file path.

## Usage

```sh
cargo test --workspace

# Discover channels and resolve room IDs
cargo run -p ttl-live-ws --example rooms -- user1 user2

# Full flow against a live channel
cargo run -p ttl-sign-webview --example live-check
cargo run -p ttl-sign-webview --example live-check -- user

# Replay a captured request
cargo run -p ttl-live-ws --example replay -- fixtures/f0/im_fetch.curl

# Start the sign server
TTL_BIND=127.0.0.1:8080 cargo run -p ttl-sign-server
```

Linux/WebKitGTK requires X11 or Wayland even when the window is hidden. On systems
without a GPU, set `WEBKIT_DISABLE_DMABUF_RENDERER=1` and
`WEBKIT_DISABLE_COMPOSITING_MODE=1`.

## Documentation

| Document | Contents |
|---|---|
| [00 — Research](docs/00-research.md) | Connection flow and signing boundaries |
| [01 — Architecture](docs/01-architecture.md) | Crates, threading model, and design decisions |
| [02 — Roadmap](docs/02-roadmap.md) | Phases, deliverables, and acceptance criteria |
| [03 — Sign-server specification](docs/03-spec-sign-server.md) | HTTP endpoints and client compatibility |
| [04 — WebView bridge specification](docs/04-spec-webview-bridge.md) | JS↔Rust IPC contract |
| [05 — WebSocket client specification](docs/05-spec-websocket-client.md) | URI construction, headers, heartbeat, and acknowledgements |
| [06 — Risks and operations](docs/06-risks-and-ops.md) | Failure modes, rate limits, and maintenance |

## Summary

1. The critical path has one signature: the HTTP `/webcast/im/fetch/` request.
2. The WebSocket URL is signed by TikTok inside the protobuf response.
3. The WebView lets TikTok's own page sign the request instead of reimplementing the algorithm.

