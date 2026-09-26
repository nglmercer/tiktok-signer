//! Schema-version-specific normalisers.
//!
//! Every function here maps one generated v3 message onto a stable event struct.
//! When a v4 schema lands, it gets its own module of normalisers producing the
//! same output types, and consumers are unaffected.

use prost::Message;
use ttl_live_proto::messages::{
    Contributor, WebcastChatMessage, WebcastGiftMessage, WebcastLikeMessage, WebcastMemberMessage,
    WebcastRoomUserSeqMessage, WebcastSocialMessage,
};
use ttl_live_proto::webcast::model::Gift;

use crate::event::{
    ChatEvent, GiftEvent, LikeEvent, LiveEvent, MemberEvent, RoomUserEvent, SocialEvent,
};
use crate::user::EventUser;

/// Protobuf integers are signed; the stable API reports counts as unsigned.
/// Negatives are impossible for these fields in practice, and clamping keeps a
/// corrupt value from wrapping into an absurd number.
fn count(value: impl Into<i64>) -> u64 {
    value.into().max(0) as u64
}

pub(crate) fn chat(payload: &[u8]) -> Result<LiveEvent, prost::DecodeError> {
    let message = WebcastChatMessage::decode(payload)?;
    Ok(LiveEvent::Chat(ChatEvent {
        user: EventUser::normalize(message.user.as_ref()),
        comment: message.content,
    }))
}

pub(crate) fn gift(payload: &[u8]) -> Result<LiveEvent, prost::DecodeError> {
    let message = WebcastGiftMessage::decode(payload)?;
    // The nested `gift` detail block is frequently omitted on repeat messages,
    // so name and diamond count fall back to empty rather than failing.
    let detail = message.gift.as_ref();
    Ok(LiveEvent::Gift(GiftEvent {
        user: EventUser::normalize(message.user.as_ref()),
        gift_id: count(message.gift_id),
        gift_name: detail.map(|gift| gift.name.clone()).unwrap_or_default(),
        diamond_count: detail.map(|gift| count(gift.diamond_count)).unwrap_or(0),
        repeat_count: count(message.repeat_count),
        combo_count: count(message.combo_count),
        group_id: count(message.group_id),
        repeat_end: message.repeat_end != 0,
        gift_image_url: detail.and_then(gift_image_url),
    }))
}

pub(crate) fn like(payload: &[u8]) -> Result<LiveEvent, prost::DecodeError> {
    let message = WebcastLikeMessage::decode(payload)?;
    Ok(LiveEvent::Like(LikeEvent {
        user: EventUser::normalize(message.user.as_ref()),
        count: count(message.count),
        total: count(message.total),
    }))
}

pub(crate) fn member(payload: &[u8]) -> Result<LiveEvent, prost::DecodeError> {
    let message = WebcastMemberMessage::decode(payload)?;
    Ok(LiveEvent::Member(MemberEvent {
        user: EventUser::normalize(message.user.as_ref()),
        member_count: count(message.member_count),
        action: message.action,
    }))
}

pub(crate) fn social(payload: &[u8]) -> Result<LiveEvent, prost::DecodeError> {
    let message = WebcastSocialMessage::decode(payload)?;
    Ok(LiveEvent::Social(SocialEvent {
        user: EventUser::normalize(message.user.as_ref()),
        action: message.action,
        follow_count: count(message.follow_count),
        share_count: count(message.share_count),
    }))
}

pub(crate) fn room_user(payload: &[u8]) -> Result<LiveEvent, prost::DecodeError> {
    let message = WebcastRoomUserSeqMessage::decode(payload)?;
    Ok(LiveEvent::RoomUser(RoomUserEvent {
        total: count(message.total),
        popularity: count(message.popularity),
        total_user: count(message.total_user),
        anonymous: count(message.anonymous),
        top_viewers: message.ranks.iter().map(top_viewer).collect(),
        ranked_viewers: message.ranks.iter().map(top_viewer).collect(),
    }))
}

/// Best available gift artwork: the full `image`, then the small `icon`,
/// then the `preview_image`. Each is an `ImageModel` whose first URL wins.
fn gift_image_url(detail: &Gift) -> Option<String> {
    [&detail.image, &detail.icon, &detail.preview_image]
        .into_iter()
        .flatten()
        .find_map(|image| {
            image
                .url_list
                .first()
                .filter(|url| !url.is_empty())
                .cloned()
        })
}

fn top_viewer(contributor: &Contributor) -> crate::event::TopViewer {
    crate::event::TopViewer {
        rank: count(contributor.rank),
        score: count(contributor.score),
        delta: count(contributor.delta),
        user: EventUser::normalize(contributor.user.as_ref()),
    }
}
