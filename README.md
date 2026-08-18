# tiktok-signer

Self-hosted TikTok LIVE transport written in Rust. **No browser anywhere:** signing runs the
real `webmssdk` bundle under a synthetic environment, and every step — listing live channels,
resolving a room, reading metadata and gifts, and bootstrapping the transport — works without
Wry, WebKit, or a display.

**Primary objective:** replace the proprietary sign server with a reproducible headless signing
path. That is now the only path; the WebView oracle has been removed.

## Status

Verified against live rooms on 2026-08-18 with no browser: discovery, `room/info` and `gift/list`
return live data, and `/webcast/room/enter/` **accepts this project's signature** — it refuses
unsigned and `X-Bogus`-only requests with 403 and accepts the full computed suffix with 200, which is
positive proof the signing is correct.

**The transport works, with no browser and no `im/fetch`.** The live room page configures its IM
SDK with `wsDirect: "1"` and a `socketHost`, and the SDK then builds and signs the socket URI itself:

```text
wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?<query>&X-Gnarly=<sig>
```

where the signature is `registerWsSigner()` over the query bytes. There is no `push_server` to
obtain, so the URI is constructible from first principles — `cargo run -p ttl-live-discovery
--example live-check` ends in decoded chat, and `tiktok-live-connector` runs against the local sign
server. `im/fetch` still runs in the player under `fetchBeforeWsSuccess`, as a best-effort first page
of messages, which is why it can answer 200 with zero bytes to a request with nothing wrong with it.

Three long-standing claims were withdrawn on the way there, all in
[docs/12](docs/12-transport-reverse-engineering.md): `room/info` and `gift/list` were believed to
prove the signer works (they verify nothing); the transport request was believed to need a signature
(the page's allowlist excludes that path); and the signature was believed to be wrong in its value
(`room/enter` verifies one and accepts ours).

Downstream is built and tested: `ttl-live-ws` connects, heartbeats, acknowledges, and reconnects,
`sanitize_uri` escapes the raw spaces the player emits in `browser_version` (which no Rust HTTP
client will parse) without disturbing the signature, and `ttl-live-events` decodes the frames.

| Crate | Status |
|---|---|
| `ttl-sign-core` | Presets, queries, cookie jar, `SignOutcome`, a stable event subset, and generated schema bindings with bounded dynamic decoding. |
| `ttl-sign-replay` | Versioned, sanitized offline signing fixtures and `ReplayBackend`. |
| `ttl-sign-native` | Deterministic staged native pipeline with an isolated signing-algorithm boundary. |
| `ttl-sign-lab` | Safe structured observations and classified backend differential reports. |
| `ttl-live-discovery` | Browser-free discovery, entirely unsigned: room lookup, `room/info`, `gift/list`, and live channels. |
| `ttl-sign-headless` | Browser-free `SignerBackend`: builds and signs the socket URL, and describes it in TikTok's own `ProtoMessageFetchResult` shape. |
| `ttl-sign-embedded` | The same signing, in-process: the real bundle in a warm QuickJS context, no `node` subprocess. |
| `ttl-live-ws` | WebSocket client with heartbeat, acknowledgements, typed rejection handling, and a reconnecting stream that re-signs each attempt. |
| `ttl-sign-server` | `GET /webcast/fetch`, `GET /webcast/rooms/{room_id}/connect` (Node client), and `GET /healthz`. |

Default tests are offline and deterministic. Run the offline server with:

```sh
cargo run -p ttl-sign-server --bin ttl-sign-replay-server --features replay
```

The live server is explicit, and needs the signing bundle plus an account session — the message
socket refuses a jar-less handshake:

```sh
curl -s -o /tmp/webmssdk.js \
  https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js

cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless
```

`TTL_SIGNER=embedded` signs in-process instead, running the same sandbox in a QuickJS context held
warm — no `node` per signature (95–105 ms → 70–89 ms, and no Node needed on the host at all). It is
opt-in until it has more mileage; the parity test that justifies it is
`cargo test -p ttl-sign-embedded`, and the measurements are in
[docs/13](docs/13-embedded-runtime.md).

End-to-end check, no browser:

```sh
cargo run -p ttl-live-discovery --example live-check
```

The workspace MSRV is Rust 1.86; CI runs the headless suite on that toolchain.

### Controlled research

Signing research runs against the real bundle without a browser. Fetch the bundle first (it is a
public static asset and is deliberately not vendored):

```sh
curl -s -o /tmp/webmssdk.js \
  https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
sha256sum /tmp/webmssdk.js   # compare with fixtures/research/webmssdk-profile-2026-08-13.json
```

```sh
# does the player still build the transport the way we do? 55 facts, exit 1 on drift
node scripts/headless/player-audit.mjs

# what each signing route produces, and which product each endpoint accepts
node scripts/headless/sign-probe.mjs /tmp/webmssdk.js
node scripts/headless/verify-probe.mjs /tmp/webmssdk.js <room_id> all

# which inputs reach a signature at all, and where in it they land
node scripts/headless/value-differential.mjs /tmp/webmssdk.js
node scripts/headless/byte-map.mjs /tmp/webmssdk.js

# the browser surface the bundle touches, as a shim specification
node scripts/headless/emit-surface.mjs /tmp/webmssdk.js \
  fixtures/research/environment-surface-v1.json

# the transport itself, and channels that are live now
node scripts/headless/ws-direct.mjs /tmp/webmssdk.js <room_id> 20
node scripts/headless/find-live.mjs /tmp/webmssdk.js
```

`player-audit.mjs` is the one to run first when something breaks. It reads the socket hosts, paths,
both `version_code` values, the query serializer's behaviour, and three protobuf field maps back out
of the shipped app, and diffs them against `fixtures/research/player-transport-v1.json`; a
`ttl-sign-core` test then asserts that fixture against the builder. So the chain is covered end to
end — player's chunk → fixture → test → `DirectSocketParams` → the wire — and a TikTok deploy fails a
test instead of producing a socket that opens and never speaks.

Offline analysis of a captured VM trace needs no network at all:

```sh
cargo run -p ttl-sign-lab --bin ttl-sign-plan -- fixtures/research/plan.example.json
cargo run -p ttl-sign-lab --bin ttl-sign-subgraph -- /tmp/vm-trace.json \
  --controlled fixtures/research/controlled-observations-2026-08-13.json
cargo run -p ttl-sign-lab --bin ttl-sign-subgraph-diff -- <baseline.json> <candidate.json>
```

Artifacts contain parameter names plus value digests, never a reusable signed URL, raw
cookie/signature value, or raw declared signing input; `ttl-fixture-hygiene` enforces that. See
[`docs/09-signing-research.md`](docs/09-signing-research.md).

The example room id is synthetic. Replace it only with a room you are authorized to test.

> **Removed with the WebView.** The oracle binaries (`ttl-sign-oracle`, `ttl-sign-url-oracle`,
> `ttl-sign-trace`, `ttl-sign-paired-trace`, `ttl-sign-vm-trace`, `ttl-sign-env-surface`) and the
> browser probes drove a page and cannot run headless. `scripts/headless/` covers signing, the
> environment surface, transport, and discovery; the paired URL/trace differentials have no
> replacement, and `ttl-sign-lab`'s offline comparison tools remain.

> `ProtoMessageFetchResult` field numbers were confirmed against a real response
> (`2 = cursor`, `5 = internal_ext`, `7 = route_params`, `10 = push_server`) and are what the sign
> server now *writes* rather than reads. The socket's own three messages — `PushFrame`, `EnterRoom`,
> `HeartBeat` — are checked against the player's descriptors by `player-audit.mjs`.

### Verified flow (2026-08-18)

Every row measured with no browser, no display, and no captured artefact.

| Step | Status |
|---|---|
| Discover live channels | `/api/search/live/full/` as JSON. Unsigned. |
| `unique_id` → `room_id` | Unsigned. |
| Room info and gift table | Unsigned — neither endpoint verifies a signature (`verify-probe.mjs`). |
| Build the socket URL | From first principles: the player's own query, signed with `registerWsSigner`. |
| Open the socket | 101, then `im_enter_room`; a jar-less handshake is refused with 1006. |
| Receive and decode frames | 203 frames and 555 events in a 150-second soak. |
| Reconnect after a close | Re-signs and reopens; a refusal is reported rather than retried. |

`/webcast/im/fetch/` is on no critical path. Clients that expect its protobuf are served a
`ProtoMessageFetchResult` assembled locally from the signed socket URL — see the connector section
below.

### Room state

The socket reports *changes*; it never reports the room as it already is. Two endpoints on
`webcast.tiktok.com` fill that gap, and both are **unsigned** — a one-character tamper and a
signature-free request return byte-identical data, which `scripts/headless/verify-probe.mjs`
measures:

```rust
let info = client.room_info(&room_id).await?;   // title, owner, viewers, likes, cover
let gifts = client.gift_list(&room_id).await?;  // gift id → name and diamond cost
```

`gift_list` returns a few megabytes (673 gifts in the verified room), so request it once per session
and keep the table: gift events carry only a `gift_id`, and pricing them requires it.

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

A refusal that means "prove you are human" has **no handler here any more**. The old one showed the
page's captcha to a person and waited — it needed a browser window, which is exactly what was
removed. Nothing in this workspace can solve one, and nothing pretends to: log the code and stop.

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

A socket signature ages out, and rooms restart their push servers. `ReconnectingConnection` owns
what happens on a close: it **re-signs** and reopens — reusing a stale URI is the failure it exists
to avoid — five attempts by default with a doubling backoff capped at a minute.

A refusal is reported rather than retried. A rejection is a verdict about the request, so a retry
loop around one looks like a network problem while being a signing one.

```rust
let mut stream = ReconnectingConnection::open(
    backend, room_id, ConnectConfig::default(), ReconnectPolicy::default(),
).await?;
while let Some(message) = stream.next_message().await { … }
```

`live-check` runs on it, and `TTL_LISTEN_SECONDS` makes the window long enough to watch it work.

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
cargo run -p ttl-live-discovery --example live-check            # full flow, no browser
cargo run -p ttl-live-discovery --example discover -- <user>    # room info + gift list
node scripts/headless/find-live.mjs /tmp/webmssdk.js            # who is live now
node scripts/headless/ws-direct.mjs /tmp/webmssdk.js <room_id>  # the transport, on its own
node scripts/headless/room-page-scan.mjs @<user>                # what the room page seeds
node scripts/headless/player-audit.mjs                          # has the player changed?
node scripts/headless/transport.mjs /tmp/webmssdk.js <user>     # the old im/fetch bootstrap
```

The browser probes (`endpoint-probe`, `ws-probe`, `page-probe`, `limit-probe`) were removed with
the WebView: they drove a page and have no headless equivalent. What a refusal looks like is now
observable directly — `room/ping/audience` reports `status_code=20003, "User doesn't login"`, and a
jar-less socket handshake is refused with an immediate 1006.

### tiktok-live-connector (Node) compatibility

`examples/node-connector` points `tiktok-live-connector` at this project instead of Euler
Stream:

```sh
cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless   # terminal 1
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

**Verified 2026-08-18 against a live room: it works, browser-free.** 324 events — chat, likes,
members, follows, gifts, viewer counts — reached `tiktok-live-connector` with no Euler Stream and no
rendering engine involved.

The earlier run of this example, on 2026-08-10, worked only because the WebView had captured a URI
the player signed for itself. That engine was removed, and so was the `--ws-uri` flag that let one be
pasted in by hand: a fallback that needs a browser once is still a browser dependency, and keeping it
meant the real problem never had to be solved.

What the server hands back now is assembled locally. `push_server` carries the socket host and path,
`route_params` every parameter of the signed query, and the client rebuilds the URL from that map —
reordered, re-encoded, with the duplicated `version_code` collapsed. The socket accepts that, which
is what makes the shape safe to hand to a client that was written for Euler Stream.

### Discovery is guest-only-friendly; the socket is not

Discovery needs no account: live search, `unique_id` → `room_id`, `room/info` and `gift/list` all
answer with no cookies at all, and none of them verifies a signature.

**The message socket does need a session.** A jar-less handshake to
`/webcast/im/ws_proxy/ws_reuse_supplement/` is refused before the socket opens — an immediate 1006,
measured 2026-08-18. The older claim here, that listening works as a guest, was about the page
WebSocket the removed WebView captured, and does not hold on this path.

`sessionid` *is* the account, so everything this sends is attributed to it, and an account is not a
fix for rate limiting — pace the connections instead.

### Providing a session

There is no interactive login any more: that example opened a real browser window, which is
exactly what was removed. Export the cookies from a browser where you are already logged in and
write them as a cookie header:

```sh
install -m 600 /dev/null "$XDG_CONFIG_HOME/ttl-signer/session"
printf 'sessionid=...; sessionid_ss=...; sid_tt=...; ttwid=...' \
  > "$XDG_CONFIG_HOME/ttl-signer/session"
```

`TTL_SESSION_FILE` changes the path. `sessionid` is the one that matters: without it the message
socket refuses the handshake outright, and the headless server refuses to start.

## Usage

```sh
cargo test                                        # headless default members

# Discover channels and resolve room IDs
cargo run -p ttl-live-ws --example rooms -- user1 user2

# Full flow against a live channel
cargo run -p ttl-live-discovery --example live-check
cargo run -p ttl-live-discovery --example live-check -- user

# Verify schema-registry decoding against a live channel
cargo run -p ttl-live-discovery --example live-check -- user

# Replay a captured request
cargo run -p ttl-live-ws --example replay -- fixtures/f0/im_fetch.curl

# Start the sign server
TTL_BIND=127.0.0.1:8080 cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless
```

## Deployment

No browser, so no WebKit, no GTK, and no `Xvfb`. A deployment is the Rust binary, Node (for the
signer script), the bundle, and a session:

```sh
docker compose up -d --build
curl http://127.0.0.1:8080/healthz
```

See [07 — Deployment](docs/07-deploy.md) for the bare-VPS setup, systemd units, environment
variables, and sessions in production.

## Documentation

| Document | Contents |
|---|---|
| [00 — Research](docs/00-research.md) | Connection flow and signing boundaries |
| [01 — Architecture](docs/01-architecture.md) | Crates, threading model, and design decisions |
| [02 — Roadmap](docs/02-roadmap.md) | Phases, deliverables, and acceptance criteria |
| [03 — Sign-server specification](docs/03-spec-sign-server.md) | HTTP endpoints and client compatibility |
| [04 — WebView bridge specification](docs/04-spec-webview-bridge.md) | *Historical.* The IPC contract of the removed browser |
| [05 — WebSocket client specification](docs/05-spec-websocket-client.md) | URI construction, headers, heartbeat, acknowledgements, and reconnection |
| [06 — Risks and operations](docs/06-risks-and-ops.md) | Failure modes, rate limits, and maintenance |
| [07 — Deployment](docs/07-deploy.md) | Docker, headless VPS, configuration, and sessions in production |
| [08 — Headless migration](docs/08-headless-migration.md) | How the browser was taken out, and what moved where |
| [09 — Signing research](docs/09-signing-research.md) | Method, fixture hygiene, and what each probe measures |
| [10 — Authorized API feasibility](docs/10-authorized-api-feasibility.md) | Whether the official APIs can do this instead |
| [11 — WebView removal](docs/11-webview-removal.md) | What the browser did, what replaced it, and what was lost |
| [12 — Transport reverse engineering](docs/12-transport-reverse-engineering.md) | **How the socket is built and signed**, and the search that preceded it |
| [13 — Embedded runtime](docs/13-embedded-runtime.md) | Which JS engine can run the signer in-process, measured |

## Summary

1. The socket URL is built from first principles and signed with `registerWsSigner`, the way the
   live player builds its own. No page, no capture, no `im/fetch`.
2. Discovery is unsigned; the socket needs a session and a signature.
3. No Euler API and no native X-Gnarly implementation is required — the real bundle runs headless.
4. `scripts/headless/player-audit.mjs` re-reads the player's transport facts and fails when they
   move, so a TikTok deploy breaks a test rather than a connection.
