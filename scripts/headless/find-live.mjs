// Find channels that are live right now — without a browser.
//
// This replaces the one discovery capability that was renderer-bound. The WebView reads live
// channels out of the rendered `/live` DOM, because that page ships no channel data in its HTML.
// `/api/search/live/full/` returns the same information as JSON, and the headless signer can sign
// it, so no rendering engine is required.
//
// AUTHORIZED USE ONLY: this sends real signed requests.
//
//   node scripts/headless/find-live.mjs /tmp/webmssdk.js [keyword] [count]
//
// Prints usernames, room ids, viewer counts, and titles. No signed URL, cookie, or token is
// printed. Each row is a room you can point the signing probes at.

import fs from 'node:fs';
import { createSandbox } from './shim.mjs';
import { USER_AGENT as UA, absorbCookies, cookieHeader as header } from './lib/session.mjs';

const bundlePath = process.argv[2];
const keyword = process.argv[3] || 'live';
const wanted = Number(process.argv[4] || 20);

if (!bundlePath) {
  console.error('usage: node scripts/headless/find-live.mjs <webmssdk.js> [keyword] [count]');
  process.exit(2);
}


const jar = new Map();
const cookieHeader = () => header(jar);
const absorb = (response) => absorbCookies(jar, response);

// A plain GET of the live page issues the guest identity cookies. No browser needed.
const page = await fetch('https://www.tiktok.com/live', {
  headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9' },
});
absorb(page);
await page.text();

// Load the signer headlessly and let it see the identity we just obtained.
const env = createSandbox();
const w = env.windowTarget;
const store = new Map();
w.localStorage = {
  getItem: (k) => store.get(k) ?? null,
  setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k),
  clear() {}, key: () => null, length: 0,
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
Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => UA });

let captured = null;
w.fetch = async (input) => {
  captured = typeof input === 'string' ? input : String(input?.url || input);
  return { ok: true, status: 200, text: async () => '', json: async () => ({}) };
};
const loadError = env.load(fs.readFileSync(bundlePath, 'utf8'));
if (loadError) {
  console.error('bundle failed to load:', loadError.message);
  process.exit(1);
}
// The patched fetch signs only paths matching this allowlist; `/api/` is required here.
await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/', '/api/'] }));

const sign = async (url) => {
  captured = null;
  await Promise.resolve(w.fetch(url, { method: 'GET' }));
  return captured;
};

const BASE = 'aid=1988&app_language=en&app_name=tiktok_web&browser_language=en-US'
  + '&browser_name=Mozilla&browser_platform=Linux%20x86_64&browser_version=5.0%20(X11)'
  + '&cookie_enabled=true&device_platform=web_pc&focus_state=true&from_page=search'
  + '&history_len=4&is_fullscreen=false&is_page_visible=true&os=linux&priority_region='
  + '&referer=&region=US&screen_height=1080&screen_width=1920'
  + '&tz_name=America%2FNew_York&webcast_language=en';

const rooms = new Map();
for (let offset = 0; rooms.size < wanted && offset < wanted * 3; offset += 10) {
  const url = `https://www.tiktok.com/api/search/live/full/?${BASE}`
    + `&keyword=${encodeURIComponent(keyword)}&offset=${offset}&search_id=&count=20`;
  const signed = await sign(url);
  if (!signed) {
    console.error('the URL was not signed; check the init path allowlist');
    process.exit(1);
  }
  const response = await fetch(signed, {
    headers: {
      'user-agent': UA, referer: 'https://www.tiktok.com/', origin: 'https://www.tiktok.com',
      cookie: cookieHeader(),
    },
  });
  absorb(response);
  const body = await response.text();
  if (!body) break;

  let payload;
  try { payload = JSON.parse(body); } catch { break; }
  const items = payload?.data || [];
  if (!items.length) break;

  for (const item of items) {
    const raw = item?.live_info?.raw_data;
    if (!raw) continue;
    let room;
    try { room = JSON.parse(raw); } catch { continue; }
    // status 2 is the only value that means "broadcasting right now"; anything else is skipped
    // rather than assumed live.
    if (room?.status !== 2) continue;
    const user = room?.owner?.display_id;
    if (!user || rooms.has(user)) continue;
    rooms.set(user, {
      user,
      room_id: room.id_str,
      viewers: room.user_count ?? 0,
      title: String(room.title || '').replace(/\s+/g, ' ').slice(0, 40),
    });
  }
}

const found = [...rooms.values()].sort((a, b) => b.viewers - a.viewers).slice(0, wanted);
if (!found.length) {
  console.error(`no live rooms found for "${keyword}"`);
  process.exit(1);
}
console.log(`${found.length} live now (keyword "${keyword}")\n`);
console.log('  viewers  room_id              user');
for (const room of found) {
  console.log(`  ${String(room.viewers).padStart(7)}  ${room.room_id.padEnd(19)}  @${room.user}`);
  if (room.title) console.log(`           ${room.title}`);
}
process.exit(0);
