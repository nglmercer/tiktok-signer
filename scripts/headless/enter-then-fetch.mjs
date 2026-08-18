// Enter the room, then fetch the transport config.
//
// AUTHORIZED USE ONLY: this sends real signed requests. Point it at a room you may test.
//
//   node scripts/headless/enter-then-fetch.mjs /tmp/webmssdk.js <unique_id>
//
// ## Why this exists
//
// Reading the live page's own chunks settled two things this repository had backwards.
//
// **`/webcast/im/fetch/` is not signed.** The page's signing allowlist for `webcast.tiktok.com` is
// seven GET paths and twenty-two POST paths — wallet, KYC, `room/chat`, `room/enter` — and
// `im/fetch` is in neither. Confirmed against the service: unsigned returns 200, while adding
// `X-Gnarly` or `X-Dynosaur` returns 403 whatever their content. Every earlier "the transport is
// refused" result was a request carrying a signature the endpoint never expects.
//
// **`/webcast/room/enter/` *is* signed**, as a POST. So the signer's real job is entering the room,
// and the unsigned fetch depends on having done that first — which is what an empty 200 to a
// well-formed unsigned fetch looks like.
//
// This runs the sequence in that order: sign and POST `room/enter`, then issue the unsigned
// `im/fetch` the transport chunk issues, with its own query — `version_code=180800`, `cursor=0`,
// `internal_ext=0`, `last_rtt=-1`, values unencoded.
//
// Statuses and byte counts only. No signed URL, cookie, or token is printed.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createSandbox } from './shim.mjs';

const bundlePath = process.argv[2];
const user = process.argv[3];
if (!bundlePath || !user) {
  console.error('usage: node scripts/headless/enter-then-fetch.mjs <webmssdk.js> <unique_id>');
  process.exit(2);
}

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';
const HOST = 'https://webcast.tiktok.com';

const jar = new Map();
const cookieHeader = () => [...jar].map(([k, v]) => `${k}=${v}`).join('; ');
const absorb = (response) => {
  for (const line of response.headers.getSetCookie ? response.headers.getSetCookie() : []) {
    const [pair] = line.split(';');
    const eq = pair.indexOf('=');
    if (eq > 0) jar.set(pair.slice(0, eq).trim(), pair.slice(eq + 1));
  }
};

function sessionPath() {
  if (process.env.TTL_SESSION_FILE) return process.env.TTL_SESSION_FILE;
  const base = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, 'ttl-signer', 'session');
}
try {
  for (const part of fs.readFileSync(sessionPath(), 'utf8').trim().split(';')) {
    const eq = part.indexOf('=');
    if (eq > 0) jar.set(part.slice(0, eq).trim(), part.slice(eq + 1).trim());
  }
} catch { /* guest */ }
console.log(`session: ${jar.size} cookies, authenticated=${Boolean(jar.get('sessionid'))}`);

const lookup = await fetch(
  `https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId=${user}`,
  { headers: { 'user-agent': UA, cookie: cookieHeader() } },
);
absorb(lookup);
const live = (await lookup.json())?.data?.user;
if (!live?.roomId || live.roomId === '0') {
  console.error(`@${user} is not live`);
  process.exit(1);
}
const roomId = live.roomId;
const ROOM_URL = `https://www.tiktok.com/@${user}/live`;
const page = await fetch(ROOM_URL, {
  headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9', cookie: cookieHeader() },
});
absorb(page);
await page.text();
const deviceId = jar.get('tt_webid_v2') || String(Math.floor(1e18 + Math.random() * 8e18));
console.log(`room_id=${roomId}, jar now ${jar.size}`);
// Names only. A browser on a live room carries a wider set than a session file plus one page load,
// and any of the missing ones could be what the origin is looking for before it answers.
console.log(`cookies held: ${[...jar.keys()].sort().join(', ')}`);

// --- the environment block both requests share ------------------------------------------------

const common = {
  aid: '1988',
  app_language: 'en',
  app_name: 'tiktok_web',
  browser_language: 'en-US',
  browser_name: 'Mozilla',
  browser_online: 'true',
  browser_platform: 'Linux x86_64',
  browser_version: UA.slice(8),
  cookie_enabled: 'true',
  device_id: deviceId,
  device_platform: 'web',
  focus_state: 'true',
  from_page: 'user',
  history_len: '3',
  is_fullscreen: 'false',
  is_page_visible: 'true',
  os: 'linux',
  priority_region: '',
  referer: '',
  region: 'US',
  screen_height: '1080',
  screen_width: '1920',
  tz_name: 'America/New_York',
  webcast_language: 'en',
};

// --- step 1: room/enter, signed --------------------------------------------------------------

const env = createSandbox();
const w = env.windowTarget;
const store = new Map();
w.localStorage = {
  getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k), clear() {}, key: () => null, length: 0,
};
if (jar.get('msToken')) store.set('xmst', jar.get('msToken'));
Object.defineProperty(w.document, 'cookie', {
  configurable: true, get: () => cookieHeader(), set: () => {},
});
Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => UA });
Object.defineProperty(w.location, 'href', { configurable: true, get: () => ROOM_URL });
const toStderr = (...parts) => process.stderr.write(`${parts.join(' ')}\n`);
w.console = Object.fromEntries(['log', 'info', 'warn', 'error', 'debug', 'trace', 'dir', 'table',
  'group', 'groupEnd', 'time', 'timeEnd', 'assert', 'count'].map((k) => [k, toStderr]));

let signedUrl = null;
w.fetch = async (input) => {
  signedUrl = typeof input === 'string' ? input : String(input?.url || input);
  return { ok: true, status: 200, headers: { get: () => null }, text: async () => '',
    json: async () => ({}) };
};

if (env.load(fs.readFileSync(bundlePath, 'utf8'))) {
  console.error('bundle failed to load');
  process.exit(1);
}
// The allowlist the page uses for this path. `room/enter` is on it; `im/fetch` is deliberately not.
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/room/enter/'] }));

const enterQuery = new URLSearchParams({
  ...common,
  room_id: roomId,
  identity: 'audience',
  enter_source: 'others-others',
  is_room_feed: '0',
  version_code: '270000',
  did_rule: '3',
  live_id: '12',
  sup_ws_ds_opt: '1',
});
const enterUrl = `${HOST}/webcast/room/enter/?${enterQuery}`;
await Promise.resolve(w.fetch(enterUrl, { method: 'POST' }));
const suffixUrl = signedUrl || enterUrl;

// Which form does room/enter accept? It is on the page's POST signing allowlist, so unlike
// `im/fetch` it is expected to carry a signature — and it is the only endpoint measured here that
// answers differently depending on one. That makes it the oracle `im/fetch` never was.
const bogus = w.byted_acrawler.frontierSign({ url: enterUrl })?.['X-Bogus'];
const bogusUrl = (() => {
  const u = new URL(enterUrl);
  if (bogus) u.searchParams.set('X-Bogus', bogus);
  return u.toString();
})();

async function postEnter(label, target) {
  const response = await fetch(target, {
    method: 'POST',
    headers: {
      'user-agent': UA, origin: 'https://www.tiktok.com', referer: ROOM_URL,
      'accept-language': 'en-US,en;q=0.9', cookie: cookieHeader(),
      'content-type': 'application/x-www-form-urlencoded; charset=UTF-8',
    },
    body: '',
  });
  absorb(response);
  const body = await response.text();
  let code = 'non-json';
  try { code = String(JSON.parse(body)?.status_code); } catch { /* keep */ }
  const added = [...new URL(target).searchParams.keys()].filter((k) => /^X-|msToken/i.test(k));
  console.log(`      ${label.padEnd(22)} HTTP ${String(response.status).padEnd(4)} `
    + `${String(body.length).padStart(6)} bytes  status_code=${String(code).padEnd(8)} `
    + `sig=${JSON.stringify(added)}`);
  return { ok: response.status === 200 && code === '0', status: response.status, code };
}

console.log('\n[1/2] room/enter — which form does it accept?');
const forms = [
  ['unsigned', enterUrl],
  ['X-Bogus only', bogusUrl],
  ['full suffix', suffixUrl],
];
let entered = null;
for (const [label, target] of forms) {
  const result = await postEnter(label, target);
  if (result.ok && !entered) entered = label;
  await new Promise((r) => setTimeout(r, 1500));
}
console.log(entered
  ? `      accepted as: ${entered}`
  : '      no form was accepted');

// --- step 2: im/fetch, unsigned, with the chunk's own query -----------------------------------

/// The transport chunk's serializer: snake_case the key, drop empty values, values written raw.
function playerQuery(fields) {
  return Object.entries(fields)
    .filter(([, value]) => value !== undefined && value !== '')
    .map(([key, value]) => `${key}=${String(value)}`)
    .join('&');
}

const fetchQuery = playerQuery({
  ...common,
  // The chunk's own constant, not the 270000 the rest of this repository sends.
  version_code: '180800',
  room_id: roomId,
  identity: 'audience',
  live_id: '12',
  fetch_rule: '1',
  did_rule: '3',
  sup_ws_ds_opt: '1',
  resp_content_type: 'protobuf',
  // The initial fetch's overrides. The chunk deletes empty values, so it can never send `cursor=`.
  last_rtt: '-1',
  cursor: '0',
  internal_ext: '0',
});
console.log(`\n[2/2] im/fetch — unsigned, ${fetchQuery.length}-byte query`);

// An empty protobuf body says nothing about why it is empty. Asking for JSON makes the service
// explain itself: a refusal carries a status_code and a message, and reading one is worth more than
// another round of guessing at the request.
async function tryFetch(label, query, {
  host = HOST, extraHeaders = {}, extraCookies = {}, dropHeaders = [],
} = {}) {
  const cookie = [
    cookieHeader(),
    ...Object.entries(extraCookies).map(([k, v]) => `${k}=${v}`),
  ].filter(Boolean).join('; ');
  const headers = {
    'user-agent': UA, origin: 'https://www.tiktok.com', referer: ROOM_URL,
    'accept-language': 'en-US,en;q=0.9', cookie,
    'content-type': 'application/x-www-form-urlencoded; charset=UTF-8',
    ...extraHeaders,
  };
  for (const name of dropHeaders) delete headers[name];
  const response = await fetch(`${host}/webcast/im/fetch/?${query}`, { headers });
  absorb(response);
  const buffer = Buffer.from(await response.arrayBuffer());
  const text = buffer.toString('utf8');
  const hasPush = buffer.toString('latin1').includes('wss://');
  let note = '';
  if (text.trim().startsWith('{')) {
    try {
      const json = JSON.parse(text);
      note = ` status_code=${json.status_code} message=${JSON.stringify(json.message
        || json.data?.message || json.extra?.error_message || '').slice(0, 80)}`;
    } catch { note = ' unparseable json'; }
  }
  console.log(`      ${label.padEnd(20)} HTTP ${String(response.status).padEnd(4)} `
    + `${String(buffer.length).padStart(7)} bytes push_server=${hasPush ? 'YES' : 'no'}${note}`);
  // A zero-byte 200 carries its explanation in the headers if anywhere: TikTok's edge stamps
  // routing and trace headers, and a redirect directive would name the data centre to retry in.
  if (process.env.TTL_SHOW_HEADERS) {
    for (const [key, value] of [...response.headers].sort()) {
      if (/^(content-|x-|server|via|tt-|set-cookie)/i.test(key)) {
        console.log(`        ${key}: ${String(value).slice(0, 100)}`);
      }
    }
  }
  return { buffer, hasPush };
}

const protobufResult = await tryFetch('protobuf', fetchQuery);
await new Promise((r) => setTimeout(r, 1200));
// Without `resp_content_type`, the service answers JSON.
await tryFetch('json (no resp_type)', fetchQuery.replace('&resp_content_type=protobuf', ''));

// A 200 with zero bytes is not an application refusal — an application refusal carries a
// `status_code`. It is an edge answering nothing, which is what happens when a request lands in a
// data centre that does not hold the session. This account's `tt-target-idc` is `alisg`, and the
// room's page reports idc `my2`, so the edge this repository has always used may simply be the
// wrong one for these two paths.
// A full Chromium header set. These were measured to change nothing on the *signed* route, but the
// signed route was refused for a different reason entirely, so the unsigned one deserves its own
// test — `sec-fetch-site` in particular is what tells an edge the request came from a page.
const CHROMIUM_HEADERS = {
  accept: '*/*',
  'accept-encoding': 'gzip, deflate, br',
  'sec-ch-ua': '"Chromium";v="131", "Not_A Brand";v="24", "Google Chrome";v="131"',
  'sec-ch-ua-mobile': '?0',
  'sec-ch-ua-platform': '"Linux"',
  'sec-fetch-dest': 'empty',
  'sec-fetch-mode': 'cors',
  'sec-fetch-site': 'same-site',
  priority: 'u=1, i',
};

const routing = [
  ['chromium headers', HOST, CHROMIUM_HEADERS],
  ['idc header', HOST, { 'x-tt-target-idc': jar.get('tt-target-idc') || 'alisg' }],
  ['webcast.us.', 'https://webcast.us.tiktok.com', {}],
  ['tiktokv.com', 'https://webcast.tiktokv.com', {}],
  // This session is pinned to `alisg`, so try the hosts that data centre answers on. A name that
  // does not resolve reports an error row, which is itself information.
  ['alisg normal-c', 'https://webcast-normal-c-alisg.tiktokv.com', {}],
  ['alisg 16', 'https://webcast16-normal-c-alisg.tiktokv.com', {}],
  ['tiktokv.eu', 'https://webcast.tiktokv.eu', {}],
  ['sg tiktok', 'https://webcast-sg.tiktok.com', {}],
];
// The cookies a browser on a live room holds that a session file plus one page load does not. The
// query's `device_id` is the page's webid, so a random one with no matching `tt_webid_v2` is a
// binding the origin can check and silently decline; `s_v_web_id` (`verifyFp`) is required outright
// by several TikTok web APIs; the `store-*` pair is what the app uses for regional routing.
const WEBID_COOKIES = { tt_webid: deviceId, tt_webid_v2: deviceId };
const VERIFY_FP = `verify_${Math.random().toString(36).slice(2, 10)}_${Math.random().toString(36).slice(2, 14)}`;
const STORE_COOKIES = {
  'store-idc': jar.get('tt-target-idc') || 'alisg',
  'store-country-code': 'pe',
  'store-country-code-src': 'uid',
};

// The gateway answers 200 with zero bytes for requests it considers invalid — `/api/user/detail/`
// with an empty `uniqueId` does the same — so it is worth removing the headers we add that a
// bodyless GET has no business carrying. `Content-Type` on a GET with no body is the obvious one: a
// form-parsing gateway can reasonably find nothing to parse.
const headerShapes = [
  ['- content-type', { dropHeaders: ['content-type'] }],
  ['- origin', { dropHeaders: ['origin'] }],
  ['- referer', { dropHeaders: ['referer'] }],
  ['accept json', { extraHeaders: { accept: 'application/json, text/plain, */*' } }],
];
for (const [label, options] of headerShapes) {
  await new Promise((r) => setTimeout(r, 1200));
  const result = await tryFetch(label, fetchQuery, options);
  if (result.hasPush) {
    console.log(`\n      push_server came back with ${label} — that is the request shape.`);
    break;
  }
}

const identity = [
  ['+ webid cookies', {}, WEBID_COOKIES, fetchQuery],
  ['+ verifyFp', {}, { s_v_web_id: VERIFY_FP }, `${fetchQuery}&verifyFp=${VERIFY_FP}`],
  ['+ store cookies', {}, STORE_COOKIES, fetchQuery],
  ['+ all of them', {}, { ...WEBID_COOKIES, s_v_web_id: VERIFY_FP, ...STORE_COOKIES },
    `${fetchQuery}&verifyFp=${VERIFY_FP}`],
];
for (const [label, extraHeaders, extraCookies, query] of identity) {
  await new Promise((r) => setTimeout(r, 1200));
  const result = await tryFetch(label, query, { extraHeaders, extraCookies });
  if (result.hasPush) {
    console.log(`\n      push_server came back with ${label} — that is the missing identity.`);
    break;
  }
}

for (const [label, host, extraHeaders] of routing) {
  await new Promise((r) => setTimeout(r, 1200));
  try {
    const result = await tryFetch(label, fetchQuery, { host, extraHeaders });
    if (result.hasPush) {
      console.log(`\n      push_server came back from ${label} — that is the routing fix.`);
      break;
    }
  } catch (error) {
    console.log(`      ${label.padEnd(20)} error ${String(error?.message).slice(0, 60)}`);
  }
}
const buffer = protobufResult.buffer;
const hasPushServer = protobufResult.hasPush;

if (hasPushServer) {
  console.log('\nThe transport bootstrapped. Signing room/enter and leaving im/fetch unsigned is the'
    + '\nsequence; the endpoint was never the problem the signature was blamed for.');
  process.exit(0);
}
console.log(`\nStill ${buffer.length === 0 ? 'an empty body' : 'no push_server'}. The request shape now`
  + '\nmatches the chunk exactly — unsigned, its query, its host — so what remains is room state or'
  + '\nidentity, not signing.');
process.exit(1);
