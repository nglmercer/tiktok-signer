// Which inputs does the signature *value* depend on?
//
//   node scripts/headless/value-differential.mjs /tmp/webmssdk.js            # the whole sweep
//   node scripts/headless/value-differential.mjs /tmp/webmssdk.js <variant>  # one variant
//
// ## Why this exists
//
// `canonical-input.mjs` measures the signing input's *length*. That measurement is converged and
// the transport is still refused, which means the remaining error is in content, not size — and
// length is blind to it (docs/12, phase C). Reaching content needs a differential over values.
//
// A captured browser signature would be one way to get that. It is not the only way. Signing is
// deterministic once the clock and the entropy are pinned, so two runs of *our own* signer are
// comparable, and mutating one environment value at a time shows whether the signature depends on
// it. That yields the dependency map: the exact set of environment values the service ends up
// evaluating. Each one can then be audited against what a real Chrome reports.
//
// This is the tool docs/12 said was missing, minus the assumption that it needed an oracle.
// It answers "which of our values reach the wire", not "is our value right" — but the second
// question only has to be asked about the inputs the first one selects.
//
// Determinism comes from a frozen `Date`, a fixed `performance` clock, `Math.random`, and
// `TTL_DETERMINISTIC` entropy. The baseline is run twice and a differing pair is reported as a
// failure, because an unstable baseline would make every row meaningless.
//
// Digests only: each signature is reported as a truncated SHA-256 and a byte length. No signature,
// cookie, or token value is printed or retained.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import {
  signOnce, sessionCookies, DEFAULT_URL as SHARED_DEFAULT_URL, FIXED_NOW as SHARED_FIXED_NOW,
  SIGNED_PARAMS,
} from './lib/sign.mjs';

const SELF = fileURLToPath(import.meta.url);
const FIXED_NOW = SHARED_FIXED_NOW;
const PARAMS = SIGNED_PARAMS;

// A real Chrome's values, for the properties the sweep moves. The point of a mutation is to be a
// plausible alternative, so that a row which does not move means "not read" rather than "rejected".
const VARIANTS = {
  'baseline': {},
  'baseline-repeat': {},
  'ua-windows': { env: { 'navigator.userAgent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36' } },
  'platform-win32': { env: { 'navigator.platform': 'Win32' } },
  'language-es': { env: { 'navigator.language': 'es-ES' } },
  'languages-es': { env: { 'navigator.languages': ['es-ES', 'es'] } },
  'concurrency-16': { env: { 'navigator.hardwareConcurrency': 16 } },
  'devicememory-4': { env: { 'navigator.deviceMemory': 4 } },
  'touchpoints-5': { env: { 'navigator.maxTouchPoints': 5 } },
  'vendor-apple': { env: { 'navigator.vendor': 'Apple Computer, Inc.' } },
  'webdriver-true': { env: { 'navigator.webdriver': true } },
  'plugins-five': { env: { 'navigator.plugins': { length: 5 } } },
  'screen-1366': { env: { 'screen.width': 1366, 'screen.height': 768 } },
  'availheight-1080': { env: { 'screen.availHeight': 1080 } },
  'colordepth-30': { env: { 'screen.colorDepth': 30 } },
  'dpr-2': { env: { 'devicePixelRatio': 2 } },
  'inner-1280': { env: { 'innerWidth': 1280, 'innerHeight': 720 } },
  'referrer-set': { env: { 'document.referrer': 'https://www.tiktok.com/' } },
  'title-set': { env: { 'document.title': 'TikTok LIVE' } },
  'href-room': { pageUrl: 'https://www.tiktok.com/@someone/live' },
  'no-webgl': { noWebgl: true },
  'no-cookies': { cookie: false },
  'xmst-absent': { xmst: 0 },
  'xmst-longer': { xmst: 132 },
  'query-room-id': { queryPatch: (u) => u.replace(/room_id=\d+/, 'room_id=7300000000000000009') },
  'query-extra-param': { queryPatch: (u) => `${u}&pad=abcdefgh` },
  'webid-setters': { setters: true },
  'clock-later': { now: FIXED_NOW + 3600_000 },
  'entropy-live': { realEntropy: true },
};

const DEFAULT_URL = SHARED_DEFAULT_URL;

async function sign(bundlePath, variant) {
  const spec = VARIANTS[variant];
  if (!spec) throw new Error(`unknown variant "${variant}"`);
  const jar = sessionCookies();
  const base = process.env.TTL_URL || DEFAULT_URL;

  // Every dimension this sweep moves is a field of the shared profile, so the signature it measures
  // is produced by exactly the code path the other tools use. A private copy of the sandbox setup
  // is how a probe's finding stops transferring.
  const added = await signOnce(fs.readFileSync(bundlePath, 'utf8'), {
    userAgent: spec.env?.['navigator.userAgent'],
    noWebgl: spec.noWebgl,
    now: spec.now,
    realEntropy: spec.realEntropy,
    xmst: spec.xmst,
    cookie: spec.cookie === false ? '' : [...jar].map(([k, v]) => `${k}=${v}`).join('; '),
    pageUrl: spec.pageUrl,
    url: spec.queryPatch ? spec.queryPatch(base) : base,
    env: spec.env,
    setters: spec.setters ? { ttwid: jar.get('ttwid') || '', webid: '7300000000000000001' } : null,
  });

  const digest = (value) => crypto.createHash('sha256').update(value ?? '').digest('hex').slice(0, 10);
  return Object.fromEntries(PARAMS.map((p) => [p,
    added[p] === undefined ? null : { digest: digest(added[p]), bytes: added[p].length }]));
}

function runChild(bundlePath, variant) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [SELF, bundlePath, variant], {
      env: { ...process.env, TTL_CHILD: '1' }, stdio: ['ignore', 'pipe', 'pipe'],
    });
    let out = '';
    let err = '';
    child.stdout.on('data', (d) => { out += d; });
    child.stderr.on('data', (d) => { err += d; });
    child.on('close', () => {
      try { resolve({ variant, result: JSON.parse(out) }); }
      catch { resolve({ variant, error: (err.trim() || 'no output').split('\n').at(-1).slice(0, 90) }); }
    });
  });
}

const bundlePath = process.argv[2];
const only = process.argv[3];
if (!bundlePath) {
  console.error('usage: node scripts/headless/value-differential.mjs <webmssdk.js> [variant]');
  process.exit(2);
}

if (only) {
  const result = await sign(bundlePath, only);
  if (process.env.TTL_CHILD) process.stdout.write(JSON.stringify(result));
  else console.log(JSON.stringify(result, null, 1));
  process.exit(0);
}

// The sweep runs each variant in its own process: the bundle installs global state, and a variant
// that mutated it in place would contaminate every later row.
const names = Object.keys(VARIANTS);
const results = new Map();
const queue = [...names];
const workers = Array.from({ length: Number(process.env.TTL_JOBS || 4) }, async () => {
  for (let next = queue.shift(); next; next = queue.shift()) {
    results.set(next, await runChild(bundlePath, next));
  }
});
await Promise.all(workers);

const baseline = results.get('baseline')?.result;
const repeat = results.get('baseline-repeat')?.result;
if (!baseline) {
  console.error(`baseline failed: ${results.get('baseline')?.error}`);
  process.exit(1);
}
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
if (!same(baseline, repeat)) {
  console.error('unstable baseline: two identical runs disagree, so no row below is attributable');
  console.error(`  baseline: ${JSON.stringify(baseline)}`);
  console.error(`  repeat:   ${JSON.stringify(repeat)}`);
  process.exit(1);
}

console.log('baseline is stable across two runs; every difference below is attributable\n');
console.log(`${'variant'.padEnd(20)}${PARAMS.map((p) => p.padEnd(14)).join('')}`);
console.log('-'.repeat(20 + 14 * PARAMS.length));
const dependsOn = { };
for (const name of names) {
  if (name === 'baseline-repeat') continue;
  const row = results.get(name);
  if (row?.error) {
    console.log(`${name.padEnd(20)}error: ${row.error}`);
    continue;
  }
  const cells = PARAMS.map((p) => {
    const a = baseline[p];
    const b = row.result[p];
    if (!b) return 'absent';
    if (name === 'baseline') return `${b.bytes}b`;
    if (a.digest === b.digest) return '=';
    (dependsOn[p] ||= []).push(name);
    return a.bytes === b.bytes ? `moved ${b.bytes}b` : `moved ${a.bytes}→${b.bytes}b`;
  });
  console.log(`${name.padEnd(20)}${cells.map((c) => c.padEnd(14)).join('')}`);
}

console.log('\ndependency map — inputs each signature actually reads:');
for (const p of PARAMS) {
  const list = dependsOn[p] || [];
  console.log(`  ${p.padEnd(12)} ${list.length ? list.join(', ') : '(nothing in this sweep)'}`);
}
console.log('\n"=" means the mutation did not reach the signature. For a value the service can'
  + '\ncross-check against the request, that is the interesting case: an input the browser reports'
  + '\nand we do not is invisible here and wrong on the wire.');
