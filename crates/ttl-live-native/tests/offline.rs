use std::{
    ffi::{CStr, CString},
    ptr,
};
use ttl_live::*;
use ttl_live_events::{EventSupport, JsonMode};
use ttl_sign_core::proto::Writer;
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/events")
            .join(name),
    )
    .unwrap()
}
unsafe fn decode(method: &str, p: &[u8]) -> *mut NativeEvent {
    ttl_event_decode(
        method.as_ptr().cast(),
        method.len(),
        u64::MAX,
        true,
        p.as_ptr(),
        p.len(),
    )
}
unsafe fn string(p: *const std::ffi::c_char) -> String {
    assert!(!p.is_null());
    CStr::from_ptr(p).to_str().unwrap().to_owned()
}
#[test]
fn native_catalog_matches_every_generated_webcast_descriptor() {
    unsafe {
        let known: Vec<_> = ttl_live_proto::schemas()
            .iter()
            .filter(|s| s.name.rsplit('.').next().unwrap().starts_with("Webcast"))
            .collect();
        assert_eq!(
            ttl_schema_event_count(),
            ttl_live_proto::GENERATED_WEBCAST_METHOD_COUNT
        );
        assert_eq!(ttl_schema_event_count(), known.len());
        for (i, d) in known.iter().enumerate() {
            let s = ttl_schema_event_at(i);
            assert_eq!(string(ttl_event_schema_full_name(s)), d.name);
            let method = string(ttl_event_schema_method(s));
            assert!(!ttl_schema_event_find(method.as_ptr().cast(), method.len()).is_null());
            assert_eq!(ttl_event_schema_field_count(s), d.fields.len());
            for (j, f) in d.fields.iter().enumerate() {
                let f_abi = ttl_event_schema_field_at(s, j);
                assert_eq!(ttl_schema_field_number(f_abi), f.number);
                assert_eq!(string(ttl_schema_field_json_name(f_abi)), f.json_name);
                assert_eq!(ttl_schema_field_is_map(f_abi), f.is_map);
                assert_eq!(
                    ttl_schema_field_oneof_index(f_abi),
                    f.oneof_index.unwrap_or(-1)
                );
            }
            let e = decode(&method, &[]);
            assert_ne!(ttl_event_support(e), EventSupport::Raw as i32);
            ttl_event_free(e);
        }
        assert!(ttl_schema_event_at(known.len()).is_null());
        assert_eq!(string(ttl_schema_fingerprint()).len(), 64);
    }
}
#[test]
fn future_method_clone_raw_json_and_strict_getters() {
    unsafe {
        let p = [8, 42, 18, 3, 0, 1, 2];
        let e = decode("WebcastFutureMessage", &p);
        assert_eq!(ttl_event_support(e), 3);
        assert_eq!(ttl_event_message_id(e), u64::MAX);
        assert!(ttl_event_is_history(e));
        let f = ttl_object_field_at(ttl_event_root(e), 0);
        assert!(ttl_field_name(f).is_null());
        assert_eq!(ttl_field_number(f), 1);
        let mut n = 123;
        assert!(ttl_field_get_u64(f, &mut n));
        assert_eq!(n, 42);
        let mut signed = 321;
        assert!(!ttl_field_get_i64(f, &mut signed));
        assert_eq!(signed, 321);
        assert!(ttl_event_to_json(e, 0).is_null());
        assert_eq!(ttl_last_error_code(), ResultCode::InvalidState);
        let clone = ttl_event_clone(e);
        ttl_event_free(e);
        assert_eq!(
            std::slice::from_raw_parts(ttl_event_raw_data(clone), ttl_event_raw_size(clone)),
            p
        );
        let j = ttl_event_to_json(clone, 4);
        let v: serde_json::Value = serde_json::from_str(&string(j)).unwrap();
        assert_eq!(v["payload"]["$bytes"], "CCoSAwABAg==");
        assert_eq!(v["messageId"], u64::MAX.to_string());
        ttl_string_free(j);
        ttl_event_free(clone);
    }
}
#[test]
fn native_normalization_matches_rust_node_goldens() {
    unsafe {
        for (method, file) in [
            ("WebcastChatMessage", "chat.pb"),
            ("WebcastGiftMessage", "gift.pb"),
            ("WebcastLikeMessage", "like.pb"),
            ("WebcastMemberMessage", "member.pb"),
            ("WebcastSocialMessage", "social.pb"),
            ("WebcastRoomUserSeqMessage", "room-user.pb"),
        ] {
            let p = fixture(file);
            let e = decode(method, &p);
            let rust = ttl_live_events::decode_event_envelope(method, u64::MAX, true, &p)
                .to_json(JsonMode::Normalized)
                .unwrap();
            let c = ttl_event_to_json(e, 0);
            assert_eq!(string(c), rust);
            ttl_string_free(c);
            ttl_event_free(e);
        }
    }
}
#[test]
fn nested_fields_unknown_occurrences_and_malformed_successors() {
    unsafe {
        use prost::Message;
        let p = Writer::new()
            .bytes_field(
                2,
                &Writer::new()
                    .u64_field(1, 6_811_680_656_739_271_686)
                    .clone()
                    .finish(),
            )
            .str_field(3, "a\0b")
            .str_field(3, "second")
            .u64_field(999, 42)
            .clone()
            .finish();
        let e = decode("WebcastChatMessage", &p);
        let root = ttl_event_root(e);
        assert_eq!(
            ttl_object_find_all(root, c"content".as_ptr(), 7, ptr::null_mut(), 0),
            2
        );
        let f = ttl_object_find(root, c"content".as_ptr(), 7);
        let mut txt = ptr::null();
        let mut len = 0;
        assert!(ttl_field_get_string(f, &mut txt, &mut len));
        assert_eq!(std::slice::from_raw_parts(txt.cast::<u8>(), len), b"a\0b");
        let user = ttl_field_get_object(ttl_object_find(root, c"user".as_ptr(), 4));
        let id = ttl_object_find(user, c"id".as_ptr(), 2);
        let mut v = 0i64;
        assert!(ttl_field_get_i64(id, &mut v));
        assert_eq!(v, 6_811_680_656_739_271_686);
        let full = ttl_event_to_json(e, 2);
        let json: serde_json::Value = serde_json::from_str(&string(full)).unwrap();
        assert_eq!(
            json["data"]["content"],
            serde_json::json!(["a\u{0000}b", "second"])
        );
        assert_eq!(json["data"]["user"]["id"], "6811680656739271686");
        ttl_string_free(full);
        ttl_event_free(e);
        let b = ttl_live_proto::ProtoMessageFetchResult {
            messages: vec![
                ttl_live_proto::BaseProtoMessage {
                    method: "WebcastChatMessage".into(),
                    payload: vec![18, 1, 255],
                    ..Default::default()
                },
                ttl_live_proto::BaseProtoMessage {
                    method: "WebcastFutureMessage".into(),
                    payload: vec![8, 42],
                    ..Default::default()
                },
            ],
            cursor: "test-cursor".into(),
            internal_ext: "test-ext".into(),
            need_ack: true,
            heartbeat_duration: 10000,
            ..Default::default()
        }
        .encode_to_vec();
        let batch = ttl_batch_decode(b.as_ptr(), b.len());
        assert_eq!(ttl_batch_event_count(batch), 2);
        assert!(ttl_event_flags(ttl_batch_event_at(batch, 0)) & 8 != 0);
        assert_eq!(ttl_event_support(ttl_batch_event_at(batch, 1)), 3);
        assert_eq!(string(ttl_batch_cursor(batch)), "test-cursor");
        assert!(ttl_batch_need_ack(batch));
        assert_eq!(ttl_batch_heartbeat_duration(batch), 10000);
        ttl_batch_free(batch);
    }
}
#[test]
fn null_bad_utf8_and_wrong_json_mode_report_explicit_errors() {
    unsafe {
        assert!(ttl_event_decode(ptr::null(), 1, 0, false, ptr::null(), 0).is_null());
        assert_eq!(ttl_last_error_code(), ResultCode::InvalidArgument);
        assert!(ttl_event_decode([255u8].as_ptr().cast(), 1, 0, false, ptr::null(), 0).is_null());
        assert_eq!(ttl_last_error_code(), ResultCode::InvalidUtf8);
        let m = CString::new("WebcastFutureMessage").unwrap();
        let e = ttl_event_decode(m.as_ptr(), 20, 0, false, ptr::null(), 0);
        assert!(!e.is_null());
        assert!(ttl_event_to_json(e, 999).is_null());
        ttl_event_free(e);
        ttl_event_free(ptr::null_mut());
        ttl_batch_free(ptr::null_mut());
        ttl_string_free(ptr::null_mut());
    }
}

#[test]
fn packed_repeated_and_map_fields_are_traversable_through_c() {
    unsafe {
        use ttl_live_proto::{FieldCardinality, FieldValueKind as K};
        let (s, d) = ttl_live_proto::schemas()
            .iter()
            .filter(|s| s.name.rsplit('.').next().unwrap().starts_with("Webcast"))
            .find_map(|s| {
                s.fields
                    .iter()
                    .find(|f| {
                        f.cardinality == FieldCardinality::Repeated
                            && matches!(f.value_kind, K::Int64)
                    })
                    .map(|d| (s, d))
            })
            .expect("repeated int64 event");
        let payload = Writer::new()
            .bytes_field(d.number, &[1, 2, 3])
            .u64_field(d.number, 4)
            .clone()
            .finish();
        let e = decode(s.name.rsplit('.').next().unwrap(), &payload);
        let o = ttl_event_root(e);
        assert_eq!(
            ttl_object_find_all(o, d.name.as_ptr().cast(), d.name.len(), ptr::null_mut(), 0),
            4
        );
        for i in 0..4 {
            let f = ttl_object_field_at(o, i);
            let mut n = 0i64;
            assert!(ttl_field_get_i64(f, &mut n));
            assert_eq!(n, i as i64 + 1);
            assert_eq!(ttl_field_cardinality(f), 1);
        }
        ttl_event_free(e);
        let (s, d) = ttl_live_proto::schemas()
            .iter()
            .filter(|s| s.name.rsplit('.').next().unwrap().starts_with("Webcast"))
            .find_map(|s| {
                s.fields
                    .iter()
                    .find(|d| {
                        d.is_map
                            && match d.value_kind {
                                K::Message(n) => {
                                    ttl_live_proto::schema_by_name(n).is_some_and(|s| {
                                        s.field(1).is_some_and(|f| f.value_kind == K::String)
                                            && s.field(2).is_some_and(|f| f.value_kind == K::String)
                                    })
                                }
                                _ => false,
                            }
                    })
                    .map(|d| (s, d))
            })
            .expect("string map event");
        let first = Writer::new()
            .str_field(1, "key")
            .str_field(2, "first")
            .clone()
            .finish();
        let last = Writer::new()
            .str_field(1, "key")
            .str_field(2, "last")
            .clone()
            .finish();
        let payload = Writer::new()
            .bytes_field(d.number, &first)
            .bytes_field(d.number, &last)
            .bytes_field(d.number, &[10, 0])
            .clone()
            .finish();
        let e = decode(s.name.rsplit('.').next().unwrap(), &payload);
        let o = ttl_event_root(e);
        assert!(ttl_field_is_map(ttl_object_field_at(o, 0)));
        assert_eq!(
            ttl_object_map_size(o, d.name.as_ptr().cast(), d.name.len()),
            3
        );
        assert_eq!(
            ttl_field_number(ttl_object_map_key_at(
                o,
                d.name.as_ptr().cast(),
                d.name.len(),
                0
            )),
            1
        );
        assert_eq!(
            ttl_field_number(ttl_object_map_value_at(
                o,
                d.name.as_ptr().cast(),
                d.name.len(),
                0
            )),
            2
        );
        let json = ttl_event_to_json(e, 2);
        let v: serde_json::Value = serde_json::from_str(&string(json)).unwrap();
        assert_eq!(v["data"][d.json_name]["key"], "last");
        assert_eq!(v["data"][d.json_name][""], "");
        assert_eq!(v["wireFields"].as_array().unwrap().len(), 3);
        ttl_string_free(json);
        ttl_event_free(e);
    }
}
#[test]
fn signed_enum_and_normalized_views_preserve_types_and_values() {
    unsafe {
        let p = Writer::new().u64_field(2, (-42i64) as u64).clone().finish();
        let e = decode("WebcastGiftMessage", &p);
        let f = ttl_object_field_at(ttl_event_root(e), 0);
        let mut signed = 0;
        let mut unsigned = 17;
        assert!(ttl_field_get_i64(f, &mut signed));
        assert_eq!(signed, -42);
        assert!(!ttl_field_get_u64(f, &mut unsigned));
        assert_eq!(unsigned, 17);
        ttl_event_free(e);
        let e = decode(
            "WebcastMemberMessage",
            &Writer::new().u64_field(10, 99999).clone().finish(),
        );
        let f = ttl_object_field_at(ttl_event_root(e), 0);
        let mut n = 0;
        assert!(ttl_field_get_enum_number(f, &mut n));
        assert_eq!(n, 99999);
        assert!(ttl_field_enum_name(f).is_null());
        assert_eq!(
            string(ttl_field_enum_type(f)),
            "webcast.im.MemberMessageAction"
        );
        ttl_event_free(e);
        let e = decode("WebcastChatMessage", &fixture("chat.pb"));
        let mut view = std::mem::MaybeUninit::<ChatView>::uninit();
        assert!(ttl_event_chat(e, view.as_mut_ptr()));
        let view = view.assume_init();
        assert_eq!(view.user.id, 6_811_680_656_739_271_686);
        assert!(view.comment_len > 0);
        ttl_event_free(e);
        let e = decode("WebcastGiftMessage", &fixture("gift.pb"));
        let mut view = std::mem::MaybeUninit::<GiftView>::uninit();
        assert!(ttl_event_gift(e, view.as_mut_ptr()));
        assert!(view.assume_init().gift_id > 0);
        ttl_event_free(e);
    }
}
