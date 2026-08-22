import assert from 'node:assert/strict';
import test from 'node:test';

import {
  GuestSessionError,
  bootstrapGuestSession,
} from '../dist/session.js';

test('guest bootstrap absorbs cookie names without requiring sessionid', async () => {
  const response = fakeResponse(200, [
    'tt_chain_token=chain-value; Path=/; Domain=.tiktok.com',
    'tt_csrf_token=csrf-value; Path=/; Domain=.tiktok.com',
    'ttwid=guest-value; Path=/; Domain=.tiktok.com',
  ]);
  await withFetch(response, async () => {
    const identity = await bootstrapGuestSession();
    assert.equal(identity.authenticated, false);
    assert.equal(identity.deviceId, undefined);
    assert.equal(identity.cookie, 'tt_chain_token=chain-value; tt_csrf_token=csrf-value; ttwid=guest-value');
    assert.doesNotMatch(identity.cookie, /sessionid/);
  });
});

test('guest bootstrap reports an unusable anonymous response', async () => {
  await withFetch(fakeResponse(200, []), async () => {
    await assert.rejects(
      () => bootstrapGuestSession({ retries: 0 }),
      (error: unknown) => error instanceof GuestSessionError
        && error.code === 'BOOTSTRAP_NO_COOKIES'
        && !error.message.includes('guest-value'),
    );
  });
});

test('guest bootstrap rejects an empty ttwid value', async () => {
  await withFetch(fakeResponse(200, ['ttwid=; Path=/; Domain=.tiktok.com']), async () => {
    await assert.rejects(
      () => bootstrapGuestSession({ retries: 0 }),
      (error: unknown) => error instanceof GuestSessionError
        && error.code === 'BOOTSTRAP_NO_COOKIES',
    );
  });
});

test('guest bootstrap retries a no-cookie response within a bounded budget', async () => {
  let calls = 0;
  await withFetch(() => {
    calls += 1;
    return fakeResponse(
      200,
      calls === 3 ? ['ttwid=guest-value; Path=/; Domain=.tiktok.com'] : [],
    );
  }, async () => {
    const identity = await bootstrapGuestSession({ retries: 2, retryDelayMs: 0 });
    assert.equal(identity.cookie, 'ttwid=guest-value');
  });
  assert.equal(calls, 3);
});

test('guest bootstrap classifies TikTok rate limiting separately', async () => {
  await withFetch(fakeResponse(429, []), async () => {
    await assert.rejects(
      () => bootstrapGuestSession(),
      (error: unknown) => error instanceof GuestSessionError
        && error.code === 'RATE_LIMIT_OR_VERIFICATION',
    );
  });
});

async function withFetch(
  response: Response | (() => Response),
  callback: () => Promise<void>,
): Promise<void> {
  const previous = globalThis.fetch;
  globalThis.fetch = async () => typeof response === 'function' ? response() : response;
  try {
    await callback();
  } finally {
    globalThis.fetch = previous;
  }
}

function fakeResponse(status: number, setCookies: string[]): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { getSetCookie: () => setCookies },
    arrayBuffer: async () => new ArrayBuffer(0),
  } as unknown as Response;
}
