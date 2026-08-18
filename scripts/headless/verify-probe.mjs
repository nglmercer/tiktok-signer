// Does the service verify our signature on an endpoint that accepts it?
//
// AUTHORIZED USE ONLY: this sends real signed requests. Point it at a room you may test.
//
//   node scripts/headless/verify-probe.mjs /tmp/webmssdk.js <room_id> [endpoint]
//
// `endpoint` is `room-info` (default), `gift-list`, `live-search`, or `all`.
//
// ## Why this exists
//
// docs/12 concluded that our `X-Gnarly` and `X-Dynosaur` are the right shape and the wrong value,
// resting on an asymmetry: `/webcast/room/info/` accepts the same computed suffix that
// `/webcast/im/fetch/` refuses. That reasoning has a hole. It assumes `room/info` *verifies* the
// signature. If `room/info` ignores it, then our signature has never been validated anywhere, the
// asymmetry is not evidence of endpoint-specific policy, and the search space is different.
//
// One tampered character settles it. Flipping a single character inside `X-Gnarly` — preserving
// length and alphabet — makes the signature arithmetically wrong while leaving every other
// dimension untouched. An endpoint that verifies must reject it; an endpoint that returns the same
// data as before was never checking.
//
// Six requests per endpoint, spaced out: signed, up to three single-character tampers, no
// signature, and the unsigned base URL. Status codes and byte counts only — no URL, cookie, or
// token is printed.
//
// Measured on 2026-08-18: `room/info` returns byte-identical data in all six cases. Every endpoint
// this repository signs deserves the same question, because an endpoint that does not verify is an
// endpoint that does not need a signer.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

const execFileAsync = promisify(execFile);
const HERE = path.dirname(fileURLToPath(import.meta.url));
const bundlePath = process.argv[2];
const roomId = process.argv[3];
const which = process.argv[4] || 'room-info';
if (!bundlePath || !roomId) {
  console.error('usage: node scripts/headless/verify-probe.mjs <webmssdk.js> <room_id> '
    + '[room-info|gift-list|live-search|all]');
  process.exit(2);
}
if (!/^\d+$/.test(roomId)) {
  console.error('room_id must be numeric');
  process.exit(2);
}

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

function sessionCookie() {
  const file = process.env.TTL_SESSION_FILE
    || path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'ttl-signer', 'session');
  try {
    const jar = [];
    for (const part of fs.readFileSync(file, 'utf8').trim().split(';')) {
      const eq = part.indexOf('=');
      if (eq > 0) jar.push(`${part.slice(0, eq).trim()}=${part.slice(eq + 1).trim()}`);
    }
    return jar.join('; ');
  } catch {
    return '';
  }
}

const cookie = sessionCookie();
console.log(`session: ${cookie ? `${cookie.split('; ').length} cookies` : 'guest'}`);

// The common parameters every webcast read carries. Endpoint-specific ones are merged per target.
const common = {
  aid: '1988', app_language: 'en', app_name: 'tiktok_web', browser_language: 'en-US',
  browser_name: 'Mozilla', browser_online: 'true', browser_platform: 'Linux x86_64',
  browser_version: '5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
    + 'Chrome/131.0.0.0 Safari/537.36',
  cookie_enabled: 'true', device_platform: 'web', did_rule: '3', identity: 'audience',
  screen_height: '1080', screen_width: '1920', tz_name: 'America/New_York',
  version_code: '270000', webcast_language: 'en',
};

const ENDPOINTS = {
  'room-info': `https://webcast.tiktok.com/webcast/room/info/?${
    new URLSearchParams({ ...common, room_id: roomId })}`,
  'gift-list': `https://webcast.tiktok.com/webcast/gift/list/?${
    new URLSearchParams({ ...common, room_id: roomId })}`,
  'live-search': `https://www.tiktok.com/api/search/live/full/?${
    new URLSearchParams({ ...common, keyword: 'live', offset: '0', search_id: '' })}`,
};

const targets = which === 'all' ? Object.keys(ENDPOINTS) : [which];
for (const name of targets) {
  if (!ENDPOINTS[name]) {
    console.error(`unknown endpoint "${name}"; known: ${Object.keys(ENDPOINTS).join(', ')}, all`);
    process.exit(2);
  }
}

async function signWith(base) {
  const { stdout } = await execFileAsync(
    process.execPath,
    [path.join(HERE, 'sign-url.mjs'), bundlePath, base, 'fetch'],
    { env: { ...process.env, TTL_COOKIE: cookie }, maxBuffer: 1 << 22 },
  );
  return stdout;
}

// Flip one character in the middle of a value, keeping its length and its alphabet. A length
// change would confound the result with the length dimension docs/12 already closed.
function tamper(url, param) {
  const u = new URL(url);
  const value = u.searchParams.get(param);
  if (!value) throw new Error(`${param} is absent from the signed URL`);
  const at = Math.floor(value.length / 2);
  const replacement = value[at] === 'A' ? 'B' : 'A';
  u.searchParams.set(param, value.slice(0, at) + replacement + value.slice(at + 1));
  return u.toString();
}

function strip(url, params) {
  const u = new URL(url);
  for (const p of params) u.searchParams.delete(p);
  return u.toString();
}

async function probe(name) {
  const base = ENDPOINTS[name];
  const signed = await signWith(base);

  // `msToken` is a passthrough of a stored token, so it is absent unless `TTL_XMST` supplied one.
  // Tampering with a parameter that is not there would abort the run rather than report a row.
  const optional = (label, param) => {
    try { return [[label, tamper(signed, param)]]; } catch { return []; }
  };
  const cases = [
    ['signed, untouched', signed],
    ...optional('X-Gnarly tampered', 'X-Gnarly'),
    ...optional('X-Dynosaur tampered', 'X-Dynosaur'),
    ...optional('msToken tampered', 'msToken'),
    ['signature removed', strip(signed, ['X-Gnarly', 'X-Dynosaur', 'X-Bogus', 'msToken'])],
    ['unsigned base URL', base],
  ];

  const rows = [];
  for (const [label, url] of cases) {
    try {
      const response = await fetch(url, {
        headers: { 'user-agent': UA, cookie, referer: 'https://www.tiktok.com/',
          origin: 'https://www.tiktok.com' },
      });
      const text = await response.text();
      let code = 'non-json';
      try { code = String(JSON.parse(text)?.status_code); } catch { /* keep non-json */ }
      rows.push({ label, status: response.status, bytes: text.length, code });
    } catch (error) {
      rows.push({ label, status: 'error', bytes: 0, code: String(error?.message).slice(0, 50) });
    }
    await new Promise((resolve) => setTimeout(resolve, 1200));
  }

  console.log(`\n=== ${name} ===`);
  console.log(`${'case'.padEnd(22)}${'HTTP'.padEnd(7)}${'bytes'.padEnd(9)}status_code`);
  for (const row of rows) {
    console.log(`${row.label.padEnd(22)}${String(row.status).padEnd(7)}`
      + `${String(row.bytes).padEnd(9)}${row.code}`);
  }

  const untouched = rows[0];
  const tampered = rows.filter((r) => r.label.endsWith('tampered'));
  // Byte counts drift between identical requests on endpoints whose payload carries live counters,
  // so a difference in status or `status_code` is the signal; length alone is not.
  const verifies = tampered.some((r) => r.status !== untouched.status || r.code !== untouched.code);
  console.log(verifies
    ? `${name} VERIFIES the signature: a one-character change is rejected.`
    : `${name} does NOT verify the signature: a deliberately wrong signature, and no signature at\n`
      + '  all, return the same answer as a correct one. It does not need a signer.');
  return verifies;
}

const verdicts = [];
for (const name of targets) verdicts.push([name, await probe(name)]);

console.log('\nsummary');
for (const [name, verifies] of verdicts) {
  console.log(`  ${name.padEnd(13)} ${verifies ? 'verifies' : 'does not verify'}`);
}
if (verdicts.every(([, v]) => !v)) {
  console.log('\nNone of these endpoints validates a signature, so signing them proves nothing about\n'
    + 'the signer. Only /webcast/im/fetch/ is known to evaluate one.');
}
process.exit(0);
