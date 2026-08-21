// The stored session, the cookie jar, and the user agent that goes with them.
//
// Every probe in this directory needs the same four things: a browser user agent, the cookies from
// the session file, a header built from a jar, and a way to absorb `Set-Cookie` from a response.
// Each one had grown its own copy — identical bodies, ten times over — which is how two of them
// ended up reading a different session path than the rest, and why a fix to one never reached the
// others.
//
// One implementation, here. Values are returned; nothing is printed, because a cookie must never
// reach a log.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

/// The user agent every probe presents, and the one the signing environment reports.
///
/// It is a single constant because it is a *matched pair* with the `browser_*` query fields: a
/// probe that sends one and describes another is testing a browser that does not exist.
export const USER_AGENT = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) '
  + 'Chrome/131.0.0.0 Safari/537.36';

/// The Windows agent, for probes that vary the platform deliberately.
export const WINDOWS_USER_AGENT = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) '
  + 'AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';

/// Where the session file lives. `TTL_SESSION_FILE` overrides it, as the Rust side expects.
export function sessionPath(): string {
  if (process.env.TTL_SESSION_FILE) return process.env.TTL_SESSION_FILE;
  const base = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, 'ttl-signer', 'session');
}

/// Parse a `k=v; k=v` cookie header into a jar.
export function parseCookies(raw: string): Map<string, string> {
  const jar = new Map<string, string>();
  for (const part of String(raw).split(';')) {
    const eq = part.indexOf('=');
    if (eq > 0) jar.set(part.slice(0, eq).trim(), part.slice(eq + 1).trim());
  }
  return jar;
}

/// The stored session as a jar. Empty when there is no session file, which is a valid state:
/// discovery works as a guest, and only the socket requires cookies.
export function sessionJar(): Map<string, string> {
  try {
    return parseCookies(fs.readFileSync(sessionPath(), 'utf8').trim());
  } catch {
    return new Map<string, string>();
  }
}

/// A `Cookie` header from a jar, optionally with extra pairs appended.
export function cookieHeader(
  jar: ReadonlyMap<string, string>,
  extra: Readonly<Record<string, string>> = {},
): string {
  return [
    ...[...jar].map(([name, value]) => `${name}=${value}`),
    ...Object.entries(extra).map(([name, value]) => `${name}=${value}`),
  ].join('; ');
}

/// Fold a response's `Set-Cookie` headers into a jar, so a probe keeps what a page issued it.
///
/// Returns the cookie *names* it absorbed — never the values, which is what a probe wants to print
/// when it reports what an endpoint handed back.
interface CookieHeaders {
  getSetCookie?: () => string[];
}

interface CookieResponse {
  headers?: CookieHeaders;
}

export function absorbCookies(jar: Map<string, string>, response: CookieResponse): string[] {
  const lines = response.headers?.getSetCookie ? response.headers.getSetCookie() : [];
  const names = [];
  for (const line of lines) {
    const pair = line.split(';', 1)[0];
    if (pair === undefined) continue;
    const eq = pair.indexOf('=');
    if (eq > 0) {
      const name = pair.slice(0, eq).trim();
      jar.set(name, pair.slice(eq + 1));
      names.push(name);
    }
  }
  return names;
}
