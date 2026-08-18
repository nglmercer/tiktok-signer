// Import one known-good signed request, and diff our signature against it.
//
//   node scripts/headless/import-capture.mjs /tmp/webmssdk.js --har ~/Downloads/tiktok.har
//   node scripts/headless/import-capture.mjs /tmp/webmssdk.js --curl /tmp/copied-as-curl.txt
//   node scripts/headless/import-capture.mjs /tmp/webmssdk.js --url-file /tmp/signed-url.txt
//   node scripts/headless/import-capture.mjs /tmp/webmssdk.js --show      # what is already stored
//
// ## Why this exists
//
// **What to capture has changed.** The signature is no longer the open question: `/webcast/im/fetch/`
// is not signed by the page at all, and `/webcast/room/enter/` verifies our suffix and accepts it, so
// signing is correct. What remains is that a well-formed unsigned request is answered 200 with zero
// bytes, and this gateway answers 200-with-zero-bytes for requests it considers invalid without
// saying why.
//
// So the useful capture is a **successful** `im/fetch` — one whose response carried a `push_server`.
// Its query, diffed against ours parameter by parameter, names what we are missing. That diff is now
// this tool's main output; the signature comparison is kept because it costs nothing and confirms
// what `room/enter` already established.
//
// It does not have to be repeatable, and it does not have to come from this repository. Any browser
// that plays a TikTok LIVE room makes one every few seconds. Getting it out:
//
//   1. Open a live room in Chrome, with devtools Network open.
//   2. Filter for `im/fetch`. Pick one whose response is **not empty** — the Size column shows it,
//      and an empty one reproduces the problem rather than explaining it.
//   3. Right-click the request → Copy → Copy as cURL, save it to a file. Or "Save all as HAR".
//   4. Point this script at that file.
//
// ## What is stored where, and why it is split
//
// A signed URL is a capability: it carries `msToken` and a session-bound signature, and replaying it
// is acting as that browser. So the bytes are **not** committed.
//
//   .local/known-good-signature.json     the raw values, mode 0600, gitignored
//   fixtures/research/known-good-signature-v1.json   structure only, safe to commit
//
// The fixture records what a differential needs to survive a fresh clone — parameter order, byte
// lengths, encodings, the constant prefix byte — and no signature value, cookie, or token. The
// existing `ttl-fixture-hygiene` check covers it.

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { signOnce, sessionCookies, SIGNED_PARAMS, DEFAULT_URL } from './lib/sign.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(HERE, '..', '..');
const RAW_STORE = path.join(ROOT, '.local', 'known-good-signature.json');
const FIXTURE = path.join(ROOT, 'fixtures', 'research', 'known-good-signature-v1.json');
const FIXTURE_VERSION = 1;

const argv = process.argv.slice(2);
const bundlePath = argv[0];
const flag = (name) => {
  const at = argv.indexOf(name);
  return at === -1 ? null : argv[at + 1];
};

// --- extracting a signed URL ------------------------------------------------------------------

function fromHar(file) {
  const har = JSON.parse(fs.readFileSync(file, 'utf8'));
  const entries = (har?.log?.entries || []).filter((entry) => {
    const url = entry?.request?.url;
    return typeof url === 'string' && url.includes('/webcast/im/fetch/');
  });
  if (!entries.length) throw new Error('no /webcast/im/fetch/ request in that HAR');
  // A request whose response was empty is the failure being investigated, not the answer to it, so
  // prefer one that actually carried a body.
  const answered = entries.filter((entry) => Number(entry?.response?.content?.size || 0) > 0);
  if (!answered.length) {
    console.log(`warning: all ${entries.length} im/fetch entries in that HAR have empty responses.`);
    console.log('         Their queries are still worth diffing, but none of them succeeded either.');
  }
  return (answered.at(-1) || entries.at(-1)).request.url;
}

function fromCurl(file) {
  const text = fs.readFileSync(file, 'utf8');
  const quoted = text.match(/curl\s+'([^']+)'/) || text.match(/curl\s+"([^"]+)"/);
  const bare = text.match(/(https?:\/\/\S*\/webcast\/im\/fetch\/\S*)/);
  const url = quoted?.[1] || bare?.[1];
  if (!url) throw new Error('no URL found in that cURL text');
  return url.replace(/^['"]|['"]$/g, '');
}

function fromUrlFile(file) {
  return fs.readFileSync(file, 'utf8').trim();
}

// --- structure --------------------------------------------------------------------------------

const BASE64 = /^[A-Za-z0-9+/]+={0,2}$/;
const BASE64URL = /^[A-Za-z0-9\-_]+={0,2}$/;

function encodingOf(value) {
  if (BASE64.test(value)) return 'base64';
  if (BASE64URL.test(value)) return 'base64url';
  return 'raw';
}

function alphabetOf(value) {
  const classes = [];
  if (/[a-z]/.test(value)) classes.push('lower');
  if (/[A-Z]/.test(value)) classes.push('upper');
  if (/[0-9]/.test(value)) classes.push('digit');
  const symbols = [...new Set(value.replace(/[A-Za-z0-9]/g, ''))].sort().join('');
  if (symbols) classes.push(`symbols:${symbols}`);
  return classes.join(',');
}

function toBytes(value) {
  const encoding = encodingOf(value);
  if (encoding !== 'raw') {
    try { return Buffer.from(value, encoding); } catch { /* fall through */ }
  }
  return Buffer.from(value, 'latin1');
}

/// Everything about a signature that is safe to commit: shape, not content.
function describe(value) {
  const bytes = toBytes(value);
  return {
    chars: value.length,
    decoded_bytes: bytes.length,
    encoding: encodingOf(value),
    alphabet: alphabetOf(value),
    // byte-map.mjs finds byte 0 constant under every input, so it is structural — a version or
    // container tag — and comparing it is comparing formats rather than secrets.
    first_byte: bytes.length ? bytes[0] : null,
  };
}

// --- import -----------------------------------------------------------------------------------

if (!bundlePath) {
  console.error('usage: node scripts/headless/import-capture.mjs <webmssdk.js> '
    + '[--har F | --curl F | --url-file F | --show]');
  process.exit(2);
}

let stored = null;
try { stored = JSON.parse(fs.readFileSync(RAW_STORE, 'utf8')); } catch { /* nothing yet */ }

const source = flag('--har') ? ['har', flag('--har')]
  : flag('--curl') ? ['curl', flag('--curl')]
    : flag('--url-file') ? ['url-file', flag('--url-file')]
      : null;

if (source) {
  const [kind, file] = source;
  const url = kind === 'har' ? fromHar(file) : kind === 'curl' ? fromCurl(file) : fromUrlFile(file);
  const parsed = new URL(url);
  if (!parsed.pathname.includes('/webcast/im/fetch/')) {
    console.error(`that URL is ${parsed.pathname}, not /webcast/im/fetch/`);
    process.exit(2);
  }
  const captured = {};
  for (const name of SIGNED_PARAMS) {
    const value = parsed.searchParams.get(name);
    if (value) captured[name] = value;
  }
  if (!captured['X-Gnarly'] && !captured['X-Dynosaur']) {
    console.error('that URL carries neither X-Gnarly nor X-Dynosaur, so it is not a signed request');
    process.exit(2);
  }

  // The unsigned query is the input the signature was computed over. It is kept raw and local,
  // because reproducing the signature means signing this exact string.
  const unsignedPairs = parsed.search.slice(1).split('&')
    .filter((pair) => !SIGNED_PARAMS.includes(pair.split('=')[0]));
  const unsigned = `${parsed.origin}${parsed.pathname}?${unsignedPairs.join('&')}`;

  fs.mkdirSync(path.dirname(RAW_STORE), { recursive: true });
  fs.writeFileSync(RAW_STORE, `${JSON.stringify({
    imported_on: new Date().toISOString().slice(0, 10),
    source: kind,
    unsigned_url: unsigned,
    signature: captured,
  }, null, 2)}\n`, { mode: 0o600 });
  stored = JSON.parse(fs.readFileSync(RAW_STORE, 'utf8'));
  console.log(`stored the raw capture at .local/known-good-signature.json (mode 0600, gitignored)`);
}

if (!stored) {
  console.log('nothing imported yet. Capture one im/fetch request from a browser — see the header '
    + 'of this file for the four steps — then re-run with --har, --curl, or --url-file.');
  process.exit(0);
}

// --- the differential -------------------------------------------------------------------------

console.log(`\ncapture: imported ${stored.imported_on} from ${stored.source}`);
const capturedQuery = stored.unsigned_url.split('?')[1] || '';
console.log(`unsigned query: ${capturedQuery.length} bytes, `
  + `${capturedQuery.split('&').length} parameters`);

// Sign the captured query ourselves. Same input, so every difference is ours.
const ours = await signOnce(fs.readFileSync(bundlePath, 'utf8'), {
  url: stored.unsigned_url,
  liveClock: true,
  realEntropy: true,
  cookie: [...sessionCookies()].map(([k, v]) => `${k}=${v}`).join('; '),
  xmst: (stored.signature.msToken || '').length || 0,
});

// --- the query diff, which is the open question ------------------------------------------------

const theirParams = new Map(capturedQuery.split('&').map((pair) => {
  const [k, ...rest] = pair.split('=');
  return [k, rest.join('=')];
}));
const ourUnsigned = new URL(DEFAULT_URL);
// Compare like for like: the captured room, not the placeholder in the default query.
if (theirParams.get('room_id')) ourUnsigned.searchParams.set('room_id', theirParams.get('room_id'));
const ourParams = new Map([...ourUnsigned.searchParams]);

const onlyTheirs = [...theirParams.keys()].filter((k) => !ourParams.has(k) && !SIGNED_PARAMS.includes(k));
const onlyOurs = [...ourParams.keys()].filter((k) => !theirParams.has(k));
const shared = [...theirParams.keys()].filter((k) => ourParams.has(k));

console.log('\nquery parameters');
console.log(`  only in the capture (${onlyTheirs.length}): ${onlyTheirs.join(', ') || 'none'}`);
console.log(`  only in ours (${onlyOurs.length}): ${onlyOurs.join(', ') || 'none'}`);
const differing = shared.filter((k) => theirParams.get(k) !== ourParams.get(k));
console.log(`  present in both but differing (${differing.length}):`);
for (const name of differing) {
  // Values are printed only for parameters that carry no identity. The rest are compared by length.
  const safe = /^(aid|app_name|app_language|browser_|cookie_enabled|cursor|device_platform|did_rule|fetch_rule|identity|internal_ext|last_rtt|live_id|os|priority_region|region|resp_content_type|screen_|sup_ws_ds_opt|tz_name|version_code|webcast_language|update_version_code|compress|debug|history_comment)/.test(name);
  const theirs = theirParams.get(name);
  const mine = ourParams.get(name);
  console.log(safe
    ? `    ${name.padEnd(24)} theirs=${JSON.stringify(theirs)} ours=${JSON.stringify(mine)}`
    : `    ${name.padEnd(24)} theirs=${theirs.length}B ours=${mine.length}B (value withheld)`);
}
if (!onlyTheirs.length && !differing.length) {
  console.log('\n  The queries agree. Whatever makes their request answer and ours not is outside');
  console.log('  the query — cookies, headers, or the account the capture came from.');
} else {
  console.log('\n  Start with the parameters only the capture has: this gateway answers 200 with an');
  console.log('  empty body for a request it considers invalid, and says nothing about which part.');
}

console.log(`\n${'parameter'.padEnd(12)}${'theirs'.padEnd(30)}${'ours'.padEnd(30)}verdict`);
console.log('-'.repeat(84));
const report = {};
for (const name of SIGNED_PARAMS) {
  const theirs = stored.signature[name];
  const mine = ours[name];
  if (!theirs && !mine) continue;
  const t = theirs ? describe(theirs) : null;
  const m = mine ? describe(mine) : null;
  const show = (d) => (d ? `${d.decoded_bytes}B ${d.encoding} b0=${d.first_byte}` : 'absent');
  const verdict = !t ? 'only ours'
    : !m ? 'MISSING from ours'
      : t.encoding !== m.encoding ? 'DIFFERENT ENCODING'
        : t.first_byte !== m.first_byte ? 'DIFFERENT FORMAT BYTE'
          : t.decoded_bytes !== m.decoded_bytes ? `length differs by ${m.decoded_bytes - t.decoded_bytes}`
            : t.alphabet !== m.alphabet ? 'same length, different alphabet'
              : 'same shape';
  console.log(`${name.padEnd(12)}${show(t).padEnd(30)}${show(m).padEnd(30)}${verdict}`);
  report[name] = { theirs: t, ours: m, verdict };
}

// Positions, for the two that are studied. This is where a capture pays for itself: with the field
// layout from byte-map.mjs, the first differing offset says which field is wrong.
console.log('\nfirst differing byte, where both are present and the same length:');
for (const name of ['X-Gnarly', 'X-Dynosaur']) {
  const theirs = stored.signature[name];
  const mine = ours[name];
  if (!theirs || !mine) continue;
  const a = toBytes(theirs);
  const b = toBytes(mine);
  const shared = Math.min(a.length, b.length);
  let first = null;
  let differing = 0;
  for (let i = 0; i < shared; i += 1) {
    if (a[i] !== b[i]) {
      differing += 1;
      if (first === null) first = i;
    }
  }
  console.log(`  ${name.padEnd(12)} first=${first ?? 'identical'} differing=${differing}/${shared}`);
  report[name].first_differing_byte = first;
  report[name].differing_bytes = differing;
  console.log(`    ${first === null ? 'identical — the algorithm and every input match'
    : first === 0 ? 'differs at byte 0, which no input moves: a different container or version'
      : `byte-map.mjs attributes offset ${first} onwards; check the input whose contribution starts there`}`);
}

fs.mkdirSync(path.dirname(FIXTURE), { recursive: true });
fs.writeFileSync(FIXTURE, `${JSON.stringify({
  fixture_version: FIXTURE_VERSION,
  note: 'Structure of one known-good im/fetch signature, and how ours compares. Shapes only: '
    + 'byte lengths, encodings, alphabets, the constant format byte, and differing-position counts. '
    + 'No signature value, cookie, or token. The raw capture stays in .local/, uncommitted.',
  imported_on: stored.imported_on,
  source: stored.source,
  unsigned_query: {
    bytes: capturedQuery.length,
    parameters: capturedQuery.split('&').map((pair) => {
      const [k, v = ''] = pair.split('=');
      return { name: k, value_bytes: v.length };
    }),
  },
  comparison: report,
  query_diff: {
    only_in_capture: onlyTheirs,
    only_in_ours: onlyOurs,
    differing: differing,
  },
  bundle_sha256: crypto.createHash('sha256').update(fs.readFileSync(bundlePath)).digest('hex'),
}, null, 2)}\n`);
console.log(`\nwrote ${path.relative(ROOT, FIXTURE)} — structure only, safe to commit`);
