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
    ChatEvent, DecodedEvent, EventBatch, EventEnvelope, EventSupport, GiftEvent, JsonMode,
    LikeEvent, LiveEvent, MemberEvent, RawEvent, RoomUserEvent, SocialEvent, TopViewer,
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
    #[error("batch exceeds 16 MiB or 8192-message limit")]
    TooLarge,
    #[error("failed to decode the event batch envelope: {0}")]
    Batch(#[from] prost::DecodeError),
}

/// Decodes one decompressed WebSocket payload into normalised events.
pub fn decode_batch(payload: &[u8]) -> Result<EventBatch, EventError> {
    if payload.len() > 16 * 1024 * 1024 {
        return Err(EventError::TooLarge);
    }
    // Preflight with borrowed wire views before Prost allocates repeated messages.
    let mut reader = ttl_sign_core::proto::Reader::new(payload);
    let mut count = 0;
    while let Some(field) = reader.next_field() {
        match field {
            Ok((1, _)) => {
                count += 1;
                if count > 8192 {
                    return Err(EventError::TooLarge);
                }
            }
            Ok(_) => (),
            Err(_) => break, // Prost reports the precise envelope error.
        }
    }
    let result = ttl_live_proto::decode_event_batch(payload)?;
    let mut schema_budget = 65_536;
    Ok(EventBatch {
        events: result
            .messages
            .iter()
            .map(|message| decode_message(message, &mut schema_budget))
            .collect(),
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
    decode_event_envelope(method, 0, false, payload).event
}

/// Normalises one `BaseProtoMessage`, keeping its raw envelope alongside.
///
/// Internal: `BaseProtoMessage` is a generated type, and this crate's public API
/// deliberately does not expose those. Use [`decode_batch`] or [`decode_event`].
fn decode_message(message: &BaseProtoMessage, budget: &mut usize) -> DecodedEvent {
    decode_envelope_budget(
        &message.method,
        message.msg_id as u64,
        message.is_history,
        &message.payload,
        budget,
    )
}

/// Transport-independent, complete event decoding. Oversized events retain a
/// 4 MiB prefix and are flagged truncated; typed normalization is skipped.
pub fn decode_event_envelope(
    method: &str,
    msg_id: u64,
    is_history: bool,
    payload: &[u8],
) -> EventEnvelope {
    decode_envelope_budget(method, msg_id, is_history, payload, &mut 4096)
}

fn decode_envelope_budget(
    method: &str,
    msg_id: u64,
    is_history: bool,
    payload: &[u8],
    budget: &mut usize,
) -> EventEnvelope {
    let oversized = payload.len() > dynamic::MAX_EVENT_BYTES;
    let payload = &payload[..payload.len().min(dynamic::MAX_EVENT_BYTES)];
    let raw = Raw {
        method: method.to_owned(),
        msg_id,
        payload: payload.to_vec(),
        is_history,
    };
    let mut schema = dynamic::decode_webcast_message_budget(method, payload, budget)
        .expect("partial decoder is infallible");
    schema.truncated |= oversized;
    let normalized = if schema.truncated {
        Ok(unknown(&raw))
    } else {
        match method {
            method::CHAT => normalize::chat(payload),
            method::GIFT => normalize::gift(payload),
            method::LIKE => normalize::like(payload),
            method::MEMBER => normalize::member(payload),
            method::SOCIAL => normalize::social(payload),
            method::ROOM_USER => normalize::room_user(payload),
            _ => Ok(unknown(&raw)),
        }
    };
    let event = normalized.unwrap_or_else(|_| unknown(&raw));
    DecodedEvent { raw, event, schema }
}

fn unknown(raw: &Raw) -> LiveEvent {
    LiveEvent::Unknown {
        method: raw.method.clone(),
        payload: raw.payload.clone(),
    }
}

pub use event::method_id as event_method_id;
