// What does the live room page itself hand the player before it ever calls the transport?
//
//   node scripts/headless/room-page-scan.mjs <room_id|@unique_id>
//
// `/webcast/im/fetch/` answers us 200 with zero bytes under every request shape tried, and the
// gateway never says why. One thing never checked is what the *page* seeds the transport with. The
// web player does not invent `cursor` and `internal_ext`; it reads them out of the room page's
// embedded rehydration data, and a transport asked to resume from a cursor the server never issued
// is a request the server can reasonably answer with nothing.
//
// So read the page and report what is in it: the rehydration keys that mention a cursor, an
// internal_ext, or a `wss://` push server. If the page carries a push server outright, the transport
// call is not even on the critical path.
//
// AUTHORIZED USE ONLY: this sends real requests. It prints key paths and value shapes; no cookie,
// token, or signed URL is printed, and a `wss://` URL is reported by host and parameter names only.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const target = process.argv[2];
if (!target) {
  console.error('usage: node scripts/headless/room-page-scan.mjs <room_id|@unique_id>');
  process.exit(2);
}

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

function sessionCookies() {
  const file = process.env.TTL_SESSION_FILE
    || path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'),
      'ttl-signer', 'session');
  try {
    return fs.readFileSync(file, 'utf8').trim();
  } catch {
    console.log('no session file — scanning as a guest');
    return '';
  }
}

// A room id alone has no page. The page lives under the creator's handle, so a numeric target is
// resolved back to one through the unsigned room lookup.
let handle = target.startsWith('@') ? target.slice(1) : null;
if (!handle) {
  const lookup = await fetch(
    `https://webcast.tiktok.com/webcast/room/info/?aid=1988&room_id=${encodeURIComponent(target)}`,
    { headers: { 'user-agent': UA } },
  );
  const json = await lookup.json().catch(() => ({}));
  handle = json?.data?.owner?.display_id || json?.data?.owner?.nickname;
  if (!handle) {
    console.error(`could not resolve room ${target} to a handle`);
    process.exit(1);
  }
}

const url = `https://www.tiktok.com/@${handle}/live`;
console.log(`GET ${url}`);
const response = await fetch(url, {
  headers: {
    'user-agent': UA,
    'accept-language': 'en-US,en;q=0.9',
    accept: 'text/html,application/xhtml+xml',
    cookie: sessionCookies(),
  },
});
const html = await response.text();
console.log(`HTTP ${response.status}, ${html.length} bytes of HTML`);

// The rehydration blob is the page's own state, which is exactly what the player starts from.
const match = html.match(
  /<script id="__UNIVERSAL_DATA_FOR_REHYDRATION__"[^>]*>([\s\S]*?)<\/script>/,
);
if (!match) {
  console.log('no rehydration blob in this page — it may be a login wall or a redirect.');
  const title = html.match(/<title>([^<]*)<\/title>/);
  if (title) console.log(`title: ${title[1]}`);
  process.exit(1);
}

let data;
try {
  data = JSON.parse(match[1]);
} catch (error) {
  console.error(`rehydration blob is not JSON: ${error.message}`);
  process.exit(1);
}

// Walk it once, collecting every leaf whose path or value looks like transport seed material.
const INTERESTING = /cursor|internal_ext|push_server|wss|route_params|ws_|im_|fetch_rule|live_id|history/i;
const hits = [];
(function walk(node, trail) {
  if (node === null || node === undefined) return;
  if (typeof node === 'object') {
    for (const [key, value] of Object.entries(node)) walk(value, trail ? `${trail}.${key}` : key);
    return;
  }
  const text = String(node);
  if (!INTERESTING.test(trail) && !text.startsWith('wss://')) return;
  hits.push([trail, text]);
})(data, '');

if (!hits.length) {
  console.log('\nThe page carries no cursor, no internal_ext, and no push server.');
  console.log('The transport call really is the only source of them.');
} else {
  console.log(`\n${hits.length} transport-shaped value(s) in the page:\n`);
  for (const [trail, text] of hits) {
    if (text.startsWith('wss://')) {
      const ws = new URL(text);
      console.log(`  ${trail}`);
      console.log(`    wss host ${ws.host}${ws.pathname}`);
      console.log(`    params   ${[...ws.searchParams.keys()].join(', ')}`);
      continue;
    }
    // Values are shown only when short and not identity-shaped; longer ones by length.
    const safe = text.length <= 40 && !/^[A-Za-z0-9+/=_-]{24,}$/.test(text);
    console.log(`  ${trail.padEnd(60)} ${safe ? JSON.stringify(text) : `${text.length}B`}`);
  }
}

// The player's own config often names the transport it intends to use at all.
const scopeKeys = Object.keys(data?.__DEFAULT_SCOPE__ || {});
console.log(`\nrehydration scopes: ${scopeKeys.join(', ') || 'none'}`);
