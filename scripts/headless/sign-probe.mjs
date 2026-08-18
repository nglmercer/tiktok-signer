// Drive the webmssdk signing routes headless and report only names and byte lengths.
//
// No browser and no network: the bundle is evaluated against the synthetic shim and the transport
// is stubbed, so nothing is sent. Signed values are never printed.
//
//   node scripts/headless/sign-probe.mjs <webmssdk.js> [xmst-length]
import fs from 'node:fs';
import { createSandbox } from './shim.mjs';

const source = fs.readFileSync(process.argv[2], 'utf8');
const xmstLength = Number(process.argv[3] || 0);

const UNSIGNED = 'https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&app_language=en'
  + '&device_platform=web&room_id=7300000000000000001&cursor=';

const env = createSandbox();
const w = env.windowTarget;
// `msToken` is read verbatim from localStorage['xmst']; a synthetic token of the requested length
// stands in for a real one.
w.localStorage = env.view('localStorage', {
  getItem: (k) => (k === 'xmst' && xmstLength > 0 ? 'x'.repeat(xmstLength) : null),
  setItem() {}, removeItem() {}, clear() {}, key: () => null, length: 0,
});

const loadError = env.load(source);
const sdk = w.byted_acrawler;

// Public route: returns X-Bogus only.
let frontier = null;
try {
  const out = sdk.frontierSign({ url: UNSIGNED });
  frontier = Object.keys(out || {}).sort().map((name) => ({ name, bytes: String(out[name]).length }));
} catch (error) { frontier = { error: String(error?.message).slice(0, 160) }; }

// Transport route: the patched fetch appends the suffix. The path allowlist gates it.
let captured = null;
w.fetch = async (input) => {
  captured = typeof input === 'string' ? input : String((input && input.url) || input);
  return { ok: true, status: 200, text: async () => '', json: async () => ({}) };
};
await Promise.resolve(sdk.init({ aid: 1988, enablePathList: ['/webcast/'] }));
await Promise.resolve(w.fetch(UNSIGNED, { method: 'GET' }));

let suffix = null;
if (captured) {
  const before = new Set([...new URL(UNSIGNED).searchParams.keys()]);
  suffix = [...new URL(captured).searchParams.entries()]
    .filter(([name]) => !before.has(name))
    .map(([name, value]) => ({ name, bytes: value.length }));
}

console.log(JSON.stringify({
  load_error: loadError,
  sdk_functions: Object.keys(sdk || {}).filter((k) => typeof sdk[k] === 'function').sort(),
  frontier_sign: frontier,
  fetch_suffix: suffix,
}, null, 2));
process.exit(0);
