use serde::{Deserialize, Serialize};

use crate::user::EventUser;

/// A live comment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEvent {
    pub user: EventUser,
    pub comment: String,
}

/// A gift send. TikTok streams repeated gifts as a burst of messages sharing a
/// `group_id`; `repeat_end` marks the final one, which carries the true total.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiftEvent {
    pub user: EventUser,
    pub gift_id: u64,
    pub gift_name: String,
    pub diamond_count: u64,
    pub repeat_count: u64,
    pub combo_count: u64,
    pub group_id: u64,
    pub repeat_end: bool,
}

/// A like burst. `count` is this batch, `total` the room-wide running total.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LikeEvent {
    pub user: EventUser,
    pub count: u64,
    pub total: u64,
}

/// A room membership change (join, follow, subscribe, moderator action, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberEvent {
    pub user: EventUser,
    pub member_count: u64,
    /// Raw `MemberMessageAction`. Left numeric on purpose: TikTok adds actions
    /// without notice, and an unknown value must not become a decode failure.
    pub action: i32,
}

/// A follow or share.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialEvent {
    pub user: EventUser,
    pub action: i64,
    pub follow_count: u64,
    pub share_count: u64,
}

/// Periodic viewer-count update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomUserEvent {
    pub total: u64,
    pub popularity: u64,
    pub total_user: u64,
    pub anonymous: u64,
}

/// The stable, listener-facing event API.
///
/// This type never exposes generated Prost structs, so the schema version can
/// change without breaking consumers. Anything we do not model yet arrives as
/// [`LiveEvent::Unknown`] with its bytes intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LiveEvent {
    Chat(ChatEvent),
    Gift(GiftEvent),
    Like(LikeEvent),
    Member(MemberEvent),
    Social(SocialEvent),
    RoomUser(RoomUserEvent),
    /// An event we have no normaliser for. The payload is preserved verbatim so
    /// callers can decode it themselves and so nothing is ever silently lost.
    Unknown {
        method: String,
        payload: Vec<u8>,
    },
}

impl LiveEvent {
    /// The schema method name this event came from.
    pub fn method(&self) -> &str {
        match self {
            Self::Chat(_) => crate::method::CHAT,
            Self::Gift(_) => crate::method::GIFT,
            Self::Like(_) => crate::method::LIKE,
            Self::Member(_) => crate::method::MEMBER,
            Self::Social(_) => crate::method::SOCIAL,
            Self::RoomUser(_) => crate::method::ROOM_USER,
            Self::Unknown { method, .. } => method,
        }
    }

    /// The user behind the event, when it has one.
    pub fn user(&self) -> Option<&EventUser> {
        match self {
            Self::Chat(event) => Some(&event.user),
            Self::Gift(event) => Some(&event.user),
            Self::Like(event) => Some(&event.user),
            Self::Member(event) => Some(&event.user),
            Self::Social(event) => Some(&event.user),
            Self::RoomUser(_) | Self::Unknown { .. } => None,
        }
    }

    /// Whether this event was preserved rather than normalised.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }
}

/// The undecoded envelope of a single event, always retained alongside the
/// normalised form so no capture is lossy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEvent {
    pub method: String,
    pub msg_id: u64,
    pub payload: Vec<u8>,
    pub is_history: bool,
}

/// One event: its raw envelope plus the normalised interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedEvent {
    pub raw: RawEvent,
    pub event: LiveEvent,
}

/// A decoded `ProtoMessageFetchResult` batch.
///
/// The three transport-relevant fields (`cursor`, `internal_ext`, `need_ack`)
/// are surfaced because the WebSocket ACK path needs them; the transport itself
/// still reads them via its own decoder during the migration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBatch {
    pub events: Vec<DecodedEvent>,
    pub cursor: String,
    pub internal_ext: String,
    pub need_ack: bool,
    pub heartbeat_duration: i64,
    pub push_server: String,
}

impl EventBatch {
    /// Iterates the normalised events, skipping the raw envelopes.
    pub fn events(&self) -> impl Iterator<Item = &LiveEvent> {
        self.events.iter().map(|decoded| &decoded.event)
    }

    /// Method names in this batch that have no normaliser yet. Useful for
    /// deciding which event to implement next from real traffic.
    pub fn unknown_methods(&self) -> Vec<&str> {
        let mut methods: Vec<&str> = self
            .events
            .iter()
            .filter(|decoded| decoded.event.is_unknown())
            .map(|decoded| decoded.raw.method.as_str())
            .collect();
        methods.sort_unstable();
        methods.dedup();
        methods
    }
}
