// Connect the live message socket the way the current web player does — no `im/fetch` at all.
//
//   node scripts/headless/ws-direct.mjs /tmp/webmssdk.js <room_id> [seconds]
//
// ## Why this exists
//
// Every request shape we could reach was answered by `/webcast/im/fetch/` with 200 and zero bytes,
// and no variation moved it. The reason is in the player's own code. The live room page configures
// its IM SDK with
//
//     socketHost: "wss://webcast-ws.tiktok.com", wsDirect: "1", fetchBeforeWsSuccess: "1"
//
// and the SDK's `start()` branches on exactly that: when `wsDirect` is "1" and a `socketHost` is
// set, it calls `createClient()` straight away and builds the socket URL itself —
// `${socketHost}/webcast/im/ws_proxy/ws_reuse_supplement/?${query}` — signing the query with
// `byted_acrawler.registerWsSigner()` and appending the result as `X-Gnarly`. The `im/fetch` call
// still happens under `fetchBeforeWsSuccess`, but only as a best-effort first page of messages; the
// push server it used to return is no longer on the path. So there is no `push_server` to obtain,
// and nothing here has to make `im/fetch` answer.
//
// This script is the shortest statement of that: build the URL the SDK builds, sign it with the
// product the SDK signs it with, connect, and report what comes back. The URL, the constants and the
// frames all come from `lib/player.mjs`, which is the one place they are written down.
//
// AUTHORIZED USE ONLY: this opens a real connection to a real room. Frame counts and byte sizes are
// printed; no cookie, token, or signed URL is. This is the empty/stored-jar control; use
// `ws-guest-probe.mjs` to bootstrap TikTok's anonymous identity first.

import fs from 'node:fs';
import { createSandbox } from './shim.mjs';
import { USER_AGENT, cookieHeader, sessionJar } from './lib/session.mjs';
import {
  PATH, SOCKET_HOST, enterRoomFrame, heartbeatFrame, socketConfig, socketQuery,
} from './lib/player.mjs';

const bundlePath = process.argv[2];
const roomId = process.argv[3];
const seconds = Number(process.argv[4] || 20);
if (!bundlePath || !roomId) {
  console.error('usage: node scripts/headless/ws-direct.mjs <webmssdk.js> <room_id> [seconds]');
  process.exit(2);
}

/// The device id the query claims is the page's own webid when the session carries one.
const jar = sessionJar();
const FALLBACK_DEVICE_ID = '7300000000000000001';
const config = socketConfig({
  roomId,
  deviceId: jar.get('tt_webid_v2') || jar.get('tt_webid') || FALLBACK_DEVICE_ID,
});
const query = socketQuery(config);

// The Rust builder in `ttl-sign-core` has to produce these bytes exactly, since the signature
// covers them. `TTL_PRINT_QUERY=1` prints them and stops, which is what its parity test compares.
if (process.env.TTL_PRINT_QUERY) {
  console.log(query);
  process.exit(0);
}

// --- the signature: `registerWsSigner`, over the query bytes --------------------------------------

const env = createSandbox();
const w = env.windowTarget;
const quiet = () => {};
w.console = Object.fromEntries(['log', 'info', 'warn', 'error', 'debug'].map((k) => [k, quiet]));
Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => USER_AGENT });
const cookie = cookieHeader(jar);
Object.defineProperty(w.document, 'cookie', { configurable: true, get: () => cookie, set() {} });

const failure = env.load(fs.readFileSync(bundlePath, 'utf8'));
if (failure) throw new Error(`the bundle failed to load: ${failure.message}`);
const sdk = w.byted_acrawler;
await Promise.resolve(sdk.init({ aid: 1988, enablePathList: ['/webcast/'] }));

if (typeof sdk.registerWsSigner !== 'function') {
  console.error('this bundle exposes no registerWsSigner — the SDK cannot sign a socket URL');
  process.exit(1);
}
const wsSigner = sdk.registerWsSigner();
if (typeof wsSigner !== 'function') {
  console.error('registerWsSigner() returned no signer');
  process.exit(1);
}
const signed = wsSigner({ 'X-MS-Q': query, 'X-MS-STUB': '' });
const gnarly = signed?.['X-Gnarly'] ?? '';
console.log(`query      ${query.length} bytes, ${query.split('&').length} parameters`);
console.log(`X-Gnarly   ${gnarly ? `${gnarly.length} bytes` : 'ABSENT — the signer returned none'}`);

const url = `${SOCKET_HOST.global}${PATH.wsReuseSupplement}?${query}`
  + (gnarly ? `&X-Gnarly=${encodeURIComponent(gnarly)}` : '');

// --- connect --------------------------------------------------------------------------------------

console.log(`\nconnecting to ${SOCKET_HOST.global}${PATH.wsReuseSupplement}`);
let socket;
try {
  socket = new WebSocket(url, {
    headers: { cookie, 'user-agent': USER_AGENT, origin: 'https://www.tiktok.com' },
  });
} catch (error) {
  console.error(`could not construct the socket: ${error.message}`);
  process.exit(1);
}
socket.binaryType = 'arraybuffer';

let heartbeat;
let opened = false;
let frames = 0;
let bytes = 0;
const started = Date.now();
const SAMPLED_FRAMES = 5;

socket.addEventListener('open', () => {
  opened = true;
  console.log(`open after ${Date.now() - started} ms`);
  // The server sends nothing until the client says which room it is in — `executeOpen` in the SDK
  // does exactly this, and then starts the heartbeat.
  socket.send(enterRoomFrame({ roomId: config.roomId, identity: config.identity }));
  console.log('sent im_enter_room');
  heartbeat = setInterval(() => {
    if (socket.readyState === WebSocket.OPEN) socket.send(heartbeatFrame(config.roomId));
  }, Number(config.heartbeatDuration));
});
socket.addEventListener('message', (event) => {
  frames += 1;
  const size = event.data?.byteLength ?? String(event.data).length;
  bytes += size;
  if (frames <= SAMPLED_FRAMES) console.log(`  frame ${frames}: ${size} bytes`);
});
socket.addEventListener('error', (event) => {
  console.log(`error: ${event?.message || event?.error?.message || 'socket error'}`);
});
socket.addEventListener('close', (event) => {
  console.log(`closed: code=${event.code} reason=${JSON.stringify(event.reason || '')}`);
  report();
  process.exit(frames ? 0 : 1);
});

setTimeout(() => {
  try { socket.close(); } catch { /* already closed */ }
  report();
  process.exit(frames ? 0 : 1);
}, seconds * 1000).unref?.();

let reported = false;
function report() {
  if (reported) return;
  reported = true;
  clearInterval(heartbeat);
  console.log(`\n${frames} frame(s), ${bytes} bytes in ${Math.round((Date.now() - started) / 1000)}s`);
  if (frames) {
    console.log('The socket carries traffic. This is the transport, and it needs no im/fetch.');
  } else if (opened) {
    console.log('No frames. The socket opened and accepted the room frame but pushed nothing.');
  } else {
    console.log('The handshake was refused before the socket opened.');
    if (!jar.size) {
      console.log('No cookies were supplied: this is the empty-jar control (measured: immediate 1006).');
      console.log('Run ws-guest-probe.mjs to test a fresh anonymous TikTok identity.');
    }
  }
}
