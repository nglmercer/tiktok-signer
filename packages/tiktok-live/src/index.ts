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
// The signature comes from TikTok's own bundle running in QuickJS compiled to WebAssembly — the same
// engine every other signer in this repository uses, with no native addon to build. TypeScript is
// compiled to the ESM in `dist`; see `README.md`.

export { TikTokLive, DEFAULT_RECONNECT, NotLiveError, cookieFromFile } from './client.js';
export type { SignerLike, TikTokLiveEvents, TikTokLiveOptions } from './client.js';
export { Discovery, WebcastRefusal, ROOM_STATUS_LIVE, parseId } from './discovery.js';
export {
  EVENT,
  METHOD,
  MAX_EMOTES_PER_EVENT,
  MAX_EMOTE_ID_LENGTH,
  MAX_EMOTE_URL_LENGTH,
  SOCIAL_ACTION,
  classifySocialAction,
  decodeEvent,
  decodeUser,
  label,
  normalizeEmote,
  normalizeInlineEmotes,
  normalizeStandaloneEmotes,
  safeCount,
} from './events.js';
export type { SocialClassification, SocialConfidence, SocialKind } from './events.js';
export { JsonValidationError } from './json-validation.js';
export { Signer, PRODUCT, BUNDLE_SHA256, BUNDLE_URL, loadBundle } from './signer.js';
export {
  GUEST_BOOTSTRAP_URL,
  GuestSessionError,
  USER_AGENT,
  bootstrapGuestSession,
  cookieHeader,
  parseCookies,
  sessionJar,
  sessionPath,
} from './session.js';
export type {
  GuestSessionErrorCode,
  GuestSessionOptions,
  SessionIdentity,
} from './session.js';
export {
  ackFrame,
  ackFrameForBatch,
  carriesEvents,
  decodeBatch,
  decodePushFrame,
  decompress,
  frameCompressType,
  frameHeaders,
} from './frames.js';
export { IDENTITY, SOCKET_HOST, PATH, socketConfig, socketQuery } from './player.js';
export type {
  BrowserBlockOptions,
  Compression,
  Identity,
  QueryPrimitive,
  QueryValue,
  SocketConfig,
  SocketConfigOptions,
} from './player.js';
export type { LoadBundleOptions, Product, SignerOptions } from './signer.js';
export type * from './types.js';

export type { DeliveryOptions, DeliveryMetrics } from './delivery.js';
