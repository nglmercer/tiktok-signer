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

## Why there is no server and no napi

The only signed thing in the whole flow is the socket's query string, and the code that signs it is
TikTok's own `webmssdk` bundle — which is JavaScript. Node already has a JavaScript engine, so
there is no native engine or addon to compile: the TypeScript package builds to ESM, the bundle runs
in a bare `node:vm` context, and the sandbox it needs (`vendor/bootstrap.js`) is a generated script.

The Rust side of this repository does link QuickJS or V8, but only because Rust has no engine of
its own. Shipping that through `napi` would put a JavaScript engine inside a JavaScript runtime and
add a prebuilt binary per platform, in exchange for nothing.

Everything else — discovery, the WebSocket, protobuf, gzip — is Node built-ins and about 700 lines
here. **This package has no runtime dependencies.**

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
session file is empty, the client performs one ordinary `GET https://www.tiktok.com/live`, absorbs
TikTok's `Set-Cookie` headers in memory, and uses that anonymous `ttwid` identity for discovery,
signing, and the WebSocket. No browser, login, or persisted guest cookie is needed.

The empty jar is still refused by the socket (typically as close code 1006); that is different from
the guest jar. If TikTok returns no usable guest cookie, `GuestSessionError` reports
`BOOTSTRAP_NO_COOKIES` (or a classified HTTP/network bootstrap failure); a rejected guest socket is
reported as `WS_HANDSHAKE_REJECTED` rather than retried forever. A supplied `sessionCookie` or a
non-empty session file continues to be used unchanged for authenticated/custom transport.

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
  WebSocketImpl,        // default: global WebSocket (Node 22+); pass `ws` if yours lacks headers
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

## Authorized use only

This opens real connections to public rooms and is subject to TikTok's terms.
