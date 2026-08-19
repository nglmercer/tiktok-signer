// Types for `ttl-live`. Hand-written: the package is plain ESM with no build step, so these are
// the contract rather than an artefact of one.

/// <reference types="node" />

import type { EventEmitter } from 'node:events';

export interface EventUser {
  /** 64-bit id as a decimal string: it exceeds `Number.MAX_SAFE_INTEGER`. */
  userId: string;
  nickname: string;
  /** The `@handle`. `display_id` in the schema, `uniqueId` in the Node connector. */
  uniqueId: string;
  secUid: string;
}

export interface BaseEvent {
  method: string;
  msgId?: string;
  isHistory?: boolean;
}

export interface ChatEvent extends BaseEvent { type: 'chat'; user: EventUser; comment: string }

export interface GiftEvent extends BaseEvent {
  type: 'gift';
  user: EventUser;
  toUser: EventUser;
  giftId: string;
  giftName: string;
  diamondCount: number;
  repeatCount: number;
  comboCount: number;
  groupId: string;
  /** Only the final message of a streak is the real total; the rest would double-count. */
  repeatEnd: boolean;
  /** Whether this gift streaks at all, from the room's gift table. */
  streakable?: boolean;
}

export interface LikeEvent extends BaseEvent { type: 'like'; user: EventUser; count: number; total: number }
export interface MemberEvent extends BaseEvent { type: 'member'; user: EventUser; memberCount: number; action: number }
export interface SocialEvent extends BaseEvent { type: 'social'; user: EventUser; action: number; followCount: number; shareCount: number }
export interface RoomUserEvent extends BaseEvent { type: 'roomUser'; viewers: number; popularity: number; totalUser: number; anonymous: number }
export interface UnknownEvent extends BaseEvent { type: 'unknown'; payload: Uint8Array }

export type LiveEvent =
  | ChatEvent | GiftEvent | LikeEvent | MemberEvent | SocialEvent | RoomUserEvent | UnknownEvent;

export interface RoomOwner {
  userId: string; uniqueId: string; nickname: string; secUid: string;
  avatarUrl: string; followerCount: number;
}

export interface RoomInfo {
  roomId: string; title: string; status: number; viewers: number; likes: number;
  comments: number; shares: number; follows: number; coverUrl: string; shareUrl: string;
  owner: RoomOwner;
}

export interface RoomLookup {
  uniqueId: string; roomId: string; nickname: string; status: number; title: string; isLive: boolean;
}

export interface LiveRoom {
  uniqueId: string; roomId: string; nickname: string; title: string; viewers: number;
}

export interface Gift {
  id: string; name: string; describe: string; diamondCount: number;
  /** Streakable: repeated sends arrive as a burst and only the last is the total. */
  combo: boolean;
  giftType: number; iconUrl: string;
}

export interface ReconnectPolicy { attempts: number; initialMs: number; maxMs: number }

export interface ClientState {
  uniqueId: string; roomId: string; connected: boolean;
  roomInfo: RoomInfo | null; giftCount: number;
}

export interface TikTokLiveOptions {
  /** Cookie header. Required: the socket refuses a jar-less handshake. Defaults to the session file. */
  sessionCookie?: string;
  userAgent?: string;
  /** Skip the lookup when the room id is already known. */
  roomId?: string;
  deviceId?: string;
  fetchGifts?: boolean;
  fetchRoomInfo?: boolean;
  identity?: 'audience' | 'anchor';
  socketHost?: string;
  reconnect?: Partial<ReconnectPolicy>;
  signer?: Signer;
  bundleSource?: string;
  bundlePath?: string;
  bundleUrl?: string;
  /** For Node without a header-capable global `WebSocket`, or to use `ws`. */
  WebSocketImpl?: unknown;
  discovery?: Discovery;
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
  constructor(uniqueId: string, options?: TikTokLiveOptions);
  readonly uniqueId: string;
  readonly roomId: string;
  readonly roomInfo: RoomInfo | null;
  readonly gifts: Map<string, Gift>;
  readonly connected: boolean;
  readonly discovery: Discovery;
  connect(): Promise<ClientState>;
  disconnect(): void;
  state(): ClientState;
}

export class Discovery {
  constructor(options?: { cookie?: string; userAgent?: string; timeoutMs?: number });
  roomLookup(uniqueId: string): Promise<RoomLookup>;
  roomInfo(roomId: string): Promise<RoomInfo>;
  giftList(roomId: string): Promise<Map<string, Gift>>;
  liveChannels(keyword?: string): Promise<LiveRoom[]>;
}

export class WebcastRefusal extends Error { statusCode: number }

export class Signer {
  static create(options?: {
    bundleSource?: string; bundlePath?: string; bundleUrl?: string;
    userAgent?: string; cookie?: string; storedToken?: string; pinned?: boolean;
  }): Promise<Signer>;
  sign(url: string, product?: 'fetch' | 'frontier' | 'ws'): string;
}

export function loadBundle(options?: { url?: string; cachePath?: string; maxAgeMs?: number }): Promise<string>;
export function decodeEvent(method: string, payload: Uint8Array): LiveEvent;
export function decodeUser(payload?: Uint8Array): EventUser;
export function label(user: EventUser): string;
export function cookieFromFile(path: string): string;
export function sessionJar(): Map<string, string>;
export function sessionPath(): string | null;
export function cookieHeader(jar: Map<string, string>, extra?: Record<string, string>): string;
export function parseCookies(raw: string): Map<string, string>;

export const DEFAULT_RECONNECT: Readonly<ReconnectPolicy>;
export const ROOM_STATUS_LIVE: 2;
export const USER_AGENT: string;
export const BUNDLE_URL: string;
export const PRODUCT: Readonly<{ fetch: 'fetch'; frontier: 'frontier'; ws: 'ws' }>;
export const EVENT: Readonly<Record<LiveEvent['type'], LiveEvent['type']>>;
export const METHOD: Readonly<Record<'chat' | 'gift' | 'like' | 'member' | 'social' | 'roomUser', string>>;
export const SOCIAL_ACTION: Readonly<{ follow: 1; share: 3 }>;
export const IDENTITY: Readonly<{ audience: 'audience'; anchor: 'anchor' }>;
export const SOCKET_HOST: Readonly<{ global: string; us: string; eu: string }>;
