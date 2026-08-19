// One class for the whole thing: resolve a room, sign the socket, connect, decode, reconnect.
//
// The flow is the web player's, not the old connector's. The player configures its IM SDK with
// `wsDirect: "1"` and a `socketHost`, and the SDK builds and signs the socket URL itself — there is
// no `/webcast/im/fetch/` on the path, and nothing here needs a `push_server` from anywhere. That
// is why this library needs no sign server: the only signed thing is a query string, and the code
// that signs it is TikTok's own bundle running in this process.
//
// What is *not* optional is a session. The socket refuses a jar-less handshake with an immediate
// 1006, before a frame is exchanged, so the cookie is required rather than an enhancement.

import { EventEmitter } from 'node:events';
import fs from 'node:fs';

import { Discovery } from './discovery.mjs';
import { EVENT, decodeEvent } from './events.mjs';
import { ackFrame, decodeBatch, decodePushFrame, decompress } from './frames.mjs';
import {
  IDENTITY, PATH, SOCKET_HOST, enterRoomFrame, heartbeatFrame, socketConfig, socketQuery,
} from './player.mjs';
import { PRODUCT, Signer } from './signer.mjs';
import { USER_AGENT, cookieHeader, sessionJar } from './session.mjs';

/// Reconnection, matching the Rust client's policy: five attempts, doubling from two seconds, and
/// never past a minute. A signature ages and a room can move its push server, so a reconnect
/// re-signs from scratch rather than reusing the URL that just failed.
export const DEFAULT_RECONNECT = Object.freeze({
  attempts: 5,
  initialMs: 2_000,
  maxMs: 60_000,
});

/// The device id the query claims. The page uses its own `tt_webid_v2`, so a session that carries
/// one is used, and this is only the fallback.
const FALLBACK_DEVICE_ID = '7300000000000000001';

/// How long to wait for the handshake before calling it refused.
const OPEN_TIMEOUT_MS = 15_000;

export class TikTokLive extends EventEmitter {
  #options;
  #socket = null;
  #heartbeat = null;
  #signer = null;
  #closing = false;
  #attempt = 0;

  /// `uniqueId` is the `@handle`. Everything else has a working default.
  constructor(uniqueId, options = {}) {
    super();
    const cookie = options.sessionCookie ?? cookieHeader(sessionJar());
    this.uniqueId = String(uniqueId).replace(/^@/, '');
    this.roomId = options.roomId ?? '';
    this.roomInfo = null;
    this.gifts = new Map();
    this.connected = false;
    this.#options = {
      cookie,
      userAgent: options.userAgent ?? USER_AGENT,
      deviceId: options.deviceId ?? '',
      fetchGifts: options.fetchGifts ?? true,
      fetchRoomInfo: options.fetchRoomInfo ?? true,
      identity: options.identity ?? IDENTITY.audience,
      socketHost: options.socketHost ?? SOCKET_HOST.global,
      reconnect: { ...DEFAULT_RECONNECT, ...(options.reconnect ?? {}) },
      signer: options.signer ?? null,
      bundleSource: options.bundleSource,
      bundlePath: options.bundlePath,
      bundleUrl: options.bundleUrl,
      // Injectable so a caller on an older Node, or one that prefers `ws`, can pass its own.
      // The global needs to accept a `headers` option, because the cookie travels there.
      WebSocketImpl: options.WebSocketImpl ?? globalThis.WebSocket,
      discovery: options.discovery ?? null,
    };
    this.discovery = this.#options.discovery ?? new Discovery({
      cookie: this.#options.cookie,
      userAgent: this.#options.userAgent,
    });
  }

  /// Resolve the room, sign, and open the socket. Resolves once the socket is open and the room
  /// has been entered; events arrive on this emitter from then on.
  async connect() {
    if (this.connected) return this.state();
    this.#closing = false;

    if (!this.#options.cookie) {
      throw new Error(
        'no session cookie. The message socket refuses a jar-less handshake (measured: an ' +
          'immediate 1006), so pass `sessionCookie`, or write a cookie header to ' +
          '$XDG_CONFIG_HOME/ttl-signer/session.',
      );
    }

    if (!this.roomId) {
      const lookup = await this.discovery.roomLookup(this.uniqueId);
      if (!lookup.isLive) {
        const error = new Error(`@${this.uniqueId} is not live (status ${lookup.status})`);
        error.code = 'NOT_LIVE';
        throw error;
      }
      this.roomId = lookup.roomId;
      this.nickname = lookup.nickname;
    }

    // Metadata and the gift table are read once, not per event: the gift list is megabytes, and it
    // is what turns a repeat gift message — which omits its own detail block — into a real value.
    if (this.#options.fetchRoomInfo) {
      this.roomInfo = await this.discovery.roomInfo(this.roomId).catch(() => null);
    }
    if (this.#options.fetchGifts) {
      this.gifts = await this.discovery.giftList(this.roomId).catch(() => new Map());
    }

    this.#signer ??=
      this.#options.signer ??
      (await Signer.create({
        bundleSource: this.#options.bundleSource,
        bundlePath: this.#options.bundlePath,
        bundleUrl: this.#options.bundleUrl,
        userAgent: this.#options.userAgent,
        cookie: this.#options.cookie,
      }));

    await this.#open();
    return this.state();
  }

  /// Room, viewers, and whether the socket is up.
  state() {
    return {
      uniqueId: this.uniqueId,
      roomId: this.roomId,
      connected: this.connected,
      roomInfo: this.roomInfo,
      giftCount: this.gifts.size,
    };
  }

  /// Close the socket and stop reconnecting.
  disconnect() {
    this.#closing = true;
    this.#stopHeartbeat();
    try {
      this.#socket?.close();
    } catch {
      // Already gone.
    }
    this.#socket = null;
    this.connected = false;
  }

  // --- the socket ------------------------------------------------------------------------------

  async #open() {
    const jar = sessionJar();
    const config = socketConfig({
      roomId: this.roomId,
      deviceId:
        this.#options.deviceId || jar.get('tt_webid_v2') || jar.get('tt_webid') || FALLBACK_DEVICE_ID,
      identity: this.#options.identity,
      socketHost: this.#options.socketHost,
    });
    const query = socketQuery(config);
    // Signed as one string, then opened verbatim. The signature covers these exact bytes, so the
    // URL must not be rebuilt, re-encoded, or reordered between here and the handshake.
    const url = this.#signer.sign(
      `${config.socketHost}${PATH.wsReuseSupplement}?${query}`,
      PRODUCT.ws,
    );

    const Impl = this.#options.WebSocketImpl;
    if (!Impl) {
      throw new Error('no WebSocket implementation: use Node 22+, or pass `WebSocketImpl`');
    }

    await new Promise((resolve, reject) => {
      let settled = false;
      const socket = new Impl(url, {
        headers: {
          cookie: this.#options.cookie,
          'user-agent': this.#options.userAgent,
          origin: 'https://www.tiktok.com',
        },
      });
      socket.binaryType = 'arraybuffer';
      this.#socket = socket;

      const timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        try {
          socket.close();
        } catch {
          // Already gone.
        }
        reject(new Error(`the socket did not open within ${OPEN_TIMEOUT_MS} ms`));
      }, OPEN_TIMEOUT_MS);

      socket.addEventListener('open', () => {
        // The server pushes nothing until the client says which room it is in. A socket that
        // opened and stayed silent is almost always a missing enter-room frame.
        socket.send(enterRoomFrame({ roomId: this.roomId, identity: config.identity }));
        this.#startHeartbeat(Number(config.heartbeatDuration));
        this.connected = true;
        this.#attempt = 0;
        clearTimeout(timer);
        if (!settled) {
          settled = true;
          resolve();
        }
        this.emit('connected', this.state());
      });

      socket.addEventListener('message', (message) => this.#receive(message.data));

      socket.addEventListener('error', (event) => {
        const error = new Error(event?.message ?? 'socket error');
        if (settled) this.emit('error', error);
      });

      socket.addEventListener('close', (event) => {
        this.connected = false;
        this.#stopHeartbeat();
        clearTimeout(timer);
        if (!settled) {
          settled = true;
          // A refusal at the handshake is not a network blip. The usual cause is a session the
          // socket will not accept, and retrying it just repeats the refusal.
          reject(
            new Error(
              `the handshake was refused (code ${event.code}). The socket rejects a session it ` +
                'does not accept before any frame is exchanged.',
            ),
          );
          return;
        }
        this.emit('disconnected', { code: event.code, reason: String(event.reason ?? '') });
        if (!this.#closing) this.#scheduleReconnect();
      });
    });
  }

  #receive(data) {
    let frame;
    try {
      frame = decodePushFrame(new Uint8Array(data));
    } catch (error) {
      this.emit('error', error);
      return;
    }
    // `hb`, `ack` and `im_enter_room_resp` are the transport talking to itself.
    if (!frame.carriesEvents) return;

    let batch;
    try {
      batch = decodeBatch(decompress(frame));
    } catch (error) {
      this.emit('error', error);
      return;
    }

    // Acknowledge before decoding events, not after: an unacknowledged frame stops the push a few
    // seconds later, and a slow listener must not be able to cause that.
    if (batch.needAck) {
      try {
        this.#socket?.send(ackFrame(frame, batch.internalExt));
      } catch (error) {
        this.emit('error', error);
      }
    }

    for (const message of batch.messages) {
      const event = this.#enrich(decodeEvent(message.method, message.payload));
      event.msgId = message.msgId;
      event.isHistory = Boolean(message.isHistory);
      this.emit('event', event);
      this.emit(event.type, event);
    }
  }

  /// Fill in what the message left out. A repeat gift carries no detail block, so its name and
  /// price come from the room's gift table.
  #enrich(event) {
    if (event.type !== EVENT.gift) return event;
    const gift = this.gifts.get(String(event.giftId));
    if (!gift) return event;
    return {
      ...event,
      giftName: event.giftName || gift.name,
      diamondCount: event.diamondCount || gift.diamondCount,
      /// Streakable gifts arrive as a burst; only `repeatEnd` is the real total. Anything that
      /// sums diamonds must ignore the rest, and this is what says which is which.
      streakable: gift.combo,
    };
  }

  #startHeartbeat(everyMs) {
    this.#stopHeartbeat();
    // The application heartbeat, not a protocol ping: the socket closes without it, and protocol
    // pings are not answered.
    this.#heartbeat = setInterval(() => {
      try {
        this.#socket?.send(heartbeatFrame(this.roomId));
      } catch {
        // The close handler deals with a dead socket.
      }
    }, everyMs || 10_000);
    this.#heartbeat.unref?.();
  }

  #stopHeartbeat() {
    if (this.#heartbeat) clearInterval(this.#heartbeat);
    this.#heartbeat = null;
  }

  #scheduleReconnect() {
    const { attempts, initialMs, maxMs } = this.#options.reconnect;
    if (this.#attempt >= attempts) {
      this.emit('error', new Error(`gave up after ${attempts} reconnection attempts`));
      return;
    }
    this.#attempt += 1;
    const delayMs = Math.min(initialMs * 2 ** (this.#attempt - 1), maxMs);
    this.emit('reconnecting', { attempt: this.#attempt, delayMs });
    const timer = setTimeout(() => {
      if (this.#closing) return;
      // Re-signed from scratch: the previous signature is what the server just stopped accepting.
      this.#open().catch((error) => {
        this.emit('error', error);
        if (!this.#closing) this.#scheduleReconnect();
      });
    }, delayMs);
    timer.unref?.();
  }
}

/// Read a cookie header from a file, for callers keeping the session somewhere of their own.
export function cookieFromFile(path) {
  return fs.readFileSync(path, 'utf8').trim();
}
