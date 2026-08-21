// The events, normalised.
//
// The socket carries dozens of message types and this file models six of them — the six that make
// up almost all of a room's traffic. Everything else arrives as `{ type: 'unknown', method,
// payload }` with its bytes intact, so nothing is lost and a caller who needs one more message can
// decode it without waiting for this file to grow.
//
// The field numbers are from `crates/ttl-live-proto/proto/v3/webcast/model/message/messages.proto`,
// which is the schema the Rust side decodes with. Only the fields a consumer actually uses are
// read; the rest are skipped by the wire reader, which is why a schema that gains a field does not
// break this.

import { asCount, asId, asString, read } from './protobuf.js';
import type { MessageShape, WireValue } from './protobuf.js';
import type {
  ChatEvent,
  EventUser,
  GiftEvent,
  LikeEvent,
  LiveEvent,
  MemberEvent,
  RoomUserEvent,
  SocialEvent,
  UnknownEvent,
} from './types.js';

/// Schema method names for the events normalised here.
export const METHOD = Object.freeze({
  chat: 'WebcastChatMessage',
  gift: 'WebcastGiftMessage',
  like: 'WebcastLikeMessage',
  member: 'WebcastMemberMessage',
  social: 'WebcastSocialMessage',
  roomUser: 'WebcastRoomUserSeqMessage',
});

/// The event names a client emits, including the two that are not schema messages.
export const EVENT = Object.freeze({
  chat: 'chat',
  gift: 'gift',
  like: 'like',
  member: 'member',
  social: 'social',
  roomUser: 'roomUser',
  unknown: 'unknown',
});

// --- user ------------------------------------------------------------------------------------

const USER = {
  1: ['id', asId],
  3: ['nickname', asString],
  38: ['displayId', asString],
  46: ['secUid', asString],
} satisfies MessageShape;

interface DecodedUser {
  id: string;
  nickname: string;
  displayId: string;
  secUid: string;
}

/// The stable slice of a user: who they are, under the names the Node ecosystem already uses.
///
/// Deliberately small. These four fields have been present across every schema version seen, so a
/// consumer keeps working when the surrounding `User` message shifts.
export function decodeUser(payload?: Uint8Array): EventUser {
  const user = payload ? read<DecodedUser>(payload, USER) : {};
  return {
    userId: user.id ?? '0',
    nickname: user.nickname ?? '',
    /// The `@handle`. `display_id` in the schema, `uniqueId` in the Node connector.
    uniqueId: user.displayId ?? '',
    secUid: user.secUid ?? '',
  };
}

const decodeUserField = (value: WireValue): EventUser =>
  decodeUser(value instanceof Uint8Array ? value : undefined);

/// Best available label for a user, preferring the stable handle.
export const label = (user: EventUser): string => user.uniqueId || user.nickname || 'unknown';

// --- the six ---------------------------------------------------------------------------------

const CHAT = { 2: ['user', decodeUserField], 3: ['content', asString] } satisfies MessageShape;

const GIFT_DETAIL = {
  5: ['id', asId], 12: ['diamondCount', asCount], 16: ['name', asString],
} satisfies MessageShape;
const GIFT = {
  2: ['giftId', asId],
  5: ['repeatCount', asCount],
  6: ['comboCount', asCount],
  7: ['user', decodeUserField],
  8: ['toUser', decodeUserField],
  9: ['repeatEnd', asCount],
  11: ['groupId', asId],
  15: ['gift', (value: WireValue) =>
    read<DecodedGiftDetail>(value instanceof Uint8Array ? value : new Uint8Array(), GIFT_DETAIL)],
} satisfies MessageShape;

interface DecodedGiftDetail {
  id: string;
  diamondCount: number;
  name: string;
}

interface DecodedChat { user: EventUser; content: string }
interface DecodedGift {
  giftId: string;
  repeatCount: number;
  comboCount: number;
  user: EventUser;
  toUser: EventUser;
  repeatEnd: number;
  groupId: string;
  gift: DecodedGiftDetail;
}
interface DecodedLike { count: number; total: number; user: EventUser }
interface DecodedMember { user: EventUser; memberCount: number; action: number }
interface DecodedSocial { user: EventUser; action: number; followCount: number; shareCount: number }
interface DecodedRoomUser { total: number; popularity: number; totalUser: number; anonymous: number }

const LIKE = {
  2: ['count', asCount], 3: ['total', asCount], 5: ['user', decodeUserField],
} satisfies MessageShape;

const MEMBER = {
  2: ['user', decodeUserField], 3: ['memberCount', asCount], 10: ['action', asCount],
} satisfies MessageShape;

const SOCIAL = {
  2: ['user', decodeUserField],
  4: ['action', asCount],
  6: ['followCount', asCount],
  8: ['shareCount', asCount],
} satisfies MessageShape;

const ROOM_USER = {
  3: ['total', asCount],
  6: ['popularity', asCount],
  7: ['totalUser', asCount],
  8: ['anonymous', asCount],
} satisfies MessageShape;

/// `WebcastSocialMessage.action`, which is how a follow and a share arrive on the same message.
export const SOCIAL_ACTION = Object.freeze({ follow: 1, share: 3 });

const NORMALIZE = {
  [METHOD.chat]: (payload: Uint8Array): Omit<ChatEvent, 'method'> => {
    const message = read<DecodedChat>(payload, CHAT);
    return { type: EVENT.chat, user: message.user ?? decodeUser(), comment: message.content ?? '' };
  },
  [METHOD.gift]: (payload: Uint8Array): Omit<GiftEvent, 'method'> => {
    const message = read<DecodedGift>(payload, GIFT);
    // The nested detail block is omitted on repeat messages of a streak, so the name and price
    // fall back rather than failing: a gift with no name is still a gift.
    return {
      type: EVENT.gift,
      user: message.user ?? decodeUser(),
      toUser: message.toUser ?? decodeUser(),
      giftId: message.giftId ?? '0',
      giftName: message.gift?.name ?? '',
      diamondCount: message.gift?.diamondCount ?? 0,
      repeatCount: message.repeatCount ?? 0,
      comboCount: message.comboCount ?? 0,
      groupId: message.groupId ?? '0',
      /// A streak sends one message per gift; only the last one is final. Counting the others
      /// double-counts diamonds, which is the classic bug in a gift tally.
      repeatEnd: Boolean(message.repeatEnd),
    };
  },
  [METHOD.like]: (payload: Uint8Array): Omit<LikeEvent, 'method'> => {
    const message = read<DecodedLike>(payload, LIKE);
    return {
      type: EVENT.like,
      user: message.user ?? decodeUser(),
      count: message.count ?? 0,
      total: message.total ?? 0,
    };
  },
  [METHOD.member]: (payload: Uint8Array): Omit<MemberEvent, 'method'> => {
    const message = read<DecodedMember>(payload, MEMBER);
    return {
      type: EVENT.member,
      user: message.user ?? decodeUser(),
      memberCount: message.memberCount ?? 0,
      action: message.action ?? 0,
    };
  },
  [METHOD.social]: (payload: Uint8Array): Omit<SocialEvent, 'method'> => {
    const message = read<DecodedSocial>(payload, SOCIAL);
    return {
      type: EVENT.social,
      user: message.user ?? decodeUser(),
      action: message.action ?? 0,
      followCount: message.followCount ?? 0,
      shareCount: message.shareCount ?? 0,
    };
  },
  [METHOD.roomUser]: (payload: Uint8Array): Omit<RoomUserEvent, 'method'> => {
    const message = read<DecodedRoomUser>(payload, ROOM_USER);
    return {
      type: EVENT.roomUser,
      viewers: message.total ?? 0,
      popularity: message.popularity ?? 0,
      totalUser: message.totalUser ?? 0,
      anonymous: message.anonymous ?? 0,
    };
  },
};

/// Normalise one message. Never throws: an unmodelled method, or a payload that does not decode,
/// becomes `unknown` with its bytes kept, so one bad event cannot take a batch down.
export function decodeEvent(method: typeof METHOD.chat, payload: Uint8Array): ChatEvent | UnknownEvent;
export function decodeEvent(method: typeof METHOD.gift, payload: Uint8Array): GiftEvent | UnknownEvent;
export function decodeEvent(method: typeof METHOD.like, payload: Uint8Array): LikeEvent | UnknownEvent;
export function decodeEvent(method: typeof METHOD.member, payload: Uint8Array): MemberEvent | UnknownEvent;
export function decodeEvent(method: typeof METHOD.social, payload: Uint8Array): SocialEvent | UnknownEvent;
export function decodeEvent(method: typeof METHOD.roomUser, payload: Uint8Array): RoomUserEvent | UnknownEvent;
export function decodeEvent(method: string, payload: Uint8Array): LiveEvent;
export function decodeEvent(method: string, payload: Uint8Array): LiveEvent {
  const normalize = (NORMALIZE as Record<string, ((bytes: Uint8Array) => object) | undefined>)[method];
  if (!normalize) return { type: EVENT.unknown, method, payload };
  try {
    return { ...normalize(payload), method } as LiveEvent;
  } catch {
    return { type: EVENT.unknown, method, payload };
  }
}
