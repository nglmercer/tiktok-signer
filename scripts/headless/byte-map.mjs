// Where in the signature does each input land?
//
//   node scripts/headless/byte-map.mjs /tmp/webmssdk.js            # the whole map
//   node scripts/headless/byte-map.mjs /tmp/webmssdk.js <variant>  # one variant, as JSON
//
// Offline. No network, no live requests.
//
// ## Why this exists
//
// `value-differential.mjs` says *which* inputs reach the signature. It cannot say where, because it
// compares whole-value digests: any change looks the same as any other. And length is exhausted —
// docs/12 records a converged 332-byte `X-Gnarly` that the service refuses exactly like a 324-byte
// one.
//
// What is left is position. Signing is reproducible once the clock and entropy are pinned, so for
// each input the interesting number is the **first byte position that changes**. That offset says
// where in the payload the input's contribution begins, and ordering the inputs by it reconstructs
// the payload's field layout — without ever needing a correct signature to compare against.
//
// Two shapes to expect, and they are worth telling apart:
//
// - **Localized.** An input changes a bounded run of bytes and leaves the rest alone. The value is a
//   structured container, the offsets are field boundaries, and a wrong field can be pointed at.
// - **Avalanching.** An input changes everything from its offset onwards, or everything full stop.
//   That is a hash or a chained cipher, and the *first* offset is still meaningful even though the
//   tail is not.
//
// A stability pass separates the two from a third case: positions that differ between two runs of
// the *same* profile under real entropy are nonce or timestamp material, not a response to any
// input, and they are excluded from every other row so they cannot be mistaken for one.
//
// Positions, counts and digests only. No signature value is printed.

import fs from 'node:fs';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { signOnce, sessionCookies, url as profileUrl, SIGNED_PARAMS, FIXED_NOW } from './lib/sign.mjs';

const SELF = fileURLToPath(import.meta.url);
const STUDIED = ['X-Gnarly', 'X-Dynosaur'];

const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

// One variant per input the dependency map found, plus the entropy pass. Each moves exactly one
// thing, and each moves it *minimally* where possible — a one-character query change rather than a
// different parameter set — because a minimal change gives the sharpest offset.
const VARIANTS = {
  'baseline': {},
  'baseline-repeat': {},
  'ua-one-char': { userAgent: 'Mozilla/5.0 (X11; Linux x86_65) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36' },
  'ua-windows': { userAgent: WINDOWS_UA },
  'query-one-char': { url: (base) => base.replace('room_id=7300000000000000001', 'room_id=7300000000000000002') },
  'query-longer': { url: (base) => `${base}&pad=abcdefgh` },
  'clock-one-second': { now: FIXED_NOW + 1000 },
  'clock-one-hour': { now: FIXED_NOW + 3600_000 },
  // Same length, different content. Separating the two is the point: a value that tracks only the
  // length would mean the token itself never reaches the signature.
  'xmst-one-char': { xmstValue: `y${'x'.repeat(123)}` },
  'xmst-absent': { xmst: 0 },
  'no-webgl': { noWebgl: true },
  'cookie-present': { cookie: true },
  'entropy-live': { realEntropy: true },
  'entropy-live-repeat': { realEntropy: true },
};

// --- value shape ------------------------------------------------------------------------------

const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;
const BASE64URL = /^[A-Za-z0-9\-_]+={0,2}$/;

/// Decode to bytes when the value is base64, so positions are payload offsets rather than
/// characters. A base64 character boundary smears a single payload byte across up to four
/// characters, which would blur every offset this tool reports.
function toBytes(value) {
  if (BASE64.test(value)) {
    try { return { bytes: Buffer.from(value, 'base64'), encoding: 'base64' }; } catch { /* fall through */ }
  }
  if (BASE64URL.test(value)) {
    try { return { bytes: Buffer.from(value, 'base64url'), encoding: 'base64url' }; } catch { /* fall through */ }
  }
  return { bytes: Buffer.from(value, 'latin1'), encoding: 'raw' };
}

function compare(a, b) {
  const left = toBytes(a).bytes;
  const right = toBytes(b).bytes;
  const shared = Math.min(left.length, right.length);
  const differing = [];
  for (let i = 0; i < shared; i += 1) if (left[i] !== right[i]) differing.push(i);
  return {
    first: differing.length ? differing[0] : null,
    last: differing.length ? differing.at(-1) : null,
    changed: differing.length + Math.abs(left.length - right.length),
    positions: new Set(differing),
    of: left.length,
    length_delta: right.length - left.length,
  };
}

// --- one variant ------------------------------------------------------------------------------

async function run(bundlePath, name) {
  const spec = VARIANTS[name];
  if (!spec) throw new Error(`unknown variant "${name}"`);
  const jar = sessionCookies();
  const profile = {
    now: spec.now,
    realEntropy: spec.realEntropy,
    noWebgl: spec.noWebgl,
    userAgent: spec.userAgent,
    xmst: spec.xmst,
    cookie: spec.cookie ? [...jar].map(([k, v]) => `${k}=${v}`).join('; ') : '',
  };
  const base = profileUrl({});
  if (spec.url) profile.url = spec.url(base);

  if (spec.xmstValue) profile.xmstValue = spec.xmstValue;
  return signOnce(fs.readFileSync(bundlePath, 'utf8'), profile);
}

function child(bundlePath, name) {
  return new Promise((resolve) => {
    const proc = spawn(process.execPath, [SELF, bundlePath, name],
      { env: { ...process.env, TTL_CHILD: '1' }, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    let err = '';
    proc.stdout.on('data', (d) => { out += d; });
    proc.stderr.on('data', (d) => { err += d; });
    proc.on('close', () => {
      try { resolve({ name, added: JSON.parse(out) }); }
      catch { resolve({ name, error: (err.trim() || 'no output').split('\n').at(-1).slice(0, 90) }); }
    });
  });
}

// --- entry ------------------------------------------------------------------------------------

const bundlePath = process.argv[2];
const only = process.argv[3];
if (!bundlePath) {
  console.error('usage: node scripts/headless/byte-map.mjs <webmssdk.js> [variant]');
  process.exit(2);
}

if (only) {
  const added = await run(bundlePath, only);
  if (process.env.TTL_CHILD) process.stdout.write(JSON.stringify(added));
  else {
    console.log(Object.fromEntries(Object.entries(added)
      .map(([k, v]) => [k, { bytes: v.length, encoding: toBytes(v).encoding }])));
  }
  process.exit(0);
}

// Each variant runs in its own process: the bundle installs global state, so a variant that mutated
// it in place would contaminate every later row.
const names = Object.keys(VARIANTS);
const results = new Map();
const queue = [...names];
await Promise.all(Array.from({ length: Number(process.env.TTL_JOBS || 4) }, async () => {
  for (let next = queue.shift(); next; next = queue.shift()) {
    results.set(next, await child(bundlePath, next));
  }
}));

const failed = names.filter((n) => results.get(n)?.error);
if (failed.length) {
  for (const n of failed) console.error(`${n}: ${results.get(n).error}`);
  process.exit(1);
}

const baseline = results.get('baseline').added;
const repeat = results.get('baseline-repeat').added;
for (const p of STUDIED) {
  if (baseline[p] !== repeat[p]) {
    console.error(`unstable baseline for ${p}: two identical profiles disagree, so no offset below`
      + ' is attributable');
    process.exit(1);
  }
}
console.log('baseline is stable across two runs; offsets below are attributable\n');

// Entropy-driven positions are excluded everywhere else, so they cannot be read as a response to an
// input. They are reported first, because their size says how much of the value is nonce material.
const entropyPositions = {};
for (const p of STUDIED) {
  const a = results.get('entropy-live').added[p];
  const b = results.get('entropy-live-repeat').added[p];
  const diff = compare(a, b);
  entropyPositions[p] = diff.positions;
  const shape = diff.changed === 0 ? 'none — the value is fully determined by the inputs'
    : diff.changed >= diff.of * 0.9 ? `${diff.changed}/${diff.of} — avalanches, so entropy feeds a hash`
      : `${diff.changed}/${diff.of} bytes, first at ${diff.first}, last at ${diff.last}`;
  console.log(`entropy-driven in ${p.padEnd(11)} ${shape}`);
}

console.log(`\n${'variant'.padEnd(22)}${STUDIED.map((p) => p.padEnd(30)).join('')}`);
console.log('-'.repeat(22 + 30 * STUDIED.length));

const layout = {};
for (const name of names) {
  if (['baseline', 'baseline-repeat', 'entropy-live', 'entropy-live-repeat'].includes(name)) continue;
  const cells = STUDIED.map((p) => {
    const diff = compare(baseline[p], results.get(name).added[p]);
    // Positions that entropy already moves say nothing about this input.
    const attributable = [...diff.positions].filter((i) => !entropyPositions[p].has(i));
    if (!attributable.length && !diff.length_delta) return 'unchanged';
    const first = attributable.length ? attributable[0] : null;
    if (first !== null) (layout[p] ||= []).push({ name, first, changed: attributable.length });
    const span = diff.of ? attributable.length / diff.of : 0;
    const shape = span >= 0.9 ? 'avalanche' : `${attributable.length}B`;
    return `from ${String(first ?? '—').padStart(4)} ${shape}`
      + `${diff.length_delta ? ` len${diff.length_delta > 0 ? '+' : ''}${diff.length_delta}` : ''}`;
  });
  console.log(`${name.padEnd(22)}${cells.map((c) => c.padEnd(30)).join('')}`);
}

// Positions no input moves are the container's own structure: version bytes, padding, framing.
for (const p of STUDIED) {
  const size = toBytes(baseline[p]).bytes.length;
  const moved = new Set();
  for (const name of names) {
    if (name.startsWith('baseline') || name.startsWith('entropy-live')) continue;
    for (const i of compare(baseline[p], results.get(name).added[p]).positions) moved.add(i);
  }
  const constant = [];
  for (let i = 0; i < size; i += 1) if (!moved.has(i)) constant.push(i);
  const runs = [];
  for (const i of constant) {
    const last = runs.at(-1);
    if (last && last[1] === i - 1) last[1] = i;
    else runs.push([i, i]);
  }
  console.log(`\nnever moved in ${p} (${constant.length}/${size} bytes): `
    + `${runs.slice(0, 12).map(([a, b]) => (a === b ? `${a}` : `${a}-${b}`)).join(', ')}`
    + `${runs.length > 12 ? ` … ${runs.length - 12} more runs` : ''}`);
}

console.log('\ninferred field order, earliest contribution first:');
for (const p of STUDIED) {
  const ordered = (layout[p] || []).sort((a, b) => a.first - b.first);
  console.log(`  ${p}`);
  for (const row of ordered) {
    console.log(`    offset ${String(row.first).padStart(4)}  ${row.name} (${row.changed} bytes)`);
  }
  if (!ordered.length) console.log('    nothing moved — check the dependency map first');
}

const digest = (v) => crypto.createHash('sha256').update(v).digest('hex').slice(0, 10);
console.log(`\nbaseline digests: ${STUDIED.map((p) => `${p}=${digest(baseline[p])}`).join(' ')}`);
console.log(`signature encodings: ${SIGNED_PARAMS
  .filter((p) => baseline[p] !== undefined)
  .map((p) => `${p}=${toBytes(baseline[p]).encoding}`).join(' ')}`);
