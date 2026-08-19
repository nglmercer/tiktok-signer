// Sign one URL with the webmssdk bundle, headlessly, and print the signed URL.
//
// Rust no longer drives this: `crates/ttl-sign-embedded` runs the same sandbox in an embedded
// engine, and the `CommandSigner` that used to spawn this script is gone. What is left is a
// hand tool — one URL, one signature, from a shell — and the signer for anything already running
// on Node, which needs no native module to use it. `verify-probe.mjs` drives it too.
//
//   node scripts/headless/sign-url.mjs <webmssdk.js> <url> [product]
//
// `product` is `fetch` (default) or `frontier`:
//
//   fetch     — the patched-fetch suffix (X-Dynosaur, msToken, X-Bogus=1, X-Gnarly). Correct for
//               /webcast/room/info/ and /webcast/gift/list/.
//   frontier  — the public frontierSign product (a real 16-byte X-Bogus).
//   ws        — registerWsSigner over the query bytes, appending X-Gnarly. Correct for the direct
//               message socket, wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/
//               ws_reuse_supplement/, which is the transport the current web player opens.
//
// Picking the wrong product yields a 403 that looks exactly like a broken signer, which is why it
// is an explicit argument rather than a guess.
//
// Cookies for the signing context are read from the `TTL_COOKIE` environment variable (a cookie
// header string) so they never appear in the process arguments. A stored token can be supplied as
// `TTL_XMST`; `msToken` is a verbatim passthrough of it.
//
// Only the signed URL is printed, on stdout, with no trailing commentary.

import fs from 'node:fs';
import { createSandbox } from './shim.mjs';

const [, , bundlePath, url, product = 'fetch'] = process.argv;
if (!bundlePath || !url) {
  console.error('usage: node scripts/headless/sign-url.mjs <webmssdk.js> <url> [fetch|frontier]');
  process.exit(2);
}
if (!['fetch', 'frontier', 'ws'].includes(product)) {
  console.error(`unknown signing product "${product}"; expected fetch, frontier or ws`);
  process.exit(2);
}

const UA = process.env.TTL_USER_AGENT
  || 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';
const cookies = process.env.TTL_COOKIE || '';

const env = createSandbox();
const w = env.windowTarget;
w.localStorage = {
  getItem: (k) => (k === 'xmst' && process.env.TTL_XMST ? process.env.TTL_XMST : null),
  setItem() {}, removeItem() {}, clear() {}, key: () => null, length: 0,
};
Object.defineProperty(w.document, 'cookie', { configurable: true, get: () => cookies, set() {} });
Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => UA });
if (process.env.TTL_PAGE_URL) {
  Object.defineProperty(w.location, 'href', { configurable: true, get: () => process.env.TTL_PAGE_URL });
}

// The bundle writes to the console while loading. stdout is this program's protocol — it must
// carry the signed URL and nothing else — so give the sandbox a console that goes to stderr.
const toStderr = (...parts) => process.stderr.write(`${parts.join(' ')}\n`);
w.console = { log: toStderr, info: toStderr, warn: toStderr, error: toStderr, debug: toStderr,
  trace: toStderr, dir: toStderr, table: toStderr, group: toStderr, groupEnd: () => {},
  time: () => {}, timeEnd: () => {}, assert: () => {}, count: () => {} };

let captured = null;
w.fetch = async (input) => {
  captured = typeof input === 'string' ? input : String(input?.url || input);
  return { ok: true, status: 200, text: async () => '', json: async () => ({}) };
};

const loadError = env.load(fs.readFileSync(bundlePath, 'utf8'));
if (loadError) {
  console.error(`bundle failed to load: ${loadError.message}`);
  process.exit(1);
}

const sdk = w.byted_acrawler;
if (!sdk) {
  console.error('the bundle did not expose byted_acrawler');
  process.exit(1);
}

if (product === 'ws') {
  // The socket signature covers the query string exactly as it is sent, so the bytes are taken
  // from the URL verbatim rather than through URL/URLSearchParams, which would re-encode them.
  const query = url.slice(url.indexOf('?') + 1);
  if (!query || query === url) {
    console.error('a socket URL must carry the query the signature covers');
    process.exit(1);
  }
  await Promise.resolve(sdk.init({ aid: 1988, enablePathList: ['/webcast/'] }));
  if (typeof sdk.registerWsSigner !== 'function') {
    console.error('this bundle exposes no registerWsSigner');
    process.exit(1);
  }
  const wsSigner = sdk.registerWsSigner();
  const gnarly = typeof wsSigner === 'function'
    ? wsSigner({ 'X-MS-Q': query, 'X-MS-STUB': '' })?.['X-Gnarly']
    : null;
  if (!gnarly) {
    console.error('registerWsSigner produced no X-Gnarly');
    process.exit(1);
  }
  process.stdout.write(`${url}&X-Gnarly=${encodeURIComponent(gnarly)}`);
  process.exit(0);
}

if (product === 'frontier') {
  const signed = new URL(url);
  const out = sdk.frontierSign({ url });
  if (!out || !out['X-Bogus']) {
    console.error('frontierSign returned no X-Bogus');
    process.exit(1);
  }
  signed.searchParams.set('X-Bogus', out['X-Bogus']);
  process.stdout.write(signed.toString());
  process.exit(0);
}

// The patched fetch only signs paths matching the allowlist built by init, so derive it from the
// URL being signed rather than hardcoding one.
const pathPrefix = new URL(url).pathname.split('/').slice(0, 3).join('/') || '/';
await Promise.resolve(sdk.init({ aid: 1988, enablePathList: [pathPrefix] }));
await Promise.resolve(w.fetch(url, { method: 'GET' }));
if (!captured) {
  console.error(`the patched fetch did not sign ${pathPrefix}; check the path allowlist`);
  process.exit(1);
}
process.stdout.write(captured);
process.exit(0);
