// Golden oracle for the Rust event decoder.
//
// Node and Rust must agree on the wire format because they read the *same*
// `.proto` sources: this script uses `tiktok-live-proto/v3` (the package the
// modern connector uses), while Rust generates Prost bindings from the schemas
// vendored in `crates/ttl-live-proto/proto/v3`. Node is a test-time oracle only
// — nothing in the Rust runtime shells out to it.
//
//   npx tsx golden-fixtures.ts        # from examples/node-connector
//
// It decodes every fixture in `fixtures/events/` and writes the normalised form
// to `fixtures/events/expected/<name>.json`. The Rust test in
// `crates/ttl-live-events/tests/golden.rs` reads the same `.pb` files and must
// produce byte-identical JSON.
//
// Fixtures are produced beforehand by:
//
//   cargo run -p ttl-live-events --example make-fixtures -- fixtures/events
//
// so this script decoding them at all is itself the cross-implementation check:
// Rust (prost) encodes, Node (@bufbuild/protobuf) decodes, and vice versa.
//
// All numbers are emitted as strings. TikTok user and message ids exceed 2^53,
// so a JSON number would silently lose precision on the Node side.

import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
    WebcastChatMessage,
    WebcastGiftMessage,
    WebcastLikeMessage,
    WebcastMemberMessage,
    WebcastRoomUserSeqMessage,
    WebcastSocialMessage,
} from 'tiktok-live-proto/v3';

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = join(here, '..', '..', 'fixtures', 'events');
const expected = join(fixtures, 'expected');

/** Mirrors `EventUser::normalize` in Rust. */
function normalizeUser(u: { id?: string; nickname?: string; displayId?: string; secUid?: string } | undefined) {
    return {
        id: String(u?.id ?? '0'),
        nickname: u?.nickname ?? '',
        unique_id: u?.displayId ?? '',
        sec_uid: u?.secUid ?? '',
    };
}

/** Protobuf counts are signed; the stable API clamps negatives, as Rust does. */
function count(value: string | number | undefined): string {
    const n = BigInt(value ?? 0);
    return (n < 0n ? 0n : n).toString();
}

/** Mirrors the Rust normalisers in `crates/ttl-live-events/src/normalize.rs`. */
const normalizers: Record<string, (payload: Uint8Array) => unknown> = {
    'chat.pb': (payload) => {
        const m = WebcastChatMessage.decode(payload);
        return { type: 'chat', user: normalizeUser(m.user), comment: m.content };
    },
    'gift.pb': (payload) => {
        const m = WebcastGiftMessage.decode(payload);
        return {
            type: 'gift',
            user: normalizeUser(m.user),
            gift_id: count(m.giftId),
            gift_name: m.gift?.name ?? '',
            diamond_count: count(m.gift?.diamondCount),
            repeat_count: count(m.repeatCount),
            combo_count: count(m.comboCount),
            group_id: count(m.groupId),
            repeat_end: m.repeatEnd !== 0,
        };
    },
    'like.pb': (payload) => {
        const m = WebcastLikeMessage.decode(payload);
        return {
            type: 'like',
            user: normalizeUser(m.user),
            count: count(m.count),
            total: count(m.total),
        };
    },
    'member.pb': (payload) => {
        const m = WebcastMemberMessage.decode(payload);
        return {
            type: 'member',
            user: normalizeUser(m.user),
            member_count: count(m.memberCount),
            action: String(m.action),
        };
    },
    'social.pb': (payload) => {
        const m = WebcastSocialMessage.decode(payload);
        return {
            type: 'social',
            user: normalizeUser(m.user),
            action: String(m.action),
            follow_count: count(m.followCount),
            share_count: count(m.shareCount),
        };
    },
    'room-user.pb': (payload) => {
        const m = WebcastRoomUserSeqMessage.decode(payload);
        return {
            type: 'room_user',
            total: count(m.total),
            popularity: count(m.popularity),
            total_user: count(m.totalUser),
            anonymous: count(m.anonymous),
        };
    },
};

mkdirSync(expected, { recursive: true });

const present = readdirSync(fixtures).filter((name) => name.endsWith('.pb')).sort();
let written = 0;

for (const name of present) {
    const normalize = normalizers[name];
    if (!normalize) {
        // Fixtures for events Rust reports as Unknown have no expected JSON;
        // the Rust test asserts they survive with method and payload intact.
        console.log(`skipped  ${name} (no normaliser — Unknown fallback)`);
        continue;
    }
    const json = JSON.stringify(normalize(readFileSync(join(fixtures, name))), null, 2);
    writeFileSync(join(expected, name.replace(/\.pb$/, '.json')), `${json}\n`);
    console.log(`expected ${name}`);
    written += 1;
}

console.log(`\n${written} golden files in ${expected}`);
