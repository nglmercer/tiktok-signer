// Probe the transport endpoint `/webcast/im/fetch/` headlessly, across signing variants.
//
// AUTHORIZED USE ONLY: this sends real signed requests. Point it at a room you may test.
//
//   node scripts/headless/im-fetch-probe.mjs /tmp/webmssdk.js <unique_id>
//
// What this establishes, and why the variants exist:
//
// The patched `fetch` appends X-Dynosaur/msToken/X-Bogus/X-Gnarly, where its X-Bogus is the
// one-byte composition constant `1`. That suffix is correct for `/webcast/room/info/` and
// `/webcast/gift/list/`, which return 200 with real data. It is *not* accepted by
// `/webcast/im/fetch/` or `/webcast/room/enter/`, which answer 403.
//
// `/webcast/im/fetch/` accepts the public `frontierSign` product instead — the real 16-byte
// X-Bogus — and answers 200. So the two signing products are not interchangeable per route, and
// picking the wrong one looks exactly like a broken signer. This is the single most useful thing
// this probe establishes.
//
// What the 200 contains is not yet settled. Across roughly a dozen guest runs on 2026-08-18, one
// returned a complete 56,724-byte protobuf **including a `wss://` push_server** — a usable
// transport bootstrap. Every other run returned an empty 200. The payload is therefore reachable
// without a browser, but not reliably, and this probe does not yet produce it on demand.
//
// Two candidate explanations, not distinguished:
//
//   - Rate limiting. The success came early, before this endpoint had been hit repeatedly from
//     one address; the empty runs came after.
//   - Session gating. The sibling `/webcast/room/ping/audience/` returns
//     `status_code=20003, "User doesn't login"`, and `/webcast/room/enter/` answers 403 under
//     both signing products, so this family of endpoints clearly treats guests differently.
//
// Distinguishing them is the open question. Re-run after an idle period, and with an
// authenticated session, before concluding either way.
//
// Only status codes, byte counts, and parameter names are printed.

import fs from 'node:fs';
import { createSandbox } from './shim.mjs';

const bundlePath = process.argv[2];
const user = process.argv[3];
if (!bundlePath || !user) {
  console.error('usage: node scripts/headless/im-fetch-probe.mjs <webmssdk.js> <unique_id>');
  process.exit(2);
}

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';
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

const lookup = await fetch(
  `https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId=${user}`,
  { headers: { 'user-agent': UA } },
);
absorb(lookup);
const roomId = (await lookup.json())?.data?.user?.roomId;
if (!roomId || roomId === '0') { console.error(`@${user} is not live`); process.exit(1); }

const ROOM_URL = `https://www.tiktok.com/@${user}/live`;
const page = await fetch(ROOM_URL, { headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9' } });
absorb(page);
await page.text();

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

let captured = null;
w.fetch = async (input) => {
  captured = typeof input === 'string' ? input : String(input?.url || input);
  return { ok: true, status: 200, text: async () => '', json: async () => ({}) };
};
if (env.load(fs.readFileSync(bundlePath, 'utf8'))) { console.error('bundle failed to load'); process.exit(1); }
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));

const deviceId = String(Math.floor(1e18 + Math.random() * 8e18));
const BASE = 'https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&app_language=en'
  + '&app_name=tiktok_web&browser_language=en-US&browser_name=Mozilla&browser_online=true'
  + '&browser_platform=Linux%20x86_64'
  + `&browser_version=${encodeURIComponent('5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36')}`
  + `&cookie_enabled=true&cursor=&debug=false&device_id=${deviceId}&device_platform=web`
  + '&did_rule=3&fetch_rule=1&identity=audience&internal_ext=&last_rtt=0&live_id=12&os=linux'
  + `&priority_region=US&region=US&resp_content_type=protobuf&room_id=${roomId}`
  + '&screen_height=1080&screen_width=1920&sup_ws_ds_opt=1&tz_name=America%2FNew_York'
  + '&version_code=270000&webcast_language=en';

const send = async (label, url) => {
  const response = await fetch(url, {
    headers: { 'user-agent': UA, origin: 'https://www.tiktok.com', referer: ROOM_URL,
      cookie: cookieHeader(), 'accept-language': 'en-US,en;q=0.9' },
  });
  absorb(response);
  const buf = Buffer.from(await response.arrayBuffer());
  const issued = response.headers.get('x-ms-token');
  if (issued) store.set('xmst', issued);
  const body = buf.toString('latin1');
  let note = '';
  try { const j = JSON.parse(body);
    note = ` status_code=${j.status_code} msg="${String(j.status_msg || j.data?.message || '').slice(0, 40)}"`;
  } catch { if (body.includes('wss://')) note = ' PUSH_SERVER PRESENT'; }
  console.log(`  ${label.padEnd(30)} http=${response.status} bytes=${buf.length}${note}`);
  if (body.includes('wss://')) {
    const host = (body.match(/wss:\/\/([a-z0-9.-]+)/) || [])[1];
    const params = [...new Set((body.match(/[a-z_]{3,24}=[^&\s\x00-\x1f]{1,40}/g) || []).map(p => p.split('=')[0]))];
    console.log(`     push_server host=${host}`);
    console.log(`     route param names=${JSON.stringify(params.slice(0, 20))}`);
    fs.writeFileSync('/tmp/im-fetch-response.pb', buf);
    console.log('     wrote /tmp/im-fetch-response.pb (not committed)');
  }
  return buf;
};

const patchedSign = async (url) => { captured = null; await Promise.resolve(w.fetch(url, { method: 'GET' })); return captured; };
const frontierSign = (url) => {
  const out = w.byted_acrawler.frontierSign({ url });
  const signed = new URL(url);
  if (out && out['X-Bogus']) signed.searchParams.set('X-Bogus', out['X-Bogus']);
  const token = jar.get('msToken');
  if (token) signed.searchParams.set('msToken', token);
  return signed.toString();
};

console.log(`room_id=${roomId}\n`);
console.log('im/fetch by signing product:');
await send('patched-fetch suffix', await patchedSign(BASE));   // expect 403
await send('frontierSign X-Bogus', frontierSign(BASE));        // expect 200, empty

console.log('\nsibling endpoints (frontierSign):');
await send('room/enter', frontierSign(BASE.replace('/im/fetch/', '/room/enter/')));
await send('room/ping/audience', frontierSign(
  BASE.replace('/im/fetch/', '/room/ping/audience/').replace('&resp_content_type=protobuf', '')));

console.log('\nExpected: the patched-fetch suffix is rejected (403) and frontierSign is accepted');
console.log('(200) — that difference is the reliable finding. A 200 carrying a push_server has');
console.log('been observed once but is not reproducible on demand; see the notes at the top.');
process.exit(0);
