//! Stable TikTok LIVE event API.
//!
//! This crate sits between the generated v3 schema (`ttl-live-proto`) and the
//! application. It owns two responsibilities and nothing else:
//!
//! 1. split a decompressed batch into individual events;
//! 2. normalise the events we understand into version-independent structs.
//!
//! Transport concerns — WebSocket, gzip, ACK, heartbeat — stay in `ttl-live-ws`,
//! and the generated Prost types are never part of this crate's public API.
//!
//! ```no_run
//! use ttl_live_events::{decode_batch, LiveEvent};
//!
//! # fn main() -> Result<(), ttl_live_events::EventError> {
//! # let payload: &[u8] = &[];
//! for event in decode_batch(payload)?.events() {
//!     match event {
//!         LiveEvent::Chat(chat) => println!("{}: {}", chat.user.label(), chat.comment),
//!         LiveEvent::Unknown { method, .. } => println!("unhandled {method}"),
//!         other => println!("{}", other.method()),
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod dynamic;
mod event;
mod normalize;
mod user;

pub use dynamic::{decode_webcast_message, SchemaField, SchemaMessage, SchemaObject, SchemaValue};
pub use event::{
    ChatEvent, DecodedEvent, EventBatch, GiftEvent, LikeEvent, LiveEvent, MemberEvent, RawEvent,
    RoomUserEvent, SocialEvent,
};
pub use user::EventUser;

use event::RawEvent as Raw;
use ttl_live_proto::BaseProtoMessage;

/// Schema method names for the events this crate normalises.
pub mod method {
    pub const CHAT: &str = "WebcastChatMessage";
    pub const GIFT: &str = "WebcastGiftMessage";
    pub const LIKE: &str = "WebcastLikeMessage";
    pub const MEMBER: &str = "WebcastMemberMessage";
    pub const SOCIAL: &str = "WebcastSocialMessage";
    pub const ROOM_USER: &str = "WebcastRoomUserSeqMessage";
}

/// Errors from decoding a batch.
///
/// Note that a *single* malformed event never produces an error: it degrades to
/// [`LiveEvent::Unknown`] so the rest of the batch still reaches the consumer.
/// Only a corrupt outer envelope fails.
#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("failed to decode the event batch envelope: {0}")]
    Batch(#[from] prost::DecodeError),
}

/// Decodes one decompressed WebSocket payload into normalised events.
pub fn decode_batch(payload: &[u8]) -> Result<EventBatch, EventError> {
    let result = ttl_live_proto::decode_event_batch(payload)?;
    Ok(EventBatch {
        events: result.messages.iter().map(decode_message).collect(),
        cursor: result.cursor,
        internal_ext: result.internal_ext,
        need_ack: result.need_ack,
        heartbeat_duration: result.heartbeat_duration,
        push_server: result.push_server,
    })
}

/// Normalises a single event payload, given the method that carried it.
///
/// The entry point for callers that already split a batch themselves, such as a
/// relay that receives one message at a time. Never fails: an unmodelled method
/// or an unreadable payload becomes [`LiveEvent::Unknown`] with its bytes kept.
pub fn decode_event(method: &str, payload: &[u8]) -> LiveEvent {
    decode_message(&BaseProtoMessage {
        method: method.to_owned(),
        payload: payload.to_vec(),
        ..Default::default()
    })
    .event
}

/// Normalises one `BaseProtoMessage`, keeping its raw envelope alongside.
///
/// Internal: `BaseProtoMessage` is a generated type, and this crate's public API
/// deliberately does not expose those. Use [`decode_batch`] or [`decode_event`].
fn decode_message(message: &BaseProtoMessage) -> DecodedEvent {
    let raw = Raw {
        method: message.method.clone(),
        msg_id: message.msg_id.max(0) as u64,
        payload: message.payload.to_vec(),
        is_history: message.is_history,
    };

    let normalized = match message.method.as_str() {
        method::CHAT => normalize::chat(&message.payload),
        method::GIFT => normalize::gift(&message.payload),
        method::LIKE => normalize::like(&message.payload),
        method::MEMBER => normalize::member(&message.payload),
        method::SOCIAL => normalize::social(&message.payload),
        method::ROOM_USER => normalize::room_user(&message.payload),
        _ => Ok(unknown(&raw)),
    };

    // A payload that fails to decode is still real traffic: keep the bytes and
    // let the caller decide, rather than dropping the event or the whole batch.
    let event = normalized.unwrap_or_else(|_| unknown(&raw));
    DecodedEvent { raw, event }
}

fn unknown(raw: &Raw) -> LiveEvent {
    LiveEvent::Unknown {
        method: raw.method.clone(),
        payload: raw.payload.clone(),
    }
}
