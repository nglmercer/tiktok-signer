use ttl_live_events::{
    decode_batch, decode_batch_with_mode, decode_event, decode_event_envelope, DecodeMode,
    LiveEvent,
};
use ttl_sign_core::proto::Writer;

const METHOD: &str = "WebcastGiftMessage";
const GIFT: &[u8] = include_bytes!("../../../fixtures/events/deep-gift.pb");
const MSG_ID: u64 = 9_007_199_254_740_994;

fn batch(payloads: &[&[u8]]) -> Vec<u8> {
    let mut batch = Writer::new();
    for (index, payload) in payloads.iter().enumerate() {
        let mut message = Writer::new();
        message
            .str_field(1, METHOD)
            .bytes_field(2, payload)
            .u64_field(3, MSG_ID + index as u64)
            .bool_field(6, true);
        batch.bytes_field(1, &message.finish());
    }
    batch.finish()
}

#[test]
fn diagnostic_depth_does_not_invalidate_a_canonical_gift() {
    let envelope = decode_event_envelope(METHOD, MSG_ID, true, GIFT);
    assert!(
        envelope.schema.truncated,
        "the diagnostic view stays bounded"
    );
    assert!(matches!(envelope.event, LiveEvent::Gift(_)));
    assert_eq!(envelope.raw.msg_id, MSG_ID);
    assert!(envelope.raw.is_history);
    assert_eq!(envelope.raw.payload, GIFT);
    assert!(matches!(decode_event(METHOD, GIFT), LiveEvent::Gift(_)));
}

#[test]
fn normalized_and_full_modes_keep_deep_gifts_and_metadata_in_order() {
    let encoded = batch(&[GIFT, &[255], GIFT]);
    let legacy = decode_batch(&encoded).unwrap();
    assert_eq!(legacy.events.len(), 3);
    assert!(matches!(legacy.events[0].event, LiveEvent::Gift(_)));
    assert!(matches!(legacy.events[1].event, LiveEvent::Unknown { .. }));
    assert!(matches!(legacy.events[2].event, LiveEvent::Gift(_)));
    for mode in [DecodeMode::Normalized, DecodeMode::Full] {
        let selected = decode_batch_with_mode(&encoded, mode).unwrap();
        for (index, (event, original)) in selected.events.iter().zip(&legacy.events).enumerate() {
            assert_eq!(event.event.as_ref(), Some(&original.event));
            assert_eq!(event.msg_id, MSG_ID + index as u64);
            assert!(event.is_history);
        }
        let first = &selected.events[0];
        if mode == DecodeMode::Full {
            assert!(first.schema.as_ref().unwrap().truncated);
            assert!(
                first.truncated,
                "reports the incomplete view, not an invalid gift"
            );
            assert_eq!(first.payload.as_deref(), Some(GIFT));
        } else {
            assert!(!first.truncated);
            assert!(first.schema.is_none() && first.payload.is_none());
        }
    }
}

#[test]
fn canonical_gifts_cannot_bypass_event_or_shared_batch_field_limits() {
    // Repeated scalar occurrences are valid protobuf (last value wins), but
    // must still be bounded even when typed normalization would accept them.
    let mut writer = Writer::new();
    for _ in 0..4096 {
        writer.u64_field(2, 1);
    }
    let bounded = writer.finish();
    let mut excessive = bounded.clone();
    excessive.extend_from_slice(&[16, 1]);
    for mode in [DecodeMode::Normalized, DecodeMode::Full] {
        let decoded = decode_batch_with_mode(&batch(&[&excessive]), mode).unwrap();
        assert!(decoded.events[0].truncated);
        assert!(matches!(
            decoded.events[0].event,
            Some(LiveEvent::Unknown { .. })
        ));
        let encoded = batch(&vec![bounded.as_slice(); 17]);
        let decoded = decode_batch_with_mode(&encoded, mode).unwrap();
        assert!(decoded.events[..16]
            .iter()
            .all(|event| matches!(event.event, Some(LiveEvent::Gift(_)))));
        assert!(decoded.events[16].truncated);
        assert!(matches!(
            decoded.events[16].event,
            Some(LiveEvent::Unknown { .. })
        ));
    }
    assert!(matches!(
        decode_event(METHOD, &excessive),
        LiveEvent::Unknown { .. }
    ));
    let legacy = decode_batch(&batch(&vec![bounded.as_slice(); 17])).unwrap();
    assert!(matches!(legacy.events[15].event, LiveEvent::Gift(_)));
    assert!(matches!(legacy.events[16].event, LiveEvent::Unknown { .. }));
}
