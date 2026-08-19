// TikTok LIVE events in Node: no sign server, no native module, no browser.
//
// ```js
// import { TikTokLive } from 'ttl-live';
//
// const live = new TikTokLive('@someone');
// live.on('chat', (event) => console.log(`${event.user.uniqueId}: ${event.comment}`));
// await live.connect();
// ```
//
// The signature comes from TikTok's own bundle running in a `node:vm` context — Node already has a
// JavaScript engine, so nothing has to be embedded or compiled. See `README.md`.

export { TikTokLive, DEFAULT_RECONNECT, cookieFromFile } from './client.mjs';
export { Discovery, WebcastRefusal, ROOM_STATUS_LIVE } from './discovery.mjs';
export { EVENT, METHOD, SOCIAL_ACTION, decodeEvent, decodeUser, label } from './events.mjs';
export { Signer, PRODUCT, BUNDLE_URL, loadBundle } from './signer.mjs';
export { USER_AGENT, cookieHeader, parseCookies, sessionJar, sessionPath } from './session.mjs';
export { ackFrame, decodeBatch, decodePushFrame, decompress } from './frames.mjs';
export { IDENTITY, SOCKET_HOST, PATH, socketConfig, socketQuery } from './player.mjs';
