// Signing, in this process, with no native module and no server.
//
// The socket URL's query is signed by TikTok's own `webmssdk` bundle. The bundle runs in QuickJS —
// the same engine every other part of this project signs with — compiled to WebAssembly
// (`quickjs-emscripten`), so there is still nothing native to build or install. What the bundle
// needs is a browser-shaped environment, and `vendor/bootstrap.js` is exactly that: the sandbox
// from `scripts/headless`, flattened into one script that installs its own platform. The same file
// runs in the Rust engine, and both are checked byte for byte against the signatures recorded in
// `fixtures/research/pinned-signatures-1.0.0.388.json`.
//
// The context is a fresh QuickJS runtime with nothing of Node in it: the bundle sees the sandbox,
// and its only way out is `__ttl_random_bytes`, the entropy source the bootstrap expects from a
// host without `crypto`. Every call into it runs under a deadline, and the runtime has a memory
// ceiling, so a hostile or wedged bundle is interrupted instead of hanging the process.
//
// Two things a caller must not have to know:
//
//   * `registerWsSigner` is a **one-shot**: it hands back the signer and deletes itself, so the
//     second signature of a long-lived connection fails unless the first one is kept. The
//     bootstrap's driver caches it; this class keeps one context alive so the cache survives.
//   * loading the bundle costs about 150 ms and signing tens of milliseconds, so the context is
//     warm and reused. A signature per reconnect is then cheap.
//
// A QuickJS context lives in WebAssembly memory, not on Node's heap: call `close()` when a signer
// is no longer needed. A collected signer is released too, but later.

import fs from 'node:fs';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  newQuickJSWASMModuleFromVariant,
  shouldInterruptAfterDeadline,
  type QuickJSContext,
  type QuickJSHandle,
  type QuickJSRuntime,
  type QuickJSWASMModule,
} from 'quickjs-emscripten-core';

import { readResponseText } from './http.js';
import { USER_AGENT } from './session.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));

/// The sandbox. Generated from `scripts/headless/shim.mjs`; see `npm run sync-bootstrap`.
export const BOOTSTRAP_PATH = path.join(HERE, '..', 'vendor', 'bootstrap.js');

/// The bundle is TikTok's, a public static asset, and deliberately not vendored. This is the
/// version this package was measured against; `player-audit.mjs` in the repository is what notices
/// when the player moves to another one.
export const BUNDLE_URL =
  'https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js';
export const BUNDLE_SHA256 = 'dee22566273d398e074df6db40f39cfd827f6b8efd6fc382de03c44c501299ac';

const VM_TIMEOUT_MS = 10_000;
/// Heap ceiling for one signing context. The bundle uses a few megabytes.
const VM_MEMORY_LIMIT = 256 * 1024 * 1024;
/// The most entropy one draw may return; matches `MAX_RANDOM_BYTES` in the Rust engine.
const MAX_RANDOM_BYTES = 8192;

// One WebAssembly module and one bootstrap source, shared by every context.
let engineModule: Promise<QuickJSWASMModule> | undefined;
function engine(): Promise<QuickJSWASMModule> {
  return engineModule ??= newQuickJSWASMModuleFromVariant(
    // A dynamic import: the variant's typings are CommonJS-shaped, and this form is accepted either way.
    import('@jitl/quickjs-wasmfile-release-sync'),
  ).catch((error: unknown) => {
    engineModule = undefined;
    throw error;
  });
}
let bootstrapSource: string | undefined;
function bootstrap(): string {
  return bootstrapSource ??= fs.readFileSync(BOOTSTRAP_PATH, 'utf8');
}

/// Which signature to produce. The socket verifies `ws` and ignores the other two.
export const PRODUCT = Object.freeze({ fetch: 'fetch', frontier: 'frontier', ws: 'ws' } as const);
export type Product = (typeof PRODUCT)[keyof typeof PRODUCT];

export interface LoadBundleOptions {
  timeoutMs?: number;
  url?: string;
  cachePath?: string;
  maxAgeMs?: number;
  expectedSha256?: string;
}

export interface SignerOptions {
  bundleSource?: string;
  bundlePath?: string;
  bundleUrl?: string;
  userAgent?: string;
  cookie?: string;
  storedToken?: string;
  pinned?: boolean;
  /// The longest one call into the engine — loading the bundle, or one signature — may run.
  timeoutMs?: number;
}

/// Fetch the bundle once and keep it in the temp directory.
///
/// Downloading is what a browser does, and it keeps the library to `npm install` with no manual
/// step. A cached copy is reused for `maxAgeMs` so a reconnect loop cannot turn into a download
/// loop; pass `bundlePath` or `bundleSource` to the signer to skip this entirely.
const bundleFlights = new Map<string, Promise<string>>();
export function loadBundle(options: LoadBundleOptions = {}): Promise<string> {
  const key = JSON.stringify([
    options.url ?? BUNDLE_URL,
    options.cachePath ?? path.join(os.tmpdir(), 'ttl-webmssdk.js'),
    options.maxAgeMs ?? 24 * 60 * 60 * 1000,
    options.timeoutMs ?? 30_000,
    options.expectedSha256 ?? (options.url === undefined || options.url === BUNDLE_URL ? BUNDLE_SHA256 : null),
  ]);
  const existing = bundleFlights.get(key);
  if (existing) return existing;
  if (bundleFlights.size >= 16) return Promise.reject(new Error('signing bundle download capacity exhausted'));
  const flight = readBundle(options).finally(() => { bundleFlights.delete(key); });
  bundleFlights.set(key, flight);
  return flight;
}

async function readBundle({
  timeoutMs = 30_000,
  url = BUNDLE_URL,
  cachePath = path.join(os.tmpdir(), 'ttl-webmssdk.js'),
  maxAgeMs = 24 * 60 * 60 * 1000,
  expectedSha256 = url === BUNDLE_URL ? BUNDLE_SHA256 : undefined,
}: LoadBundleOptions = {}): Promise<string> {
  try {
    const stat = await fs.promises.stat(cachePath);
    if (Date.now() - stat.mtimeMs < maxAgeMs && stat.size > 0) {
      const cached = await fs.promises.readFile(cachePath, 'utf8');
      if (matchesDigest(cached, expectedSha256)) return cached;
    }
  } catch {
    // No cache yet.
  }
  const response = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
  if (!response.ok) {
    throw new Error(`could not download the signing bundle: HTTP ${response.status} from ${url}`);
  }
  const source = await readResponseText(response, 8 * 1024 * 1024);
  if (!matchesDigest(source, expectedSha256)) {
    throw new Error(`the signing bundle from ${url} did not match its expected SHA-256`);
  }
  const temporary = `${cachePath}.${process.pid}.${randomUUID()}.tmp`;
  try {
    await fs.promises.writeFile(temporary, source, { mode: 0o600 });
    await fs.promises.rename(temporary, cachePath);
  } catch {
    // A read-only temp directory is not a reason to fail a signature.
    try {
      await fs.promises.rm(temporary, { force: true });
    } catch {
      // Nothing was written.
    }
  }
  return source;
}

function matchesDigest(source: string, expectedSha256?: string): boolean {
  if (!expectedSha256) return true;
  return createHash('sha256').update(source).digest('hex') === expectedSha256.toLowerCase();
}

interface Engine {
  runtime: QuickJSRuntime;
  context: QuickJSContext;
  timeoutMs: number;
}

function release(engine: Engine): void {
  try { engine.context.dispose(); } catch { /* Already released. */ }
  try { engine.runtime.dispose(); } catch { /* Already released. */ }
}

// A signer that is collected without `close()` still gives its WebAssembly memory back.
const released = new FinalizationRegistry<Engine>(release);

/// Run `call` with the runtime's interrupt armed for `timeoutMs`.
function bounded<T>(runtime: QuickJSRuntime, call: () => T, timeoutMs = VM_TIMEOUT_MS): T {
  runtime.setInterruptHandler(shouldInterruptAfterDeadline(Date.now() + timeoutMs));
  try {
    return call();
  } finally {
    runtime.removeInterruptHandler();
  }
}

/// Call a global function with string arguments and read its string answer.
function callGlobal(context: QuickJSContext, name: string, ...args: string[]): string {
  const fn = context.getProp(context.global, name);
  const handles = args.map((arg) => context.newString(arg));
  try {
    const result = context.callFunction(fn, context.undefined, ...handles);
    if (result.error) {
      const error: unknown = context.dump(result.error);
      result.error.dispose();
      throw new Error(describe(error));
    }
    const value = result.value;
    try {
      if (context.typeof(value) !== 'string') throw new Error(`${name} returned no string`);
      return context.getString(value);
    } finally {
      value.dispose();
    }
  } finally {
    fn.dispose();
    for (const handle of handles) handle.dispose();
  }
}

function describe(error: unknown): string {
  if (isRecord(error) && typeof error.message === 'string') return error.message;
  return String(error);
}

/// A fresh QuickJS context with the sandbox installed.
function startEngine(module: QuickJSWASMModule, timeoutMs: number): Engine {
  const runtime = module.newRuntime();
  runtime.setMemoryLimit(VM_MEMORY_LIMIT);
  let context: QuickJSContext | undefined;
  try {
    context = runtime.newContext();
    const ctx = context;
    // QuickJS has no `crypto`; the bootstrap wires its random source to this before anything runs.
    const random = ctx.newFunction('__ttl_random_bytes', (count: QuickJSHandle) => {
      const requested = ctx.getNumber(count);
      const length = Number.isFinite(requested) ? Math.max(0, Math.min(MAX_RANDOM_BYTES, Math.floor(requested))) : 0;
      const bytes = randomBytes(length);
      const array = ctx.newArray();
      for (let at = 0; at < length; at += 1) {
        const value = ctx.newNumber(bytes[at] ?? 0);
        ctx.setProp(array, at, value);
        value.dispose();
      }
      return array;
    });
    ctx.setProp(ctx.global, '__ttl_random_bytes', random);
    random.dispose();
    bounded(runtime, () => {
      const result = ctx.evalCode(bootstrap(), BOOTSTRAP_PATH);
      if (result.error) {
        const error: unknown = ctx.dump(result.error);
        result.error.dispose();
        throw new Error(`the signing sandbox failed to start: ${describe(error)}`);
      }
      result.value.dispose();
    }, timeoutMs);
    return { runtime, context: ctx, timeoutMs };
  } catch (error) {
    try { context?.dispose(); } catch { /* Partially built. */ }
    runtime.dispose();
    throw error;
  }
}

/// A warm signing context.
export class Signer {
  #engine: Engine | null;

  private constructor(engine: Engine) {
    this.#engine = engine;
    released.register(this, engine, this);
  }

  /// Load a bundle into a fresh context and prepare it.
  ///
  /// `cookie` is exposed to the bundle as `document.cookie` so the signing environment matches the
  /// network identity. Signature validity and WebSocket identity acceptance remain separate checks.
  static async create({
    bundleSource,
    bundlePath,
    bundleUrl = BUNDLE_URL,
    userAgent = USER_AGENT,
    cookie = '',
    storedToken,
    pinned = false,
    timeoutMs = VM_TIMEOUT_MS,
  }: SignerOptions = {}): Promise<Signer> {
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) throw new RangeError('timeoutMs must be a positive number');
    const source =
      bundleSource ??
      (bundlePath ? await fs.promises.readFile(bundlePath, 'utf8') : await loadBundle({ url: bundleUrl }));

    const started = startEngine(await engine(), timeoutMs);
    try {
      const options: { pinned: boolean; userAgent: string; cookie: string; xmst?: string } = {
        pinned,
        userAgent,
        cookie,
      };
      if (storedToken) options.xmst = storedToken;
      let encoded: string;
      try {
        encoded = bounded(started.runtime, () =>
          callGlobal(started.context, 'ttlPrepare', source, JSON.stringify(options)), timeoutMs);
      } catch (error) {
        throw new Error(`the signing bundle failed to load: ${describe(error)}`);
      }
      const prepared = parseAnswer(encoded);
      if (prepared.error) throw new Error(`the signing bundle failed to load: ${prepared.error}`);
    } catch (error) {
      release(started);
      throw error;
    }
    return new Signer(started);
  }

  /// Sign one URL. Returns the signed URL, with its query bytes untouched.
  sign(url: string, product: Product = PRODUCT.ws): string {
    const engine = this.#engine;
    if (!engine) throw new Error('the signer is closed');
    let encoded: string;
    try {
      encoded = bounded(
        engine.runtime,
        () => callGlobal(engine.context, 'ttlSignUrl', url, product),
        engine.timeoutMs,
      );
    } catch (error) {
      throw new Error(`signing failed: ${describe(error)}`);
    }
    const answer = parseAnswer(encoded);
    if (answer.error) throw new Error(`signing failed: ${answer.error}`);
    if (!answer.signed) throw new Error('the signer produced no URL');
    return answer.signed;
  }

  /// Release the context. Idempotent; signing afterwards throws.
  close(): void {
    const engine = this.#engine;
    if (!engine) return;
    this.#engine = null;
    released.unregister(this);
    release(engine);
  }
}

interface SignerAnswer {
  error?: string;
  signed?: string;
}

function parseAnswer(encoded: string): SignerAnswer {
  const value: unknown = JSON.parse(encoded);
  if (!isRecord(value)) {
    throw new Error('the signer returned a malformed response');
  }
  return {
    ...(typeof value.error === 'string' ? { error: value.error } : {}),
    ...(typeof value.signed === 'string' ? { signed: value.signed } : {}),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
