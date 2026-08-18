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
// The SDK wraps these methods. A stub that dropped writes would make its hooks no-ops and look
// exactly like an SDK that adds nothing — the same trap the Headers stub set earlier.
const observed = [];

function makeXhr() {
  return class XMLHttpRequest {
    constructor() {
      this.readyState = 0;
      this.status = 0;
      this.statusText = '';
      this.response = null;
      this.responseText = '';
      this.responseURL = '';
      this.responseType = '';
      this.withCredentials = false;
      this.timeout = 0;
      this._headers = {};
      this._listeners = {};
      // The SDK reads and reassigns the whole handler surface while wrapping `send`; a missing
      // slot surfaces as "cannot read properties of undefined", not as a clear error.
      this.onreadystatechange = null;
      this.onload = null;
      this.onloadstart = null;
      this.onloadend = null;
      this.onprogress = null;
      this.onerror = null;
      this.onabort = null;
      this.ontimeout = null;
      this.upload = {
        onloadstart: null, onprogress: null, onload: null, onloadend: null,
        onerror: null, onabort: null, ontimeout: null,
        addEventListener() {}, removeEventListener() {}, dispatchEvent: () => true,
      };
    }

    dispatchEvent() {
      return true;
    }

    overrideMimeType() {}

    open(method, url, async = true) {
      this._method = String(method).toUpperCase();
      this._url = String(url);
      this._async = async;
      this.readyState = 1;
    }

    setRequestHeader(name, value) {
      this._headers[String(name)] = String(value);
    }

    addEventListener(type, handler) {
      (this._listeners[type] ||= []).push(handler);
    }

    removeEventListener() {}

    abort() {}

    getAllResponseHeaders() {
      return this._responseHeaders || '';
    }

    getResponseHeader(name) {
      return this._responseHeaderMap?.get(String(name).toLowerCase()) ?? null;
    }

    send(body) {
      // Everything the SDK added is visible here: the final URL and the final header set.
      const added = this._headers;
      (async () => {
        const headers = {
          'user-agent': UA,
          origin: 'https://www.tiktok.com',
          referer: ROOM_URL,
          'accept-language': 'en-US,en;q=0.9',
          ...added,
        };
        if (this.withCredentials) {
          const cookie = cookieHeader();
          if (cookie) headers.cookie = cookie;
        }
        let record = { url: this._url, method: this._method, sdk_headers: Object.keys(added),
          with_credentials: this.withCredentials };
        try {
          const response = await fetch(this._url, { method: this._method, headers, body });
          absorb(response);
          const buffer = Buffer.from(await response.arrayBuffer());
          this.status = response.status;
          this.readyState = 4;
          this.response = this.responseType === 'arraybuffer'
            ? buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength)
            : buffer.toString('utf8');
          this._responseHeaderMap = new Map([...response.headers].map(([k, v]) => [k.toLowerCase(), v]));
          this._responseHeaders = [...response.headers].map(([k, v]) => `${k}: ${v}`).join('\r\n');
          record = { ...record, status: response.status, bytes: buffer.length,
            push_server: buffer.toString('latin1').includes('wss://') };
          observed.push(record);
          if (this.onreadystatechange) this.onreadystatechange();
          if (this.onload) this.onload();
          for (const handler of this._listeners.load || []) handler({});
        } catch (error) {
          record = { ...record, error: String(error?.message).slice(0, 80) };
          observed.push(record);
          this.readyState = 4;
          if (this.onerror) this.onerror(error);
          for (const handler of this._listeners.error || []) handler(error);
        }
      })();
    }
  };
}

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
w.XMLHttpRequest = makeXhr();

if (env.load(fs.readFileSync(bundlePath, 'utf8'))) {
  console.error('bundle failed to load');
  process.exit(1);
}
// The path allowlist gates the hooks; `/webcast/` covers the transport route.
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));
const patched = w.XMLHttpRequest !== makeXhr;
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
  request.open('GET', `https://webcast.tiktok.com/webcast/im/fetch/?${params}`, true);
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
