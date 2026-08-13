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
| `ttl-sign-core` | Presets, queries, cookie jar, `SignOutcome`, a stable event subset, and generated schema bindings with bounded dynamic decoding. |
| `ttl-sign-webview` | Wry engine, JS bridge, session bootstrap, navigation, and page-WebSocket relay. |
| `ttl-live-ws` | WebSocket client with heartbeat, acknowledgements, and typed rejection handling. |
| `ttl-sign-server` | `GET /webcast/fetch`, `GET /webcast/rooms/{room_id}/connect` (Node client), and `GET /healthz`. |

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
| Room info and gift table | Works; `/webcast/room/info/` and `/webcast/gift/list/` are read as JSON from the page. |
| Page-signed URI reused outside the WebView | Works; delivers `msg` frames with no `/webcast/im/fetch/` call. |
| Page-owned WebSocket | Works; TikTok signs and opens it. |
| Room entry and heartbeat | Managed by TikTok's page and observed over IPC. |
| Receive protobuf frames | Works; multiple `msg` frames received in live validation. |

`/webcast/im/fetch/` is no longer used anywhere on a critical path: it returns a silent
empty-body rejection for this signer, with or without a session. Clients that need its
protobuf are served a result rebuilt from the player's own signed socket URI instead — see
the connector section below.

### Room state

The WebSocket reports *changes*; it never reports the room as it already is. Two endpoints
on `webcast.tiktok.com` fill that gap. They are signed, but the page's patched `fetch` signs
them, and unlike `/webcast/im/fetch/` they answer with CORS headers, so the body is readable
from inside the page:

```rust
let info = signer.room_info(&room_id).await?;   // title, owner, viewers, likes, cover
let gifts = signer.gift_list(&room_id).await?;  // gift id → name and diamond cost
```

Call both after navigating to the room's live page. `gift_list` returns a few megabytes
(626 gifts in the verified room), so request it once per session and keep the table: gift
events carry only a `gift_id`, and pricing them requires it.

### Refusals and verification

Webcast JSON endpoints answer `200` and report failure inside the envelope. A non-zero
`status_code` becomes `SignError::Refused`, carrying TikTok's own code and message rather
than being flattened into a parse error:

```text
room_info("1") -> TikTok refused the request (status_code=10011): Request params error
```

There is deliberately **no table of known codes**. Rate-limit and verification codes have
not been observed first-hand here, and constants invented for them would mislabel whatever
TikTok actually sends. Check `error.is_refusal()`, log the real code, and back off.

When a refusal might mean "prove you are human", ask the page directly:

```rust
if signer.challenge().await?.present {
    signer.set_window_visible(true).await?;      // hand the puzzle to a person
    signer.wait_for_challenge(Duration::from_secs(180)).await?;
    signer.set_window_visible(false).await?;
}
```

Detection is by observable effect — a captcha container that is actually laid out — not by
guessed status codes, so an ordinary error is not reported as a challenge. Measured as
`present: false` on a healthy page.

### Rotating a throttled identity

`ttwid` and `msToken` identify the *device*, not a person, so a guest that TikTok has
started refusing can simply become a different guest:

```rust
if error.is_refusal() {
    signer.rotate_guest_identity().await?;   // new device, then reload
}
```

Verified: `ttwid` and `msToken` both change and the engine keeps working. This is the
concrete payoff of staying a guest — an account cannot be rotated. Rotation discards warmed
browsing state, so use it in response to a refusal, not on a timer. A deliberately
configured session survives, because the bridge reinstalls those cookies on every document.

### Gift streaks

A held send button produces a run of `WebcastGiftMessage`s with a rising `repeat_count`;
only the last one carries the real total. Counting each message reports 15 roses where 9
were sent. `GiftStreaks` collapses the burst into one gift:

```rust
let mut streaks = GiftStreaks::new();
if let Some(gift) = streaks.observe(user_id, gift_id, repeat_count, repeat_end, streakable) {
    println!("{} × {} = {} diamonds", gift.gift_id, gift.count, gift.diamonds(price));
}
```

`streakable` comes from `Gift::is_streakable()` in the gift table. A gift missing from the
table completes immediately — reporting it once beats never reporting it.

### Recovery

`subscribe_live_events` and `subscribe_schema_events` survive the page losing its transport.
TikTok's page normally reconnects on its own; if it has not done so within 15 seconds, the
engine reloads the page, which makes it sign, connect, and re-enter the room from scratch.
After three failed attempts the subscription reports that the room is gone rather than
reloading forever. `Signer::reload()` exposes the same step manually.

Subscriber queues are bounded. A consumer that stops reading is disconnected instead of
being served a stream with silently missing frames — a closed channel is diagnosable, a gap
in a protobuf event stream is not.

### Handling every event

Nothing is ever dropped, but two different labels say "unknown", and they mean different
things:

| Label | Meaning | Fix |
|---|---|---|
| `[other] method=…` | Outside the six-method `LiveEvent` enum | Use the schema layer below |
| `type=Unknown` | Method not in the pinned v3 schema | Re-pin the schema |

`LiveEvent` is a deliberately small, stable subset (chat, gift, like, member, social, room
user). Everything else lands in `LiveEvent::Unknown` — that is the enum's boundary, not a
decoding failure.

The schema layer covers all of it. Read any of the schema's methods by field name, without
a hand-written struct per message type:

```rust
let event = ttl_live_events::decode_webcast_message(&method, &payload)?;
if let Some(text) = event.text("content") { … }
if let Some(user) = event.message("user") {
    let nickname = user.text("nickname");
    let id = user.number("id");
}
```

Accessors exist on both events and nested messages, so walking `user.badge.name` meets the
same API at each level. Asking for the wrong shape returns `None` rather than panicking.

When `is_known()` is `false`, TikTok shipped a message type newer than the pinned schema. The
event is still decoded — fields keep their wire numbers and values, only the names are
missing, which `live-check` shows as `#1=<60 bytes>`. To name them, move the pin with
`scripts/update-tiktok-protos.sh <commit>`.

### Schema coverage

`ttl-live-proto` builds the pinned TikTok Webcast **v3** schemas into generated protobuf
bindings plus descriptors for 730 message types, 64 of them addressable as `Webcast*` methods.
The dynamic listener in `ttl-live-events` exposes known field names and values while retaining
unknown fields structurally; the generated types are also available under `ttl_live_proto::v3`
for trusted, explicit decoding.

These schemas are **not** MIT-licensed like the rest of the workspace. See
[`crates/ttl-live-proto/README.md`](crates/ttl-live-proto/README.md) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) before redistributing or hosting.

## Tools

```sh
cargo run -p ttl-sign-webview --example endpoint-probe -- <user>
cargo run -p ttl-sign-webview --example fetch-dump -- <user>
cargo run -p ttl-sign-webview --example ws-probe -- <user>
cargo run -p ttl-sign-webview --example page-probe -- <user> "<js>"
cargo run -p ttl-sign-webview --example limit-probe -- [user] [requests]
```

`limit-probe` captures what a refusal actually looks like: it reports the verification
infrastructure the page carries, then calls `room/info` repeatedly as a guest until TikTok
stops answering `status_code: 0`. Use it to obtain real signatures before writing anything
that reacts to them.

### tiktok-live-connector (Node) compatibility

`examples/node-connector` points `tiktok-live-connector` at this project instead of Euler
Stream:

```sh
cargo run -p ttl-sign-server                      # terminal 1
cd examples/node-connector && npm install
bun run verify-signer.ts <username>               # terminal 2
npm run verify:node -- <username>                 # or tsx, on plain Node
npm run typecheck                                 # types only, no live channel
```

The example is TypeScript because the connector types its event handlers, so each payload is
inferred from the protobuf schema.

The Node client calls `GET /webcast/rooms/{room_id}/connect` (the Python client uses
`/webcast/fetch`); both are served, and the connect route also returns `X-Room-Id`, which
the Node client reads back.

**Verified 2026-08-10 against live rooms: it works.** 203 events — chat, likes, members,
follows, viewer counts — reached `tiktok-live-connector` with no Euler Stream involved.

The interesting part is how, because `/webcast/im/fetch/` answers this signer with an empty
body and cannot supply the `ProtoMessageFetchResult` the client expects. It is never asked
to. The player signs its **own** WebSocket URI, and `bridge.js` records that URI before the
connection is attempted, so with `block_page_websockets` enabled the engine captures a
signed transport nothing has used. `ws_uri::fetch_result_from_ws_uri` rebuilds the protobuf
from it: `push_server` from the URI base, `route_params` from its query, and a synthetic
cursor, since the `ws_reuse_supplement` transport carries none and clients reject an empty
one.

Two details make the reconstruction survive:

- **Values are decoded.** Clients re-encode `route_params` when rebuilding the query, so a
  stored `%2F` would become `%252F` and break the signature. Storing `/` round-trips.
- **The client's own parameters are removed.** The example empties
  `WebSocketConfigDefaults.DEFAULT_WS_CLIENT_PARAMS`; otherwise the connector merges ~27
  parameters of its own and appends `&version_code=270000`, sending a query TikTok never
  signed.

A browser also emits URIs that no Rust HTTP client will parse — `browser_version=5.0 (X11;
Linux x86_64)` contains raw spaces, which fail with "invalid uri character".
`ws_uri::sanitize_uri` percent-encodes only those, which does not disturb the signature, and
`LiveConnection::open_uri` applies it automatically.

### Accounts are optional

Listening works as a **guest**. Verified anonymously against a real live room on
2026-08-10: discovery, `unique_id` → `room_id`, `room/info`, `gift/list`, and the page
WebSocket with its chat events all work with no cookies at all, because TikTok's page serves
logged-out viewers.

Prefer guest. `sessionid` *is* the account, so using one attributes everything the automated
browser does to it, and an account is not a fix for rate limiting — a fresh guest identity
is. Log in only for what genuinely needs an identity, such as subscriber-only rooms.

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

# Verify schema-registry decoding against a live channel
cargo run -p ttl-sign-webview --example schema-check -- user

# Replay a captured request
cargo run -p ttl-live-ws --example replay -- fixtures/f0/im_fetch.curl

# Start the sign server
TTL_BIND=127.0.0.1:8080 cargo run -p ttl-sign-server
```

## Deployment

The engine is a real browser: Linux/WebKitGTK requires X11 or Wayland even when the window
is hidden, so a headless VPS needs `Xvfb` — not a desktop environment. The container does
this for you:

```sh
docker compose up -d --build
curl http://127.0.0.1:8080/healthz
```

See [07 — Deployment](docs/07-deploy.md) for the bare-VPS setup, systemd units, environment
variables, and how sessions and captchas work on a host with no screen.

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
| [07 — Deployment](docs/07-deploy.md) | Docker, headless VPS, configuration, and sessions in production |

## Summary

1. TikTok's page signs and owns the WebSocket.
2. The initialization bridge mirrors binary frames to Rust without altering the socket.
3. No Euler API, custom sign server, or native X-Gnarly implementation is required.
