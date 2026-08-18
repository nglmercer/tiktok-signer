// Bootstrap the transport the way the page does: over XMLHttpRequest, not fetch.
//
// AUTHORIZED USE ONLY: this sends real signed requests.
//
//   node scripts/headless/xhr-transport.mjs /tmp/webmssdk.js <unique_id>
//
// ## Why this exists
//
// The live web player's transport client (`static/js/async/9894.*.js`) issues
// `/webcast/im/fetch/` with `new XMLHttpRequest()`, `withCredentials = true`, and a
// `Content-Type: application/x-www-form-urlencoded` header — never with `fetch`. webmssdk hooks
// both paths and times them separately (`fetchSignTime` and `XHRSignTime` are distinct fields in
// its state), so the XHR path is a different signing route, not a stylistic difference.
//
// Everything headless so far drove the patched `fetch`, which is what `/webcast/room/info/` and
// `/webcast/gift/list/` accept. `im/fetch` answers those an empty 200 regardless of parameters —
// including for a room id that does not exist — which is what an unevaluated request looks like.
// This probe runs the XHR route instead and reports what the SDK adds to it.
//
// The shim's XMLHttpRequest is a real implementation backed by Node's fetch, so the SDK's hooks on
// `open`, `setRequestHeader`, and `send` run against something that behaves like the browser's.
// Whatever the SDK adds — query parameters and headers alike — is captured and reported.
//
// Only names, status codes, and byte counts are printed. No signed URL, cookie, or token.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createSandbox } from './shim.mjs';
import { createXhrClass } from './lib/xhr.mjs';

const bundlePath = process.argv[2];
const user = process.argv[3];
if (!bundlePath || !user) {
  console.error('usage: node scripts/headless/xhr-transport.mjs <webmssdk.js> <unique_id>');
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
const info = (await lookup.json())?.data?.user;
if (!info?.roomId || info.roomId === '0') { console.error(`@${user} is not live`); process.exit(1); }
const roomId = info.roomId;
const ROOM_URL = `https://www.tiktok.com/@${user}/live`;
const page = await fetch(ROOM_URL, {
  headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9', cookie: cookieHeader() },
});
absorb(page);
await page.text();
console.log(`room_id=${roomId}`);

// --- a real XMLHttpRequest for the sandbox ----------------------------------------------------
//
// `lib/xhr.mjs` carries the implementation and the reasoning; it is shared with
// `im-fetch-bisect.mjs` so the probe that measures a variant and the probe that sends it cannot
// drift apart.
const observed = [];
const XhrClass = createXhrClass({
  userAgent: UA,
  referer: ROOM_URL,
  cookieHeader,
  absorb,
  onRecord: (record) => observed.push(record),
  // TTL_NO_CLIENT_HINTS drops the Chromium hints so the difference stays measurable.
  clientHints: !process.env.TTL_NO_CLIENT_HINTS,
});

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
w.XMLHttpRequest = XhrClass;

if (env.load(fs.readFileSync(bundlePath, 'utf8'))) {
  console.error('bundle failed to load');
  process.exit(1);
}
// The path allowlist gates the hooks; `/webcast/` covers the transport route.
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));
console.log(`XMLHttpRequest present after init: ${typeof w.XMLHttpRequest}`);

// The parameter set the player builds, in its own order (see chunk 9894).
const params = new URLSearchParams({
  aid: '1988', app_language: 'en', app_name: 'tiktok_web', browser_language: 'en-US',
  browser_name: 'Mozilla',
  browser_online: 'true', browser_platform: 'Linux x86_64',
  browser_version: '5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
    + 'Chrome/131.0.0.0 Safari/537.36',
  cookie_enabled: 'true', cursor: '', device_id: String(Math.floor(1e18 + Math.random() * 8e18)),
  device_platform: 'web', did_rule: '3', fetch_rule: '1', identity: 'audience',
  internal_ext: '', last_rtt: '0', live_id: '12', resp_content_type: 'protobuf',
  room_id: roomId, screen_height: '1080', screen_width: '1920', sup_ws_ds_opt: '1',
  tz_name: 'America/New_York', version_code: '270000', webcast_language: 'en',
});

// Three attempts. The service issues an `msToken` on rejection, and `msToken` in the signed query
// is a verbatim passthrough of `localStorage['xmst']`, so a first attempt necessarily carries an
// empty one. Feeding the issued token back is what a page does across a session.
for (let attempt = 1; attempt <= 3; attempt += 1) {
  const issued = jar.get('msToken');
  if (issued) store.set('xmst', issued);
  console.log(`attempt ${attempt}: xmst held = ${(store.get('xmst') || '').length} bytes`);

  const request = new w.XMLHttpRequest();
  request.timeout = 10000;
  request.responseType = 'arraybuffer';
  // TTL_URL overrides the query. The parameter set matters: signing over the project's
  // `FetchParams` query reproduces the oracle's `X-Gnarly` length exactly, while a shorter
  // hand-written one does not.
  const target = process.env.TTL_URL
    ? process.env.TTL_URL.replace(/room_id=\d+/, `room_id=${roomId}`)
    : `https://webcast.tiktok.com/webcast/im/fetch/?${params}`;
  request.open('GET', target, true);
  request.setRequestHeader('Content-Type', 'application/x-www-form-urlencoded; charset=UTF-8');
  request.withCredentials = true;
  request.send();

  // Give the SDK's asynchronous signing hook time to rewrite the request and complete it.
  await new Promise((resolve) => setTimeout(resolve, 9000));
  if (observed.at(-1)?.push_server) break;
}

if (!observed.length) {
  console.log('no request was issued — the SDK hook may have swallowed it');
  process.exit(1);
}
for (const record of observed) {
  const url = new URL(record.url);
  const signing = [...url.searchParams.keys()].filter((k) => /^X-|msToken/i.test(k));
  console.log(`${record.method} ${url.pathname}`);
  console.log(`  signed query params: ${JSON.stringify(signing)}`);
  console.log(`  headers set by the SDK: ${JSON.stringify(record.sdk_headers)}`);
  console.log(`  withCredentials=${record.with_credentials} status=${record.status ?? '-'} `
    + `bytes=${record.bytes ?? 0} push_server=${record.push_server ? 'YES' : 'no'}`);
  if (record.error) console.log(`  error: ${record.error}`);
}
process.exit(0);
