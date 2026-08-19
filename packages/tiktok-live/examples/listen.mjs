// Listen to a live room and print what arrives.
//
//   node examples/listen.mjs @someone [seconds]
//   node examples/listen.mjs                      # pick a room that is live now
//
// AUTHORIZED USE ONLY: this opens a real connection to a real room.

import { Discovery, TikTokLive, cookieHeader, label, sessionJar } from '../src/index.mjs';

const requested = process.argv[2];
const seconds = Number(process.argv[3] || 20);

const user = requested ?? (await pickALiveRoom());
const live = new TikTokLive(user);

live.on('chat', (event) => console.log(`  💬 ${label(event.user)}: ${event.comment}`));
live.on('gift', (event) => {
  if (event.streakable && !event.repeatEnd) return; // only the final message of a streak is real
  console.log(
    `  🎁 ${label(event.user)} sent ${event.repeatCount}× ${event.giftName}` +
      ` (${event.diamondCount * event.repeatCount} diamonds)`,
  );
});
live.on('member', (event) => console.log(`  👋 ${label(event.user)} joined`));
live.on('social', (event) => console.log(`  ⭐ ${label(event.user)} followed or shared`));
live.on('like', (event) => console.log(`  ❤️  ${label(event.user)} +${event.count}`));
live.on('roomUser', (event) => console.log(`  👀 ${event.viewers} viewers`));
live.on('reconnecting', ({ attempt, delayMs }) =>
  console.log(`  reconnecting (attempt ${attempt}) in ${delayMs} ms`));
live.on('disconnected', ({ code }) => console.log(`  disconnected (${code})`));
live.on('error', (error) => console.error(`  error: ${error.message}`));

const state = await live.connect();
console.log(`connected to @${state.uniqueId}, room ${state.roomId}, ${state.giftCount} gifts\n`);

setTimeout(() => {
  live.disconnect();
  console.log('\ndone');
  process.exit(0);
}, seconds * 1000);

async function pickALiveRoom() {
  // Search wants the session too — without one it answers "Please login your account first".
  const rooms = await new Discovery({ cookie: cookieHeader(sessionJar()) }).liveChannels('live');
  if (!rooms.length) throw new Error('no live rooms found');
  console.log(`picked @${rooms[0].uniqueId} (${rooms[0].viewers} viewers)`);
  return rooms[0].uniqueId;
}
