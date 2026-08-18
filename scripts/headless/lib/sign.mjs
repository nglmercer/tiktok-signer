// Sign one URL under a described environment, reproducibly.
//
// Every differential in this directory needs the same thing: a signature produced under an
// environment stated as data, with the sources of nondeterminism pinned so that two runs of the
// same profile are byte-identical. Without that, no comparison means anything — a differing
// signature could be the variable under test or could be the clock.
//
// So the clock, `performance`, `Math.random` and `crypto.getRandomValues` are all fixed by default,
// and a profile turns individual dimensions back on or moves them. `value-differential.mjs` uses
// this to find which inputs reach the value at all; `byte-map.mjs` uses it to find *where* in the
// value they land.
//
// Values are returned to the caller, which is what a differential needs. Nothing is printed here;
// callers report digests, lengths, and positions.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createSandbox } from '../shim.mjs';

/// The clock every pinned profile reports, unless it overrides `now`.
export const FIXED_NOW = 1787000000000;

/// The signature parameters the patched fetch appends, in the order it appends them.
export const SIGNED_PARAMS = ['X-Dynosaur', 'msToken', 'X-Bogus', 'X-Gnarly'];

const CHROME_LINUX_UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

/** The session jar, if one is stored. Used as signing context, never printed. */
export function sessionCookies() {
  const file = process.env.TTL_SESSION_FILE
    || path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'),
      'ttl-signer', 'session');
  try {
    const jar = new Map();
    for (const part of fs.readFileSync(file, 'utf8').trim().split(';')) {
      const eq = part.indexOf('=');
      if (eq > 0) jar.set(part.slice(0, eq).trim(), part.slice(eq + 1).trim());
    }
    return jar;
  } catch {
    return new Map();
  }
}

/**
 * Sign `url` under `profile` and return the parameters the SDK appended.
 *
 * @param {string} bundle  the webmssdk source
 * @param {object} [profile]
 * @param {string} [profile.userAgent]
 * @param {number} [profile.now]           milliseconds; the frozen clock
 * @param {boolean} [profile.liveClock]    use the real clock instead of a frozen one
 * @param {boolean} [profile.realEntropy]  use real `getRandomValues` instead of the fixed sequence
 * @param {boolean} [profile.noWebgl]      make `getContext` return null, collapsing the fingerprint
 * @param {number} [profile.xmst]          stored token length; 0 for none
 * @param {string} [profile.xmstValue]     the exact stored token, when its content matters
 * @param {string} [profile.cookie]        cookie header for the signing context
 * @param {string} [profile.pageUrl]       `location.href`
 * @param {object} [profile.env]           dotted overrides, e.g. `{'navigator.platform': 'Win32'}`
 * @param {object} [profile.setters]       call the SDK's webid setters before signing
 * @returns {Promise<Record<string, string>>} the appended parameters, by name
 */
export async function signOnce(bundle, profile = {}) {
  // The shim reads these at collection time rather than at construction, so they are switches
  // rather than state — but they are process-wide, which is why a sweep runs one variant per
  // process.
  if (profile.noWebgl) process.env.TTL_NO_WEBGL = '1';
  else delete process.env.TTL_NO_WEBGL;
  if (profile.realEntropy) delete process.env.TTL_DETERMINISTIC;
  else process.env.TTL_DETERMINISTIC = '1';

  const env = createSandbox();
  const w = env.windowTarget;
  const quiet = () => {};
  w.console = Object.fromEntries(['log', 'info', 'warn', 'error', 'debug', 'trace', 'dir', 'table',
    'group', 'groupEnd', 'time', 'timeEnd', 'assert', 'count'].map((k) => [k, quiet]));

  if (!profile.liveClock) {
    const now = profile.now ?? FIXED_NOW;
    class FrozenDate extends Date {
      constructor(...args) { super(...(args.length ? args : [now])); }
      static now() { return now; }
    }
    w.Date = FrozenDate;
    w.performance = { now: () => 1234.5, timeOrigin: now, timing: { navigationStart: now },
      getEntriesByType: () => [] };
    w.Math = Object.create(Math);
    w.Math.random = () => 0.42;
  }

  // A length produces a synthetic token; `xmstValue` supplies an exact one, which is what a
  // content-versus-length differential needs.
  const xmstLength = profile.xmst ?? 124;
  const token = profile.xmstValue ?? (xmstLength > 0 ? 'x'.repeat(xmstLength) : null);
  w.localStorage = {
    getItem: (k) => (k === 'xmst' ? token : null),
    setItem() {}, removeItem() {}, clear() {}, key: () => null, length: 0,
  };
  const cookie = profile.cookie ?? '';
  Object.defineProperty(w.document, 'cookie', { configurable: true, get: () => cookie, set() {} });
  Object.defineProperty(w.navigator, 'userAgent', {
    configurable: true, get: () => profile.userAgent || CHROME_LINUX_UA,
  });
  if (profile.pageUrl) {
    Object.defineProperty(w.location, 'href', { configurable: true, get: () => profile.pageUrl });
  }
  for (const [dotted, value] of Object.entries(profile.env || {})) {
    const [root, key] = dotted.split('.');
    const target = key ? w[root] : w;
    const name = key || root;
    try {
      Object.defineProperty(target, name, { configurable: true, get: () => value, set() {} });
    } catch {
      target[name] = value;
    }
  }

  let captured = null;
  w.fetch = async (input) => {
    captured = typeof input === 'string' ? input : String(input?.url);
    return { ok: true, status: 200, headers: { get: () => null }, text: async () => '',
      json: async () => ({}) };
  };

  const failure = env.load(bundle);
  if (failure) throw new Error(`the bundle failed to load: ${failure.message}`);
  const sdk = w.byted_acrawler;
  await Promise.resolve(sdk.init({ aid: 1988, enablePathList: ['/webcast/'] }));
  // The SDK exposes setTTWid/setTTWebid/setTTWebidV2 and the page calls them. They are measured to
  // populate nothing in `_mssdk._sharedCache` and to move no signature, which is worth being able
  // to re-check rather than remembering.
  if (profile.setters) {
    sdk.setTTWid(profile.setters.ttwid ?? '');
    sdk.setTTWebid(profile.setters.webid ?? '');
    sdk.setTTWebidV2(profile.setters.webid_v2 ?? profile.setters.webid ?? '');
  }
  await Promise.resolve(w.fetch(url(profile), { method: 'GET' }));
  if (!captured) throw new Error('the patched fetch did not sign the request');

  const before = new URL(url(profile)).searchParams;
  const added = {};
  for (const [k, v] of new URL(captured).searchParams) if (!before.has(k)) added[k] = v;
  return added;
}

/// The URL a profile signs. Overridable, because the query is itself one of the inputs.
export function url(profile = {}) {
  return profile.url || process.env.TTL_URL || DEFAULT_URL;
}

export const DEFAULT_URL = 'https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&app_language=en'
  + '&app_name=tiktok_web&browser_language=en-US&browser_name=Mozilla&browser_online=true'
  + '&browser_platform=Linux%20x86_64'
  + `&browser_version=${encodeURIComponent(CHROME_LINUX_UA.slice(8))}`
  + '&cookie_enabled=true&cursor=&device_id=7300000000000000001&device_platform=web&did_rule=3'
  + '&fetch_rule=1&identity=audience&internal_ext=&last_rtt=0&live_id=12&resp_content_type=protobuf'
  + '&room_id=7300000000000000001&screen_height=1080&screen_width=1920&sup_ws_ds_opt=1'
  + '&tz_name=America%2FNew_York&version_code=270000&webcast_language=en';
