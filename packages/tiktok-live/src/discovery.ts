// Everything about a room that needs no signature.
//
// Which is nearly everything: `unique_id` → `room_id`, room metadata, the gift table, and the list
// of who is broadcasting now. All four were signed here until a one-character tamper test showed
// none of them verifies a signature — a correct signature, a corrupted one, and none at all return
// the same data. Only the socket checks. `scripts/headless/verify-probe.mjs` reproduces it.
//
// The Rust statement of these URLs and shapes is `crates/ttl-sign-core/src/room.rs`; this is the
// same set, and `test/discovery.test.ts` pins the parsing against recorded shapes.

import { USER_AGENT } from './session.js';
import type { Gift, LiveRoom, RoomInfo, RoomLookup } from './types.js';

const WEBCAST_BASE = 'https://webcast.tiktok.com/webcast';
const AID = '1988';

/// TikTok reports `2` while broadcasting and `4` once a session ends. The `room_id` survives the
/// end of a broadcast, so "has a room id" is not "is live": signing a finished room yields a
/// handshake that is refused in a way indistinguishable from a bad signature.
export const ROOM_STATUS_LIVE = 2;

export const roomLookupUrl = (uniqueId: string): string =>
  `https://www.tiktok.com/api-live/user/room/?aid=${AID}&sourceType=54&uniqueId=${encodeURIComponent(strip(uniqueId))}`;

const webcastUrl = (path: string, roomId: string): string =>
  `${WEBCAST_BASE}/${path}/?aid=${AID}&app_language=en&device_platform=web&room_id=${encodeURIComponent(roomId)}`;

export const roomInfoUrl = (roomId: string): string => webcastUrl('room/info', roomId);
export const giftListUrl = (roomId: string): string => webcastUrl('gift/list', roomId);

/// The live search endpoint, which returns as JSON what `/live` renders with JavaScript.
export function liveSearchUrl(keyword = 'live', offset = 0): string {
  const pairs: ReadonlyArray<readonly [string, string]> = [
    ['aid', AID], ['app_language', 'en'], ['app_name', 'tiktok_web'],
    ['browser_language', 'en-US'], ['browser_name', 'Mozilla'],
    ['browser_platform', 'Linux x86_64'], ['browser_version', '5.0 (X11)'],
    ['cookie_enabled', 'true'], ['count', '20'], ['device_platform', 'web_pc'],
    ['focus_state', 'true'], ['from_page', 'search'], ['history_len', '4'],
    ['is_fullscreen', 'false'], ['is_page_visible', 'true'], ['keyword', keyword],
    ['offset', String(offset)], ['os', 'linux'], ['priority_region', 'US'], ['region', 'US'],
    ['screen_height', '1080'], ['screen_width', '1920'], ['tz_name', 'America/New_York'],
    ['webcast_language', 'en'],
  ];
  return `https://www.tiktok.com/api/search/live/full/?${pairs
    .map(([key, value]) => `${key}=${encodeURIComponent(value)}`)
    .join('&')}`;
}

const strip = (uniqueId: string): string => uniqueId.replace(/^@/, '');

/// A refusal TikTok reports while still answering 200.
export class WebcastRefusal extends Error {
  readonly statusCode: number;

  constructor(statusCode: number, message?: string) {
    super(message || `TikTok refused the request (status_code=${statusCode})`);
    this.name = 'WebcastRefusal';
    this.statusCode = statusCode;
  }
}

/// Reads the unsigned endpoints.
export class Discovery {
  readonly cookie: string;
  readonly userAgent: string;
  readonly timeoutMs: number;

  constructor({ cookie = '', userAgent = USER_AGENT, timeoutMs = 10_000 }: DiscoveryOptions = {}) {
    this.cookie = cookie;
    this.userAgent = userAgent;
    this.timeoutMs = timeoutMs;
  }

  async #json<T>(url: string): Promise<T> {
    const response = await fetch(url, {
      headers: {
        'user-agent': this.userAgent,
        referer: 'https://www.tiktok.com/',
        ...(this.cookie ? { cookie: this.cookie } : {}),
      },
      signal: AbortSignal.timeout(this.timeoutMs),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status} from ${new URL(url).pathname}`);
    return response.json() as Promise<T>;
  }

  /// `@handle` → room. Returns `{ uniqueId, roomId, nickname, status, title, isLive }`.
  async roomLookup(uniqueId: string): Promise<RoomLookup> {
    const body = await this.#json<LookupResponse>(roomLookupUrl(uniqueId));
    const user = body?.data?.user;
    if (!user) throw new Error(`no room data for @${strip(uniqueId)}`);
    const liveRoom = body?.data?.liveRoom;
    const status = user.status ?? liveRoom?.status ?? 0;
    const roomId = String(user.roomId ?? '');
    return {
      uniqueId: user.uniqueId ?? strip(uniqueId),
      roomId,
      nickname: user.nickname ?? '',
      status,
      title: liveRoom?.title ?? '',
      isLive: status === ROOM_STATUS_LIVE && roomId !== '' && roomId !== '0',
    };
  }

  /// Room metadata: title, owner, counters.
  async roomInfo(roomId: string): Promise<RoomInfo> {
    const body = await this.#json<RoomInfoResponse>(roomInfoUrl(roomId));
    if (body?.status_code !== 0) throw new WebcastRefusal(body?.status_code ?? -1, body?.data?.message);
    const data = body.data ?? {};
    const stats = data.stats ?? {};
    const owner = data.owner ?? {};
    return {
      roomId: data.id_str ?? String(roomId),
      title: data.title ?? '',
      status: data.status ?? 0,
      viewers: stats.total_user ?? data.user_count ?? 0,
      likes: stats.like_count ?? 0,
      comments: stats.comment_count ?? 0,
      shares: stats.share_count ?? 0,
      follows: stats.follow_count ?? 0,
      coverUrl: firstUrl(data.cover),
      shareUrl: data.share_url ?? '',
      owner: {
        userId: owner.id_str ?? '',
        uniqueId: owner.display_id ?? '',
        nickname: owner.nickname ?? '',
        secUid: owner.sec_uid ?? '',
        avatarUrl: firstUrl(owner.avatar_thumb),
        followerCount: owner.follow_info?.follower_count ?? 0,
      },
    };
  }

  /// Every gift the room offers, keyed by id.
  ///
  /// About 2.6 MB for 600-odd gifts, so read it once per connection rather than per event. It is
  /// what turns a `gift` event into a diamond value when the event's own detail block is omitted,
  /// which happens on every repeat of a streak.
  async giftList(roomId: string): Promise<Map<string, Gift>> {
    const body = await this.#json<GiftListResponse>(giftListUrl(roomId));
    if (body?.status_code !== 0) throw new WebcastRefusal(body?.status_code ?? -1, body?.data?.message);
    const gifts = new Map<string, Gift>();
    for (const gift of body.data?.gifts ?? []) {
      gifts.set(String(gift.id), {
        id: String(gift.id),
        name: gift.name ?? '',
        describe: gift.describe ?? '',
        diamondCount: gift.diamond_count ?? 0,
        /// Streakable gifts arrive as a burst with a rising `repeatCount`, and only the last one
        /// is the real total. Counting every message multiplies what the sender actually spent.
        combo: Boolean(gift.combo),
        giftType: gift.type ?? 0,
        iconUrl: firstUrl(gift.icon),
      });
    }
    return gifts;
  }

  /// Rooms broadcasting now, most viewers first.
  ///
  /// Results follow the keyword, so this samples live rooms rather than enumerating them. Each
  /// entry's real content is a JSON *string* under `live_info.raw_data` — the search response
  /// carries the room object serialised inside itself, which is why the outer fields look empty.
  async liveChannels(keyword = 'live'): Promise<LiveRoom[]> {
    const body = await this.#json<LiveSearchResponse>(liveSearchUrl(keyword));
    // Search is the one unsigned endpoint that wants a session: without one it answers 200 with
    // `status_code: 2483, "Please login your account first"`, which as an empty list would look
    // like "nobody is live".
    if (body?.status_code) throw new WebcastRefusal(body.status_code, body.status_msg);
    const rooms: LiveRoom[] = [];
    for (const item of body?.data ?? []) {
      let room: SearchRoom;
      try {
        room = JSON.parse(item.live_info?.raw_data ?? '') as SearchRoom;
      } catch {
        continue;
      }
      if (room?.status !== ROOM_STATUS_LIVE) continue;
      const uniqueId = room?.owner?.display_id ?? '';
      const roomId = String(room?.id_str ?? '');
      if (!uniqueId || !roomId || roomId === '0') continue;
      rooms.push({
        uniqueId,
        roomId,
        nickname: room?.owner?.nickname ?? '',
        title: room?.title ?? '',
        viewers: Number(room?.user_count ?? 0),
      });
    }
    rooms.sort((left, right) => right.viewers - left.viewers);
    return rooms;
  }
}

const firstUrl = (image?: Image): string => image?.url_list?.[0] ?? '';

export interface DiscoveryOptions {
  cookie?: string;
  userAgent?: string;
  timeoutMs?: number;
}

interface Image { url_list?: string[] }
interface LookupResponse {
  data?: {
    user?: { uniqueId?: string; roomId?: string | number; nickname?: string; status?: number };
    liveRoom?: { status?: number; title?: string };
  };
}
interface OwnerResponse {
  id_str?: string;
  display_id?: string;
  nickname?: string;
  sec_uid?: string;
  avatar_thumb?: Image;
  follow_info?: { follower_count?: number };
}
interface RoomInfoResponse {
  status_code?: number;
  data?: {
    message?: string;
    id_str?: string;
    title?: string;
    status?: number;
    user_count?: number;
    cover?: Image;
    share_url?: string;
    owner?: OwnerResponse;
    stats?: {
      total_user?: number;
      like_count?: number;
      comment_count?: number;
      share_count?: number;
      follow_count?: number;
    };
  };
}
interface GiftResponse {
  id: string | number;
  name?: string;
  describe?: string;
  diamond_count?: number;
  combo?: boolean;
  type?: number;
  icon?: Image;
}
interface GiftListResponse {
  status_code?: number;
  data?: { message?: string; gifts?: GiftResponse[] };
}
interface SearchRoom {
  status?: number;
  id_str?: string;
  title?: string;
  user_count?: number;
  owner?: { display_id?: string; nickname?: string };
}
interface LiveSearchResponse {
  status_code?: number;
  status_msg?: string;
  data?: Array<{ live_info?: { raw_data?: string } }>;
}
