import assert from 'node:assert/strict';
import test from 'node:test';

import { TikTokLive } from '../dist/client.js';
import { GuestSessionError } from '../dist/session.js';
import type { WebSocketConstructor, WebSocketOptions } from '../dist/types.js';

test('the supplied session drives both the handshake and device query', async () => {
  let unsignedUrl = '';
  let handshake: WebSocketOptions | undefined;
  let socketCount = 0;

  class FakeWebSocket {
    binaryType = '';
    readonly #listeners = new Map<string, Function[]>();

    constructor(_url: string, options: WebSocketOptions) {
      socketCount += 1;
      handshake = options;
      queueMicrotask(() => this.#dispatch('open'));
    }

    send(_data: string | ArrayBuffer | ArrayBufferView): void {}
    close(): void { this.#dispatch('close', { code: 1000, reason: 'done' }); }

    addEventListener(type: 'open', listener: () => void): void;
    addEventListener(
      type: 'message',
      listener: (event: { data: ArrayBuffer | ArrayBufferView }) => void,
    ): void;
    addEventListener(type: 'error', listener: (event: { message?: string }) => void): void;
    addEventListener(
      type: 'close',
      listener: (event: { code: number; reason?: string }) => void,
    ): void;
    addEventListener(type: string, listener: Function): void {
      const listeners = this.#listeners.get(type) ?? [];
      listeners.push(listener);
      this.#listeners.set(type, listeners);
    }

    #dispatch(type: string, event?: unknown): void {
      for (const listener of this.#listeners.get(type) ?? []) listener(event);
    }
  }

  const live = new TikTokLive('@someone', {
    roomId: '7300000000000000001',
    sessionCookie: 'sessionid=test; tt_webid_v2=7999000000000000001',
    fetchGifts: false,
    fetchRoomInfo: false,
    signer: {
      sign(url) {
        unsignedUrl = url;
        return `${url}&X-Gnarly=test`;
      },
    },
    WebSocketImpl: FakeWebSocket as WebSocketConstructor,
  });

  const firstConnect = live.connect();
  const secondConnect = live.connect();
  assert.equal(secondConnect, firstConnect, 'concurrent callers must share one connection attempt');
  await firstConnect;
  assert.equal(socketCount, 1);
  assert.match(unsignedUrl, /device_id=7999000000000000001/);
  assert.equal(handshake?.headers.cookie, 'sessionid=test; tt_webid_v2=7999000000000000001');
  live.disconnect();
});

test('no supplied session bootstraps a guest jar and reuses its device identity', async () => {
  let unsignedUrl = '';
  let handshake: WebSocketOptions | undefined;
  const previousFetch = globalThis.fetch;
  const previousSessionFile = process.env.TTL_SESSION_FILE;
  process.env.TTL_SESSION_FILE = `/tmp/ttl-live-missing-session-${process.pid}`;
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    headers: {
      getSetCookie: () => [
        'ttwid=guest-value; Path=/; Domain=.tiktok.com',
        'tt_webid_v2=7999000000000000001; Path=/; Domain=.tiktok.com',
      ],
    },
    arrayBuffer: async () => new ArrayBuffer(0),
  } as unknown as Response);

  class FakeWebSocket {
    binaryType = '';
    readonly #listeners = new Map<string, Function[]>();

    constructor(url: string, options: WebSocketOptions) {
      unsignedUrl = url;
      handshake = options;
      queueMicrotask(() => this.#dispatch('open'));
    }

    send(_data: string | ArrayBuffer | ArrayBufferView): void {}
    close(): void { this.#dispatch('close', { code: 1000, reason: 'done' }); }

    addEventListener(type: 'open', listener: () => void): void;
    addEventListener(
      type: 'message',
      listener: (event: { data: ArrayBuffer | ArrayBufferView }) => void,
    ): void;
    addEventListener(type: 'error', listener: (event: { message?: string }) => void): void;
    addEventListener(
      type: 'close',
      listener: (event: { code: number; reason?: string }) => void,
    ): void;
    addEventListener(type: string, listener: Function): void {
      const listeners = this.#listeners.get(type) ?? [];
      listeners.push(listener);
      this.#listeners.set(type, listeners);
    }

    #dispatch(type: string, event?: unknown): void {
      for (const listener of this.#listeners.get(type) ?? []) listener(event);
    }
  }

  try {
    const live = new TikTokLive('@someone', {
      roomId: '7300000000000000001',
      fetchGifts: false,
      fetchRoomInfo: false,
      signer: {
        sign(url) {
          unsignedUrl = url;
          return `${url}&X-Gnarly=test`;
        },
      },
      WebSocketImpl: FakeWebSocket as WebSocketConstructor,
    });

    await live.connect();
    assert.match(unsignedUrl, /device_id=7999000000000000001/);
    assert.equal(
      handshake?.headers.cookie,
      'ttwid=guest-value; tt_webid_v2=7999000000000000001',
    );
    assert.doesNotMatch(handshake?.headers.cookie ?? '', /sessionid/);
    live.disconnect();
  } finally {
    globalThis.fetch = previousFetch;
    if (previousSessionFile === undefined) delete process.env.TTL_SESSION_FILE;
    else process.env.TTL_SESSION_FILE = previousSessionFile;
  }
});

test('a rejected anonymous handshake is typed and is not retried', async () => {
  const previousFetch = globalThis.fetch;
  const previousSessionFile = process.env.TTL_SESSION_FILE;
  process.env.TTL_SESSION_FILE = `/tmp/ttl-live-missing-session-${process.pid}`;
  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    headers: { getSetCookie: () => ['ttwid=guest-value; Path=/; Domain=.tiktok.com'] },
    arrayBuffer: async () => new ArrayBuffer(0),
  } as unknown as Response);

  class RejectingWebSocket {
    binaryType = '';
    readonly #listeners = new Map<string, Function[]>();

    constructor(_url: string, _options: WebSocketOptions) {
      queueMicrotask(() => {
        for (const listener of this.#listeners.get('close') ?? []) listener({ code: 1006 });
      });
    }

    send(_data: string | ArrayBuffer | ArrayBufferView): void {}
    close(): void {}
    addEventListener(type: string, listener: Function): void {
      const listeners = this.#listeners.get(type) ?? [];
      listeners.push(listener);
      this.#listeners.set(type, listeners);
    }
  }

  try {
    const live = new TikTokLive('@someone', {
      roomId: '7300000000000000001',
      fetchGifts: false,
      fetchRoomInfo: false,
      signer: { sign: (url) => `${url}&X-Gnarly=test` },
      WebSocketImpl: RejectingWebSocket as unknown as WebSocketConstructor,
    });

    await assert.rejects(
      () => live.connect(),
      (error: unknown) => error instanceof GuestSessionError
        && error.code === 'WS_HANDSHAKE_REJECTED'
        && error.closeCode === 1006,
    );
    assert.equal(live.connected, false);
  } finally {
    globalThis.fetch = previousFetch;
    if (previousSessionFile === undefined) delete process.env.TTL_SESSION_FILE;
    else process.env.TTL_SESSION_FILE = previousSessionFile;
  }
});

test('a rejected guest reconnect refreshes identity once instead of cycling the old jar', async () => {
  let fetchCalls = 0;
  const previousFetch = globalThis.fetch;
  const previousSessionFile = process.env.TTL_SESSION_FILE;
  process.env.TTL_SESSION_FILE = `/tmp/ttl-live-missing-session-${process.pid}-${Date.now()}`;
  globalThis.fetch = async () => {
    fetchCalls += 1;
    const suffix = fetchCalls === 1 ? 'one' : 'two';
    const webId = fetchCalls === 1 ? '7999000000000000001' : '7999000000000000002';
    return {
      ok: true,
      status: 200,
      headers: {
        getSetCookie: () => [
          `ttwid=guest-${suffix}; Path=/; Domain=.tiktok.com`,
          `tt_webid_v2=${webId}; Path=/; Domain=.tiktok.com`,
        ],
      },
      arrayBuffer: async () => new ArrayBuffer(0),
    } as unknown as Response;
  };

  const cookies: string[] = [];
  const signedInputs: string[] = [];
  const socketUrls: string[] = [];
  const sockets: GuestReconnectWebSocket[] = [];

  class GuestReconnectWebSocket {
    binaryType = '';
    readonly #listeners = new Map<string, Function[]>();

    constructor(url: string, options: WebSocketOptions) {
      sockets.push(this);
      cookies.push(options.headers.cookie ?? '');
      socketUrls.push(url);
      const index = sockets.length;
      queueMicrotask(() => {
        if (index === 2) this.#dispatch('close', { code: 1006 });
        else this.#dispatch('open');
      });
    }

    send(_data: string | ArrayBuffer | ArrayBufferView): void {}
    close(): void { this.#dispatch('close', { code: 1000, reason: 'done' }); }
    closeWith(code: number): void { this.#dispatch('close', { code }); }

    addEventListener(type: string, listener: Function): void {
      const listeners = this.#listeners.get(type) ?? [];
      listeners.push(listener);
      this.#listeners.set(type, listeners);
    }

    #dispatch(type: string, event?: unknown): void {
      for (const listener of this.#listeners.get(type) ?? []) listener(event);
    }
  }

  try {
    const live = new TikTokLive('@someone', {
      roomId: '7300000000000000001',
      fetchGifts: false,
      fetchRoomInfo: false,
      reconnect: { attempts: 4, initialMs: 0, maxMs: 0 },
      signer: {
        sign(url) {
          signedInputs.push(url);
          return `${url}&X-Gnarly=test`;
        },
      },
      WebSocketImpl: GuestReconnectWebSocket as unknown as WebSocketConstructor,
    });

    await live.connect();
    const reconnected = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('guest refresh did not reconnect')), 1_000);
      live.once('connected', () => {
        clearTimeout(timer);
        resolve();
      });
      live.once('error', (error) => {
        clearTimeout(timer);
        reject(error);
      });
    });
    sockets[0]?.closeWith(1000);
    await reconnected;

    assert.equal(fetchCalls, 2, 'refresh should bootstrap exactly one new guest jar');
    assert.deepEqual(cookies, [
      'ttwid=guest-one; tt_webid_v2=7999000000000000001',
      'ttwid=guest-one; tt_webid_v2=7999000000000000001',
      'ttwid=guest-two; tt_webid_v2=7999000000000000002',
    ]);
    assert.match(signedInputs[0] ?? '', /device_id=7999000000000000001/);
    assert.match(signedInputs[2] ?? '', /device_id=7999000000000000002/);
    live.disconnect();
  } finally {
    globalThis.fetch = previousFetch;
    if (previousSessionFile === undefined) delete process.env.TTL_SESSION_FILE;
    else process.env.TTL_SESSION_FILE = previousSessionFile;
  }
});

test('an authenticated handshake refusal stops reconnecting with the same session', async () => {
  let socketCount = 0;
  const sockets: AuthReconnectWebSocket[] = [];

  class AuthReconnectWebSocket {
    binaryType = '';
    readonly #listeners = new Map<string, Function[]>();

    constructor(_url: string, _options: WebSocketOptions) {
      sockets.push(this);
      socketCount += 1;
      const index = sockets.length;
      queueMicrotask(() => {
        if (index === 1) this.#dispatch('open');
        else this.#dispatch('close', { code: 1006 });
      });
    }

    send(_data: string | ArrayBuffer | ArrayBufferView): void {}
    close(): void { this.#dispatch('close', { code: 1000, reason: 'done' }); }
    closeWith(code: number): void { this.#dispatch('close', { code }); }

    addEventListener(type: string, listener: Function): void {
      const listeners = this.#listeners.get(type) ?? [];
      listeners.push(listener);
      this.#listeners.set(type, listeners);
    }

    #dispatch(type: string, event?: unknown): void {
      for (const listener of this.#listeners.get(type) ?? []) listener(event);
    }
  }

  const live = new TikTokLive('@someone', {
    roomId: '7300000000000000001',
    sessionCookie: 'sessionid=test; tt_webid_v2=7999000000000000001',
    fetchGifts: false,
    fetchRoomInfo: false,
    reconnect: { attempts: 4, initialMs: 0, maxMs: 0 },
    signer: { sign: (url) => `${url}&X-Gnarly=test` },
    WebSocketImpl: AuthReconnectWebSocket as unknown as WebSocketConstructor,
  });
  const refusal = new Promise<Error>((resolve) => live.once('error', resolve));

  await live.connect();
  sockets[0]?.closeWith(1000);
  const error = await refusal;
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.match(error.message, /provided authenticated session/);
  assert.equal(socketCount, 2);
  live.disconnect();
});
