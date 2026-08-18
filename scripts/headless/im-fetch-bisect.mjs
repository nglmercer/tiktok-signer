// Bisect `/webcast/im/fetch/` against the service's own answer.
//
// AUTHORIZED USE ONLY: this sends real signed requests. Point it at a room you may test.
//
//   node scripts/headless/im-fetch-bisect.mjs /tmp/webmssdk.js <unique_id>
//   node scripts/headless/im-fetch-bisect.mjs /tmp/webmssdk.js <unique_id> --budget 6
//   node scripts/headless/im-fetch-bisect.mjs /tmp/webmssdk.js <unique_id> --only ua-windows-coherent
//   node scripts/headless/im-fetch-bisect.mjs /tmp/webmssdk.js <unique_id> --rerun
//
// ## Why this exists
//
// `verify-probe.mjs` established that `/webcast/room/info/` does not verify the signature at all: a
// one-character tamper and a wholly unsigned request return byte-identical data. That removes the
// asymmetry docs/12 used to conclude our signature is "the right shape and the wrong value", and it
// leaves `im/fetch` as the only endpoint in the system known to evaluate a signature.
//
// So `im/fetch` is the oracle. It is a *binary* one — refused, answered empty, or answered with a
// `push_server` — which is weak per request but decisive in aggregate, because
// `value-differential.mjs` proved the signature reads only five inputs:
//
//     navigator.userAgent, the canvas/WebGL fingerprint, the query, the clock, and `xmst`
//
// Nothing else reaches it: not platform, screen, language, `plugins`, `webdriver`, `referrer`, nor
// `document.cookie`. Five inputs against a three-way answer is a bounded search, and this walks it.
//
// ## Why a ledger
//
// Live requests are the scarce resource, and the reason the earlier diagnosis went wrong twice is
// that results lived in terminal scrollback. Every outcome is appended to
// `fixtures/research/bisect-ledger.json` — variant name, its input fingerprint, and the classified
// outcome — so a variant is never spent twice and a contradiction with an earlier run is visible
// rather than forgotten. Pass `--rerun` to deliberately repeat.
//
// Statuses, byte counts and digests only. No signed URL, cookie, or token is printed or stored.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createSandbox } from './shim.mjs';
import { createXhrClass } from './lib/xhr.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const LEDGER = path.join(HERE, '..', '..', 'fixtures', 'research', 'bisect-ledger.json');

const argv = process.argv.slice(2);
const bundlePath = argv[0];
const user = argv[1];
const flag = (name, fallback) => {
  const at = argv.indexOf(name);
  return at === -1 ? fallback : argv[at + 1];
};
const budget = Number(flag('--budget', 12));
const only = flag('--only', null);
const rerun = argv.includes('--rerun');

if (!bundlePath || !user || user.startsWith('--')) {
  console.error('usage: node scripts/headless/im-fetch-bisect.mjs <webmssdk.js> <unique_id> '
    + '[--budget N] [--only <variant>] [--rerun]');
  process.exit(2);
}

const LINUX_UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';
const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

// --- identity ---------------------------------------------------------------------------------

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
  { headers: { 'user-agent': LINUX_UA, cookie: cookieHeader() } },
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
  headers: { 'user-agent': LINUX_UA, 'accept-language': 'en-US,en;q=0.9', cookie: cookieHeader() },
});
absorb(page);
await page.text();
console.log(`room_id=${roomId}, page cookies absorbed (jar now ${jar.size})`);

// --- parameter sets ---------------------------------------------------------------------------
//
// Three, because the query is one of the five inputs and its content is not verifiable from here.
// The service recomputes over the query it receives, so a parameter set cannot invalidate a
// signature by itself — but a *missing required* parameter can produce a refusal that looks like a
// signature failure, which is exactly the confusion worth eliminating.

const deviceId = String(Math.floor(1e18 + Math.random() * 8e18));

const paramSets = {
  // What the transport chunk documents it sends.
  player: (browser) => new URLSearchParams({
    aid: '1988', app_language: 'en', app_name: 'tiktok_web', browser_language: 'en-US',
    browser_name: 'Mozilla', browser_online: 'true', browser_platform: browser.platform,
    browser_version: browser.version, cookie_enabled: 'true', cursor: '', device_id: deviceId,
    device_platform: 'web', did_rule: '3', fetch_rule: '1', identity: 'audience',
    internal_ext: '', last_rtt: '0', live_id: '12', resp_content_type: 'protobuf',
    room_id: roomId, screen_height: '1080', screen_width: '1920', sup_ws_ds_opt: '1',
    tz_name: 'America/New_York', version_code: '270000', webcast_language: 'en',
  }),
  // The set `ttl_sign_core::params::FetchParams` builds, whose length matched the oracle's.
  project: (browser) => new URLSearchParams({
    aid: '1988', app_language: 'en', app_name: 'tiktok_web', browser_language: 'en-US',
    browser_name: 'Mozilla', browser_online: 'true', browser_platform: browser.platform,
    browser_version: browser.version, cookie_enabled: 'true', cursor: '', debug: 'false',
    device_id: deviceId, device_platform: 'web', did_rule: '3', fetch_rule: '1',
    history_comment_count: '6', history_comment_cursor: '', identity: 'audience',
    internal_ext: '', last_rtt: '0', live_id: '12', os: 'linux', priority_region: 'US',
    region: 'US', resp_content_type: 'protobuf', room_id: roomId, screen_height: '1080',
    screen_width: '1920', sup_ws_ds_opt: '1', tz_name: 'America/New_York',
    version_code: '270000', webcast_language: 'en',
  }),
  // Only what the chunk reads back out of the response cycle.
  minimal: () => new URLSearchParams({
    aid: '1988', app_name: 'tiktok_web', device_platform: 'web', fetch_rule: '1',
    identity: 'audience', last_rtt: '0', resp_content_type: 'protobuf', room_id: roomId,
    sup_ws_ds_opt: '1', version_code: '270000',
  }),
};

const LINUX = { platform: 'Linux x86_64', version: LINUX_UA.slice(8) };
const WINDOWS = { platform: 'Win32', version: WINDOWS_UA.slice(8) };

// --- raw query surgery ------------------------------------------------------------------------
//
// Deliberately string-level. `URL`/`URLSearchParams` re-serialize a query — spaces become `+`,
// escapes are normalized — and the service recomputes the signature over the query it receives, so
// a round trip through them would break the signature for a reason unrelated to the experiment.

function dropParam(url, name) {
  const at = url.indexOf('?');
  if (at === -1) return url;
  const kept = url.slice(at + 1).split('&').filter((pair) => {
    const eq = pair.indexOf('=');
    return (eq === -1 ? pair : pair.slice(0, eq)) !== name;
  });
  return `${url.slice(0, at)}?${kept.join('&')}`;
}

function replaceParam(url, name, value) {
  const at = url.indexOf('?');
  if (at === -1) return url;
  const rebuilt = url.slice(at + 1).split('&').map((pair) => {
    const eq = pair.indexOf('=');
    return (eq === -1 ? pair : pair.slice(0, eq)) === name ? `${name}=${value}` : pair;
  });
  return `${url.slice(0, at)}?${rebuilt.join('&')}`;
}

// --- variants ---------------------------------------------------------------------------------
//
// Each is one request. `ua` is what both `navigator.userAgent` and the `user-agent` header carry;
// `browser` is what the query declares, so the two can be moved together or deliberately apart.

const VARIANTS = {
  'baseline': {
    what: 'the current signer, unchanged — the reference row',
    ua: LINUX_UA, browser: LINUX, params: 'player',
  },
  'params-project': {
    what: 'the query whose length matched the oracle exactly',
    ua: LINUX_UA, browser: LINUX, params: 'project',
  },
  'params-minimal': {
    what: 'only the parameters the transport chunk actually reads',
    ua: LINUX_UA, browser: LINUX, params: 'minimal',
  },
  'no-webgl': {
    what: 'canvas absent, so the fingerprint field collapses — does a shorter signature fare better',
    ua: LINUX_UA, browser: LINUX, params: 'player', noWebgl: true,
  },
  'ua-windows-coherent': {
    what: 'a Windows UA with the query moved to match it',
    ua: WINDOWS_UA, browser: WINDOWS, params: 'player',
  },
  'ua-query-mismatch': {
    what: 'a Linux UA against a Windows query — is the pair cross-checked',
    ua: LINUX_UA, browser: WINDOWS, params: 'player',
  },
  'xmst-absent': {
    what: 'no stored token, so msToken goes out empty',
    ua: LINUX_UA, browser: LINUX, params: 'player', xmst: 'none',
  },
  // Which of the four appended parameters draws the refusal. The suffix ships `X-Bogus=1`, a
  // literal placeholder, so "is the placeholder itself rejected" is a real question.
  'no-bogus': {
    what: 'the suffix with the X-Bogus=1 placeholder removed after signing',
    ua: LINUX_UA, browser: LINUX, params: 'player', strip: ['X-Bogus'],
  },
  'real-bogus': {
    what: 'the placeholder replaced by a genuine frontierSign X-Bogus',
    ua: LINUX_UA, browser: LINUX, params: 'player', realBogus: true,
  },
  'gnarly-only': {
    what: 'X-Dynosaur removed, X-Gnarly kept',
    ua: LINUX_UA, browser: LINUX, params: 'player', strip: ['X-Dynosaur'],
  },
  'dynosaur-only': {
    what: 'X-Gnarly removed, X-Dynosaur kept',
    ua: LINUX_UA, browser: LINUX, params: 'player', strip: ['X-Gnarly'],
  },
  'no-mstoken': {
    what: 'msToken removed from the signed query after signing',
    ua: LINUX_UA, browser: LINUX, params: 'player', strip: ['msToken'],
  },
  'no-client-hints': {
    what: 'without the Chromium client hints Node does not send',
    ua: LINUX_UA, browser: LINUX, params: 'player', clientHints: false,
  },
  'fixed-entropy': {
    what: 'deterministic getRandomValues — are the entropy fields evaluated at all',
    ua: LINUX_UA, browser: LINUX, params: 'player', deterministic: true,
  },
  'frontier-only': {
    what: 'the public frontierSign X-Bogus alone — the control, expected to answer empty',
    ua: LINUX_UA, browser: LINUX, params: 'player', product: 'frontier',
  },
};

// --- ledger -----------------------------------------------------------------------------------

function readLedger() {
  try {
    return JSON.parse(fs.readFileSync(LEDGER, 'utf8'));
  } catch {
    return { ledger_version: 1, note: 'Outcomes of im/fetch bisection variants. No secrets: '
      + 'variant names, input fingerprints, classified outcomes, and byte counts only.', runs: [] };
  }
}

// The fingerprint covers what was varied, so a row from an earlier day is comparable to this one.
// The room is deliberately excluded: docs/12 established the outcome does not depend on it.
function fingerprint(name, spec) {
  const material = JSON.stringify({
    name, ua: spec.ua, browser: spec.browser, params: spec.params,
    noWebgl: !!spec.noWebgl, xmst: spec.xmst || 'issued', strip: spec.strip || [],
    clientHints: spec.clientHints !== false, deterministic: !!spec.deterministic,
    realBogus: !!spec.realBogus, product: spec.product || 'suffix',
  });
  return crypto.createHash('sha256').update(material).digest('hex').slice(0, 12);
}

const ledger = readLedger();
const alreadySpent = new Map(ledger.runs.map((r) => [r.fingerprint, r]));

// --- one request ------------------------------------------------------------------------------

function classify(record) {
  if (!record) return 'no-request';
  if (record.error) return 'error';
  if (record.push_server) return 'ACCEPTED';
  if (record.status === 403) return 'refused';
  if (record.status === 200) return record.bytes === 0 ? 'empty' : 'answered';
  return `http-${record.status}`;
}

async function attempt(name, spec) {
  // The shim reads these at collection time, so they are per-variant switches rather than state.
  if (spec.noWebgl) process.env.TTL_NO_WEBGL = '1'; else delete process.env.TTL_NO_WEBGL;
  if (spec.deterministic) process.env.TTL_DETERMINISTIC = '1'; else delete process.env.TTL_DETERMINISTIC;

  const observed = [];
  const env = createSandbox();
  const w = env.windowTarget;
  const store = new Map();
  const issued = jar.get('msToken');
  if (issued && spec.xmst !== 'none') store.set('xmst', issued);

  w.localStorage = {
    getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, String(v)),
    removeItem: (k) => store.delete(k), clear() {}, key: () => null, length: 0,
  };
  Object.defineProperty(w.document, 'cookie', {
    configurable: true,
    get: () => cookieHeader(),
    set: (v) => {
      const [pair] = String(v).split(';');
      const eq = pair.indexOf('=');
      if (eq > 0) jar.set(pair.slice(0, eq).trim(), pair.slice(eq + 1));
    },
  });
  Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => spec.ua });
  Object.defineProperty(w.location, 'href', { configurable: true, get: () => ROOM_URL });
  // Runs after the SDK has rewritten the URL with the signature, so a removal removes something.
  const mutateUrl = (url) => {
    let out = url;
    for (const name of spec.strip || []) out = dropParam(out, name);
    if (spec.realBogus) {
      const real = w.byted_acrawler?.frontierSign?.({ url: out })?.['X-Bogus'];
      if (real) out = replaceParam(out, 'X-Bogus', encodeURIComponent(real));
    }
    return out;
  };
  w.XMLHttpRequest = createXhrClass({
    userAgent: spec.ua,
    referer: ROOM_URL,
    cookieHeader,
    absorb,
    onRecord: (record) => observed.push(record),
    clientHints: spec.clientHints !== false,
    mutateUrl,
  });

  const toStderr = () => {};
  w.console = Object.fromEntries(['log', 'info', 'warn', 'error', 'debug', 'trace', 'dir', 'table',
    'group', 'groupEnd', 'time', 'timeEnd', 'assert', 'count'].map((k) => [k, toStderr]));

  if (env.load(fs.readFileSync(bundlePath, 'utf8'))) {
    return { outcome: 'bundle-load-failed' };
  }
  const sdk = w.byted_acrawler;
  await Promise.resolve(sdk.init({ aid: 1988, enablePathList: ['/webcast/'] }));

  const query = paramSets[spec.params](spec.browser);
  const unsigned = `https://webcast.tiktok.com/webcast/im/fetch/?${query}`;

  if (spec.product === 'frontier') {
    // The public product signs a URL rather than hooking a channel, so it is issued directly.
    const out = sdk.frontierSign({ url: unsigned });
    const target = new URL(unsigned);
    if (out?.['X-Bogus']) target.searchParams.set('X-Bogus', out['X-Bogus']);
    const response = await fetch(target.toString(), {
      headers: {
        'user-agent': spec.ua, origin: 'https://www.tiktok.com', referer: ROOM_URL,
        cookie: cookieHeader(),
        'content-type': 'application/x-www-form-urlencoded; charset=UTF-8',
      },
    });
    absorb(response);
    const buffer = Buffer.from(await response.arrayBuffer());
    const record = { status: response.status, bytes: buffer.length,
      push_server: buffer.toString('latin1').includes('wss://'), sdk_headers: [] };
    return { outcome: classify(record), record, signed: { 'X-Bogus': out?.['X-Bogus']?.length ?? 0 } };
  }

  const request = new w.XMLHttpRequest();
  request.timeout = 10000;
  request.responseType = 'arraybuffer';
  request.open('GET', unsigned, true);
  request.setRequestHeader('Content-Type', 'application/x-www-form-urlencoded; charset=UTF-8');
  request.withCredentials = true;

  request.send();

  // The SDK's signing hook is asynchronous; the request is rewritten and issued inside it.
  await new Promise((resolve) => setTimeout(resolve, 9000));
  const record = observed.at(-1);
  const signed = {};
  if (record) {
    const before = new URL(unsigned).searchParams;
    for (const [k, v] of new URL(record.url).searchParams) {
      if (!before.has(k)) signed[k] = v.length;
    }
  }
  return { outcome: classify(record), record, signed };
}

// --- run --------------------------------------------------------------------------------------

// `--only` takes one variant or a comma-separated list, so a follow-up run does not repeat the
// room lookup and page fetch for every variant it wants.
const selected = only ? only.split(',').map((n) => n.trim()).filter(Boolean) : null;
const planned = Object.entries(VARIANTS).filter(([name]) => !selected || selected.includes(name));
if (selected && planned.length !== selected.length) {
  const unknown = selected.filter((n) => !VARIANTS[n]);
  console.error(`unknown variant(s) ${unknown.join(', ')}; known: ${Object.keys(VARIANTS).join(', ')}`);
  process.exit(2);
}

console.log(`\n${planned.length} variants planned, budget ${budget} request(s)\n`);
const rows = [];
let spent = 0;

for (const [name, spec] of planned) {
  const print = `${name.padEnd(21)}`;
  const fp = fingerprint(name, spec);
  const seen = alreadySpent.get(fp);
  if (seen && !rerun) {
    console.log(`${print} skipped — ledger already records "${seen.outcome}" (--rerun to repeat)`);
    rows.push({ name, outcome: seen.outcome, from: 'ledger', signed: seen.signed });
    continue;
  }
  if (spent >= budget) {
    console.log(`${print} not run — request budget exhausted`);
    continue;
  }
  spent += 1;
  let result;
  try {
    result = await attempt(name, spec);
  } catch (error) {
    result = { outcome: 'error', record: { error: String(error?.message).slice(0, 90) } };
  }
  const r = result.record || {};
  console.log(`${print} ${String(result.outcome).padEnd(10)} `
    + `http=${r.status ?? '-'} bytes=${r.bytes ?? 0} `
    + `sig=${JSON.stringify(result.signed || {})}`
    + `${r.error ? ` error=${r.error}` : ''}`);
  rows.push({ name, outcome: result.outcome, from: 'run', signed: result.signed,
    status: r.status ?? null, bytes: r.bytes ?? 0 });
  ledger.runs.push({
    fingerprint: fp, variant: name, what: spec.what, outcome: result.outcome,
    status: r.status ?? null, bytes: r.bytes ?? 0, signed_lengths: result.signed || {},
    observed_on: new Date().toISOString().slice(0, 10),
  });
  fs.writeFileSync(LEDGER, `${JSON.stringify(ledger, null, 2)}\n`);
  if (result.outcome === 'ACCEPTED') {
    console.log('\nACCEPTED: this variant returned a push_server. That is the transport.');
    break;
  }
  await new Promise((resolve) => setTimeout(resolve, 2000));
}

// --- report -----------------------------------------------------------------------------------

const accepted = rows.filter((r) => r.outcome === 'ACCEPTED');
const outcomes = new Map();
for (const row of rows) outcomes.set(row.outcome, (outcomes.get(row.outcome) || 0) + 1);

console.log(`\nspent ${spent} live request(s); ledger at fixtures/research/bisect-ledger.json`);
console.log(`outcomes: ${[...outcomes].map(([k, v]) => `${k}×${v}`).join(', ') || 'none'}`);

if (accepted.length) {
  console.log(`\nthe transport works under: ${accepted.map((r) => r.name).join(', ')}`);
  process.exit(0);
}
// The control answers empty by design, so it is excluded from the comparison. Fewer than three
// signed rows is not enough to conclude anything about the input dimensions.
const signedRows = rows.filter((r) => r.name !== 'frontier-only');
const distinct = new Set(signedRows.map((r) => r.outcome));
if (signedRows.length < 3) {
  console.log(`\nOnly ${signedRows.length} signed variant(s) ran — too few to compare. Run the sweep.`);
  process.exit(0);
}
console.log(distinct.size <= 1
  ? '\nEvery variant landed on the same outcome. None of the five inputs moves the verdict, which\n'
    + 'places the refusal outside the dimensions this probe can reach — the next lever is Phase 3\n'
    + 'introspection (per-byte field mapping) or one captured known-good signature.'
  : '\nThe outcomes differ between variants, so at least one of the five inputs moves the verdict.\n'
    + 'Bisect within the input that moved it.');
process.exit(0);
