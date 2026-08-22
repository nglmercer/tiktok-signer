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
      () => bootstrapGuestSession(),
      (error: unknown) => error instanceof GuestSessionError
        && error.code === 'BOOTSTRAP_NO_COOKIES'
        && !error.message.includes('guest-value'),
    );
  });
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

async function withFetch(response: Response, callback: () => Promise<void>): Promise<void> {
  const previous = globalThis.fetch;
  globalThis.fetch = async () => response;
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
