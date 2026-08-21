// The unsigned endpoints: their URLs, and the shapes they answer with.
//
// Parsing is pinned against recorded shapes rather than live responses, so this runs offline. The
// live half is `examples/listen.ts` through `npm run listen`.

import assert from 'node:assert/strict';
import test from 'node:test';

import { Discovery, ROOM_STATUS_LIVE, giftListUrl, roomInfoUrl, roomLookupUrl, liveSearchUrl } from '../dist/discovery.js';

test('the endpoints are the ones the player uses', () => {
  assert.equal(
    roomLookupUrl('@someone'),
    'https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId=someone',
  );
  assert.ok(roomInfoUrl('7300').startsWith('https://webcast.tiktok.com/webcast/room/info/?'));
  assert.ok(giftListUrl('7300').includes('room_id=7300'));
  assert.ok(liveSearchUrl('live').includes('keyword=live'));
});

// `room_id` outlives the broadcast, so "has a room id" is not "is live". Signing a finished room
// produces a handshake refused in a way indistinguishable from a bad signature.
test('a room that ended is not live even though it still has an id', async () => {
  const ended = await parse({
    data: { user: { uniqueId: 'someone', roomId: '7300000000000000001', status: 4 } },
  });
  assert.equal(ended.isLive, false);

  const live = await parse({
    data: {
      user: { uniqueId: 'someone', roomId: '7300000000000000001', status: ROOM_STATUS_LIVE },
      liveRoom: { title: 'a title' },
    },
  });
  assert.equal(live.isLive, true);
  assert.equal(live.title, 'a title');
});

// Every webcast JSON endpoint reports failure as a non-zero `status_code` while still answering
// 200. Treating that as data is how a refusal becomes "the room has no gifts".
test('a refusal is an error, not an empty room', async () => {
  const discovery = withResponse({ status_code: 4003110, data: { message: 'rate limited' } });
  await assert.rejects(() => discovery.roomInfo('7300'), /rate limited/);

  const search = withResponse({ status_code: 2483, status_msg: 'Please login your account first' });
  await assert.rejects(() => search.liveChannels('live'), /login/);
});

test('the gift table is keyed by id and keeps what a gift event omits', async () => {
  const discovery = withResponse({
    status_code: 0,
    data: {
      gifts: [
        { id: 5655, name: 'Rose', describe: 'sent Rose', diamond_count: 1, combo: true, type: 1,
          icon: { url_list: ['https://p16.tiktokcdn.com/rose.webp'] } },
      ],
    },
  });
  const gifts = await discovery.giftList('7300');
  const rose = gifts.get('5655');
  assert.ok(rose);
  assert.equal(rose.name, 'Rose');
  assert.equal(rose.diamondCount, 1);
  assert.equal(rose.combo, true, 'streakable gifts must be identifiable, or diamonds double-count');
  assert.equal(rose.iconUrl, 'https://p16.tiktokcdn.com/rose.webp');
});

// The search response carries each room as a JSON string inside itself, which is why reading the
// outer fields yields nothing.
test('live search reads the room out of raw_data', async () => {
  const discovery = withResponse({
    data: [
      { live_info: { raw_data: JSON.stringify({
        status: 2, id_str: '7300000000000000001', title: 'a title', user_count: 42,
        owner: { display_id: 'someone', nickname: 'Some One' },
      }) } },
      { live_info: { raw_data: JSON.stringify({ status: 4, id_str: '7300000000000000002' }) } },
    ],
  });
  const rooms = await discovery.liveChannels('live');
  assert.equal(rooms.length, 1, 'a room that ended is not a live channel');
  assert.deepEqual(rooms[0], {
    uniqueId: 'someone', roomId: '7300000000000000001', nickname: 'Some One',
    title: 'a title', viewers: 42,
  });
});

function withResponse(body: unknown): Discovery {
  const discovery = new Discovery();
  globalThis.fetch = async () => Response.json(body);
  return discovery;
}

const parse = async (body: unknown) => withResponse(body).roomLookup('@someone');
