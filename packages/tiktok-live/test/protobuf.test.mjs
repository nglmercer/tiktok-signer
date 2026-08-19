// The wire reader. These are the assumptions everything else in the package rests on.

import assert from 'node:assert/strict';
import test from 'node:test';

import { asCount, asId, asString, read } from '../src/protobuf.mjs';

const SHAPE = { 1: ['method', asString], 2: ['payload', (v) => v], 3: ['msgId', asId] };

test('reads a message by field number', () => {
  const bytes = Uint8Array.from([0x0a, 0x01, 0x78, 0x12, 0x02, 0x01, 0x02, 0x18, 0x05]);
  const message = read(bytes, SHAPE);
  assert.equal(message.method, 'x');
  assert.deepEqual([...message.payload], [1, 2]);
  assert.equal(message.msgId, '5');
});

// A schema that gains a field must not break a reader that predates it, which is the whole reason
// this is a wire reader and not a generated one.
test('skips fields it has no name for', () => {
  const bytes = Uint8Array.from([0x0a, 0x01, 0x78, 0x62, 0x03, 0x61, 0x62, 0x63, 0x18, 0x07]);
  const message = read(bytes, SHAPE);
  assert.equal(message.method, 'x');
  assert.equal(message.msgId, '7');
});

// Room and user ids exceed Number.MAX_SAFE_INTEGER. One rounded digit is a different user, so ids
// stay decimal strings and only counts become numbers.
test('ids survive past 2^53', () => {
  const big = 7675590159819148052n;
  const bytes = encodeVarintField(3, big);
  assert.equal(read(bytes, SHAPE).msgId, big.toString());
  assert.notEqual(Number(big).toString(), big.toString());
});

test('counts clamp at zero rather than wrapping', () => {
  const negative = encodeVarintField(1, BigInt.asUintN(64, -1n));
  assert.equal(read(negative, { 1: ['count', asCount] }).count, 0);
});

// A frame cut short mid-field is a transport failure. Decoding whatever follows would invent
// events out of the tail of a buffer.
test('a truncated length-delimited field ends the read', () => {
  const bytes = Uint8Array.from([0x0a, 0x08, 0x61, 0x62]);
  assert.deepEqual(read(bytes, SHAPE), {});
});

function encodeVarintField(number, value) {
  const out = [(number << 3) | 0];
  let remaining = value;
  do {
    let byte = Number(remaining & 0x7fn);
    remaining >>= 7n;
    if (remaining) byte |= 0x80;
    out.push(byte);
  } while (remaining);
  return Uint8Array.from(out);
}
