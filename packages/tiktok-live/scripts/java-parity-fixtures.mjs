// Rebuild the shared transport oracle from the existing Node client, offline.
import fs from 'node:fs';
import {create,toBinary} from '@bufbuild/protobuf';
import {enterRoomFrame,heartbeatFrame,pushFrame} from '../dist/player.js';
import {ackFrame} from '../dist/frames.js';
import {WebcastPushFrameSchema} from '../dist/gen/webcast/synthetic_proto_pb.js';
const roomId='7300000000000000001';
const incoming=create(WebcastPushFrameSchema,{logId:9007199254740993n,payloadEncoding:'pb',payloadType:'msg',payload:fs.readFileSync(new URL('../../../fixtures/events/batch.pb',import.meta.url))});
const hex=x=>Buffer.from(x).toString('hex');
const fixture={source:'packages/tiktok-live/src/player.ts and frames.ts',roomId,logId:incoming.logId.toString(),
 enter:hex(enterRoomFrame({roomId})),heartbeat:hex(heartbeatFrame(roomId)),ack:hex(ackFrame(incoming,'fixture-ext')),ackEmpty:hex(ackFrame(incoming,'')),incoming:hex(toBinary(WebcastPushFrameSchema,incoming))};
fs.writeFileSync(new URL('../../../fixtures/events/transport-parity.json',import.meta.url),JSON.stringify(fixture,null,2)+'\n');
