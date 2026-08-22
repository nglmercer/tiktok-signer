export interface EventUser {
  /** 64-bit id as a decimal string: it exceeds `Number.MAX_SAFE_INTEGER`. */
  userId: string;
  nickname: string;
  /** The `@handle`. */
  uniqueId: string;
  secUid: string;
  /** Avatar thumbnail URL extracted from field 9 if present. */
  avatarUrl?: string;
}

export interface BaseEvent {
  method: string;
  msgId?: string;
  isHistory?: boolean;
}

export interface ChatEvent extends BaseEvent {
  type: 'chat';
  user: EventUser;
  comment: string;
}

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
  repeatEnd: boolean;
  streakable?: boolean;
  giftIconUrl?: string;
}

export interface LikeEvent extends BaseEvent {
  type: 'like';
  user: EventUser;
  count: number;
  total: number;
}

export interface MemberEvent extends BaseEvent {
  type: 'member';
  user: EventUser;
  memberCount: number;
  action: number;
}

export interface SocialEvent extends BaseEvent {
  type: 'social';
  user: EventUser;
  action: number;
  followCount: number;
  shareCount: number;
}

export interface RoomUserEvent extends BaseEvent {
  type: 'roomUser';
  viewers: number;
  popularity: number;
  totalUser: number;
  anonymous: number;
}

export interface UnknownEvent extends BaseEvent {
  type: 'unknown';
  payload: Uint8Array;
}

export type LiveEvent =
  | ChatEvent
  | GiftEvent
  | LikeEvent
  | MemberEvent
  | SocialEvent
  | RoomUserEvent
  | UnknownEvent;

export interface RoomOwner {
  userId: string;
  uniqueId: string;
  nickname: string;
  secUid: string;
  avatarUrl: string;
  followerCount: number;
}

export interface RoomInfo {
  roomId: string;
  title: string;
  status: number;
  viewers: number;
  likes: number;
  comments: number;
  shares: number;
  follows: number;
  coverUrl: string;
  shareUrl: string;
  owner: RoomOwner;
}

export interface RoomLookup {
  uniqueId: string;
  roomId: string;
  nickname: string;
  status: number;
  title: string;
  isLive: boolean;
}

export interface LiveRoom {
  uniqueId: string;
  roomId: string;
  nickname: string;
  title: string;
  viewers: number;
}

export interface Gift {
  id: string;
  name: string;
  describe: string;
  diamondCount: number;
  combo: boolean;
  giftType: number;
  iconUrl: string;
}

export interface ReconnectPolicy {
  attempts: number;
  initialMs: number;
  maxMs: number;
}

export interface ClientState {
  uniqueId: string;
  roomId: string;
  connected: boolean;
  roomInfo: RoomInfo | null;
  giftCount: number;
}

export interface WebSocketOptions {
  headers: Record<string, string>;
}

export interface WebSocketLike {
  binaryType: string;
  send(data: string | ArrayBuffer | ArrayBufferView): void;
  close(): void;
  addEventListener(type: 'open', listener: () => void): void;
  addEventListener(type: 'message', listener: (event: { data: ArrayBuffer | ArrayBufferView }) => void): void;
  addEventListener(type: 'error', listener: (event: { message?: string }) => void): void;
  addEventListener(type: 'close', listener: (event: { code: number; reason?: string }) => void): void;
}

export interface WebSocketConstructor {
  new (url: string, options: WebSocketOptions): WebSocketLike;
}
