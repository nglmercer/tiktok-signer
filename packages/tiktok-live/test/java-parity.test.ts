import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import { fromBinary } from '@bufbuild/protobuf';
import { WebcastPushFrameSchema } from '../dist/gen/webcast/synthetic_proto_pb.js';
import { ackFrame } from '../dist/frames.js';
import { enterRoomFrame, heartbeatFrame } from '../dist/player.js';
import { Discovery } from '../dist/discovery.js';
const fixtures = new URL('../../../fixtures/events/', import.meta.url);
test('shared Java/Rust/Node transport oracle', () => {
 const f=JSON.parse(fs.readFileSync(new URL('transport-parity.json',fixtures),'utf8'));
 const incoming=fromBinary(WebcastPushFrameSchema,Buffer.from(f.incoming,'hex'));
 assert.equal(incoming.logId.toString(),f.logId);
 assert.equal(enterRoomFrame({roomId:f.roomId}).toString('hex'),f.enter);
 assert.equal(heartbeatFrame(f.roomId).toString('hex'),f.heartbeat);
 assert.equal(ackFrame(incoming,'fixture-ext').toString('hex'),f.ack);
 assert.equal(ackFrame(incoming,'').toString('hex'),f.ackEmpty);
});
test('shared room presence interpretation', async () => {
 const original=globalThis.fetch;
 try {
  for(const f of JSON.parse(fs.readFileSync(new URL('presence/cases.json',fixtures),'utf8'))) {
   globalThis.fetch=async () => new Response(f.body,{status:f.httpStatus});
   let status='UNKNOWN';
   try { status=(await new Discovery().roomLookup(f.id)).isLive?'LIVE':'OFFLINE'; } catch {}
   assert.equal(status,f.expected,f.name);
  }
 } finally { globalThis.fetch=original; }
});
