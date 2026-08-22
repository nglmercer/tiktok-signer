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

/// A normal anonymous web visit establishes this identity; it is not an account session.
export const GUEST_BOOTSTRAP_URL = 'https://www.tiktok.com/live';

export type GuestSessionErrorCode =
  | 'BOOTSTRAP_NO_COOKIES'
  | 'RATE_LIMIT_OR_VERIFICATION'
  | 'WS_HANDSHAKE_REJECTED'
  | 'UNKNOWN';

export class GuestSessionError extends Error {
  readonly code: GuestSessionErrorCode;
  readonly status?: number;
  readonly closeCode?: number;

  constructor(
    code: GuestSessionErrorCode,
    message: string,
    status?: number,
    closeCode?: number,
  ) {
    super(message);
    this.name = 'GuestSessionError';
    this.code = code;
    this.status = status;
    this.closeCode = closeCode;
  }
}

export interface SessionIdentity {
  cookie: string;
  authenticated: boolean;
  deviceId?: string;
}

export interface GuestSessionOptions {
  userAgent?: string;
  url?: string;
  timeoutMs?: number;
}

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
/// the client will bootstrap a memory-only anonymous jar before opening the socket.
export function sessionJar(): Map<string, string> {
  try {
    return parseCookies(fs.readFileSync(sessionPath(), 'utf8').trim());
  } catch {
    return new Map<string, string>();
  }
}

/// Create the anonymous TikTok identity used by the direct message socket.
///
/// The endpoint is an ordinary public web navigation. TikTok currently issues `ttwid` (plus
/// short-lived companion cookies) there; the direct socket accepts that identity without an
/// account `sessionid`. Keep the jar in memory and return only the serialized header needed by
/// discovery, signing, and the socket.
export async function bootstrapGuestSession({
  userAgent = USER_AGENT,
  url = GUEST_BOOTSTRAP_URL,
  timeoutMs = 15_000,
}: GuestSessionOptions = {}): Promise<SessionIdentity> {
  let response: Response;
  try {
    response = await fetch(url, {
      headers: {
        'user-agent': userAgent,
        'accept-language': 'en-US,en;q=0.9',
      },
      signal: AbortSignal.timeout(timeoutMs),
    });
  } catch {
    throw new GuestSessionError(
      'UNKNOWN',
      'anonymous TikTok guest bootstrap did not complete',
    );
  }

  const jar = new Map<string, string>();
  absorbCookies(jar, response);
  // Drain the navigation before using the identity. This also makes the helper work with fetch
  // implementations that defer connection cleanup until the response body is consumed.
  await response.arrayBuffer();

  if (response.status === 403 || response.status === 429) {
    throw new GuestSessionError(
      'RATE_LIMIT_OR_VERIFICATION',
      `TikTok refused anonymous guest bootstrap (HTTP ${response.status})`,
      response.status,
    );
  }
  if (!response.ok) {
    throw new GuestSessionError(
      'UNKNOWN',
      `TikTok anonymous guest bootstrap returned HTTP ${response.status}`,
      response.status,
    );
  }
  if (!jar.has('ttwid')) {
    throw new GuestSessionError(
      'BOOTSTRAP_NO_COOKIES',
      'TikTok anonymous guest bootstrap returned no usable guest identity',
      response.status,
    );
  }

  const deviceId = jar.get('tt_webid_v2') || jar.get('tt_webid');
  return {
    cookie: cookieHeader(jar),
    authenticated: false,
    ...(deviceId ? { deviceId } : {}),
  };
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
