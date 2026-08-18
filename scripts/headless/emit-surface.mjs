// Emit the Phase 0 environment surface in the committed schema, from a headless signing run.
import fs from 'node:fs';
import crypto from 'node:crypto';
import { createSandbox } from './shim.mjs';

const bundlePath = process.argv[2];
const raw = fs.readFileSync(bundlePath);
const source = raw.toString('utf8');

const env = createSandbox();
const w = env.windowTarget;
// Wrap the fixture provider in the recording view so storage accesses are captured like any
// other root. A synthetic `xmst` stands in for the stored token; its value is never emitted.
w.localStorage = env.view('localStorage', {
  getItem: (k) => (k === 'xmst' ? 'x'.repeat(124) : null),
  setItem() {}, removeItem() {}, clear() {}, key: () => null, length: 0,
});
env.load(source);
let captured = null;
w.fetch = async (i) => { captured = String(i); return { ok: true, status: 200, text: async () => '' }; };
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));
await Promise.resolve(w.fetch('https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001&cursor=', { method: 'GET' }));

const ROOTS = { window: 'window', global: 'window', document: 'document', navigator: 'navigator',
  screen: 'screen', location: 'location', localStorage: 'storage', sessionStorage: 'storage',
  crypto: 'crypto', Intl: 'intl', Date: 'date' };
const TYPES = new Set(['string','number','boolean','object','function','undefined']);

const merged = new Map();
for (const row of env.surface()) {
  // `global.x` and `window.x` are the same object; normalize so the shim spec has one name.
  const [head, ...rest] = row.path.split('.');
  const path = (head === 'global' ? 'window' : head) + (rest.length ? '.' + rest.join('.') : '');
  const root = ROOTS[head] || 'other';
  const key = path;
  const entry = merged.get(key) || { path, root, operations: { gets:0, sets:0, calls:0, has:0 },
    value_type: TYPES.has(row.type) ? row.type : 'unknown', byte_lengths: [] };
  const op = row.op === 'get' ? 'gets' : row.op === 'set' ? 'sets' : row.op === 'has' ? 'has' : 'calls';
  entry.operations[op] += row.gets;
  if (entry.value_type !== (TYPES.has(row.type) ? row.type : 'unknown')) entry.value_type = 'unknown';
  merged.set(key, entry);
}

const document_ = {
  surface_version: 1,
  source: {
    case_id: 'headless-node-baseline',
    bundle_endpoint: 'https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js',
    bundle: { sha256: crypto.createHash('sha256').update(raw).digest('hex'), bytes: raw.length },
    product: 'fetch',
    clock_ms: 0,
  },
  instrumentation: [
    { root: 'window', installed: true },
    { root: 'document', installed: true },
    { root: 'navigator', installed: true },
    { root: 'screen', installed: true },
    { root: 'location', installed: true },
    { root: 'storage', installed: true },
    { root: 'crypto', installed: true },
  ],
  properties: [...merged.values()].sort((a, b) => a.path.localeCompare(b.path)),
};
fs.writeFileSync(process.argv[3], JSON.stringify(document_, null, 2) + '\n');
console.log('properties:', document_.properties.length);
process.exit(0);
