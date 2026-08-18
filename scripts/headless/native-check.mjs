// Headless equivalent of `cargo run -p ttl-sign-webview --example live-check`.
//
// Runs the same flow with no browser: unsigned lookup, native guest-identity bootstrap, then
// real webcast requests signed by the bundle running under the synthetic shim.
//
// AUTHORIZED USE ONLY. Unlike the other probes in this directory, this one sends real signed
// requests to TikTok. Point it at a room you are authorized to test.
//
//   node scripts/headless/native-check.mjs /tmp/webmssdk.js <unique_id>
//
// It prints status codes, byte counts, and parameter *names* only. No signed URL, cookie value,
// or token is ever printed.
import fs from 'node:fs';
import { createSandbox } from './shim.mjs';

const source = fs.readFileSync(process.argv[2], 'utf8');
const uniqueId = process.argv[3];

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';
const jar = new Map();
const cookieHeader = () => [...jar].map(([k, v]) => `${k}=${v}`).join('; ');
const absorb = (response) => {
  const list = response.headers.getSetCookie ? response.headers.getSetCookie() : [];
  for (const line of list) { const [pair] = line.split(';'); const i = pair.indexOf('='); if (i > 0) jar.set(pair.slice(0, i).trim(), pair.slice(i + 1)); }
  return list.map((c) => c.split('=')[0]);
};

// Step 1: unsigned lookup (this is what ttl-live-discovery does natively in Rust).
const lookupUrl = `https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId=${uniqueId}`;
const lookupResponse = await fetch(lookupUrl, { headers: { 'user-agent': UA } });
const lookupCookies = absorb(lookupResponse);
const lookup = await lookupResponse.json();
const user = lookup?.data?.user || {};
const roomId = user.roomId;
console.log(`[1] lookup   status=${lookupResponse.status} room_id=${roomId} live_status=${user.status} set-cookie=${JSON.stringify(lookupCookies)}`);
if (!roomId || roomId === '0') { console.log('    no live room; stopping'); process.exit(0); }

// Step 1b: a plain GET of the live page issues the guest identity cookies — the anti-bot id
// plus the CSRF pair. This is what the WebView got from navigating; no browser is required.
const pageResponse = await fetch(`https://www.tiktok.com/@${uniqueId}/live`, {
  headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9', cookie: cookieHeader() },
});
const pageCookies = absorb(pageResponse);
await pageResponse.text();
console.log(`[2] live page status=${pageResponse.status} set-cookie=${JSON.stringify(pageCookies)}`);

// Step 2: load the signer headlessly.
const env = createSandbox();
const w = env.windowTarget;
const store = new Map();
w.localStorage = { getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => store.set(k, String(v)), removeItem: (k) => store.delete(k),
  clear() {}, key: () => null, length: 0 };
Object.defineProperty(w.document, 'cookie', { configurable: true,
  get: () => cookieHeader(),
  set: (v) => { const [pair] = String(v).split(';'); const i = pair.indexOf('='); if (i > 0) jar.set(pair.slice(0, i).trim(), pair.slice(i + 1)); } });
Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => UA });

let captured = null;
w.fetch = async (input) => { captured = typeof input === 'string' ? input : String(input?.url || input);
  return { ok: true, status: 200, text: async () => '', json: async () => ({}) }; };
env.load(source);
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));

const signHeadless = async (url) => { captured = null; await Promise.resolve(w.fetch(url, { method: 'GET' })); return captured; };

// Step 3: sign and send the real webcast REST endpoints.
const common = `aid=1988&app_language=en&app_name=tiktok_web&browser_language=en-US&browser_name=Mozilla`
  + `&browser_platform=Linux%20x86_64&browser_version=5.0%20(X11)&cookie_enabled=true&device_platform=web_pc`
  + `&focus_state=true&from_page=user&history_len=4&is_fullscreen=false&is_page_visible=true`
  + `&os=linux&priority_region=&referer=&region=US&screen_height=1080&screen_width=1920`
  + `&tz_name=America%2FNew_York&webcast_language=en&room_id=${roomId}`;
const imExtra = '&cursor=&internal_ext=&identity=audience&resp_content_type=protobuf&fetch_rule=1&last_rtt=0&live_id=12&did_rule=3&version_code=270000';

for (const [label, path] of [['room/info', 'room/info'], ['im/fetch', 'im/fetch'],
                             ['im/fetch#2', 'im/fetch'], ['im/fetch#3', 'im/fetch']]) {
  // After the first rejection TikTok issues an msToken cookie. Feed it to the signer the way the
  // page does, so the retry carries a real token instead of an empty one.
  const issued = jar.get('msToken');
  if (issued) store.set('xmst', issued);
  const unsigned = `https://webcast.tiktok.com/webcast/${path}/?${common}` + (path === 'im/fetch' ? imExtra : '');
  if (label.startsWith('im/fetch')) console.log(`    (xmst held: ${(store.get('xmst') || '').length} bytes)`);
  const signed = await signHeadless(unsigned);
  const added = signed ? [...new URL(signed).searchParams.keys()].filter((k) => !new URL(unsigned).searchParams.has(k)) : [];
  if (!signed) { console.log(`[3] ${label}: NOT SIGNED`); continue; }
  const response = await fetch(signed, { headers: { 'user-agent': UA, referer: 'https://www.tiktok.com/',
    origin: 'https://www.tiktok.com', cookie: cookieHeader() } });
  const cookies = absorb(response);
  const text = await response.text();
  let verdict = 'unparsed';
  try { const json = JSON.parse(text);
    verdict = `status_code=${json.status_code} msg=${String(json.status_msg || json.data?.message || '').slice(0, 60)}`;
    if (label === 'room/info' && json?.data?.title) verdict += ` title_len=${json.data.title.length} viewers=${json.data.user_count}`;
    if (label === 'gift/list' && json?.data?.gifts) verdict += ` gifts=${json.data.gifts.length}`;
  } catch {
    // im/fetch answers protobuf. Look for a push_server host without printing any of it.
    const hasWss = text.includes('wss://');
    const host = hasWss ? (text.match(/wss:\/\/([a-z0-9.-]+)/) || [])[1] : null;
    verdict = `protobuf bytes=${text.length} push_server_present=${hasWss} host=${host || '-'}`;
  }
  console.log(`[3] ${label.padEnd(9)} signed_with=${JSON.stringify(added)} http=${response.status} bytes=${text.length} ${verdict} set-cookie=${JSON.stringify(cookies)}`);
}
console.log(`[4] cookie jar now: ${JSON.stringify([...jar.keys()])}`);
process.exit(0);
