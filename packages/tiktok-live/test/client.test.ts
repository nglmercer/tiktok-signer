import assert from 'node:assert/strict';
import test from 'node:test';

import { TikTokLive } from '../dist/client.js';
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
