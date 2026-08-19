// Signing, in this process, with no native module and no server.
//
// The socket URL's query is signed by TikTok's own `webmssdk` bundle, and the bundle is
// JavaScript — so Node needs no engine embedded in it, unlike the Rust build, which had to link
// QuickJS or V8 precisely because Rust has none. What the bundle does need is a browser-shaped
// environment, and `vendor/bootstrap.js` is exactly that: the sandbox from `scripts/headless`,
// flattened into one script that installs its own platform. The same file runs in the Rust engines,
// and `crates/ttl-sign-embedded/tests/parity.rs` asserts all of them produce identical bytes.
//
// The context is bare on purpose — `vm.createContext({})` — so the bundle sees the sandbox and
// nothing of Node.
//
// Two things a caller must not have to know:
//
//   * `registerWsSigner` is a **one-shot**: it hands back the signer and deletes itself, so the
//     second signature of a long-lived connection fails unless the first one is kept. The
//     bootstrap's driver caches it; this class keeps one context alive so the cache survives.
//   * loading the bundle costs about 60 ms and signing costs about 3 ms, so the context is warm
//     and reused. A signature per reconnect is then free.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

import { USER_AGENT } from './session.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));

/// The sandbox. Generated from `scripts/headless/shim.mjs`; see `npm run sync-bootstrap`.
export const BOOTSTRAP_PATH = path.join(HERE, '..', 'vendor', 'bootstrap.js');

/// The bundle is TikTok's, a public static asset, and deliberately not vendored. This is the
/// version this package was measured against; `player-audit.mjs` in the repository is what notices
/// when the player moves to another one.
export const BUNDLE_URL =
  'https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js';

/// Which signature to produce. The socket verifies `ws` and ignores the other two.
export const PRODUCT = Object.freeze({ fetch: 'fetch', frontier: 'frontier', ws: 'ws' });

/// Fetch the bundle once and keep it in the temp directory.
///
/// Downloading is what a browser does, and it keeps the library to `npm install` with no manual
/// step. A cached copy is reused for `maxAgeMs` so a reconnect loop cannot turn into a download
/// loop; pass `bundlePath` or `bundleSource` to the signer to skip this entirely.
export async function loadBundle({
  url = BUNDLE_URL,
  cachePath = path.join(os.tmpdir(), 'ttl-webmssdk.js'),
  maxAgeMs = 24 * 60 * 60 * 1000,
} = {}) {
  try {
    const stat = fs.statSync(cachePath);
    if (Date.now() - stat.mtimeMs < maxAgeMs && stat.size > 0) {
      return fs.readFileSync(cachePath, 'utf8');
    }
  } catch {
    // No cache yet.
  }
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`could not download the signing bundle: HTTP ${response.status} from ${url}`);
  }
  const source = await response.text();
  try {
    fs.writeFileSync(cachePath, source);
  } catch {
    // A read-only temp directory is not a reason to fail a signature.
  }
  return source;
}

/// A warm signing context.
export class Signer {
  #context;

  constructor(context) {
    this.#context = context;
  }

  /// Load a bundle into a fresh context and prepare it.
  ///
  /// `cookie` matters: the sandbox reports it to the bundle as `document.cookie`, and the socket
  /// refuses a handshake whose signature was produced without the session it presents.
  static async create({
    bundleSource,
    bundlePath,
    bundleUrl = BUNDLE_URL,
    userAgent = USER_AGENT,
    cookie = '',
    storedToken,
    pinned = false,
  } = {}) {
    const source =
      bundleSource ??
      (bundlePath ? fs.readFileSync(bundlePath, 'utf8') : await loadBundle({ url: bundleUrl }));

    const context = vm.createContext({});
    vm.runInContext(fs.readFileSync(BOOTSTRAP_PATH, 'utf8'), context);

    const options = { pinned, userAgent, cookie };
    if (storedToken) options.xmst = storedToken;
    const prepared = JSON.parse(
      vm.runInContext('ttlPrepare', context)(source, JSON.stringify(options)),
    );
    if (prepared.error) throw new Error(`the signing bundle failed to load: ${prepared.error}`);
    return new Signer(context);
  }

  /// Sign one URL. Returns the signed URL, with its query bytes untouched.
  sign(url, product = PRODUCT.ws) {
    const answer = JSON.parse(vm.runInContext('ttlSignUrl', this.#context)(url, product));
    if (answer.error) throw new Error(`signing failed: ${answer.error}`);
    if (!answer.signed) throw new Error('the signer produced no URL');
    return answer.signed;
  }
}
