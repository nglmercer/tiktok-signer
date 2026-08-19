// The live player's transport, transcribed once.
//
// The web player builds its message-socket URL itself and signs the query bytes, so every byte of
// that query — key spelling, key order, the absence of percent-encoding — is part of the signature.
// This module is the single JavaScript statement of it: the constants, the serializer, and the two
// frames the socket exchanges.
//
// Nothing here is invented. Every value is read out of the player's own chunk, and
// `player-audit.mjs` re-reads them from the shipped app on demand and fails when they move. The
// numbers in `PUSH_FRAME_FIELD` and friends are the protobuf field ids from the SDK's descriptors,
// named rather than spelled inline, because a bare `6` in an encoder is unreviewable.
//
// `crates/ttl-sign-core/src/params.rs` is the Rust statement of the same thing, and the two are held
// together by a test rather than by discipline: `TTL_PRINT_QUERY=1 node ws-direct.mjs` prints the
// query this module builds, and `direct_socket_query_matches_the_player` asserts the Rust builder
// produces it byte for byte.

import { USER_AGENT } from './session.mjs';

/// Socket hosts, by cluster region. The player picks between exactly these three.
export const SOCKET_HOST = Object.freeze({
  global: 'wss://webcast-ws.tiktok.com',
  us: 'wss://webcast-ws.us.tiktok.com',
  eu: 'wss://webcast-ws.eu.tiktok.com',
});

/// The webcast paths the transport knows.
export const PATH = Object.freeze({
  fetch: '/webcast/im/fetch/',
  fetchHistory: '/webcast/im/fetch/history/',
  fetchPreview: '/webcast/im/fetch/preview/',
  /// What `wsDirect` opens. This is the transport.
  wsReuseSupplement: '/webcast/im/ws_proxy/ws_reuse_supplement/',
  wsFromPreview: '/webcast/im/ws_proxy/from_preview/',
});

/// The two `version_code` values, which are different and both present.
///
/// The SDK's browser block carries `SDK_VERSION_CODE` under a snake_case key and the page config
/// `CONFIG_VERSION_CODE` under a camelCase one, so the serializer emits `version_code` twice. It
/// looks like a bug and is not: a query with only one of them is not the query that gets signed.
export const SDK_VERSION_CODE = '180800';
export const CONFIG_VERSION_CODE = '270000';
/// The IM SDK's own version, sent alongside both.
export const UPDATE_VERSION_CODE = '2.0.0';

export const AID = '1988';
export const APP_NAME = 'tiktok_web';
export const LIVE_ID = '12';
export const HEARTBEAT_MS = '10000';

/// Who the client claims to be in the room.
export const IDENTITY = Object.freeze({ audience: 'audience', anchor: 'anchor' });

/// Frame compression the socket will negotiate.
export const COMPRESSION = Object.freeze({ gzip: 'gzip', none: '' });

/// `payload_type` values on a `PushFrame`.
export const FRAME_TYPE = Object.freeze({
  enterRoom: 'im_enter_room',
  previewRoom: 'im_preview_room',
  heartbeat: 'hb',
  ack: 'ack',
});

/// `payload_encoding`: protobuf, for every frame this module builds.
const PAYLOAD_ENCODING_PB = 'pb';

// --- protobuf field numbers, from the SDK's own descriptors ---------------------------------------

export const PUSH_FRAME_FIELD = Object.freeze({
  seqId: 1, logId: 2, service: 3, method: 4, headers: 5,
  payloadEncoding: 6, payloadType: 7, payload: 8,
});

export const ENTER_ROOM_FIELD = Object.freeze({
  roomId: 1, roomTag: 2, liveRegion: 3, liveId: 4, identity: 5, cursor: 6,
  accountType: 7, enterUniqId: 8, filterWelcomeMsg: 9, isAnchorContinueKeepMsg: 10,
});

export const HEARTBEAT_FIELD = Object.freeze({ roomId: 1, sendPacketSeqId: 2 });

const WIRE_VARINT = 0;
const WIRE_LENGTH_DELIMITED = 2;

// --- the query the signature covers ----------------------------------------------------------------

/// The SDK's browser block, `k()`. Its `version_code` is the SDK default, which the config shadows.
export function browserBlock({
  userAgent = USER_AGENT,
  screenWidth = 1920,
  screenHeight = 1080,
  browserLanguage = 'en-US',
  browserPlatform = 'Linux x86_64',
  tzName = 'America/New_York',
} = {}) {
  return {
    version_code: SDK_VERSION_CODE,
    device_platform: 'web',
    cookie_enabled: 'true',
    screen_width: String(screenWidth),
    screen_height: String(screenHeight),
    browser_language: browserLanguage,
    browser_platform: browserPlatform,
    // `navigator.appCodeName` and `navigator.appVersion`: always "Mozilla", and the agent minus it.
    browser_name: 'Mozilla',
    browser_version: userAgent.replace(/^Mozilla\//, ''),
    browser_online: 'true',
    tz_name: tzName,
  };
}

/// `F()`: drop empties, objects, and the config keys that are not request parameters.
export function strip(props) {
  const out = { ...props };
  for (const key of Object.keys(out)) {
    if (out[key] === undefined || out[key] === '' || typeof out[key] === 'object') delete out[key];
  }
  for (const key of ['socketHost', 'host', 'fetchBeforeWsSuccess', 'debug', 'filterByRoomId']) {
    delete out[key];
  }
  return out;
}

/// `V()`: the browser block, then the config, then the fixed tail.
export function withDefaults(props, block = browserBlock()) {
  const { didRule, deviceId, ...rest } = props;
  const merged = {
    ...block,
    ...strip(rest),
    supWsDsOpt: '1',
    respContentType: 'protobuf',
    // 3 only when there is no device id to rule on.
    didRule: didRule ?? (deviceId ? 0 : 3),
    deviceId,
    webcastLanguage: rest.appLanguage,
  };
  for (const key of Object.keys(merged)) {
    if (merged[key] === undefined || merged[key] === '') delete merged[key];
  }
  return merged;
}

/// `H()`: camelCase to snake_case, and **no** percent-encoding. The signature covers these bytes.
export function serialize(params) {
  return Object.keys(params).reduce((acc, key) => {
    const name = key
      .replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`)
      .replace(/\s+/g, '_')
      .replace(/[^a-zA-Z0-9_]/g, '')
      .toLowerCase();
    return `${acc}${acc ? '&' : ''}${name}=${String(params[key])}`;
  }, '');
}

/// The config the live room page hands its IM SDK, with the caller's room and device filled in.
export function socketConfig({
  roomId,
  deviceId,
  identity = IDENTITY.audience,
  compress = COMPRESSION.gzip,
  socketHost = SOCKET_HOST.global,
  appLanguage = 'en',
} = {}) {
  return {
    aid: AID,
    appName: APP_NAME,
    liveId: LIVE_ID,
    versionCode: CONFIG_VERSION_CODE,
    appLanguage,
    socketHost,
    wsDirect: '1',
    fetchBeforeWsSuccess: '1',
    clientEnter: '1',
    roomId,
    identity,
    deviceId,
    compress,
    // `createClient` seeds these from the message state, which starts empty on a cold connect.
    lastRtt: '-1',
    cursor: '',
    internalExt: '',
    historyCommentCursor: '',
    heartbeatDuration: HEARTBEAT_MS,
  };
}

/// The query string the socket URL carries, exactly as the SDK builds it.
export function socketQuery(config, block = browserBlock()) {
  const { appName, didRule, routeParamsMap, pushServer, ...rest } = config;
  return serialize(withDefaults({
    appName,
    didRule,
    supWsDsOpt: '1',
    updateVersionCode: UPDATE_VERSION_CODE,
    compress: config.compress,
    webcastLanguage: config.appLanguage,
    ...block,
    ...(routeParamsMap || {}),
    ...strip(rest),
  }, block));
}

/// The unsigned socket URL. The signature is appended as `&X-Gnarly=<percent-encoded>`.
export function socketUrl(config, block = browserBlock()) {
  return `${config.socketHost}${PATH.wsReuseSupplement}?${socketQuery(config, block)}`;
}

// --- the frames ------------------------------------------------------------------------------------

function varint(value) {
  const out = [];
  let remaining = BigInt(value);
  do {
    let byte = Number(remaining & 0x7fn);
    remaining >>= 7n;
    if (remaining) byte |= 0x80;
    out.push(byte);
  } while (remaining);
  return out;
}

const tag = (field, wire) => varint((field << 3) | wire);
const int64Field = (field, value) => [...tag(field, WIRE_VARINT), ...varint(value)];
const bytesField = (field, value) => {
  const body = typeof value === 'string' ? Buffer.from(value, 'utf8') : Buffer.from(value);
  return [...tag(field, WIRE_LENGTH_DELIMITED), ...varint(body.length), ...body];
};

/// Wrap a payload in the `PushFrame` the socket expects.
///
/// `logId` is echoed back on an acknowledgement so the server can match it to the frame it sent;
/// frames the client originates leave it at zero, which is what the SDK does.
export function pushFrame(payloadType, payload, { logId = 0 } = {}) {
  return Buffer.from([
    ...(logId ? int64Field(PUSH_FRAME_FIELD.logId, logId) : []),
    ...bytesField(PUSH_FRAME_FIELD.payloadEncoding, PAYLOAD_ENCODING_PB),
    ...bytesField(PUSH_FRAME_FIELD.payloadType, payloadType),
    ...bytesField(PUSH_FRAME_FIELD.payload, payload),
  ]);
}

/// The frame that makes the server start pushing. Without it a healthy socket stays silent.
export function enterRoomFrame({ roomId, identity = IDENTITY.audience, liveId = LIVE_ID }) {
  const payload = Buffer.from([
    ...int64Field(ENTER_ROOM_FIELD.roomId, roomId),
    ...int64Field(ENTER_ROOM_FIELD.liveId, liveId),
    ...bytesField(ENTER_ROOM_FIELD.identity, identity),
    ...bytesField(ENTER_ROOM_FIELD.cursor, ''),
    ...int64Field(ENTER_ROOM_FIELD.accountType, 0),
    ...bytesField(ENTER_ROOM_FIELD.filterWelcomeMsg, '0'),
  ]);
  return pushFrame(FRAME_TYPE.enterRoom, payload);
}

/// The application keepalive. The socket closes without it; protocol pings are not answered.
export function heartbeatFrame(roomId) {
  return pushFrame(FRAME_TYPE.heartbeat, Buffer.from(int64Field(HEARTBEAT_FIELD.roomId, roomId)));
}
