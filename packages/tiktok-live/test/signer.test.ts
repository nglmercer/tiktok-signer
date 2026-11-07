// Signing, offline. Needs the bundle, which is TikTok's and deliberately not vendored:
//
//   curl -s -o /tmp/webmssdk.js \
//     https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
//
// Without it these skip rather than fail, so a fresh clone can still run the suite offline.

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { BOOTSTRAP_PATH, PRODUCT, Signer, loadBundle } from '../dist/signer.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BUNDLE = process.env.TTL_BUNDLE ?? '/tmp/webmssdk.js';
const URL_UNDER_TEST =
  'wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?room_id=7300000000000000001&aid=1988';

const bundle = fs.existsSync(BUNDLE) ? fs.readFileSync(BUNDLE, 'utf8') : null;
const bundleSource = bundle ?? undefined;
const needsBundle = { skip: bundle ? false : `no signing bundle at ${BUNDLE}` };

test('a downloaded bundle must match its expected digest', async () => {
  await assert.rejects(
    () => loadBundle({
      url: 'data:text/javascript,wrong',
      cachePath: path.join(os_tmpdir(), 'ttl-deliberately-invalid-bundle.js'),
      expectedSha256: '0'.repeat(64),
    }),
    /did not match its expected SHA-256/,
  );
});

test('a signed socket URL keeps its query and gains a signature', needsBundle, async () => {
  const signer = await Signer.create({ bundleSource, cookie: 'sessionid=test' });
  const signed = signer.sign(URL_UNDER_TEST, PRODUCT.ws);
  assert.ok(signed.startsWith(URL_UNDER_TEST), 'the signed bytes must be the bytes that were signed');
  assert.match(signed, /&X-Gnarly=/);
});

// `registerWsSigner` hands back the signer and deletes itself. A process that signs once and exits
// never notices; a connection that reconnects does, and the second signature fails. This is the
// regression that kept the Rust engine honest and it is the same driver here.
test('a warm signer keeps signing', needsBundle, async () => {
  const signer = await Signer.create({ bundleSource, cookie: 'sessionid=test' });
  const signatures = new Set();
  for (let round = 0; round < 10; round += 1) {
    signatures.add(signer.sign(URL_UNDER_TEST, PRODUCT.ws));
  }
  assert.equal(signatures.size, 10, 'each signature must differ, as it does in a browser');
});

// The frozen profile is what makes a differential mean anything: two runs must agree, and a live
// one must not agree with them.
test('pinning decides whether a signature repeats', needsBundle, async () => {
  const pinned = async () =>
    (await Signer.create({ bundleSource, pinned: true })).sign(URL_UNDER_TEST, PRODUCT.ws);
  assert.equal(await pinned(), await pinned());

  const live = await Signer.create({ bundleSource });
  assert.notEqual(live.sign(URL_UNDER_TEST, PRODUCT.ws), await pinned());
});

test('all three products sign', needsBundle, async () => {
  const signer = await Signer.create({ bundleSource, cookie: 'sessionid=test' });
  for (const product of Object.values(PRODUCT)) {
    const url = product === PRODUCT.ws
      ? URL_UNDER_TEST
      : 'https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001';
    assert.ok(signer.sign(url, product).length > url.length, product);
  }
});

// The sandbox is generated from `scripts/headless/shim.mjs`. Committing it is what lets this
// package be installed without the repository, but it also means an edit to the shim can leave
// this shipping last week's sandbox — signing correctly, and differently.
test('the shipped sandbox matches the shim it is generated from', async (t) => {
  const generator = path.join(HERE, '..', '..', '..', 'scripts', 'headless', 'tools', 'build-bootstrap.mjs');
  if (!fs.existsSync(generator)) {
    t.skip('not inside the repository');
    return;
  }
  const { execFileSync } = await import('node:child_process');
  const temporary = path.join(os_tmpdir(), 'ttl-bootstrap-freshness.js');
  execFileSync(process.execPath, [generator, temporary]);
  assert.equal(
    fs.readFileSync(temporary, 'utf8'),
    fs.readFileSync(BOOTSTRAP_PATH, 'utf8'),
    'vendor/bootstrap.js is stale — run `npm run sync-bootstrap`',
  );
  fs.rmSync(temporary, { force: true });
});

function os_tmpdir() {
  return process.env.TMPDIR ?? '/tmp';
}

test('one bootstrap source preserves mutable context and identity isolation', async () => {
  const source = `window.byted_acrawler = {
    init: function() {},
    registerWsSigner: function() {
      let calls = 0;
      return function() { return {'X-Gnarly': document.cookie + '_' + (++calls)}; };
    }
  };`;
  const a = await Signer.create({ bundleSource: source, cookie: 'sessionid=first' });
  const b = await Signer.create({ bundleSource: source, cookie: 'sessionid=second' });
  const first = a.sign(URL_UNDER_TEST, PRODUCT.ws);
  const second = b.sign(URL_UNDER_TEST, PRODUCT.ws);
  assert.match(first, /first.*1/);
  assert.match(second, /second.*1/);
  assert.match(a.sign(URL_UNDER_TEST, PRODUCT.ws), /first.*2/);
});

test('embedded pending timer retention has an explicit capacity and supports cleanup', async () => {
  const { newQuickJSWASMModuleFromVariant } = await import('quickjs-emscripten-core');
  const quickjs = await newQuickJSWASMModuleFromVariant(import('@jitl/quickjs-wasmfile-release-sync'));
  const context = quickjs.newContext();
  try {
    const run = (code: string): unknown => {
      const result = context.evalCode(code);
      if (result.error) {
        const error = context.dump(result.error);
        result.error.dispose();
        throw new Error(String((error as { message?: unknown })?.message ?? error));
      }
      const value: unknown = context.dump(result.value);
      result.value.dispose();
      return value;
    };
    run(fs.readFileSync(BOOTSTRAP_PATH, 'utf8'));
    run('for (let i = 0; i < 1024; i++) setTimeout(() => {}, 1000)');
    assert.equal(run('__ttlPendingTimers.size'), 1024);
    assert.throws(() => run('setTimeout(() => {}, 1000)'), /timer capacity exceeded/);
    run('clearTimeout(1); setTimeout(() => {}, 1000)');
    assert.equal(run('__ttlPendingTimers.size'), 1024);
    run('for (const id of __ttlPendingTimers.keys()) clearTimeout(id)');
    assert.equal(run('__ttlPendingTimers.size'), 0);
  } finally {
    context.dispose();
  }
});

test('the signing context holds nothing of Node', async () => {
  const source = `window.byted_acrawler = {
    init: function() {},
    registerWsSigner: function() {
      const seen = [typeof process, typeof require, typeof Buffer, typeof globalThis.fetch].join('.');
      return function() { return {'X-Gnarly': seen}; };
    }
  };`;
  const signer = await Signer.create({ bundleSource: source });
  try {
    assert.match(signer.sign(URL_UNDER_TEST, PRODUCT.ws), /X-Gnarly=undefined\.undefined\.undefined\./);
  } finally {
    signer.close();
  }
});

test('a wedged bundle is interrupted rather than hanging the process', async () => {
  const started = Date.now();
  await assert.rejects(() => Signer.create({ bundleSource: 'while (true) {}', timeoutMs: 200 }), /failed to load/);
  assert.ok(Date.now() - started < 10_000);

  const wedges = `window.byted_acrawler = {
    init: function() {},
    registerWsSigner: function() { return function() { while (true) {} }; }
  };`;
  const signer = await Signer.create({ bundleSource: wedges, timeoutMs: 200 });
  try {
    assert.throws(() => signer.sign(URL_UNDER_TEST, PRODUCT.ws), /signing failed/);
  } finally {
    signer.close();
  }
});

test('a closed signer refuses to sign, and closing twice is harmless', async () => {
  const source = `window.byted_acrawler = { init: function() {},
    registerWsSigner: function() { return function() { return {'X-Gnarly': 'ok'}; }; } };`;
  const signer = await Signer.create({ bundleSource: source });
  assert.match(signer.sign(URL_UNDER_TEST, PRODUCT.ws), /X-Gnarly=ok/);
  signer.close();
  signer.close();
  assert.throws(() => signer.sign(URL_UNDER_TEST, PRODUCT.ws), /closed/);
});

// The recorded digests are of what V8 signed before it left the repository; QuickJS must match.
test('pinned signatures match the recorded ones byte for byte', needsBundle, async (t) => {
  const recorded = JSON.parse(fs.readFileSync(
    path.resolve(HERE, '../../../fixtures/research/pinned-signatures-1.0.0.388.json'), 'utf8')) as {
    bundle_sha256: string;
    cases: { product: keyof typeof PRODUCT; url: string; signed_sha256: string; signed_length: number }[];
  };
  const sha256 = (text: string) => createHash('sha256').update(text).digest('hex');
  if (sha256(bundle ?? '') !== recorded.bundle_sha256) {
    t.skip('the bundle is not the one the signatures were recorded from');
    return;
  }
  for (const recordedCase of recorded.cases) {
    const signer = await Signer.create({ bundleSource, pinned: true });
    try {
      const signed = signer.sign(recordedCase.url, PRODUCT[recordedCase.product]);
      assert.equal(signed.length, recordedCase.signed_length, `${recordedCase.product} length`);
      assert.equal(sha256(signed), recordedCase.signed_sha256, `${recordedCase.product} differs`);
    } finally {
      signer.close();
    }
  }
});

test('sandbox access diagnostics are bounded without changing property reads', async () => {
  const shimPath = path.resolve(HERE, '../../../scripts/headless/shim.mjs');
  const { createSandbox } = await import(shimPath);
  const sandbox = createSandbox();
  for (let i = 0; i < 5000; i++) assert.equal(sandbox.sandbox[`unknown${i}`], undefined);
  assert.equal(sandbox.accesses.size, 4096);
  assert.equal(sandbox.omittedAccesses, 904);
});

test('concurrent bundle loads share a download and release the flight after failure', async () => {
  const original = globalThis.fetch;
  let downloads = 0;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  globalThis.fetch = async () => {
    downloads += 1;
    await gate;
    return new Response('test bundle');
  };
  const options = { url: 'https://example.invalid/bundle', cachePath: `/missing-dir/ttl-bundle-${process.pid}`, maxAgeMs: 0 };
  try {
    const first = loadBundle(options);
    const second = loadBundle(options);
    assert.equal(first, second);
    release();
    assert.equal(await first, 'test bundle');
    assert.equal(downloads, 1);
    globalThis.fetch = async () => { downloads += 1; throw new Error('offline failure'); };
    await assert.rejects(loadBundle(options), /offline failure/);
    await assert.rejects(loadBundle(options), /offline failure/);
    assert.equal(downloads, 3);
  } finally {
    globalThis.fetch = original;
  }
});

test('a blocked bundle download has a deadline and releases its single-flight entry', async () => {
  const original = globalThis.fetch;
  let calls = 0;
  globalThis.fetch = async (_url, init) => {
    calls++;
    const signal = init?.signal;
    return new Promise<Response>((_resolve, reject) => {
      signal!.addEventListener('abort', () => reject(signal!.reason), { once: true });
    });
  };
  const keepAlive = setTimeout(() => {}, 1000);
  try {
    const options = { url: 'https://example.invalid/blocked', cachePath: '/missing-dir/blocked.js', timeoutMs: 5 };
    await assert.rejects(loadBundle(options), /timeout/i);
    await assert.rejects(loadBundle(options), /timeout/i);
    assert.equal(calls, 2);
  } finally { clearTimeout(keepAlive); globalThis.fetch = original; }
});
