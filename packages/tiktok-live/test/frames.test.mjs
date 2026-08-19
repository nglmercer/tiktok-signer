// The transport envelope, and the one frame that is a reply.

import assert from 'node:assert/strict';
import test from 'node:test';
import { gzipSync } from 'node:zlib';

import { ackFrame, decodeBatch, decodePushFrame, decompress } from '../src/frames.mjs';
import { enterRoomFrame, heartbeatFrame, pushFrame } from '../src/player.mjs';

test('a frame this package builds is one it can read', () => {
  const frame = decodePushFrame(pushFrame('msg', Buffer.from([1, 2, 3])));
  assert.equal(frame.payloadType, 'msg');
  assert.equal(frame.payloadEncoding, 'pb');
  assert.equal(frame.carriesEvents, true);
  assert.deepEqual([...frame.payload], [1, 2, 3]);
});

test('only msg frames carry events', () => {
  for (const type of ['hb', 'ack', 'im_enter_room_resp']) {
    assert.equal(decodePushFrame(pushFrame(type, '')).carriesEvents, false, type);
  }
});

test('a gzipped payload is decompressed by the frame’s own header', () => {
  const body = Buffer.from('the batch would be here');
  const frame = decodePushFrame(pushFrame('msg', gzipSync(body)));
  frame.headers.set('compress_type', 'gzip');
  frame.compressType = 'gzip';
  assert.equal(decompress(frame).toString(), body.toString());
});

// An unrecognised compression should degrade to "cannot read this batch", not to a dead socket.
test('an unknown compression passes the payload through', () => {
  const frame = decodePushFrame(pushFrame('msg', Buffer.from('plain')));
  frame.compressType = 'brotli-someday';
  assert.equal(Buffer.from(decompress(frame)).toString(), 'plain');
});

// Unacknowledged frames stop the push a few seconds later, which looks exactly like a quiet room.
test('an ack echoes the log id and never sends an empty payload', () => {
  const received = decodePushFrame(pushFrame('msg', ''));
  received.logId = '42';
  const empty = decodePushFrame(ackFrame(received, ''));
  assert.equal(empty.payloadType, 'ack');
  assert.equal(empty.logId, '42');
  assert.equal(Buffer.from(empty.payload).toString(), '-');

  const carried = decodePushFrame(ackFrame(received, 'internal-ext-value'));
  assert.equal(Buffer.from(carried.payload).toString(), 'internal-ext-value');
});

test('a batch envelope yields its messages and its ack state', () => {
  const batch = decodeBatch(
    Buffer.concat([
      lengthDelimited(1, encodeMessage('WebcastChatMessage', Buffer.from([9]))),
      lengthDelimited(2, Buffer.from('cursor-1')),
      lengthDelimited(5, Buffer.from('ext-1')),
      Buffer.from([(9 << 3) | 0, 1]),
    ]),
  );
  assert.equal(batch.messages.length, 1);
  assert.equal(batch.messages[0].method, 'WebcastChatMessage');
  assert.equal(batch.cursor, 'cursor-1');
  assert.equal(batch.internalExt, 'ext-1');
  assert.equal(batch.needAck, true);
});

// The two frames a client originates. If either stops matching what the SDK sends, a healthy
// socket goes silent instead of failing, which is the hardest kind of break to notice.
test('the enter-room and heartbeat frames keep their shape', () => {
  const enter = decodePushFrame(enterRoomFrame({ roomId: '7675590159819148052' }));
  assert.equal(enter.payloadType, 'im_enter_room');
  assert.ok(enter.payload.length > 0);

  const heartbeat = decodePushFrame(heartbeatFrame('7675590159819148052'));
  assert.equal(heartbeat.payloadType, 'hb');
});

function lengthDelimited(number, body) {
  return Buffer.concat([Buffer.from([(number << 3) | 2, body.length]), Buffer.from(body)]);
}

function encodeMessage(method, payload) {
  return Buffer.concat([
    lengthDelimited(1, Buffer.from(method)),
    lengthDelimited(2, payload),
  ]);
}
