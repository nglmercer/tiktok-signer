//! Golden tests: Node and Rust must normalise the same bytes identically.
//!
//! The expected JSON is produced by `examples/node-connector/golden-fixtures.ts`
//! using `tiktok-live-proto/v3` — the same package the modern Node connector
//! uses. Nothing here shells out to Node; the files are committed.
//!
//! Every number is compared as a string. TikTok ids exceed 2^53, so a JSON
//! number would lose precision on the Node side and make the comparison lie.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use ttl_live_events::{decode_batch, method, LiveEvent};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn read_fixture(name: &str) -> Vec<u8> {
    let path = fixtures().join("events").join(name);
    fs::read(&path).unwrap_or_else(|err| panic!("missing fixture {}: {err}", path.display()))
}

/// Decodes a single event payload as if it had arrived under `method`.
fn decode_one(method: &str, payload: &[u8]) -> LiveEvent {
    ttl_live_events::decode_event(method, payload)
}

fn user_json(user: &ttl_live_events::EventUser) -> Value {
    json!({
        "id": user.id.to_string(),
        "nickname": user.nickname,
        "unique_id": user.unique_id,
        "sec_uid": user.sec_uid,
    })
}

/// The shared comparison format. Kept separate from the crate's `Serialize`
/// impl on purpose: the public JSON shape is free to change, but this contract
/// with the Node oracle must be explicit.
fn golden_json(event: &LiveEvent) -> Value {
    match event {
        LiveEvent::Chat(chat) => json!({
            "type": "chat",
            "user": user_json(&chat.user),
            "comment": chat.comment,
        }),
        LiveEvent::Gift(gift) => json!({
            "type": "gift",
            "user": user_json(&gift.user),
            "gift_id": gift.gift_id.to_string(),
            "gift_name": gift.gift_name,
            "diamond_count": gift.diamond_count.to_string(),
            "repeat_count": gift.repeat_count.to_string(),
            "combo_count": gift.combo_count.to_string(),
            "group_id": gift.group_id.to_string(),
            "repeat_end": gift.repeat_end,
        }),
        LiveEvent::Like(like) => json!({
            "type": "like",
            "user": user_json(&like.user),
            "count": like.count.to_string(),
            "total": like.total.to_string(),
        }),
        LiveEvent::Member(member) => json!({
            "type": "member",
            "user": user_json(&member.user),
            "member_count": member.member_count.to_string(),
            "action": member.action.to_string(),
        }),
        LiveEvent::Social(social) => json!({
            "type": "social",
            "user": user_json(&social.user),
            "action": social.action.to_string(),
            "follow_count": social.follow_count.to_string(),
            "share_count": social.share_count.to_string(),
        }),
        LiveEvent::RoomUser(room) => json!({
            "type": "room_user",
            "total": room.total.to_string(),
            "popularity": room.popularity.to_string(),
            "total_user": room.total_user.to_string(),
            "anonymous": room.anonymous.to_string(),
        }),
        LiveEvent::Unknown { method, .. } => json!({ "type": "unknown", "method": method }),
    }
}

fn assert_matches_node(fixture: &str, method: &str) {
    let event = decode_one(method, &read_fixture(fixture));

    let expected_path = fixtures()
        .join("events/expected")
        .join(fixture.replace(".pb", ".json"));
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(&expected_path)
            .unwrap_or_else(|err| panic!("missing golden {}: {err}", expected_path.display())),
    )
    .expect("golden file is not valid JSON");

    assert_eq!(
        golden_json(&event),
        expected,
        "Rust and Node disagree on {fixture}"
    );
}

#[test]
fn chat_matches_node() {
    assert_matches_node("chat.pb", method::CHAT);
}

#[test]
fn gift_matches_node() {
    assert_matches_node("gift.pb", method::GIFT);
}

#[test]
fn like_matches_node() {
    assert_matches_node("like.pb", method::LIKE);
}

#[test]
fn member_matches_node() {
    assert_matches_node("member.pb", method::MEMBER);
}

#[test]
fn social_matches_node() {
    assert_matches_node("social.pb", method::SOCIAL);
}

#[test]
fn room_user_matches_node() {
    assert_matches_node("room-user.pb", method::ROOM_USER);
}

/// The real capture decodes end to end, and the events we do not model yet are
/// preserved rather than dropped.
#[test]
fn real_capture_decodes_and_keeps_unknowns() {
    let payload = read_fixture("batch.pb");
    let batch = decode_batch(&payload).expect("batch decodes");

    // The committed fixture must stay redacted: `cursor` and `internal_ext` are
    // fed back into the signed socket URI and identify a session. If a raw
    // capture is ever committed by mistake, this fails.
    assert_eq!(batch.cursor, "redacted-cursor", "batch.pb is not redacted");
    assert_eq!(
        batch.internal_ext, "redacted-internal-ext",
        "batch.pb is not redacted"
    );

    assert_eq!(batch.events.len(), 13, "captured batch event count");

    let chats: Vec<_> = batch
        .events()
        .filter_map(|event| match event {
            LiveEvent::Chat(chat) => Some(chat),
            _ => None,
        })
        .collect();
    assert_eq!(chats.len(), 6, "captured batch chat count");
    assert!(
        chats.iter().all(|chat| !chat.comment.is_empty()),
        "every captured chat has a comment"
    );

    // Unknown events keep their method and their bytes, byte for byte.
    for decoded in &batch.events {
        if let LiveEvent::Unknown { method, payload } = &decoded.event {
            assert_eq!(*method, decoded.raw.method);
            assert_eq!(*payload, decoded.raw.payload);
            assert!(!payload.is_empty(), "{method} payload was discarded");
        }
    }

    assert!(
        batch.unknown_methods().contains(&"WebcastLiveIntroMessage"),
        "expected unmodelled methods to surface, got {:?}",
        batch.unknown_methods()
    );
}

/// An unmodelled method must never decode into a typed event.
#[test]
fn unmodelled_method_falls_back_to_unknown() {
    let payload = read_fixture("live-intro.pb");
    let event = decode_one("WebcastLiveIntroMessage", &payload);

    match event {
        LiveEvent::Unknown {
            method,
            payload: kept,
        } => {
            assert_eq!(method, "WebcastLiveIntroMessage");
            assert_eq!(kept, payload);
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}

/// A payload that cannot be parsed as its declared type degrades to Unknown
/// with the bytes intact, instead of failing the whole batch.
#[test]
fn corrupt_payload_degrades_to_unknown() {
    let garbage = vec![0xff, 0xff, 0xff, 0xff];
    let event = decode_one(method::CHAT, &garbage);

    match event {
        LiveEvent::Unknown { method, payload } => {
            assert_eq!(method, ttl_live_events::method::CHAT);
            assert_eq!(payload, garbage);
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}
