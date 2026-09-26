//! Gift artwork: the normaliser exposes the detail block's image URLs so
//! renderers can show the gift without a second lookup.

use prost::Message;
use ttl_live_events::{method, LiveEvent};
use ttl_live_proto::messages::WebcastGiftMessage;
use ttl_live_proto::webcast::model::base::ImageModel;
use ttl_live_proto::webcast::model::Gift;

fn image(url: &str) -> Option<ImageModel> {
    Some(ImageModel {
        url_list: vec![url.to_string()],
        ..Default::default()
    })
}

fn decode(gift: Option<Gift>) -> LiveEvent {
    let message = WebcastGiftMessage {
        gift,
        gift_id: 1,
        ..Default::default()
    };
    let mut payload = Vec::new();
    message.encode(&mut payload).unwrap();
    ttl_live_events::decode_event(method::GIFT, &payload)
}

fn gift_of(event: &LiveEvent) -> &ttl_live_events::GiftEvent {
    match event {
        LiveEvent::Gift(gift) => gift,
        other => panic!("expected a gift event, got {other:?}"),
    }
}

#[test]
fn gift_image_prefers_image_then_icon_then_preview() {
    let full = decode(Some(Gift {
        image: image("https://cdn/image.webp"),
        icon: image("https://cdn/icon.webp"),
        preview_image: image("https://cdn/preview.webp"),
        ..Default::default()
    }));
    assert_eq!(
        gift_of(&full).gift_image_url.as_deref(),
        Some("https://cdn/image.webp"),
    );

    let icon_only = decode(Some(Gift {
        icon: image("https://cdn/icon.webp"),
        ..Default::default()
    }));
    assert_eq!(
        gift_of(&icon_only).gift_image_url.as_deref(),
        Some("https://cdn/icon.webp"),
    );

    let preview_only = decode(Some(Gift {
        preview_image: image("https://cdn/preview.webp"),
        ..Default::default()
    }));
    assert_eq!(
        gift_of(&preview_only).gift_image_url.as_deref(),
        Some("https://cdn/preview.webp"),
    );
}

#[test]
fn gift_without_detail_has_no_image() {
    // Repeat messages omit the detail block: the event still decodes, imageless.
    let event = decode(None);
    assert_eq!(gift_of(&event).gift_image_url, None);
    assert!(gift_of(&event).gift_name.is_empty());
}

#[test]
fn gift_image_serializes_for_hosts() {
    let event = decode(Some(Gift {
        name: "Rose".to_string(),
        image: image("https://cdn/rose.webp"),
        ..Default::default()
    }));
    let json: serde_json::Value = serde_json::to_string(&event)
        .and_then(|text| serde_json::from_str(&text))
        .unwrap();
    assert_eq!(json["type"], "gift");
    assert_eq!(json["gift_name"], "Rose");
    assert_eq!(json["gift_image_url"], "https://cdn/rose.webp");
}
