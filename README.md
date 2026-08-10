# tiktok-signer

Self-hosted TikTok LIVE transport written in Rust. A Wry WebView runs TikTok's page and
relays its already-authenticated WebSocket frames to Rust over IPC.

**Primary objective:** receive TikTok LIVE protobuf frames without a proprietary sign
server or a reimplementation of TikTok's anti-bot signatures.

## Status

The page-owned WebSocket relay was validated against a real room on 2026-08-10. It
received room-entry, heartbeat, and `msg` frames without Euler and without calling
`/webcast/im/fetch/`.

| Crate | Status |
|---|---|
| `ttl-sign-core` | Presets, queries, cookie jar, `SignOutcome`, and minimal protobuf decoding. |
| `ttl-sign-webview` | Wry engine, JS bridge, session bootstrap, navigation, and page-WebSocket relay. |
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
| Page-owned WebSocket | Works; TikTok signs and opens it. |
| Room entry and heartbeat | Managed by TikTok's page and observed over IPC. |
| Receive protobuf frames | Works; multiple `msg` frames received in live validation. |

The old `/webcast/im/fetch/` replay remains diagnostic code but is no longer on the
live-check critical path. It is unreliable because TikTok may return a silent empty-body
rejection even with an authenticated session.

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

1. TikTok's page signs and owns the WebSocket.
2. The initialization bridge mirrors binary frames to Rust without altering the socket.
3. No Euler API, custom sign server, or native X-Gnarly implementation is required.
