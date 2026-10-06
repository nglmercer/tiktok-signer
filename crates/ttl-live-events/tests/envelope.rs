use ttl_live_events::{decode_event_envelope, EventSupport, JsonMode, SchemaValue};
use ttl_sign_core::proto::Writer;
#[test]
fn future_method_and_field_keep_metadata_wire_and_raw() {
    let p = Writer::new()
        .str_field(3, "hello")
        .u64_field(999, 42)
        .clone()
        .finish();
    let e = decode_event_envelope("WebcastChatMessage", u64::MAX, true, &p);
    assert_eq!(e.support(), EventSupport::Normalized);
    assert_eq!(e.schema.fields[1].number, 999);
    assert_eq!(e.schema.fields[1].value, SchemaValue::Varint(42));
    let f = decode_event_envelope("WebcastFutureMessage", u64::MAX, true, &p);
    assert_eq!(f.support(), EventSupport::Raw);
    assert_eq!(f.raw_payload(), p);
    assert!(f.to_json(JsonMode::Normalized).is_none());
    let j: serde_json::Value = serde_json::from_str(&e.to_json(JsonMode::Full).unwrap()).unwrap();
    assert_eq!(j["messageId"], u64::MAX.to_string());
    assert_eq!(j["wireFields"][1]["number"], 999);
    assert_eq!(e.to_json(JsonMode::Full), e.to_json(JsonMode::Full));
}
#[test]
fn every_webcast_descriptor_resolves_and_has_schema_support() {
    for s in ttl_live_proto::schemas()
        .iter()
        .filter(|s| s.name.rsplit('.').next().unwrap().starts_with("Webcast"))
    {
        let name = s.name.rsplit('.').next().unwrap();
        let e = decode_event_envelope(name, 1, false, &[]);
        assert!(e.schema.is_known(), "{name}");
        assert_ne!(e.support(), EventSupport::Raw);
    }
}
#[test]
fn malformed_nested_event_does_not_drop_batch_successors() {
    use prost::Message;
    use ttl_live_proto::{BaseProtoMessage, ProtoMessageFetchResult};
    let p = Writer::new()
        .bytes_field(2, &[0xff])
        .str_field(3, "still delivered")
        .clone()
        .finish();
    let b = ProtoMessageFetchResult {
        messages: vec![
            BaseProtoMessage {
                method: "WebcastChatMessage".into(),
                payload: p.clone(),
                ..Default::default()
            },
            BaseProtoMessage {
                method: "WebcastFutureMessage".into(),
                payload: vec![8, 42],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let b = ttl_live_events::decode_batch(&b.encode_to_vec()).unwrap();
    assert_eq!(b.events.len(), 2);
    assert!(b.events[0].schema.warning);
    assert_eq!(
        b.events[0].schema.fields[0].value,
        SchemaValue::Bytes(vec![0xff])
    );
    assert_eq!(b.events[1].schema.fields[0].value, SchemaValue::Varint(42));
}
#[test]
fn oversized_event_retains_bounded_prefix_and_truncation() {
    let p = vec![0; ttl_live_events::dynamic::MAX_EVENT_BYTES + 1];
    let e = decode_event_envelope("WebcastChatMessage", 0, false, &p);
    assert!(e.schema.truncated);
    assert!(e.normalized().is_none());
    assert_eq!(
        e.raw_payload().len(),
        ttl_live_events::dynamic::MAX_EVENT_BYTES
    );
}
#[test]
fn captured_schema_only_events_are_available_in_full_json() {
    for (method, file) in [
        ("WebcastLiveIntroMessage", "live-intro.pb"),
        ("WebcastGiftPanelUpdateMessage", "gift-panel-update.pb"),
        (
            "WebcastGiftDynamicRestrictionMessage",
            "gift-dynamic-restriction.pb",
        ),
        (
            "WebcastLinkMicLayoutStateMessage",
            "link-mic-layout-state.pb",
        ),
    ] {
        let p = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/events")
                .join(file),
        )
        .unwrap();
        let e = decode_event_envelope(method, 1, false, &p);
        assert_eq!(e.support(), EventSupport::Schema);
        assert!(!e.schema.fields.is_empty());
        assert!(!e.to_json(JsonMode::Full).unwrap().contains("\"data\":{}"));
    }
}
#[test]
fn legacy_serialization_rebuilds_complete_schema() {
    let e = decode_event_envelope("WebcastFutureMessage", 1, true, &[8, 42]);
    let json = serde_json::to_string(&e).unwrap();
    let parsed: ttl_live_events::DecodedEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(e, parsed);
}
#[test]
fn batch_message_limit_is_checked_before_allocating_generated_objects() {
    let p = [10, 0].repeat(8193);
    assert!(matches!(
        ttl_live_events::decode_batch(&p),
        Err(ttl_live_events::EventError::TooLarge)
    ));
}
#[test]
fn exhausted_batch_field_budget_truncates_trees_without_dropping_events() {
    use prost::Message;
    use ttl_live_proto::{BaseProtoMessage, ProtoMessageFetchResult};
    let batch = ProtoMessageFetchResult {
        messages: (0..17)
            .map(|i| BaseProtoMessage {
                method: "WebcastFutureMessage".into(),
                msg_id: i,
                payload: [8, 42].repeat(4096),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let decoded = ttl_live_events::decode_batch(&batch.encode_to_vec()).unwrap();
    assert_eq!(decoded.events.len(), 17);
    assert_eq!(decoded.events[15].schema.fields.len(), 4096);
    assert!(decoded.events[16].schema.truncated);
    assert!(decoded.events[16].schema.fields.is_empty());
    assert_eq!(decoded.events[16].raw_payload(), [8, 42].repeat(4096));
}
#[test]
fn logical_json_types_are_stable_protobuf_names() {
    let e = decode_event_envelope(
        "WebcastMemberMessage",
        1,
        false,
        &Writer::new().u64_field(10, 99999).clone().finish(),
    );
    let j: serde_json::Value = serde_json::from_str(&e.to_json(JsonMode::Full).unwrap()).unwrap();
    assert_eq!(j["wireFields"][0]["protoType"], "enum");
    assert_eq!(
        j["wireFields"][0]["enumType"],
        "webcast.im.MemberMessageAction"
    );
    assert!(j["wireFields"][0]["enumName"].is_null());
    assert_eq!(j["wireFields"][0]["value"], 99999);
}
