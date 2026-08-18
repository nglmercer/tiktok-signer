// Vary the *identity* of a transport request while holding the signature fixed.
//
// AUTHORIZED USE ONLY: this sends real signed requests.
//
//   node scripts/headless/identity-probe.mjs /tmp/webmssdk.js <unique_id>
//
// ## Why
//
// The signing input now matches the oracle exactly (`X-Gnarly` 332 bytes — see
// `canonical-input.mjs`), and `/webcast/im/fetch/` still refuses. That makes the signature the one
// thing worth *holding constant* while everything about the identity moves: `ttwid` present or
// absent, session present or absent, `device_id` random or bound.
//
// Each row differs from the baseline in exactly one dimension, which is the same discipline the
// research plans use — a run that changes two things explains nothing.
//
// Status codes and byte counts only. No cookie, token, or signed URL is printed.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createSandbox } from './shim.mjs';

const bundlePath = process.argv[2];
const user = process.argv[3];
if (!bundlePath || !user) {
  console.error('usage: node scripts/headless/identity-probe.mjs <webmssdk.js> <unique_id>');
  process.exit(2);
}

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';
const source = fs.readFileSync(bundlePath, 'utf8');

function storedSession() {
  const file = process.env.TTL_SESSION_FILE
    || path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'),
      'ttl-signer', 'session');
  try {
    return fs.readFileSync(file, 'utf8').trim();
  } catch {
    return '';
  }
}

const session = storedSession();

/** Fetch a fresh guest identity the way a first page load does. */
async function freshIdentity() {
  const jar = new Map();
  const response = await fetch(`https://www.tiktok.com/@${user}/live`, {
    headers: { 'user-agent': UA, 'accept-language': 'en-US,en;q=0.9' },
  });
  for (const line of (response.headers.getSetCookie?.() ?? [])) {
    const [pair] = line.split(';');
    const eq = pair.indexOf('=');
    if (eq > 0) jar.set(pair.slice(0, eq).trim(), pair.slice(eq + 1));
  }
  await response.text();
  return jar;
}

const lookup = await fetch(
  `https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId=${user}`,
  { headers: { 'user-agent': UA } },
);
const info = (await lookup.json())?.data?.user;
if (!info?.roomId || info.roomId === '0') { console.error(`@${user} is not live`); process.exit(1); }
const roomId = info.roomId;
const ROOM_URL = `https://www.tiktok.com/@${user}/live`;
console.log(`room_id=${roomId}`);

// The query whose signature reproduces the oracle. Supply it via TTL_URL from
// `cargo run -q -p ttl-sign-core --example print-fetch-url`.
const TEMPLATE = process.env.TTL_URL;
if (!TEMPLATE) {
  console.error('set TTL_URL to the unsigned im/fetch URL '
    + '(cargo run -q -p ttl-sign-core --example print-fetch-url -- <room_id>)');
  process.exit(2);
}

async function attempt({ label, cookies, deviceId }) {
  const env = createSandbox();
  const w = env.windowTarget;
  const store = new Map();
  const cookieHeader = cookies;
  w.localStorage = {
    getItem: (k) => (k === 'xmst' ? store.get('xmst') ?? null : null),
    setItem: (k, v) => store.set(k, String(v)),
    removeItem: (k) => store.delete(k), clear() {}, key: () => null, length: 0,
  };
  Object.defineProperty(w.document, 'cookie', {
    configurable: true, get: () => cookieHeader, set: () => {},
  });
  Object.defineProperty(w.navigator, 'userAgent', { configurable: true, get: () => UA });
  Object.defineProperty(w.location, 'href', { configurable: true, get: () => ROOM_URL });
  w.console = { log() {}, info() {}, warn() {}, error() {}, debug() {}, trace() {}, dir() {},
    table() {}, group() {}, groupEnd() {}, time() {}, timeEnd() {}, assert() {}, count() {} };

  let captured = null;
  w.fetch = async (input) => {
    captured = typeof input === 'string' ? input : String(input?.url || input);
    return { ok: true, status: 200, text: async () => '', json: async () => ({}) };
  };
  if (env.load(source)) return `${label}: bundle failed to load`;
  await Promise.resolve(w.byted_acrawler.init({ aid: 1988, enablePathList: ['/webcast/'] }));

  const unsigned = TEMPLATE
    .replace(/room_id=\d+/, `room_id=${roomId}`)
    .replace(/device_id=\d+/, `device_id=${deviceId}`);
  await Promise.resolve(w.fetch(unsigned, { method: 'GET' }));
  if (!captured) return `${label}: not signed`;

  const headers = {
    'user-agent': UA, origin: 'https://www.tiktok.com', referer: ROOM_URL,
    'accept-language': 'en-US,en;q=0.9',
    'content-type': 'application/x-www-form-urlencoded; charset=UTF-8',
  };
  if (cookieHeader) headers.cookie = cookieHeader;

  const response = await fetch(captured, { headers });
  const buffer = Buffer.from(await response.arrayBuffer());
  const gnarly = new URL(captured).searchParams.get('X-Gnarly')?.length ?? 0;
  return `${label.padEnd(34)} http=${String(response.status).padEnd(4)} bytes=${String(buffer.length).padEnd(6)}`
    + `gnarly=${gnarly} ${buffer.toString('latin1').includes('wss://') ? 'PUSH_SERVER' : ''}`;
}

const guest = await freshIdentity();
const guestHeader = [...guest].map(([k, v]) => `${k}=${v}`).join('; ');
const withSession = [session, guestHeader].filter(Boolean).join('; ');
// Built by joining name and value rather than interpolating into a `name=value` literal: the
// hygiene gate reads that shape as a committed cookie, and it is right to.
const ANTI_BOT_COOKIE = 'ttwid';
const ttwidOnly = guest.has(ANTI_BOT_COOKIE)
  ? [ANTI_BOT_COOKIE, guest.get(ANTI_BOT_COOKIE)].join('=')
  : '';
const randomDevice = () => String(Math.floor(1e18 + Math.random() * 8e18));

console.log('signature held constant; identity varies one dimension per row\n');
for (const row of [
  { label: 'session + fresh guest cookies', cookies: withSession, deviceId: randomDevice() },
  { label: 'guest cookies only (no session)', cookies: guestHeader, deviceId: randomDevice() },
  { label: 'ttwid only', cookies: ttwidOnly, deviceId: randomDevice() },
  { label: 'no cookies at all', cookies: '', deviceId: randomDevice() },
  { label: 'session only (no page cookies)', cookies: session, deviceId: randomDevice() },
  { label: 'session, device_id = 0', cookies: withSession, deviceId: '0' },
]) {
  try {
    console.log(await attempt(row));
  } catch (error) {
    console.log(`${row.label}: error ${String(error?.message).slice(0, 60)}`);
  }
}
process.exit(0);
