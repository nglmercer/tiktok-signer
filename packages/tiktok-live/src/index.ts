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
// JavaScript engine, so no native engine has to be embedded. TypeScript is compiled to the ESM in
// `dist`; see `README.md`.

export { TikTokLive, DEFAULT_RECONNECT, NotLiveError, cookieFromFile } from './client.js';
export type { SignerLike, TikTokLiveEvents, TikTokLiveOptions } from './client.js';
export { Discovery, WebcastRefusal, ROOM_STATUS_LIVE } from './discovery.js';
export { EVENT, METHOD, SOCIAL_ACTION, decodeEvent, decodeUser, label } from './events.js';
export { Signer, PRODUCT, BUNDLE_SHA256, BUNDLE_URL, loadBundle } from './signer.js';
export { USER_AGENT, cookieHeader, parseCookies, sessionJar, sessionPath } from './session.js';
export { ackFrame, decodeBatch, decodePushFrame, decompress } from './frames.js';
export { IDENTITY, SOCKET_HOST, PATH, socketConfig, socketQuery } from './player.js';
export type { BrowserBlockOptions, Compression, Identity, SocketConfig, SocketConfigOptions } from './player.js';
export type { LoadBundleOptions, Product, SignerOptions } from './signer.js';
export type * from './types.js';
