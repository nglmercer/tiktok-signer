// One class for the whole thing: resolve a room, sign the socket, connect, decode, reconnect.
//
// The flow is the web player's, not the old connector's. The player configures its IM SDK with
// `wsDirect: "1"` and a `socketHost`, and the SDK builds and signs the socket URL itself — there is
// no `/webcast/im/fetch/` on the path, and nothing here needs a `push_server` from anywhere. That
// is why this library needs no sign server: the only signed thing is a query string, and the code
// that signs it is TikTok's own bundle running in this process.
//
// What is required is a TikTok-issued client identity. A normal public `/live` navigation supplies
// an anonymous `ttwid` identity, so an account session is optional for public rooms.

import { EventEmitter } from 'node:events';
import fs from 'node:fs';

import { Discovery } from './discovery.js';
import { EVENT, decodeEvent } from './events.js';
import { ackFrame, decodeBatch, decodePushFrame, decompress } from './frames.js';
import {
  IDENTITY, PATH, SOCKET_HOST, enterRoomFrame, heartbeatFrame, socketConfig, socketQuery,
} from './player.js';
import { PRODUCT, Signer } from './signer.js';
import {
  GuestSessionError, USER_AGENT, bootstrapGuestSession, cookieHeader, parseCookies, sessionJar,
} from './session.js';
import type {
  ClientState,
  ChatEvent,
  Gift,
  GiftEvent,
  LikeEvent,
  LiveEvent,
  MemberEvent,
  ReconnectPolicy,
  RoomInfo,
  RoomUserEvent,
  SocialEvent,
  UnknownEvent,
  WebSocketConstructor,
  WebSocketLike,
} from './types.js';
import type { Identity } from './player.js';

/// Reconnection, matching the Rust client's policy: five attempts, doubling from two seconds, and
/// never past a minute. A signature ages and a room can move its push server, so a reconnect
/// re-signs from scratch rather than reusing the URL that just failed.
export const DEFAULT_RECONNECT: Readonly<ReconnectPolicy> = Object.freeze({
  attempts: 5,
  initialMs: 2_000,
  maxMs: 60_000,
});

/// The device id the query claims. The page uses its own `tt_webid_v2`, so a session that carries
/// one is used, and this is only the fallback.
const FALLBACK_DEVICE_ID = '7300000000000000001';

/// How long to wait for the handshake before calling it refused.
const OPEN_TIMEOUT_MS = 15_000;

export interface TikTokLiveOptions {
  sessionCookie?: string;
  userAgent?: string;
  roomId?: string;
  deviceId?: string;
  fetchGifts?: boolean;
  fetchRoomInfo?: boolean;
  identity?: Identity;
  socketHost?: string;
  reconnect?: Partial<ReconnectPolicy>;
  signer?: SignerLike;
  bundleSource?: string;
  bundlePath?: string;
  bundleUrl?: string;
  WebSocketImpl?: WebSocketConstructor;
  discovery?: Discovery;
}

export interface SignerLike {
  sign(url: string, product?: (typeof PRODUCT)[keyof typeof PRODUCT]): string;
}

interface ResolvedOptions {
  cookie: string;
  userAgent: string;
  deviceId: string;
  fetchGifts: boolean;
  fetchRoomInfo: boolean;
  identity: Identity;
  socketHost: string;
  reconnect: ReconnectPolicy;
  signer: SignerLike | null;
  bundleSource?: string;
  bundlePath?: string;
  bundleUrl?: string;
  WebSocketImpl?: WebSocketConstructor;
  discovery: Discovery | null;
}

export interface TikTokLiveEvents {
  connected: [ClientState];
  disconnected: [{ code: number; reason: string }];
  reconnecting: [{ attempt: number; delayMs: number }];
  error: [Error];
  event: [LiveEvent];
  chat: [ChatEvent];
  gift: [GiftEvent];
  like: [LikeEvent];
  member: [MemberEvent];
  social: [SocialEvent];
  roomUser: [RoomUserEvent];
  unknown: [UnknownEvent];
}

export class TikTokLive extends EventEmitter<TikTokLiveEvents> {
  readonly uniqueId: string;
  roomId: string;
  nickname = '';
  roomInfo: RoomInfo | null = null;
  gifts = new Map<string, Gift>();
  connected = false;
  readonly discovery: Discovery;

  readonly #options: ResolvedOptions;
  #socket: WebSocketLike | null = null;
  #heartbeat: NodeJS.Timeout | null = null;
  #signer: SignerLike | null = null;
  #guestSession: Promise<void> | null = null;
  #connecting: Promise<ClientState> | null = null;
  #closing = false;
  #attempt = 0;

  /// `uniqueId` is the `@handle`. Everything else has a working default.
  constructor(uniqueId: string, options: TikTokLiveOptions = {}) {
    super();
    const cookie = options.sessionCookie ?? cookieHeader(sessionJar());
    this.uniqueId = String(uniqueId).replace(/^@/, '');
    this.roomId = options.roomId ?? '';
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
      WebSocketImpl:
        options.WebSocketImpl ?? (globalThis.WebSocket as unknown as WebSocketConstructor | undefined),
      discovery: options.discovery ?? null,
    };
    this.discovery = this.#options.discovery ?? new Discovery({
      cookie: this.#options.cookie,
      userAgent: this.#options.userAgent,
    });
  }

  /// Resolve the room, sign, and open the socket. Resolves once the socket is open and the room
  /// has been entered; events arrive on this emitter from then on.
  connect(): Promise<ClientState> {
    if (this.connected) return Promise.resolve(this.state());
    if (this.#connecting) return this.#connecting;
    const connecting = this.#connect();
    this.#connecting = connecting;
    void connecting.finally(() => {
      if (this.#connecting === connecting) this.#connecting = null;
    }).catch(() => undefined);
    return connecting;
  }

  async #connect(): Promise<ClientState> {
    this.#closing = false;

    await this.#ensureSession();

    if (!this.roomId) {
      const lookup = await this.discovery.roomLookup(this.uniqueId);
      if (!lookup.isLive) {
        throw new NotLiveError(`@${this.uniqueId} is not live (status ${lookup.status})`);
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
      this.gifts = await this.discovery.giftList(this.roomId).catch(() => new Map<string, Gift>());
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

    if (this.#closing) throw new Error('connection cancelled');
    await this.#open();
    return this.state();
  }

  /// Use a supplied/stored jar, or create one anonymous TikTok guest identity for this client.
  /// The promise prevents concurrent callers from bootstrapping two identities and keeps the
  /// in-memory jar stable across reconnects.
  async #ensureSession(): Promise<void> {
    if (this.#options.cookie) {
      this.discovery.setCookie(this.#options.cookie);
      return;
    }
    if (!this.#guestSession) {
      this.#guestSession = bootstrapGuestSession({ userAgent: this.#options.userAgent })
        .then((identity) => {
          this.#options.cookie = identity.cookie;
          this.discovery.setCookie(identity.cookie);
        })
        .catch((error) => {
          this.#guestSession = null;
          throw error;
        });
    }
    await this.#guestSession;
  }

  /// Room, viewers, and whether the socket is up.
  state(): ClientState {
    return {
      uniqueId: this.uniqueId,
      roomId: this.roomId,
      connected: this.connected,
      roomInfo: this.roomInfo,
      giftCount: this.gifts.size,
    };
  }

  /// Close the socket and stop reconnecting.
  disconnect(): void {
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

  async #open(): Promise<void> {
    const jar = parseCookies(this.#options.cookie);
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
    const signer = this.#signer;
    if (!signer) throw new Error('signer was not initialized');
    const url = signer.sign(
      `${config.socketHost}${PATH.wsReuseSupplement}?${query}`,
      PRODUCT.ws,
    );

    const Impl = this.#options.WebSocketImpl;
    if (!Impl) {
      throw new Error('no WebSocket implementation: use Node 22+, or pass `WebSocketImpl`');
    }

    await new Promise<void>((resolve, reject) => {
      let settled = false;
      let opened = false;
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
        if (settled) return;
        if (this.#closing) {
          settled = true;
          clearTimeout(timer);
          socket.close();
          reject(new Error('connection cancelled'));
          return;
        }
        opened = true;
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
        const error = new Error(event.message ?? 'socket error');
        if (settled) this.emit('error', error);
      });

      socket.addEventListener('close', (event) => {
        this.connected = false;
        this.#stopHeartbeat();
        clearTimeout(timer);
        if (!opened) {
          if (!settled) {
            settled = true;
            // A refusal at the handshake is not a network blip. Retrying the same identity just
            // repeats the refusal, so identify whether the caller supplied an account session.
            const authenticated = parseCookies(this.#options.cookie).has('sessionid');
            reject(
              authenticated
                ? new Error(
                    `the handshake was refused (code ${event.code}). The socket rejected the ` +
                      'provided authenticated session before any frame was exchanged.',
                  )
                : new GuestSessionError(
                    'WS_HANDSHAKE_REJECTED',
                    `anonymous guest WebSocket handshake was rejected (code ${event.code}). ` +
                      'Provide a TikTok session cookie if authenticated transport is required.',
                    undefined,
                    event.code,
                  ),
            );
          }
          return;
        }
        this.emit('disconnected', { code: event.code, reason: String(event.reason ?? '') });
        if (!this.#closing) this.#scheduleReconnect();
      });
    });
  }

  #receive(data: ArrayBuffer | ArrayBufferView): void {
    let frame;
    try {
      frame = decodePushFrame(data);
    } catch (error) {
      this.emit('error', toError(error));
      return;
    }
    // `hb`, `ack` and `im_enter_room_resp` are the transport talking to itself.
    if (!frame.carriesEvents) return;

    let batch;
    try {
      batch = decodeBatch(decompress(frame));
    } catch (error) {
      this.emit('error', toError(error));
      return;
    }

    // Acknowledge before decoding events, not after: an unacknowledged frame stops the push a few
    // seconds later, and a slow listener must not be able to cause that.
    if (batch.needAck) {
      try {
        this.#socket?.send(ackFrame(frame, batch.internalExt));
      } catch (error) {
        this.emit('error', toError(error));
      }
    }

    for (const message of batch.messages) {
      const event = this.#enrich(decodeEvent(message.method, message.payload));
      event.msgId = message.msgId;
      event.isHistory = Boolean(message.isHistory);
      this.emit('event', event);
      this.#emitTypedEvent(event);
    }
  }

  /// Fill in what the message left out. A repeat gift carries no detail block, so its name and
  /// price come from the room's gift table.
  #enrich(event: LiveEvent): LiveEvent {
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

  #startHeartbeat(everyMs: number): void {
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

  #stopHeartbeat(): void {
    if (this.#heartbeat) clearInterval(this.#heartbeat);
    this.#heartbeat = null;
  }

  #scheduleReconnect(): void {
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
        this.emit('error', toError(error));
        if (!this.#closing) this.#scheduleReconnect();
      });
    }, delayMs);
    timer.unref?.();
  }

  #emitTypedEvent(event: LiveEvent): void {
    switch (event.type) {
      case EVENT.chat: this.emit('chat', event); break;
      case EVENT.gift: this.emit('gift', event); break;
      case EVENT.like: this.emit('like', event); break;
      case EVENT.member: this.emit('member', event); break;
      case EVENT.social: this.emit('social', event); break;
      case EVENT.roomUser: this.emit('roomUser', event); break;
      case EVENT.unknown: this.emit('unknown', event); break;
    }
  }
}

/// Read a cookie header from a file, for callers keeping the session somewhere of their own.
export function cookieFromFile(path: string): string {
  return fs.readFileSync(path, 'utf8').trim();
}

export class NotLiveError extends Error {
  readonly code = 'NOT_LIVE';
}

function toError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
