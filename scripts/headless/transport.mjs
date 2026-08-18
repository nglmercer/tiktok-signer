// Bootstrap the LIVE transport headlessly: sign `/webcast/im/fetch/` and read the push_server
// and route params out of the protobuf response. No browser.
//
// AUTHORIZED USE ONLY: this sends real signed requests.
//
//   node scripts/headless/transport.mjs /tmp/webmssdk.js <unique_id>
//
// Verified working: this returned a 74,670-byte protobuf carrying
// `wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/` on 2026-08-18, with no
// browser involved. The WebView oracle returned 74,273 bytes for the same room minutes earlier.
//
// Two requirements, both discovered by measurement:
//
//  1. **An authenticated account session.** A guest identity — however correctly signed — gets an
//     empty 200, and `/webcast/room/ping/audience/` says `status_code=20003, "User doesn't login"`
//     outright. The session is read from the same file the WebView path uses: `TTL_SESSION_FILE`,
//     else `$XDG_CONFIG_HOME/ttl-signer/session`, in the same cookie-header format. Cookie values
//     are never printed.
//  2. **The public signing product.** This endpoint wants the `frontierSign` X-Bogus, not the
//     patched-fetch suffix, which it rejects with 403. The two products are per-route.
//
// `room/enter` answers 403 under both products and with or without a session, and is not required.
//
// ## Empty responses are upstream, not a defect here
//
// The endpoint frequently answers 200 with an empty body. When the WebView oracle still existed,
// it returned `Rejected(EmptyBody)` for the same rooms at the same moments — verified paired
// across two rooms and both `sup_ws_ds_opt` values. The two paths succeeded together and failed
// together, so an empty response is a server-side condition, not a gap here. That comparison can
// no longer be re-run; see docs/11-webview-removal.md.
//
//     node scripts/headless/transport.mjs /tmp/webmssdk.js <user>
//
// Which `sup_ws_ds_opt` value works varies, so both are tried.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createSandbox } from './shim.mjs';

const bundlePath = process.argv[2];
const user = process.argv[3];
if (!bundlePath || !user) {
  console.error('usage: node scripts/headless/transport.mjs <webmssdk.js> <unique_id>');
  process.exit(2);
}

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

function sessionPath() {
  if (process.env.TTL_SESSION_FILE) return process.env.TTL_SESSION_FILE;
  const base = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, 'ttl-signer', 'session');
}

const jar = new Map();
const cookieHeader = () => [...jar].map(([k, v]) => `${k}=${v}`).join('; ');
const absorb = (response) => {
  const lines = response.headers.getSetCookie ? response.headers.getSetCookie() : [];
  for (const line of lines) {
    const [pair] = line.split(';');
    const eq = pair.indexOf('=');
    if (eq > 0) jar.set(pair.slice(0, eq).trim(), pair.slice(eq + 1));
  }
};

// Load the stored session, if there is one. Names are reported; values never are.
let authenticated = false;
try {
  const raw = fs.readFileSync(sessionPath(), 'utf8').trim();
  for (const part of raw.split(';')) {
    const eq = part.indexOf('=');
    if (eq <= 0) continue;
    const name = part.slice(0, eq).trim();
    const value = part.slice(eq + 1).trim();
    if (name && value) jar.set(name, value);
  }
  authenticated = Boolean(jar.get('sessionid'));
  console.log(`session: ${jar.size} cookies, authenticated=${authenticated}`);
} catch {
  console.log('session: none found — running as guest');
}
if (!authenticated) {
  console.log('  note: /webcast/im/fetch/ requires a logged-in session; expect an empty 200.');
}

const lookup = await fetch(
  `https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId=${user}`,
  { headers: { 'user-agent': UA, cookie: cookieHeader() } },
);
absorb(lookup);
const info = (await lookup.json())?.data?.user;
if (!info?.roomId || info.roomId === '0' || info.status !== 2) {
  console.error(`@${user} is not live`);
  process.exit(1);
}
const roomId = info.roomId;
const ROOM_URL = `https://www.tiktok.com/@${user}/live`;
const page = await fetch(ROOM_URL, {
  headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9', cookie: cookieHeader() },
});
absorb(page);
await page.text();
console.log(`room_id=${roomId}`);

const env = createSandbox();
const w = env.windowTarget;
const store = new Map();
w.localStorage = {
  getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k), clear() {}, key: () => null, length: 0,
};
Object.defineProperty(w.document, 'cookie', {
  configurable: true, get: () => cookieHeader(),
  set: (v) => { const [p] = String(v).split(';'); const eq = p.indexOf('=');
    if (eq > 0) jar.set(p.slice(0, eq).trim(), p.slice(eq + 1)); },
});
Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => UA });
Object.defineProperty(w.location, 'href', { configurable: true, get: () => ROOM_URL });
w.fetch = async () => ({ ok: true, status: 200, text: async () => '', json: async () => ({}) });

if (env.load(fs.readFileSync(bundlePath, 'utf8'))) {
  console.error('bundle failed to load');
  process.exit(1);
}
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));

const deviceId = String(Math.floor(1e18 + Math.random() * 8e18));
function fetchUrl(supWsDsOpt) {
  const u = new URL('https://webcast.tiktok.com/webcast/im/fetch/');
  for (const [k, v] of Object.entries({
    aid: '1988', app_language: 'en', app_name: 'tiktok_web', browser_language: 'en-US',
    browser_name: 'Mozilla', browser_online: 'true', browser_platform: 'Linux x86_64',
    browser_version: '5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
      + 'Chrome/131.0.0.0 Safari/537.36',
    cookie_enabled: 'true', cursor: '', debug: 'false', device_id: deviceId,
    device_platform: 'web', did_rule: '3', fetch_rule: '1', identity: 'audience',
    internal_ext: '', last_rtt: '0', live_id: '12', os: 'linux', priority_region: 'US',
    region: 'US', resp_content_type: 'protobuf', room_id: roomId, screen_height: '1080',
    screen_width: '1920', sup_ws_ds_opt: String(supWsDsOpt), tz_name: 'America/New_York',
    version_code: '270000', webcast_language: 'en',
  })) u.searchParams.set(k, v);
  // The stored token belongs in the query before signing, so X-Bogus covers what is sent.
  const token = jar.get('msToken');
  if (token) u.searchParams.set('msToken', token);
  return u.toString();
}

// Minimal heuristic scan for the push_server URI. This is a byte search, not a protobuf decode:
// `ttl_sign_core::proto` is the real parser. Good enough to prove the transport was returned.
function describe(buf) {
  const text = buf.toString('latin1');
  const match = text.match(/wss:\/\/[^\x00-\x1f"'<>]{10,300}/);
  if (!match) return null;
  const url = match[0];
  const [base, query = ''] = url.split('?');
  const names = query ? [...new Set(query.split('&').map((p) => p.split('=')[0]))] : [];
  return { base, param_names: names };
}

// Enter the room first. The page does this before fetching, and the endpoint is session-gated:
// as a guest it answers 403, so this step only becomes available once a session is loaded.
const signWith = (unsigned) => {
  const signed = new URL(unsigned);
  const product = w.byted_acrawler.frontierSign({ url: unsigned });
  if (product && product['X-Bogus']) signed.searchParams.set('X-Bogus', product['X-Bogus']);
  return signed.toString();
};
{
  const enterUrl = fetchUrl(1)
    .replace('/webcast/im/fetch/', '/webcast/room/enter/')
    .replace('&resp_content_type=protobuf', '');
  const response = await fetch(signWith(enterUrl), {
    headers: { 'user-agent': UA, origin: 'https://www.tiktok.com', referer: ROOM_URL,
      cookie: cookieHeader(), 'accept-language': 'en-US,en;q=0.9' },
  });
  absorb(response);
  const body = await response.text();
  let note = '';
  try { const j = JSON.parse(body);
    note = ` status_code=${j.status_code} msg="${String(j.status_msg || j.data?.message || '').slice(0, 40)}"`;
  } catch {}
  console.log(`room/enter: http=${response.status} bytes=${body.length}${note}`);
}

let ok = false;
for (const opt of [1, 0]) {
  const unsigned = fetchUrl(opt);
  const response = await fetch(signWith(unsigned), {
    headers: { 'user-agent': UA, origin: 'https://www.tiktok.com', referer: ROOM_URL,
      cookie: cookieHeader(), 'accept-language': 'en-US,en;q=0.9' },
  });
  absorb(response);
  const buf = Buffer.from(await response.arrayBuffer());
  const found = buf.length ? describe(buf) : null;
  console.log(`sup_ws_ds_opt=${opt}: http=${response.status} bytes=${buf.length}`
    + (found ? ' PUSH_SERVER' : buf.length ? ' (no push_server)' : ' (empty)'));
  if (found) {
    ok = true;
    console.log(`  push_server: ${found.base}`);
    console.log(`  route params: ${JSON.stringify(found.param_names)}`);
    if (process.env.TTL_TRANSPORT_OUT) {
      fs.writeFileSync(process.env.TTL_TRANSPORT_OUT, buf);
      console.log(`  wrote ${process.env.TTL_TRANSPORT_OUT} (contains session data — do not commit)`);
    }
    break;
  }
}
process.exit(ok ? 0 : 1);
