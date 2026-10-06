use super::*;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::OnceLock};
use ttl_live_proto::{FieldCardinality, FieldSchema, FieldValueKind as K, MessageSchema};

pub struct NativeSchema {
    pub descriptor: &'static MessageSchema,
}
pub type NativeSchemaField = FieldSchema;
static CATALOG: OnceLock<Vec<NativeSchema>> = OnceLock::new();
fn catalog() -> &'static [NativeSchema] {
    CATALOG.get_or_init(|| {
        ttl_live_proto::schemas()
            .iter()
            .filter(|s| short(s.name).starts_with("Webcast"))
            .map(|s| NativeSchema { descriptor: s })
            .collect()
    })
}
fn short(s: &str) -> &str {
    s.rsplit('.').next().unwrap_or(s)
}
/// Immortal, immutable descriptor strings. Initialized only from pinned descriptors.
pub(super) fn static_text(s: &str) -> *const c_char {
    static STRINGS: OnceLock<HashMap<String, CString>> = OnceLock::new();
    STRINGS
        .get_or_init(|| {
            let mut m = HashMap::new();
            let mut add = |s: &str| {
                m.entry(s.to_owned()).or_insert_with(|| cstring(s));
            };
            for s in ttl_live_proto::schemas() {
                add(s.name);
                add(short(s.name));
                for f in s.fields {
                    add(f.name);
                    add(f.json_name);
                    if let Some(n) = f.oneof_name {
                        add(n);
                    }
                    if let K::Message(n) | K::Enum(n) = f.value_kind {
                        add(n);
                    }
                }
            }
            for e in ttl_live_proto::registry::ENUMS {
                add(e.name);
                for v in e.values {
                    add(v.name);
                }
            }
            m
        })
        .get(s)
        .map_or(ptr::null(), |s| s.as_ptr())
}
pub(super) fn proto_type(k: Option<K>) -> i32 {
    match k {
        Some(K::Double) => 0,
        Some(K::Float) => 1,
        Some(K::Int64) => 2,
        Some(K::Uint64) => 3,
        Some(K::Sint64) => 4,
        Some(K::Fixed64) => 5,
        Some(K::Sfixed64) => 6,
        Some(K::Int32) => 7,
        Some(K::Uint32) => 8,
        Some(K::Sint32) => 9,
        Some(K::Fixed32) => 10,
        Some(K::Sfixed32) => 11,
        Some(K::Bool) => 12,
        Some(K::String) => 13,
        Some(K::Bytes) => 14,
        Some(K::Enum(_)) => 15,
        Some(K::Message(_)) => 16,
        None => 17,
    }
}
pub(super) fn cardinality(d: Option<&FieldSchema>) -> i32 {
    if d.is_some_and(|d| d.cardinality == FieldCardinality::Repeated) {
        1
    } else {
        0
    }
}
#[no_mangle]
pub unsafe extern "C" fn ttl_schema_event_count() -> usize {
    super::guard(0, || catalog().len())
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_event_at(i: usize) -> *const NativeSchema {
    super::guard(ptr::null(), || catalog().get(i).map_or(ptr::null(), |s| s))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_event_find(
    method: *const c_char,
    len: usize,
) -> *const NativeSchema {
    super::guard(ptr::null(), || {
        let Some(m) = text(method, len, 4096) else {
            return ptr::null();
        };
        let Some(d) = ttl_live_proto::schema_for_method(m) else {
            return ptr::null();
        };
        catalog()
            .iter()
            .find(|s| std::ptr::eq(s.descriptor, d))
            .map_or(ptr::null(), |s| s)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_schema_method(s: *const NativeSchema) -> *const c_char {
    super::guard(ptr::null(), || {
        s.as_ref()
            .map_or(ptr::null(), |s| static_text(short(s.descriptor.name)))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_schema_full_name(s: *const NativeSchema) -> *const c_char {
    super::guard(ptr::null(), || {
        s.as_ref()
            .map_or(ptr::null(), |s| static_text(s.descriptor.name))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_schema_type(s: *const NativeSchema) -> u32 {
    super::guard(0, || {
        s.as_ref().map_or(0, |s| {
            ttl_live_events::event_method_id(short(s.descriptor.name))
        })
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_schema_field_count(s: *const NativeSchema) -> usize {
    super::guard(0, || s.as_ref().map_or(0, |s| s.descriptor.fields.len()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_event_schema_field_at(
    s: *const NativeSchema,
    i: usize,
) -> *const NativeSchemaField {
    super::guard(ptr::null(), || {
        s.as_ref()
            .and_then(|s| s.descriptor.fields.get(i))
            .map_or(ptr::null(), |s| s)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_number(f: *const NativeSchemaField) -> u32 {
    super::guard(0, || f.as_ref().map_or(0, |f| f.number))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_name(f: *const NativeSchemaField) -> *const c_char {
    super::guard(ptr::null(), || {
        f.as_ref().map_or(ptr::null(), |f| static_text(f.name))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_json_name(f: *const NativeSchemaField) -> *const c_char {
    super::guard(ptr::null(), || {
        f.as_ref().map_or(ptr::null(), |f| static_text(f.json_name))
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_proto_type(f: *const NativeSchemaField) -> i32 {
    super::guard(17, || proto_type(f.as_ref().map(|f| f.value_kind)))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_cardinality(f: *const NativeSchemaField) -> i32 {
    super::guard(0, || cardinality(f.as_ref()))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_is_map(f: *const NativeSchemaField) -> bool {
    super::guard(false, || f.as_ref().is_some_and(|f| f.is_map))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_is_deprecated(f: *const NativeSchemaField) -> bool {
    super::guard(false, || f.as_ref().is_some_and(|f| f.deprecated))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_oneof_name(f: *const NativeSchemaField) -> *const c_char {
    super::guard(ptr::null(), || {
        f.as_ref()
            .and_then(|f| f.oneof_name)
            .map_or(ptr::null(), static_text)
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_oneof_index(f: *const NativeSchemaField) -> i32 {
    super::guard(-1, || f.as_ref().and_then(|f| f.oneof_index).unwrap_or(-1))
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_message_name(
    f: *const NativeSchemaField,
) -> *const c_char {
    super::guard(ptr::null(), || match f.as_ref().map(|f| f.value_kind) {
        Some(K::Message(n)) => static_text(n),
        _ => ptr::null(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_enum_type(f: *const NativeSchemaField) -> *const c_char {
    super::guard(ptr::null(), || match f.as_ref().map(|f| f.value_kind) {
        Some(K::Enum(n)) => static_text(n),
        _ => ptr::null(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_field_wire_type(f: *const NativeSchemaField) -> u8 {
    super::guard(255, || {
        f.as_ref().map_or(255, |f| match f.kind {
            ttl_live_proto::FieldKind::Varint => 0,
            ttl_live_proto::FieldKind::Fixed64 => 1,
            ttl_live_proto::FieldKind::Fixed32 => 5,
            _ => 2,
        })
    })
}

#[no_mangle]
pub unsafe extern "C" fn ttl_schema_fingerprint() -> *const c_char {
    super::guard(ptr::null(), || {
        static HASH: OnceLock<CString> = OnceLock::new();
        HASH.get_or_init(|| {
            cstring(&format!(
                "{:x}",
                Sha256::digest(ttl_live_proto::registry::DESCRIPTOR_BYTES)
            ))
        })
        .as_ptr()
    })
}
