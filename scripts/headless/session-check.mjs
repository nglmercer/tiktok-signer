// Is the stored session actually still logged in?
//
//   node scripts/headless/session-check.mjs
//
// The transport endpoint is documented to answer a guest with an empty body, and this repository has
// been reading empty bodies while assuming the session was live. An expired `sessionid` looks
// exactly like a signing failure from the outside: `room/enter` still gates on the signature and
// answers 200, `room/info` and `gift/list` never needed a user, and only the paths that resolve a
// viewer go quiet.
//
// So check the session directly, against endpoints that say who you are. Names, statuses and
// booleans only — no cookie, token, or profile field is printed.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

function sessionPath() {
  if (process.env.TTL_SESSION_FILE) return process.env.TTL_SESSION_FILE;
  const base = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, 'ttl-signer', 'session');
}

const file = sessionPath();
let raw = '';
try {
  raw = fs.readFileSync(file, 'utf8').trim();
} catch {
  console.error(`no session file at ${file}`);
  process.exit(2);
}
const jar = new Map();
for (const part of raw.split(';')) {
  const eq = part.indexOf('=');
  if (eq > 0) jar.set(part.slice(0, eq).trim(), part.slice(eq + 1).trim());
}
const cookie = [...jar].map(([k, v]) => `${k}=${v}`).join('; ');
const stat = fs.statSync(file);
const ageDays = Math.floor((Date.now() - stat.mtimeMs) / 86_400_000);
console.log(`session file: ${jar.size} cookies, written ${ageDays} day(s) ago`);
console.log(`cookies: ${[...jar.keys()].sort().join(', ')}`);

const checks = [
  // Reports the logged-in user, or an error for a guest.
  ['passport account info', 'https://www.tiktok.com/passport/web/account/info/'],
  // The web app's own login probe.
  ['user detail (self)', 'https://www.tiktok.com/api/user/detail/?aid=1988&uniqueId='],
];

let loggedIn = null;
for (const [label, url] of checks) {
  try {
    const response = await fetch(url, {
      headers: { 'user-agent': UA, cookie, referer: 'https://www.tiktok.com/' },
    });
    const text = await response.text();
    let verdict = 'unreadable';
    try {
      const json = JSON.parse(text);
      // passport replies {message:"success", data:{...}} when the session is live.
      const ok = json.message === 'success' || json.status_code === 0;
      const hasUser = Boolean(json.data?.user_id_str || json.data?.email
        || json.userInfo?.user?.id);
      verdict = `message=${json.message ?? json.status_code} identifies_user=${hasUser}`;
      if (label.startsWith('passport')) loggedIn = ok && hasUser;
    } catch {
      verdict = `non-json, ${text.length} bytes`;
    }
    console.log(`  ${label.padEnd(24)} HTTP ${String(response.status).padEnd(4)} ${verdict}`);
  } catch (error) {
    console.log(`  ${label.padEnd(24)} error ${String(error?.message).slice(0, 60)}`);
  }
  await new Promise((r) => setTimeout(r, 800));
}

console.log();
if (loggedIn === true) {
  console.log('The session is live. An empty transport body is not an expired login.');
  process.exit(0);
}
if (loggedIn === false) {
  console.log('The session is NOT logged in. That alone explains every empty body: the transport');
  console.log('endpoint answers a guest with nothing, room/info and gift/list never needed a user,');
  console.log('and room/enter gates on the signature rather than the session. Re-export the cookies');
  console.log('from a logged-in browser into the session file and re-run the transport check.');
  process.exit(1);
}
console.log('Could not determine the login state from these endpoints.');
process.exit(1);
