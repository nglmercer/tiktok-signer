// The transport envelope: what a socket frame carries, and how to answer it.
//
// Every WebSocket message is a `PushFrame`. Only `payload_type: "msg"` carries events; `hb`,
// `ack` and `im_enter_room_resp` are the transport talking to itself. A `msg` frame's payload is
// a `ProtoMessageFetchResult` — the same envelope `/webcast/im/fetch/` used to return — usually
// gzipped, and it must be acknowledged or the server stops pushing.
//
// The encoders (`pushFrame`, `enterRoomFrame`, `heartbeatFrame`) are in `player.mjs`, beside the
// constants they serialise. This file only reads, plus the one frame that is a reply.

import { gunzipSync } from 'node:zlib';

import { asCount, asId, asString, fields, read } from './protobuf.mjs';
import { FRAME_TYPE, PUSH_FRAME_FIELD, pushFrame } from './player.mjs';

/// `payload_type` of a frame that carries events. Everything else is transport.
export const MESSAGE_PAYLOAD_TYPE = 'msg';

/// Header the server sets when the payload is compressed.
const COMPRESS_TYPE_HEADER = 'compress_type';

const MAP_ENTRY = { 1: ['key', asString], 2: ['value', asString] };

const PUSH_FRAME = {
  [PUSH_FRAME_FIELD.seqId]: ['seqId', asId],
  [PUSH_FRAME_FIELD.logId]: ['logId', asId],
  [PUSH_FRAME_FIELD.headers]: ['headers[]', (value) => read(value, MAP_ENTRY)],
  [PUSH_FRAME_FIELD.payloadEncoding]: ['payloadEncoding', asString],
  [PUSH_FRAME_FIELD.payloadType]: ['payloadType', asString],
  [PUSH_FRAME_FIELD.payload]: ['payload', (value) => value],
};

const BASE_MESSAGE = {
  1: ['method', asString],
  2: ['payload', (value) => value],
  3: ['msgId', asId],
  6: ['isHistory', (value) => Boolean(asCount(value))],
};

const FETCH_RESULT = {
  1: ['messages[]', (value) => read(value, BASE_MESSAGE)],
  2: ['cursor', asString],
  5: ['internalExt', asString],
  8: ['heartbeatDuration', asCount],
  9: ['needAck', (value) => Boolean(asCount(value))],
  10: ['pushServer', asString],
};

/// Read one WebSocket frame.
export function decodePushFrame(bytes) {
  const frame = read(bytes, PUSH_FRAME);
  const headers = new Map((frame.headers ?? []).map((entry) => [entry.key, entry.value]));
  return {
    seqId: frame.seqId ?? '0',
    logId: frame.logId ?? '0',
    headers,
    payloadEncoding: frame.payloadEncoding ?? '',
    payloadType: frame.payloadType ?? '',
    payload: frame.payload ?? new Uint8Array(),
    compressType: headers.get(COMPRESS_TYPE_HEADER) ?? '',
    carriesEvents: frame.payloadType === MESSAGE_PAYLOAD_TYPE,
  };
}

/// Decompress a frame's payload according to its own header.
///
/// An unrecognised `compress_type` is passed through rather than refused: the payload is still the
/// envelope, and a new compression name should degrade to "cannot read this batch", not "the
/// connection is broken".
export function decompress(frame) {
  if (frame.compressType === 'gzip') return gunzipSync(frame.payload);
  return frame.payload;
}

/// Read the event batch inside a `msg` frame's payload.
export function decodeBatch(payload) {
  const batch = read(payload, FETCH_RESULT);
  return {
    messages: batch.messages ?? [],
    cursor: batch.cursor ?? '',
    internalExt: batch.internalExt ?? '',
    heartbeatDuration: batch.heartbeatDuration ?? 0,
    needAck: batch.needAck ?? false,
    pushServer: batch.pushServer ?? '',
  };
}

/// The acknowledgement for a frame.
///
/// The payload is the batch's `internal_ext`, or `-` when it is empty — the server rejects an
/// empty one. Unacknowledged frames stop the push after a few seconds, which looks exactly like a
/// quiet room.
export function ackFrame(frame, internalExt) {
  return pushFrame(FRAME_TYPE.ack, internalExt || '-', { logId: frame.logId });
}

/// Whether a frame is one of the transport's own, for logging.
export function isTransportFrame(frame) {
  return !frame.carriesEvents;
}

/// Every field of a frame, for a caller that wants what this module chose not to model.
export { fields as rawFields };
