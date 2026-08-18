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
// product the SDK signs it with, connect, and report what comes back.
//
// The URL construction below mirrors the SDK's `k()`, `F()`, `V()` and `H()` — including two
// quirks that matter to the signature, because it signs the query string verbatim: `H()` does not
// percent-encode, and the query carries `version_code` twice (`k()`'s default 180800, then the
// configured 270000, because one key arrives snake_case and the other camelCase).
//
// AUTHORIZED USE ONLY: this opens a real connection to a real room. Frame counts, methods and byte
// sizes are printed; no cookie, token, or signed URL is.

import fs from 'node:fs';
import { createSandbox } from './shim.mjs';
import { sessionCookies } from './lib/sign.mjs';

const bundlePath = process.argv[2];
const roomId = process.argv[3];
const seconds = Number(process.argv[4] || 20);
if (!bundlePath || !roomId) {
  console.error('usage: node scripts/headless/ws-direct.mjs <webmssdk.js> <room_id> [seconds]');
  process.exit(2);
}

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

// --- the SDK's own query builders, transcribed ---------------------------------------------------

/// `k()`: the browser block. Its `version_code` is the SDK default, which the config then shadows.
const browserBlock = () => ({
  version_code: '180800',
  device_platform: 'web',
  cookie_enabled: 'true',
  screen_width: '1920',
  screen_height: '1080',
  browser_language: 'en-US',
  browser_platform: 'Linux x86_64',
  browser_name: 'Mozilla',
  browser_version: UA.slice('Mozilla/'.length),
  browser_online: 'true',
  tz_name: 'America/New_York',
});

/// `F()`: drop empties, objects, and the config keys that are not request parameters.
function strip(props) {
  const out = { ...props };
  for (const key of Object.keys(out)) {
    if (out[key] === undefined || out[key] === '' || typeof out[key] === 'object') delete out[key];
  }
  for (const key of ['socketHost', 'host', 'fetchBeforeWsSuccess', 'debug', 'filterByRoomId']) {
    delete out[key];
  }
  return out;
}

/// `V()`: the browser block, then the config, then the fixed tail.
function withDefaults(props) {
  const { didRule, deviceId, ...rest } = props;
  const merged = {
    ...browserBlock(),
    ...strip(rest),
    supWsDsOpt: '1',
    respContentType: 'protobuf',
    didRule: didRule ?? (deviceId ? 0 : 3),
    deviceId,
    webcastLanguage: rest.appLanguage,
  };
  for (const key of Object.keys(merged)) {
    if (merged[key] === undefined || merged[key] === '') delete merged[key];
  }
  return merged;
}

/// `H()`: camelCase to snake_case, and *no* percent-encoding. The signature covers these bytes.
function serialize(params) {
  return Object.keys(params).reduce((acc, key) => {
    const name = key
      .replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`)
      .replace(/\s+/g, '_')
      .replace(/[^a-zA-Z0-9_]/g, '')
      .toLowerCase();
    return `${acc}${acc ? '&' : ''}${name}=${String(params[key])}`;
  }, '');
}

const WS_REUSE = '/webcast/im/ws_proxy/ws_reuse_supplement/';

// --- the two frames the socket needs, encoded by hand ---------------------------------------------
//
// The SDK sends a `PushFrame` whose payload is an `EnterRoom` message, then a `hb` frame every ten
// seconds. Both protos are small enough to write out rather than take a protobuf dependency, and
// they come from the SDK's own descriptors:
//
//   PushFrame  { seq_id 1, log_id 2, service 3, method 4, headers 5,
//                payload_encoding 6, payload_type 7, payload 8 }
//   EnterRoom  { room_id 1, room_tag 2, live_region 3, live_id 4, identity 5, cursor 6,
//                account_type 7, enter_uniq_id 8, filter_welcome_msg 9,
//                is_anchor_continue_keep_msg 10 }
//   HeartBeat  { room_id 1, send_packet_seq_id 2 }

function varint(value) {
  const out = [];
  let n = BigInt(value);
  do {
    let byte = Number(n & 0x7fn);
    n >>= 7n;
    if (n) byte |= 0x80;
    out.push(byte);
  } while (n);
  return out;
}

const tag = (field, wire) => varint((field << 3) | wire);
const int64Field = (field, value) => [...tag(field, 0), ...varint(value)];
const bytesField = (field, value) => {
  const body = typeof value === 'string' ? Buffer.from(value, 'utf8') : Buffer.from(value);
  return [...tag(field, 2), ...varint(body.length), ...body];
};

function pushFrame(payloadType, payload) {
  return Buffer.from([
    ...bytesField(6, 'pb'),
    ...bytesField(7, payloadType),
    ...bytesField(8, payload),
  ]);
}

const enterRoom = ({ roomId, identity, liveId }) => Buffer.from([
  ...int64Field(1, roomId),
  ...int64Field(4, liveId),
  ...bytesField(5, identity),
  ...bytesField(6, ''),
  ...int64Field(7, 0),
  ...bytesField(9, '0'),
]);

const heartBeat = (roomId) => Buffer.from(int64Field(1, roomId));

// --- the config the live room page uses ----------------------------------------------------------

const jar = sessionCookies();
const deviceId = jar.get('tt_webid_v2') || jar.get('tt_webid') || '7300000000000000001';

const config = {
  aid: '1988',
  appName: 'tiktok_web',
  liveId: '12',
  versionCode: '270000',
  appLanguage: 'en',
  socketHost: 'wss://webcast-ws.tiktok.com',
  wsDirect: '1',
  fetchBeforeWsSuccess: '1',
  clientEnter: '1',
  roomId,
  identity: 'audience',
  deviceId,
  // `createClient` seeds these from the message state, which starts empty.
  lastRtt: '-1',
  cursor: '',
  internalExt: '',
  historyCommentCursor: '',
  heartbeatDuration: '10000',
};

const { appName, didRule, routeParamsMap, pushServer, ...rest } = config;
const query = serialize(withDefaults({
  appName,
  didRule,
  supWsDsOpt: '1',
  updateVersionCode: '2.0.0',
  compress: 'gzip',
  webcastLanguage: config.appLanguage,
  ...browserBlock(),
  ...(routeParamsMap || {}),
  ...strip(rest),
}));

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
Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => UA });
const cookieHeader = [...jar].map(([k, v]) => `${k}=${v}`).join('; ');
Object.defineProperty(w.document, 'cookie', {
  configurable: true, get: () => cookieHeader, set() {},
});

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

const url = `${config.socketHost}${WS_REUSE}?${query}`
  + (gnarly ? `&X-Gnarly=${encodeURIComponent(gnarly)}` : '');

// --- connect --------------------------------------------------------------------------------------

console.log(`\nconnecting to ${config.socketHost}${WS_REUSE}`);
let socket;
try {
  socket = new WebSocket(url, {
    headers: { cookie: cookieHeader, 'user-agent': UA, origin: 'https://www.tiktok.com' },
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

socket.addEventListener('open', () => {
  opened = true;
  console.log(`open after ${Date.now() - started} ms`);
  // The server sends nothing until the client says which room it is in — `executeOpen` in the SDK
  // does exactly this, and then starts the heartbeat.
  socket.send(pushFrame('im_enter_room', enterRoom({
    roomId: config.roomId, identity: config.identity, liveId: config.liveId,
  })));
  console.log('sent im_enter_room');
  heartbeat = setInterval(() => {
    if (socket.readyState === 1) socket.send(pushFrame('hb', heartBeat(config.roomId)));
  }, Number(config.heartbeatDuration));
});
socket.addEventListener('message', (event) => {
  frames += 1;
  const size = event.data?.byteLength ?? String(event.data).length;
  bytes += size;
  if (frames <= 5) console.log(`  frame ${frames}: ${size} bytes`);
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
      console.log('No session was loaded, and this endpoint refuses a jar-less handshake outright');
      console.log('(measured: immediate 1006). Store cookies and re-run.');
    }
  }
}
