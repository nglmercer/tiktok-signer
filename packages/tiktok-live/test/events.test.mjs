// The six normalisers, and the fallback that keeps the seventh from being lost.

import assert from 'node:assert/strict';
import test from 'node:test';

import { EVENT, METHOD, decodeEvent, decodeUser, label } from '../src/events.mjs';

const USER_ID = 6810000000000000123n;

test('a chat message carries its user and its text', () => {
  const event = decodeEvent(
    METHOD.chat,
    concat(sub(2, user('someone', 'Some One')), str(3, 'hola')),
  );
  assert.equal(event.type, EVENT.chat);
  assert.equal(event.comment, 'hola');
  assert.equal(event.user.uniqueId, 'someone');
  assert.equal(event.user.nickname, 'Some One');
  assert.equal(event.user.userId, USER_ID.toString());
});

test('a gift reads its detail block when there is one', () => {
  const detail = concat(varint(5, 5655n), varint(12, 1n), str(16, 'Rose'));
  const event = decodeEvent(
    METHOD.gift,
    concat(varint(2, 5655n), varint(5, 3n), sub(7, user('someone')), varint(9, 1n), sub(15, detail)),
  );
  assert.equal(event.type, EVENT.gift);
  assert.equal(event.giftId, '5655');
  assert.equal(event.giftName, 'Rose');
  assert.equal(event.diamondCount, 1);
  assert.equal(event.repeatCount, 3);
  assert.equal(event.repeatEnd, true);
});

// Every repeat of a streak omits the detail block, so a gift with no name is normal, not broken —
// the client fills it in from the room's gift table.
test('a gift without a detail block still decodes', () => {
  const event = decodeEvent(METHOD.gift, concat(varint(2, 5655n), sub(7, user('someone'))));
  assert.equal(event.giftId, '5655');
  assert.equal(event.giftName, '');
  assert.equal(event.diamondCount, 0);
  assert.equal(event.repeatEnd, false);
});

test('viewer counts come off the room-user message', () => {
  const event = decodeEvent(METHOD.roomUser, concat(varint(3, 35075n), varint(6, 41n)));
  assert.equal(event.type, EVENT.roomUser);
  assert.equal(event.viewers, 35075);
  assert.equal(event.popularity, 41);
});

// The room sends dozens of methods this package does not model — link-mic state, gift-panel
// updates. They must arrive whole rather than be dropped, so a caller can decode one without
// waiting for this file to grow.
test('an unmodelled method keeps its bytes', () => {
  const payload = Uint8Array.from([1, 2, 3]);
  const event = decodeEvent('WebcastLinkMicMethod', payload);
  assert.equal(event.type, EVENT.unknown);
  assert.equal(event.method, 'WebcastLinkMicMethod');
  assert.deepEqual([...event.payload], [1, 2, 3]);
});

test('a payload that cannot be read degrades instead of throwing', () => {
  const event = decodeEvent(METHOD.chat, Uint8Array.from([0x0a, 0xff]));
  assert.ok(event.type === EVENT.chat || event.type === EVENT.unknown);
});

test('a missing user is a blank user, not a crash', () => {
  const event = decodeEvent(METHOD.chat, str(3, 'orphan comment'));
  assert.equal(event.user.uniqueId, '');
  assert.equal(label(event.user), 'unknown');
  assert.deepEqual(decodeUser(), { userId: '0', nickname: '', uniqueId: '', secUid: '' });
});

// --- encoders, so the fixtures above are readable rather than hex ------------------------------

function user(uniqueId, nickname = '') {
  return concat(varint(1, USER_ID), nickname ? str(3, nickname) : new Uint8Array(), str(38, uniqueId));
}

function varint(number, value) {
  const out = [(number << 3) | 0];
  let remaining = BigInt(value);
  do {
    let byte = Number(remaining & 0x7fn);
    remaining >>= 7n;
    if (remaining) byte |= 0x80;
    out.push(byte);
  } while (remaining);
  return Uint8Array.from(out);
}

function bytes(number, body) {
  return concat(varintValue((number << 3) | 2), varintValue(body.length), body);
}

const str = (number, value) => bytes(number, new TextEncoder().encode(value));
const sub = (number, body) => bytes(number, body);

function varintValue(value) {
  const out = [];
  let remaining = BigInt(value);
  do {
    let byte = Number(remaining & 0x7fn);
    remaining >>= 7n;
    if (remaining) byte |= 0x80;
    out.push(byte);
  } while (remaining);
  return Uint8Array.from(out);
}

function concat(...parts) {
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}
