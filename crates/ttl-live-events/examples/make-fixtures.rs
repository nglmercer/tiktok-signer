//! Writes the synthetic event fixtures used by the golden tests.
//!
//! Our real capture (`fixtures/f0/im_fetch.pb`) is the initial HTTP batch and
//! only contains chat among the six events we normalise; gift, like, member,
//! social and room-user only appear on the live WebSocket. Rather than make the
//! test suite depend on an active room, we encode those five from the v3 schema
//! here. They use the same field numbers real traffic does, and Node decodes
//! them independently in `examples/node-connector/golden-fixtures.ts`, so the
//! wire format is checked in both directions.
//!
//! `chat.pb` is a genuine capture and is never overwritten.
//!
//! Given a capture as a second argument it also writes `batch.pb`: the real
//! `ProtoMessageFetchResult` with its session-bearing fields redacted, so a
//! whole-batch fixture can be committed. See [`redact_batch`].
//!
//! ```sh
//! cargo run -p ttl-live-events --example make-fixtures -- \
//!     fixtures/events fixtures/f0/im_fetch.pb
//! ```

use std::path::PathBuf;
use std::{env, fs};

use prost::Message;
use ttl_live_proto::messages::{
    WebcastGiftMessage, WebcastLikeMessage, WebcastMemberMessage, WebcastRoomUserSeqMessage,
    WebcastSocialMessage,
};
use ttl_live_proto::webcast::model::base::user::User;
use ttl_live_proto::webcast::model::Gift;
use ttl_live_proto::ProtoMessageFetchResult;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let out_dir = PathBuf::from(
        args.next()
            .ok_or("usage: make-fixtures <fixtures/events> [capture.pb]")?,
    );
    fs::create_dir_all(&out_dir)?;

    let mut files: Vec<(&str, Vec<u8>)> = vec![
        ("gift.pb", gift().encode_to_vec()),
        ("like.pb", like().encode_to_vec()),
        ("member.pb", member().encode_to_vec()),
        ("social.pb", social().encode_to_vec()),
        ("room-user.pb", room_user().encode_to_vec()),
    ];

    if let Some(capture) = args.next() {
        let batch = redact_batch(&fs::read(capture)?)?;
        files.push(("batch.pb", batch.encode_to_vec()));
    }

    for (name, bytes) in files {
        fs::write(out_dir.join(name), &bytes)?;
        println!("{name}  ({} bytes)", bytes.len());
    }
    Ok(())
}

/// Strips the session-bearing fields from a captured batch so it can be
/// committed.
///
/// `cursor`, `internal_ext` and `route_params` (which carries `wrss`/`imprp`)
/// are what the client feeds back into the signed WebSocket URI — they identify
/// a session and must not land in the repository. They are replaced with
/// placeholders rather than cleared, so tests still exercise non-empty string
/// passthrough.
///
/// The `messages` themselves are left untouched: they are the point of the
/// fixture, and they carry only what was already public in the room's chat.
fn redact_batch(payload: &[u8]) -> Result<ProtoMessageFetchResult, prost::DecodeError> {
    let mut batch = ttl_live_proto::decode_event_batch(payload)?;
    batch.cursor = "redacted-cursor".to_owned();
    batch.internal_ext = "redacted-internal-ext".to_owned();
    batch.route_params = batch
        .route_params
        .into_keys()
        .map(|key| (key, "redacted".to_owned()))
        .collect();
    Ok(batch)
}

/// Shared identity, so every fixture normalises to the same user block.
fn user() -> User {
    User {
        id: 6_820_914_516_086_834,
        nickname: "Golden Tester".to_owned(),
        display_id: "golden.tester".to_owned(),
        sec_uid: "MS4wLjABAAAAgolden".to_owned(),
        ..Default::default()
    }
}

fn gift() -> WebcastGiftMessage {
    WebcastGiftMessage {
        gift_id: 5655,
        repeat_count: 7,
        combo_count: 3,
        group_id: 1_739_382_910_000,
        repeat_end: 1,
        user: Some(user()),
        gift: Some(Gift {
            id: 5655,
            name: "Rose".to_owned(),
            diamond_count: 1,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn like() -> WebcastLikeMessage {
    WebcastLikeMessage {
        count: 12,
        total: 4821,
        user: Some(user()),
        ..Default::default()
    }
}

fn member() -> WebcastMemberMessage {
    WebcastMemberMessage {
        member_count: 1284,
        action: 1,
        user: Some(user()),
        ..Default::default()
    }
}

fn social() -> WebcastSocialMessage {
    WebcastSocialMessage {
        action: 1,
        follow_count: 9312,
        share_count: 4,
        user: Some(user()),
        ..Default::default()
    }
}

fn room_user() -> WebcastRoomUserSeqMessage {
    WebcastRoomUserSeqMessage {
        total: 1284,
        popularity: 2048,
        total_user: 55391,
        anonymous: 17,
        ..Default::default()
    }
}
