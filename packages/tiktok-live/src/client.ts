import { EventDelivery, type DeliveryOptions, type DeliveryMetrics } from "./delivery.js";
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
import NodeWebSocket from 'ws';

import { Discovery } from './discovery.js';
import { EVENT, decodeEvent } from './events.js';
import { SocketWriteOverflowError, sendBoundedFrame, writeBufferLimit, ackFrameForBatch, carriesEvents, decodeBatch, decodePushFrame, decompress } from './frames.js';
import {
  IDENTITY, PATH, SOCKET_HOST, enterRoomFrame, heartbeatFrame, socketConfig, socketQuery,
} from './player.js';
import { PRODUCT, Signer } from './signer.js';
import type { Product } from './signer.js';
import {
  GuestSessionError, USER_AGENT, bootstrapGuestSession, cookieHeader, parseCookies, sessionJar,
} from './session.js';
import type {
  ClientState,
  ChatEvent,
  EmoteChatEvent,
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

/// Node timers clamp larger delays to a one-millisecond timeout, so keep a server-provided
/// heartbeat duration within the largest delay they can represent.
const MAX_HEARTBEAT_MS = 2_147_483_647;

class HandshakeRejectedError extends Error {
  readonly code = 'WS_HANDSHAKE_REJECTED' as const;
  readonly closeCode: number;

  constructor(message: string, closeCode: number) {
    super(message);
    this.name = 'HandshakeRejectedError';
    this.closeCode = closeCode;
  }
}

export interface TikTokLiveOptions {
  eventDelivery?: DeliveryOptions;
  /** Pending WebSocket write bytes, 32 MiB by default. */
  maxBufferedBytes?: number;
  sessionCookie?: string;
  userAgent?: string;
  roomId?: string;
  deviceId?: string;
  fetchGifts?: boolean;
  fetchRoomInfo?: boolean;
  identity?: Identity;
  socketHost?: string;
  reconnect?: Partial<ReconnectPolicy>;
  /** Positive jitter fraction 0..1; zero preserves legacy retry timing. */
  reconnectJitter?: number;
  signer?: SignerLike;
  bundleSource?: string;
  bundlePath?: string;
  bundleUrl?: string;
  WebSocketImpl?: WebSocketConstructor;
  discovery?: Discovery;
}

export interface SignerLike {
  sign(url: string, product?: Product): string;
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
  reconnectJitter: number;
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
  emoteChat: [EmoteChatEvent];
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
  #reconnectTimer: NodeJS.Timeout | null = null;
  #signer: SignerLike | null = null;
  #guestSession: Promise<void> | null = null;
  #guestMode = false;
  #guestRefreshAttempted = false;
  #connecting: Promise<ClientState> | null = null;
  #closing = false;
  #generation = 0;
  #connectAbort: AbortController | null = null;
  #listenerErrors = 0;
  #writeBufferRejections = 0;
  readonly #delivery: EventDelivery<LiveEvent>;
  readonly #maxBufferedBytes: number;
  #cancelOpen: (() => void) | null = null;
  #attempt = 0;

  /// `uniqueId` is the `@handle`. Everything else has a working default.
  constructor(uniqueId: string, options: TikTokLiveOptions = {}) {
    super();
    this.#maxBufferedBytes = writeBufferLimit(options.maxBufferedBytes);
    this.#delivery = new EventDelivery(options.eventDelivery,
      (event, generation) => this.#dispatch(event, generation),
      (error) => { this.disconnect(); this.#emitError(error); });
    if (!Number.isFinite(options.reconnectJitter ?? 0) || (options.reconnectJitter ?? 0) < 0 || (options.reconnectJitter ?? 0) > 1) {
      throw new RangeError('reconnectJitter must be between 0 and 1');
    }
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
      reconnectJitter: options.reconnectJitter ?? 0,
      signer: options.signer ?? null,
      bundleSource: options.bundleSource,
      bundlePath: options.bundlePath,
      bundleUrl: options.bundleUrl,
      // Use a header-capable Node transport by default; custom implementations must honor headers.
      WebSocketImpl:
        options.WebSocketImpl ?? (NodeWebSocket as unknown as WebSocketConstructor),
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
    this.#closing = false;
    if (this.#reconnectTimer) clearTimeout(this.#reconnectTimer);
    this.#reconnectTimer = null;
    this.#connectAbort = new AbortController();
    const generation = ++this.#generation;
    const connecting = this.#connect(generation);
    this.#connecting = connecting;
    void connecting.finally(() => {
      if (this.#connecting === connecting) this.#connecting = null;
    }).catch(() => undefined);
    return connecting;
  }

  async #connect(generation: number): Promise<ClientState> {

    await this.#ensureSession(generation);
    this.#checkGeneration(generation);

    if (!this.roomId) {
      const lookup = await this.discovery.roomLookup(this.uniqueId, this.#connectAbort?.signal);
      this.#checkGeneration(generation);
      if (!lookup.isLive) {
        throw new NotLiveError(`@${this.uniqueId} is not live (status ${lookup.status})`);
      }
      this.roomId = lookup.roomId;
      this.nickname = lookup.nickname;
    }

    // Metadata and the gift table are read once, not per event: the gift list is megabytes, and it
    // is what turns a repeat gift message — which omits its own detail block — into a real value.
    if (this.#options.fetchRoomInfo) {
      const roomInfo = await this.discovery.roomInfo(this.roomId, this.#connectAbort?.signal).catch(() => null);
      this.#checkGeneration(generation);
      this.roomInfo = roomInfo;
    }
    if (this.#options.fetchGifts) {
      const gifts = await this.discovery.giftList(this.roomId, this.#connectAbort?.signal).catch(() => new Map<string, Gift>());
      this.#checkGeneration(generation);
      this.gifts = gifts;
    }

    await this.#ensureSigner(generation);

    this.#checkGeneration(generation);
    await this.#open(generation);
    return this.state();
  }

  /// Use a supplied/stored jar, or create one anonymous TikTok guest identity for this client.
  /// The promise prevents concurrent callers from bootstrapping two identities and keeps the
  /// in-memory jar stable across reconnects.
  async #ensureSession(generation: number): Promise<void> {
    if (this.#options.cookie) {
      this.discovery.setCookie(this.#options.cookie);
      return;
    }
    if (!this.#guestSession) {
      this.#guestMode = true;
      const pending = bootstrapGuestSession({ userAgent: this.#options.userAgent, signal: this.#connectAbort?.signal })
        .then((identity) => {
          this.#checkGeneration(generation);
          this.#options.cookie = identity.cookie;
          this.discovery.setCookie(identity.cookie);
        })
        .catch((error) => {
          if (this.#guestSession === pending) this.#guestSession = null;
          throw error;
        });
      this.#guestSession = pending;
    }
    await this.#guestSession;
  }

  async #ensureSigner(generation: number): Promise<void> {
    if (this.#signer) return;
    const signer =
      this.#options.signer ??
      (await Signer.create({
        bundleSource: this.#options.bundleSource,
        bundlePath: this.#options.bundlePath,
        bundleUrl: this.#options.bundleUrl,
        userAgent: this.#options.userAgent,
        cookie: this.#options.cookie,
      }));
    this.#checkGeneration(generation);
    this.#signer = signer;
  }

  /// A guest identity can expire while a long-lived socket is down. Replace the whole identity,
  /// including the signer context, once; a deterministic refusal after that is terminal.
  async #refreshGuestSession(generation: number): Promise<void> {
    this.#guestSession = null;
    this.#options.cookie = '';
    // A signer this client created belongs to the old identity; give its context back.
    const previous = this.#signer;
    this.#signer = null;
    if (previous !== this.#options.signer && previous instanceof Signer) previous.close();
    await this.#ensureSession(generation);
    await this.#ensureSigner(generation);
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
    this.#connectAbort?.abort();
    this.#connectAbort = null;
    this.#guestSession = null;
    this.#generation += 1;
    this.#delivery.clear();
    this.#connecting = null;
    this.#cancelOpen?.();
    this.#cancelOpen = null;
    if (this.#reconnectTimer) clearTimeout(this.#reconnectTimer);
    this.#reconnectTimer = null;
    this.#stopHeartbeat();
    const socket = this.#socket;
    try {
      socket?.close();
    } catch {
      // Already gone.
    }
    try { socket?.terminate?.(); } catch { /* Already closed. */ }
    this.#socket = null;
    this.connected = false;
  }

  // --- the socket ------------------------------------------------------------------------------

  async #open(generation: number): Promise<void> {
    this.#checkGeneration(generation);
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
    this.#checkGeneration(generation);

    const Impl = this.#options.WebSocketImpl;
    if (!Impl) {
      throw new Error('no WebSocket implementation configured');
    }

    await new Promise<void>((resolve, reject) => {
      let settled = false;
      let opened = false;
      const socket = new Impl(url, {
        maxPayload: 16 * 1024 * 1024,
        perMessageDeflate: false,
        headers: {
          cookie: this.#options.cookie,
          'user-agent': this.#options.userAgent,
          origin: 'https://www.tiktok.com',
        },
      });
      if (this.#closing || generation !== this.#generation) {
        try { socket.close(); } catch { /* Already closed. */ }
        reject(new Error('connection cancelled'));
        return;
      }
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

      this.#cancelOpen = () => {
        clearTimeout(timer);
        if (!settled) {
          settled = true;
          reject(new Error('connection cancelled'));
        }
      };

      socket.addEventListener('open', () => {
        if (settled) return;
        if (this.#closing || generation !== this.#generation || this.#socket !== socket) {
          settled = true;
          clearTimeout(timer);
          socket.close();
          reject(new Error('connection cancelled'));
          return;
        }
        opened = true;
        // The server pushes nothing until the client says which room it is in. A socket that
        // opened and stayed silent is almost always a missing enter-room frame.
        try {
          this.#send(socket, enterRoomFrame({ roomId: this.roomId, identity: config.identity }));
        } catch (error) {
          settled = true;
          clearTimeout(timer);
          this.#cancelOpen = null;
          this.disconnect();
          reject(error);
          return;
        }
        this.#startHeartbeat(Number(config.heartbeatDuration));
        this.connected = true;
        this.#attempt = 0;
        this.#guestRefreshAttempted = false;
        clearTimeout(timer);
        if (!settled) {
          settled = true;
          resolve();
        }
        this.#cancelOpen = null;
        this.#notify(() => this.emit('connected', this.state()));
      });

      socket.addEventListener('message', (message) => {
        if (!this.#closing && this.#socket === socket && generation === this.#generation) this.#receive(message.data);
      });

      socket.addEventListener('error', (event) => {
        if (this.#socket !== socket || generation !== this.#generation) return;
        const error = new Error(event.message ?? 'socket error');
        if (settled) this.#emitError(error);
      });

      socket.addEventListener('close', (event) => {
        clearTimeout(timer);
        if (this.#socket !== socket) return;
        this.#cancelOpen = null;
        this.#socket = null;
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
                ? new HandshakeRejectedError(
                    `the handshake was refused (code ${event.code}). The socket rejected the ` +
                      'provided authenticated session before any frame was exchanged.',
                    event.code,
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
        this.#notify(() => this.emit('disconnected', { code: event.code, reason: String(event.reason ?? '') }));
        if (!this.#closing) this.#scheduleReconnect();
      });
    });
  }

  #receive(data: ArrayBuffer | ArrayBufferView): void {
    const generation = this.#generation;
    let frame;
    try {
      frame = decodePushFrame(data);
    } catch (error) {
      this.#emitError(error);
      return;
    }
    // `hb`, `ack` and `im_enter_room_resp` are the transport talking to itself.
    if (!carriesEvents(frame)) return;

    let batch;
    try {
      batch = decodeBatch(decompress(frame));
    } catch (error) {
      this.#emitError(error);
      return;
    }

    // The batch owns the transport's heartbeat hint. Keep it as generated bigint until this
    // application boundary, then clamp it to the range accepted by Node's timers.
    if (batch.heartbeatDuration > 0n) {
      const maxHeartbeat = BigInt(MAX_HEARTBEAT_MS);
      const boundedHeartbeat = batch.heartbeatDuration > maxHeartbeat
        ? maxHeartbeat
        : batch.heartbeatDuration;
      const heartbeatMs = Number(boundedHeartbeat);
      if (heartbeatMs > 0) this.#startHeartbeat(heartbeatMs);
    }

    // Acknowledge before decoding events, not after: an unacknowledged frame stops the push a few
    // seconds later, and a slow listener must not be able to cause that.
    if (batch.needAck) {
      try {
        if (this.#socket) this.#send(this.#socket, ackFrameForBatch(frame, batch.internalExt));
      } catch (error) {
        this.#emitError(error);
      }
    }

    for (const message of batch.messages) {
      const event = this.#enrich(decodeEvent(message.method, message.payload));
      event.msgId = message.msgId.toString();
      event.isHistory = message.isHistory;
      if (this.#closing || generation !== this.#generation) break;
      // Include the backing buffer for unknown events: a small view can retain a whole batch.
      const metadataBytes = event.method.length * 2 + (event.msgId?.length ?? 0) * 2 + 256;
      const bytes = event.type === 'unknown' ? event.payload.buffer.byteLength + metadataBytes
        : message.payload.byteLength * 64 + metadataBytes;
      this.#delivery.submit(event, generation, bytes);
    }
  }

  /// Fill in what the message left out. A repeat gift carries no detail block, so its name and
  /// price come from the room's gift table. Also attach icon URL for renderer.
  #enrich(event: LiveEvent): LiveEvent {
    if (event.type !== EVENT.gift) return event;
    const gift = this.gifts.get(event.giftId);
    if (!gift) return event;
    event.giftName ||= gift.name;
    event.diamondCount ||= gift.diamondCount;
    event.giftIconUrl = gift.iconUrl || undefined;
    event.streakable = gift.combo;
    return event;
  }

  #startHeartbeat(everyMs: number): void {
    this.#stopHeartbeat();
    // The application heartbeat, not a protocol ping: the socket closes without it, and protocol
    // pings are not answered.
    this.#heartbeat = setInterval(() => {
      try {
        if (this.#socket) this.#send(this.#socket, heartbeatFrame(this.roomId));
      } catch (error) {
        if (error instanceof SocketWriteOverflowError) this.#emitError(error);
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
    if (this.#closing || this.#reconnectTimer) return;
    const { attempts, initialMs, maxMs } = this.#options.reconnect;
    if (this.#attempt >= attempts) {
      this.#emitError(new Error(`gave up after ${attempts} reconnection attempts`));
      return;
    }
    this.#attempt += 1;
    const baseMs = Math.min(initialMs * 2 ** Math.min(this.#attempt - 1, 30), maxMs);
    const delayMs = Math.min(MAX_HEARTBEAT_MS, maxMs,
      baseMs * (1 + this.#options.reconnectJitter * Math.random()));
    const generation = this.#generation;
    this.#notify(() => this.emit('reconnecting', { attempt: this.#attempt, delayMs }));
    if (this.#closing || generation !== this.#generation) return;
    const timer = setTimeout(() => {
      this.#reconnectTimer = null;
      if (this.#closing || generation !== this.#generation) return;
      // Re-signed from scratch: the previous signature is what the server just stopped accepting.
      void this.#reconnect();
    }, delayMs);
    timer.unref?.();
    this.#reconnectTimer = timer;
  }

  async #reconnect(): Promise<void> {
    let generation = this.#generation;
    try {
      const connecting = this.connect();
      generation = this.#generation;
      await connecting;
      return;
    } catch (error) {
      if (this.#closing || generation !== this.#generation) return;
      const refusal = isHandshakeRejected(error);
      if (!refusal) {
        this.#emitError(error);
        if (!this.#closing) this.#scheduleReconnect();
        return;
      }

      // Never cycle a rejected identity. Automatic guest mode gets one fresh bootstrap and one
      // fresh signer context; supplied/stored identities stop immediately on refusal.
      if (this.#guestMode && !this.#guestRefreshAttempted && !this.#closing) {
        this.#guestRefreshAttempted = true;
        try {
          await this.#refreshGuestSession(generation);
          if (!this.#closing && generation === this.#generation) await this.connect();
        } catch (refreshError) {
          if (!this.#closing) this.#emitError(refreshError);
        }
        return;
      }

      if (!this.#closing) this.#emitError(error);
    }
  }

  #send(socket: WebSocketLike, frame: ArrayBuffer | ArrayBufferView): void {
    try { sendBoundedFrame(socket, frame, this.#maxBufferedBytes); }
    catch (error) { if (error instanceof SocketWriteOverflowError) { this.#writeBufferRejections++; this.#cancelOpen = null; this.disconnect(); } throw error; }
  }

  #emitError(error: unknown): void {
    if (this.listenerCount('error') > 0) {
      try { this.emit('error', toError(error)); } catch { this.#listenerErrors++; }
    }
  }

  deliveryMetrics(): DeliveryMetrics & { listenerErrors: number; writeBufferRejections: number } {
    return { ...this.#delivery.metrics(), listenerErrors: this.#listenerErrors, writeBufferRejections: this.#writeBufferRejections };
  }

  #dispatch(event: LiveEvent, generation: number): boolean {
    if (this.#closing || generation !== this.#generation) return false;
    try { this.emit('event', event); } catch (error) { this.#listenerErrors++; this.#emitError(error); }
    if (!this.#closing && generation === this.#generation) {
      try { this.#emitTypedEvent(event); } catch (error) { this.#listenerErrors++; this.#emitError(error); }
    }
    return true;
  }
  #notify(action: () => void): void {
    try { action(); } catch (error) { this.#listenerErrors++; this.#emitError(error); }
  }

  #checkGeneration(generation: number): void {
    if (this.#closing || generation !== this.#generation) throw new Error('connection cancelled');
  }

  #emitTypedEvent(event: LiveEvent): void {
    switch (event.type) {
      case EVENT.chat: this.emit('chat', event); break;
      case EVENT.emoteChat: this.emit('emoteChat', event); break;
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

function isHandshakeRejected(error: unknown): boolean {
  return (error instanceof GuestSessionError && error.code === 'WS_HANDSHAKE_REJECTED')
    || error instanceof HandshakeRejectedError;
}
