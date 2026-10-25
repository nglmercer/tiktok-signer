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
| `gift.pb`, `like.pb`, `member.pb`, `social.pb`, `share.pb`, `social-unknown.pb`, `room-user.pb` | Synthesised from the v3 schema |
| `chat-inline-emote.pb`, `chat-multiple-emotes.pb`, `chat-unicode-emote.pb`, `chat-invalid-emote.pb`, `emote-chat.pb` | Synthesised from the v3 schema — inline and standalone emote coverage (see `docs/20-live-emotes.md`) |
| `social-classification.json` | The `action` → kind table shared by the Rust, TypeScript and Java tests |
| `*-*.pb` (live-intro, room, gift-panel-update, …) | Real capture — events we do not normalise yet, kept to test the `Unknown` fallback |
| `expected/*.json` | Normalised form, produced by **Node** |

The synthesised fixtures exist because the captured batch is the initial HTTP
response, which in practice only contains chat; the rest arrive later over the
WebSocket. They are encoded from the same schema and so use the same field
numbers real traffic does.

Three social fixtures cover the classification surface:

* `social.pb` — a confirmed **follow** (`action == 1`).
* `share.pb` — a confirmed **share** (`action == 3`) that also carries the room's
  cumulative `follow_count`. This is the regression fixture: any rule that picks
  "the first positive counter" labels it a follow.
* `social-unknown.pb` — an unobserved `action` (`97`), which must stay
  unclassified rather than being counted as a follow or a share.

The `action` → kind table itself (`1` follow, `3` share, `0` unknown, anything
else other) has no confirmed meaning beyond those two values. TikTok adds codes
without notice, so a new value must be observed in anonymised captures before a
rule is added; until then it is reported with its raw code.

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

# 3. expected JSON, via the Node oracle (local protoc-gen-es bindings)
cd examples/node-connector && npx tsx golden-fixtures.ts
```

Step 3 is what makes these golden tests meaningful: the expected values come
from the *other* implementation, so Rust cannot mark its own homework.

## Java SDK parity

`transport-parity.json` is generated offline by the existing Node player/frame encoders:
`node packages/tiktok-live/scripts/java-parity-fixtures.mjs` after building that package.
It contains synthetic transport envelopes around the already-redacted `batch.pb`, not real
signed URLs or cookies. Java compares exact protobuf bytes. Rust compares envelope and
payload semantics, accounting for proto3's omission of zero-valued fields and Rust's explicit
heartbeat sequence counter. `presence/cases.json` supplies the same HTTP/status cases to
Java, Node, and Rust. A Rust `NoRoom` maps to OFFLINE at the presence boundary.

`deep-gift.pb` is a structural regression fixture for a valid public LIVE gift
whose diagnostic view exceeded eight nesting levels. All numeric values were
replaced with 1, all strings with `fixture`, and all opaque bytes/unknown
fields removed. No original LIVE sender, ID, text or URL is retained.
