# Event fixtures

Unlike the rest of `fixtures/`, **this directory is versioned**. The decoder
tests must run on a fresh clone and in CI without an active LIVE room, and these
files carry no cookies or signing material.

Consumed by `crates/ttl-live-events/tests/{golden,parity}.rs`.

## Contents

| File | Origin |
| --- | --- |
| `batch.pb` | Real capture — a full `ProtoMessageFetchResult`, **redacted** (see below) |
| `chat.pb` | Real capture — the first `WebcastChatMessage` payload from that batch |
| `gift.pb`, `like.pb`, `member.pb`, `social.pb`, `room-user.pb` | Synthesised from the v3 schema |
| `*-*.pb` (live-intro, room, gift-panel-update, …) | Real capture — events we do not normalise yet, kept to test the `Unknown` fallback |
| `expected/*.json` | Normalised form, produced by **Node** |

The five synthesised fixtures exist because the captured batch is the initial
HTTP response, which in practice only contains chat; gift, like, member, social
and room-user arrive later over the WebSocket. They are encoded from the same
schema and so use the same field numbers real traffic does.

## Redaction

`batch.pb` has `cursor`, `internal_ext` and `route_params` replaced with
placeholders. Those are the values the client feeds back into the signed
WebSocket URI, and they identify a session. A test asserts the placeholders are
still present, so an unredacted capture cannot be committed by accident.

The `messages` inside are untouched. They contain what was already public in the
room's chat — nicknames, handles and comments of participants.

## Regenerating

```sh
# 1. per-event payloads from a capture (writes only events not already present)
cargo run -p ttl-live-events --example extract-fixtures -- fixtures/f0/im_fetch.pb fixtures/events

# 2. synthetic events + the redacted batch
cargo run -p ttl-live-events --example make-fixtures -- fixtures/events fixtures/f0/im_fetch.pb

# 3. expected JSON, via the Node oracle (tiktok-live-proto/v3)
cd examples/node-connector && npx tsx golden-fixtures.ts
```

Step 3 is what makes these golden tests meaningful: the expected values come
from the *other* implementation, so Rust cannot mark its own homework.
