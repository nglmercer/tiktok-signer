// Measure the canonical input the VM receives at each signing route.
//
//   node scripts/headless/canonical-input.mjs /tmp/webmssdk.js [xmst-length]
//
// ## Why
//
// Our computed signatures are shorter than the ones a real browser produced: `X-Gnarly` 324 bytes
// against a recorded 332, `X-Dynosaur` 384 against 388, while the two passthrough values match
// exactly. Short *computed* values and correct *passthrough* values point at the signing input
// rather than the algorithm.
//
// The committed profile records the oracle's side of that input: `X-Gnarly` was handed a
// **1274-byte** canonical string at baseline. This prints ours next to it, so the gap is a number
// rather than a hypothesis.
//
// ## How
//
// The VM's call wrapper `L(n,t,r,i,o,e)` receives the entry offset as `n` and the arguments as
// `i`. Patching its head to record `typeof i[0]` and its length, for a watched set of entries,
// yields the input shape without touching the algorithm or retaining any value. This is the same
// hook the old VM tracer used, reduced to the one measurement this needs.
//
// Lengths only. No signing input, signature, or cookie is printed.

import fs from 'node:fs';
import { createSandbox } from './shim.mjs';

const bundlePath = process.argv[2];
const xmstLength = Number(process.argv[3] || 124);
// The canonical input includes `document.cookie`, so an empty jar makes it short by however many
// bytes a real one carries. TTL_COOKIE supplies one; TTL_SESSION_FILE reads the stored session.
let cookies = process.env.TTL_COOKIE || '';
if (!cookies && process.env.TTL_SESSION_FILE) {
  try { cookies = fs.readFileSync(process.env.TTL_SESSION_FILE, 'utf8').trim(); } catch { /* none */ }
}
if (!bundlePath) {
  console.error('usage: node scripts/headless/canonical-input.mjs <webmssdk.js> [xmst-length]');
  process.exit(2);
}

// Route entries, from fixtures/research/signing-subgraph-v1.json.
const ROUTES = {
  48886: { name: 'X-Gnarly', oracle_input: 1274, oracle_output: 332 },
  55188: { name: 'X-Dynosaur', oracle_input: 4, oracle_output: 388 },
  58628: { name: 'fetch composition', oracle_input: 786, oracle_output: 1 },
};

const VM_CALL_HEAD = 'function L(n,t,r,i,o,e){var u={o:n,u:[],C:[],I:[],A:t,M:e};';
// Record the first argument's type and length for watched entries. `y` is the bundle's global
// object, which is the sandbox window, so the arrays can be installed from outside.
const VM_CALL_HEAD_PATCHED = VM_CALL_HEAD
  + 'if(y.__ttlInputs&&y.__ttlWatch&&y.__ttlWatch[String(n)]&&y.__ttlInputs.length<512){'
  + 'try{var __a=i&&i[0];var __b=0;var __t=typeof __a;'
  + "if(__t==='string')__b=__a.length;else if(__a&&typeof __a.byteLength==='number')__b=__a.byteLength;"
  + 'y.__ttlInputs.push({entry:n,type:__t,bytes:__b});}catch(__e){}}';

const source = fs.readFileSync(bundlePath, 'utf8');
const occurrences = source.split(VM_CALL_HEAD).length - 1;
if (occurrences !== 1) {
  console.error(`unsupported VM call wrapper shape: found ${occurrences} matches, expected 1`);
  process.exit(1);
}
const patched = source.replace(VM_CALL_HEAD, VM_CALL_HEAD_PATCHED);

const env = createSandbox();
const w = env.windowTarget;
w.__ttlInputs = [];
w.__ttlWatch = Object.fromEntries(Object.keys(ROUTES).map((entry) => [entry, true]));
w.localStorage = {
  getItem: (k) => (k === 'xmst' && xmstLength > 0 ? 'x'.repeat(xmstLength) : null),
  setItem() {}, removeItem() {}, clear() {}, key: () => null, length: 0,
};
w.console = {
  log() {}, info() {}, warn() {}, error() {}, debug() {}, trace() {}, dir() {}, table() {},
  group() {}, groupEnd() {}, time() {}, timeEnd() {}, assert() {}, count() {},
};

// TTL_ENV overrides shim properties as JSON, e.g. {"navigator.platform":"Win32"}. Phase B of
// docs/12 bisects the canonical-input gap by moving one of these at a time.
if (process.env.TTL_ENV) {
  for (const [path, value] of Object.entries(JSON.parse(process.env.TTL_ENV))) {
    const [root, key] = path.split('.');
    const target = key ? w[root] : w;
    const name = key || root;
    try {
      Object.defineProperty(target, name, { configurable: true, get: () => value, set: () => {} });
    } catch {
      target[name] = value;
    }
  }
}

Object.defineProperty(w.document, 'cookie', {
  configurable: true,
  get: () => cookies,
  set: () => {},
});

let captured = null;
w.fetch = async (input) => {
  captured = typeof input === 'string' ? input : String(input?.url || input);
  return { ok: true, status: 200, text: async () => '', json: async () => ({}) };
};

const loadError = env.load(patched);
if (loadError) {
  console.error(`patched bundle failed to load: ${loadError.message}`);
  process.exit(1);
}

// TTL_URL overrides the probe's own query, so the parameter set the project already builds
// (`cargo run -p ttl-sign-core --example print-fetch-url`) can be measured directly.
const UNSIGNED = process.env.TTL_URL || 'https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&app_language=en'
  + '&app_name=tiktok_web&browser_language=en-US&browser_name=Mozilla&browser_online=true'
  + '&browser_platform=Linux%20x86_64'
  + `&browser_version=${encodeURIComponent('5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36')}`
  + '&cookie_enabled=true&cursor=&debug=false&device_id=7300000000000000001&device_platform=web'
  + '&did_rule=3&fetch_rule=1&identity=audience&internal_ext=&last_rtt=0&live_id=12&os=linux'
  + '&priority_region=US&region=US&resp_content_type=protobuf&room_id=7300000000000000001'
  + '&screen_height=1080&screen_width=1920&sup_ws_ds_opt=1&tz_name=America%2FNew_York'
  + '&version_code=270000&webcast_language=en'
  // TTL_PAD appends a parameter of a known length, to test whether the canonical input tracks the
  // query one byte for one byte.
  + (process.env.TTL_PAD ? `&pad=${'p'.repeat(Number(process.env.TTL_PAD))}` : '');

await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));
await Promise.resolve(w.fetch(UNSIGNED, { method: 'GET' }));

const outputs = {};
if (captured) {
  const before = new URL(UNSIGNED).searchParams;
  for (const [name, value] of new URL(captured).searchParams) {
    if (!before.has(name)) outputs[name] = value.length;
  }
}

// Keep the largest input seen per entry: a route may be entered more than once, and the canonical
// string is the substantial one.
const largest = new Map();
for (const record of w.__ttlInputs) {
  const previous = largest.get(record.entry);
  if (!previous || record.bytes > previous.bytes) largest.set(record.entry, record);
}

console.log(`query: ${new URL(UNSIGNED).search.length - 1} bytes`);
console.log(`cookie header: ${cookies.length} bytes, ${cookies ? cookies.split(';').length : 0} cookies`);
console.log('route              entry   input(ours)  input(oracle)   delta   output(ours)  output(oracle)');
let converged = true;
for (const [entry, meta] of Object.entries(ROUTES)) {
  const seen = largest.get(Number(entry));
  const ours = seen ? seen.bytes : null;
  const delta = ours === null ? '—' : `${ours - meta.oracle_input >= 0 ? '+' : ''}${ours - meta.oracle_input}`;
  const output = outputs[meta.name] ?? null;
  if (ours !== meta.oracle_input || output !== meta.oracle_output) converged = false;
  console.log(
    `${meta.name.padEnd(18)} ${entry.padEnd(7)} ${String(ours ?? 'not seen').padStart(11)}`
    + ` ${String(meta.oracle_input).padStart(13)} ${delta.padStart(7)}`
    + ` ${String(output ?? '-').padStart(13)} ${String(meta.oracle_output).padStart(15)}`,
  );
}
console.log();
console.log(converged
  ? 'converged: inputs and outputs match the oracle'
  : 'not converged: the gap is the environment feeding the canonical input (docs/12, phase B)');
process.exit(0);
