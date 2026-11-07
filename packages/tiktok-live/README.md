# ttl-live

TikTok LIVE events in Node. No sign server, no native module, no browser, no Node addon to compile.

```js
import { TikTokLive, label } from 'ttl-live';

const live = new TikTokLive('@someone');

live.on('chat', (event) => console.log(`${label(event.user)}: ${event.comment}`));
live.on('gift', (event) => {
  if (event.streakable && !event.repeatEnd) return;      // only the last message of a streak counts
  console.log(`${label(event.user)} sent ${event.repeatCount}× ${event.giftName}`);
});

await live.connect();
```

## Why there is no server and no native addon

The only signed thing in the whole flow is the socket's query string, and the code that signs it is
TikTok's own `webmssdk` bundle — which is JavaScript. It runs in **QuickJS compiled to
WebAssembly** (`quickjs-emscripten-core` with the `@jitl/quickjs-wasmfile-release-sync` variant),
the same engine the Rust, Java and Android signers use, so every frontend produces the same bytes.
The sandbox it needs (`vendor/bootstrap.js`) is a generated script, shared with those engines.

WebAssembly rather than `node:vm` or a `napi` addon: the bundle sees none of Node's globals, each
call runs under a deadline (`timeoutMs`, default 10 s) and the context has a memory ceiling, and
there is still no prebuilt binary per platform to install. Loading the bundle takes about 150 ms and
a signature tens of milliseconds; a `Signer` is kept warm and reused, and `signer.close()` releases
its WebAssembly memory when it is no longer needed.

The WebSocket uses `ws` so authenticated handshakes preserve cookie, user-agent, and origin headers.
Discovery and gzip use Node built-ins. Generated schema bindings use the small
`@bufbuild/protobuf` runtime.

## What it does

1. resolves `@handle` → `room_id` (unsigned);
2. reads room metadata and the gift table once (unsigned);
3. builds the socket query exactly as the web player does, signs it, and opens
   `wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/` directly — there is no
   `/webcast/im/fetch/` on this path, and no `push_server` to obtain from anywhere;
4. sends the enter-room frame, keeps the application heartbeat, and acknowledges every batch;
5. decodes frames into typed events, and reconnects with backoff, re-signing each attempt.

## Anonymous guest mode

An account session is optional for public rooms. When `sessionCookie` is absent and the configured
session file is empty, the client performs an ordinary `GET https://www.tiktok.com/live`, absorbs
TikTok's `Set-Cookie` headers in memory, and uses that anonymous `ttwid` identity for discovery,
signing, and the WebSocket. A response with no usable cookie gets at most two additional attempts
with bounded backoff. No browser, login, or persisted guest cookie is needed.

The empty jar is still refused by the socket (typically as close code 1006); that is different from
the guest jar. If TikTok returns no usable guest cookie, `GuestSessionError` reports
`BOOTSTRAP_NO_COOKIES` (or a classified HTTP/network bootstrap failure). A guest handshake refusal
during reconnect triggers at most one fresh guest bootstrap and signer rebuild; if that also fails,
the client stops instead of cycling the same identity. A supplied `sessionCookie` or a non-empty
session file continues to be used unchanged, and a deterministic handshake refusal stops its
reconnect loop as well.

## Events

`chat`, `gift`, `like`, `member`, `social`, `roomUser`, and `unknown`. Every event is also emitted
on `event`.

Measured over 45 seconds of a 35,000-viewer room: 534 chats, 108 members, 48 likes, 22 room-user
updates, 21 socials, 9 gifts, and 26 unmodelled messages — all of them link-mic or gift-panel
internals. Those arrive as `unknown` with `payload` intact, so decoding one yourself needs nothing
from this package but the bytes.

Gifts are enriched from the room's gift table, because a repeat message omits its own detail block:
`giftName` and `diamondCount` are filled in, and `streakable` says whether summing every message
would double-count what the sender spent.

## API

```js
new TikTokLive(uniqueId, {
  sessionCookie,        // optional; otherwise stored jar, then anonymous guest bootstrap
  roomId,               // skip the lookup if you already know it
  fetchGifts: true,     // ~2.6 MB, once per connection
  reconnect: { attempts: 5, initialMs: 2000, maxMs: 60000 },
  bundlePath,           // default: hash-verified, downloaded once, and cached in the temp directory
  WebSocketImpl,        // default: ws; custom transports must forward cookie/user-agent/origin headers
})
```

The pieces are exported too, for anything the client does not shape the way you want: `Discovery`
(the unsigned endpoints), `Signer` (a warm signing context), `decodeEvent`, `decodePushFrame`,
`decodeBatch`, `socketQuery`.

## Tests

```sh
npm install
npm run check              # strict type-check, build, and offline tests
npm run listen -- @someone # live, against a room that is broadcasting now
```

`test/signer.test.ts` also checks that `vendor/bootstrap.js` still matches the shim it is
generated from — run `npm run sync-bootstrap` after editing `scripts/headless/shim.mjs`.

## Generated protobuf schemas

The complete TypeScript schema bindings under `src/gen/` are generated from the same vendored
`.proto` tree used by Rust:

```sh
npm install
npm run proto:generate
npm run proto:check       # regenerate, then fail if src/gen is stale
```

The generator reads only `crates/ttl-live-proto/proto/v3/`; it does not download schemas during a
build. Generated files are not hand-edited. The event and frame decoders consume these generated
schemas directly. They intentionally remain separate from the package's normalized runtime event
types, which form the stable listener-facing API.

## Authorized use only

This opens real connections to public rooms and is subject to TikTok's terms.

### Resource limits and optional asynchronous delivery

Existing synchronous event delivery remains the default. Set
`eventDelivery: { capacity: 1024, maxBytes: 64 * 1024 * 1024 }` to use one bounded,
ordered asynchronous FIFO. Overflow disconnects and reports an error; disconnect
clears queued events. ACKs precede delivery. `deliveryMetrics()` reports retained
queue charge, delivery/discard/rejection counts and listener failures. Byte charges
are conservative estimates, not heap measurements. Listener exceptions cannot
prevent transport cleanup or typed delivery to other listeners.

Frames/batches are limited to 16 MiB and 8192 messages. Bundle downloads are limited
to 8 MiB, retain hash verification and share at most 16 concurrent distinct loads.
Room/gift HTTP response bodies are limited to 16 MiB. Pending discovery requests
are aborted on disconnect. Run `node scripts/decode-bench.mjs` after building for an
offline decode benchmark, or `node scripts/lifecycle-soak.mjs 1800` for independent
1000/5000/20000-event/sec targets and repeated connection cleanup. These synthetic
tools do not contact TikTok. See [configuration](../../docs/18-live-resource-configuration.md)
and [measurements](../../docs/17-performance-audit.md).
