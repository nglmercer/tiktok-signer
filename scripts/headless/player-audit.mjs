// Re-read the player's transport code, and fail when it stops matching ours.
//
//   node scripts/headless/player-audit.mjs            # check against the fixture
//   node scripts/headless/player-audit.mjs --update    # accept what the player says now
//   node scripts/headless/player-audit.mjs --print     # just show the facts
//
// It reads `https://www.tiktok.com/live`, which ships the same app bundle as a room page and needs
// no creator to be broadcasting. A room page would do as well in principle, but those answer a
// bare client with a 1155-byte anti-bot shell, so this deliberately does not depend on one.
//
// ## Why this exists
//
// Everything the transport depends on was read out of the live player's own JavaScript: the socket
// host, the `ws_reuse_supplement` path, the two `version_code` values, the order and spelling of the
// query keys, the fact that the serializer does not percent-encode, and the field numbers of the
// three protobuf messages the socket exchanges. None of that is documented anywhere, none of it is
// versioned, and TikTok can change any of it in a deploy.
//
// When that happens the failure is silent in the worst way: the query still builds, the signature is
// still computed, the handshake still succeeds, and no frames arrive — or frames arrive and decode
// into nothing. Days of that were what the search behind `docs/12` cost.
//
// So this reads the facts back out of the shipped code and diffs them against
// `fixtures/research/player-transport-v1.json`. A drift is a failing exit code with a named
// parameter, not a mystery. `crates/ttl-sign-core` has a test asserting the same fixture matches
// what `DirectSocketParams` builds, so one audit run covers the whole chain:
//
//     player's chunk  →  this fixture  →  DirectSocketParams  →  the URL on the wire
//
// Nothing here signs anything, connects to a socket, or sends a cookie: it fetches one public page
// and the public static JavaScript it references. It is safe to run on a schedule.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(HERE, '..', '..');
const FIXTURE = path.join(ROOT, 'fixtures', 'research', 'player-transport-v1.json');

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const pageUrl = args[args.indexOf('--page') + 1]?.startsWith('http')
  ? args[args.indexOf('--page') + 1]
  : 'https://www.tiktok.com/live';

const get = async (url) => {
  const response = await fetch(url, {
    headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9' },
  });
  if (!response.ok) throw new Error(`${response.status} for ${url}`);
  return response.text();
};

// --- source location ------------------------------------------------------------------------------
//
// Chunk names carry content hashes and the ids are assigned by the bundler, so nothing here may be
// hardcoded. The path taken is the same one a browser takes: page → route manifest → live page chunk
// → the chunks it dynamically imports → the one that contains the socket path.

const html = await get(pageUrl);
const scripts = [...html.matchAll(/<script[^>]+src="([^"]+\.js)"/g)].map(([, src]) => (
  src.startsWith('//') ? `https:${src}` : src
));
const assetBase = scripts
  .find((src) => src.includes('/static/js/'))
  ?.replace(/\/static\/js\/.*$/, '');
if (!assetBase) {
  console.error(`no /static/js/ asset on ${pageUrl}`);
  console.error('A 1155-byte body means the anti-bot shell answered instead of the app.');
  process.exit(1);
}

const manifestUrl = scripts.find((src) => src.includes('route-manifest'));
const runtimeUrl = scripts.find((src) => src.includes('builder-runtime'));
if (!manifestUrl || !runtimeUrl) {
  console.error('the page ships no route manifest or builder runtime; the app was restructured');
  process.exit(1);
}
const [manifest, runtime] = await Promise.all([get(manifestUrl), get(runtimeUrl)]);

// The room route's own chunk. Named after the route rather than an id, which survives rebuilds.
const roomAsset = [...manifest.matchAll(/"(static\/js\/async\/[^"]*\.live\/page\.[0-9a-f]+\.js)"/g)]
  .map(([, asset]) => asset)[0];
if (!roomAsset) {
  console.error('no live room page asset in the route manifest');
  process.exit(1);
}
const roomChunk = await get(`${assetBase}/${roomAsset}`);

/// Take a `{`-delimited slice starting at `open`, counting braces so nested objects survive.
function braceSlice(source, open) {
  let depth = 0;
  for (let at = open; at < source.length; at += 1) {
    if (source[at] === '{') depth += 1;
    else if (source[at] === '}') {
      depth -= 1;
      if (depth === 0) return source.slice(open, at + 1);
    }
  }
  return '';
}

/// Resolve a minified `let X="value"` binding by the name the call site used.
const binding = (source, name) => source.match(new RegExp(`\\b${name}\\s*=\\s*"([^"]*)"`))?.[1] ?? null;

// The IM SDK is a dynamic import, so its chunk ids appear as `r.e("57373")` in the statement that
// configures the socket. Resolve them through the runtime's id → hash map.
const configAt = roomChunk.indexOf('socketHost:');
if (configAt < 0) {
  console.error('the room chunk no longer configures a socketHost — the transport moved');
  process.exit(1);
}
const around = roomChunk.slice(Math.max(0, configAt - 4000), configAt + 4000);
const chunkIds = [...new Set([...around.matchAll(/\.e\("(\d+)"\)/g)].map(([, id]) => id))];
const imChunks = await Promise.all(chunkIds.map(async (id) => {
  const hash = runtime.match(new RegExp(`"?${id}"?\\s*:\\s*"([0-9a-f]{6,})"`))?.[1];
  if (!hash) return null;
  return get(`${assetBase}/static/js/async/${id}.${hash}.js`).catch(() => null);
}));
const imChunk = imChunks.find((source) => source && source.includes('ws_reuse_supplement'));
if (!imChunk) {
  console.error(`none of the ${chunkIds.length} imported chunks contains the socket path`);
  process.exit(1);
}

// --- the facts ------------------------------------------------------------------------------------

const facts = {};

// The hosts the room chunk picks between, by cluster region.
facts.socket_hosts = [...new Set([...roomChunk.matchAll(/"(wss:\/\/[^"]+)"/g)].map(([, h]) => h))]
  .sort();

// Direct-socket mode. If either of these stops being set the player is back on the polling path,
// and so is everything built on it.
facts.ws_direct = /wsDirect:"1"/.test(roomChunk);
facts.fetch_before_ws_success = /fetchBeforeWsSuccess:"1"/.test(roomChunk);
facts.config_version_code = roomChunk.match(/versionCode:"(\d+)"/)?.[1] ?? null;

// Every webcast path the transport knows about, and specifically the one the direct branch opens.
facts.im_paths = [...new Set([...imChunk.matchAll(/"(\/webcast\/im\/[^"]*)"/g)].map(([, p]) => p))]
  .sort();
const directPath = imChunk.match(/\$\{(\w+)\.socketHost\}\$\{\w+\.isPreview\?(\w+):(\w+)\}/);
facts.direct_socket_path = directPath ? binding(imChunk, directPath[3]) : null;
facts.preview_socket_path = directPath ? binding(imChunk, directPath[2]) : null;

// The browser block: the SDK's default `version_code` and the key order it emits.
const blockAt = imChunk.indexOf('{version_code:');
const block = blockAt < 0 ? '' : braceSlice(imChunk, blockAt);
facts.browser_block_keys = [...block.matchAll(/(?:^|[{,])(\w+):/g)].map(([, key]) => key);
const versionVar = block.match(/version_code:(\w+)/)?.[1];
facts.sdk_version_code = versionVar ? binding(imChunk, versionVar) : null;
const updateVar = imChunk.match(/updateVersionCode:(\w+)/)?.[1];
facts.update_version_code = updateVar ? binding(imChunk, updateVar) : null;

// The signature: which product signs the socket, and under which header names.
facts.ws_signer = /registerWsSigner\(\)/.test(imChunk) ? 'registerWsSigner' : null;
facts.ws_sign_input = imChunk.match(/\{"(X-MS-[A-Z]+)":\w+,"(X-MS-[A-Z]+)":""\}/)?.slice(1, 3) ?? [];
facts.ws_sign_output = imChunk.match(/\["(X-[A-Za-z]+)"\]/)?.[1] ?? null;

// The serializer. Ours reproduces its bytes, so an added `encodeURIComponent` here invalidates every
// signature we compute — this is the single most consequential line in the audit.
const serializerAt = imChunk.search(/function \w+\(\w+\)\{let \w+=Object\.keys\(\w+\)/);
const serializer = serializerAt < 0 ? '' : imChunk.slice(serializerAt, serializerAt + 400);
facts.serializer_percent_encodes = /encodeURIComponent|encodeURI\(/.test(serializer);
facts.serializer_camel_to_snake = /\[A-Z\]\/g/.test(serializer);

// The three protobuf messages the socket exchanges, by field number.
function protoFields(name) {
  const at = imChunk.indexOf(`${name}:{fields:`);
  if (at < 0) return null;
  const body = braceSlice(imChunk, imChunk.indexOf('{', at + name.length));
  const fields = {};
  for (const [, field, inner] of body.matchAll(/(\w+):\{([^{}]*)\}/g)) {
    const id = inner.match(/id:(\d+)/)?.[1];
    const type = inner.match(/type:"([^"]+)"/)?.[1];
    if (id) fields[field] = { id: Number(id), type: type ?? null };
  }
  return fields;
}
facts.protos = {
  PushFrame: protoFields('PushFrame'),
  EnterRoom: protoFields('EnterRoom'),
  HeartBeat: protoFields('HeartBeat'),
};

// --- report ---------------------------------------------------------------------------------------

if (flag('--print')) {
  console.log(JSON.stringify(facts, null, 2));
  process.exit(0);
}

const stored = fs.existsSync(FIXTURE) ? JSON.parse(fs.readFileSync(FIXTURE, 'utf8')) : null;
const record = {
  what: 'Facts read out of the live player\'s transport code, which ttl-sign-core reproduces.',
  how: 'node scripts/headless/player-audit.mjs --update',
  read_on: new Date().toISOString().slice(0, 10),
  facts,
};

if (!stored || flag('--update')) {
  fs.mkdirSync(path.dirname(FIXTURE), { recursive: true });
  fs.writeFileSync(FIXTURE, `${JSON.stringify(record, null, 2)}\n`);
  console.log(`wrote ${path.relative(ROOT, FIXTURE)}`);
  if (stored) {
    console.log('Re-run `cargo test -p ttl-sign-core`: it compares this fixture against the builder,');
    console.log('so anything that moved will now name itself there.');
  }
  process.exit(0);
}

/// Flatten to dotted paths so a diff can name the parameter that moved.
function flatten(value, trail = '', out = {}) {
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    for (const [key, inner] of Object.entries(value)) flatten(inner, trail ? `${trail}.${key}` : key, out);
    return out;
  }
  out[trail] = JSON.stringify(value);
  return out;
}

const before = flatten(stored.facts);
const after = flatten(facts);
const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])].sort();
const drift = keys.filter((key) => before[key] !== after[key]);

console.log(`player transport audit — fixture read on ${stored.read_on}`);
console.log(`${keys.length} facts compared`);
if (!drift.length) {
  console.log('\nno drift. The player still builds the transport the way this repository does.');
  process.exit(0);
}

console.log(`\n${drift.length} fact(s) moved:\n`);
for (const key of drift) {
  console.log(`  ${key}`);
  console.log(`    was ${before[key] ?? '(absent)'}`);
  console.log(`    now ${after[key] ?? '(absent)'}`);
}
console.log('\nWhat each of these breaks:');
console.log('  serializer_percent_encodes  every signature — the signed bytes stop matching the sent ones');
console.log('  browser_block_keys, *version_code, direct_socket_path, socket_hosts');
console.log('                              the query, silently: the handshake still succeeds, no frames arrive');
console.log('  ws_direct, fetch_before_ws_success');
console.log('                              the whole approach — the player went back to polling im/fetch');
console.log('  protos.*                    frame encoding or decoding, after a socket that looks healthy');
console.log('\nFix the builder in crates/ttl-sign-core/src/params.rs, then re-run with --update.');
process.exit(1);
