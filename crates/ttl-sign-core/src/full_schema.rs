//! Generated TikTok Webcast schema bindings and bounded dynamic decoding.
//!
//! The bundled MIT-licensed schema snapshot generates Rust bindings for every protobuf
//! message it declares. Page-owned WebSocket traffic is decoded through its descriptors,
//! rather than directly into generated recursive structs: TikTok can add fields and send
//! payloads that are newer than the snapshot. The dynamic representation therefore preserves
//! every decoded field and reports explicit depth and field-count limits.

use crate::proto::{ProtoError, RawProtoField, RawProtoValue, Reader};

/// Maximum number of schema-described nested messages decoded from one event.
///
/// A limit keeps untrusted page traffic bounded when a future TikTok field recursively embeds
/// a message type that is not present in this snapshot.
const MAX_SCHEMA_DEPTH: usize = 8;

/// Maximum protobuf fields decoded from one event, including nested messages.
const MAX_SCHEMA_FIELDS: usize = 4_096;

// `prost-build` owns this module's layout and emits enum payloads directly from the schema.
// Linting generated third-party code would otherwise reject valid bindings because some TikTok
// oneofs have intentionally large variants.
#[allow(clippy::all)]
mod generated_bindings {
    include!(concat!(env!("OUT_DIR"), "/tiktok_schema.rs"));
}

/// Every `prost` binding generated from the bundled TikTok schema snapshot.
pub use generated_bindings::tik_tok;

/// Protobuf wire representation expected for a schema field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Varint,
    Fixed64,
    Fixed32,
    String,
    Bytes,
    Message(&'static str),
}

/// Descriptor for one field in the vendored schema snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSchema {
    pub number: u32,
    pub name: &'static str,
    pub kind: FieldKind,
}

/// Descriptor for one protobuf message in the vendored schema snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageSchema {
    pub name: &'static str,
    pub fields: &'static [FieldSchema],
}

include!(concat!(env!("OUT_DIR"), "/schema_registry.rs"));

/// A page WebSocket event decoded against its known schema, when available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaMessage {
    /// TikTok transport method, for example `WebcastChatMessage`.
    pub method: String,
    /// Schema matched from the method name. `None` means this TikTok method is newer than
    /// the bundled snapshot or is not a standard `Webcast*` message.
    pub schema: Option<&'static MessageSchema>,
    /// Top-level protobuf fields decoded before a resource limit, including fields not found
    /// in the schema.
    pub fields: Vec<SchemaField>,
    /// `true` when a resource limit stopped recursive decoding before this event ended.
    pub truncated: bool,
}

impl SchemaMessage {
    /// Fully qualified schema name, or a stable marker for an unknown method.
    pub fn schema_name(&self) -> &str {
        self.schema.map_or("Unknown", |schema| schema.name)
    }

    /// Was this method found in the bundled snapshot?
    ///
    /// `false` means TikTok shipped a message type newer than the snapshot. The event is
    /// still decoded: its fields are in [`SchemaMessage::fields`] with wire numbers and
    /// values, only without names.
    pub fn is_known(&self) -> bool {
        self.schema.is_some()
    }

    /// First top-level field whose schema name matches `name`, ignoring ASCII case.
    pub fn field_named(&self, name: &str) -> Option<&SchemaField> {
        find_field(&self.fields, name)
    }

    /// Text value of a named field, if it is text.
    pub fn text(&self, name: &str) -> Option<&str> {
        field_text(&self.fields, name)
    }

    /// Numeric value of a named field, whatever integer width it arrived as.
    pub fn number(&self, name: &str) -> Option<u64> {
        field_number(&self.fields, name)
    }

    /// Boolean value of a named field. Protobuf sends these as varints.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        field_number(&self.fields, name).map(|value| value != 0)
    }

    /// Nested message under a named field, for reaching into `user`, `gift`, and similar.
    pub fn message(&self, name: &str) -> Option<&SchemaObject> {
        field_message(&self.fields, name)
    }
}

impl SchemaObject {
    /// First field whose schema name matches `name`, ignoring ASCII case.
    pub fn field_named(&self, name: &str) -> Option<&SchemaField> {
        find_field(&self.fields, name)
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        field_text(&self.fields, name)
    }

    pub fn number(&self, name: &str) -> Option<u64> {
        field_number(&self.fields, name)
    }

    pub fn boolean(&self, name: &str) -> Option<bool> {
        field_number(&self.fields, name).map(|value| value != 0)
    }

    pub fn message(&self, name: &str) -> Option<&SchemaObject> {
        field_message(&self.fields, name)
    }
}

// Field lookup is shared: an event and a nested object are the same shape, and a caller
// walking `user.badge.name` should not meet a different API at each level.

fn find_field<'a>(fields: &'a [SchemaField], name: &str) -> Option<&'a SchemaField> {
    fields.iter().find(|field| {
        field
            .name
            .is_some_and(|field_name| field_name.eq_ignore_ascii_case(name))
    })
}

fn field_text<'a>(fields: &'a [SchemaField], name: &str) -> Option<&'a str> {
    match &find_field(fields, name)?.value {
        SchemaValue::Text(text) => Some(text),
        _ => None,
    }
}

fn field_number(fields: &[SchemaField], name: &str) -> Option<u64> {
    match find_field(fields, name)?.value {
        SchemaValue::Varint(value) | SchemaValue::Fixed64(value) => Some(value),
        SchemaValue::Fixed32(value) => Some(u64::from(value)),
        _ => None,
    }
}

fn field_message<'a>(fields: &'a [SchemaField], name: &str) -> Option<&'a SchemaObject> {
    match &find_field(fields, name)?.value {
        SchemaValue::Message(object) => Some(object),
        _ => None,
    }
}

/// A schema-described nested protobuf object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaObject {
    /// Descriptor used to decode this object, if its message type is in the snapshot.
    pub schema: Option<&'static MessageSchema>,
    /// Fields in original wire order.
    pub fields: Vec<SchemaField>,
    /// `true` when a resource limit stopped recursive decoding before this object ended.
    pub truncated: bool,
}

/// A protobuf field with its descriptor name when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    pub number: u32,
    pub name: Option<&'static str>,
    pub value: SchemaValue,
}

/// Lossless-or-bounded value from a schema-aware protobuf decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaValue {
    Varint(u64),
    Fixed64(u64),
    Fixed32(u32),
    Text(String),
    Bytes(Vec<u8>),
    Message(Box<SchemaObject>),
    /// The decoder reached a resource limit before interpreting the enclosed bytes.
    Truncated(Vec<u8>),
}

/// Decode one event payload using the generated schema descriptors.
///
/// This function does not instantiate generated `prost` message structs for live traffic.
/// That avoids a stale recursive schema turning an otherwise valid future event into an
/// unbounded decode. Generated types remain available in [`tik_tok`] for callers decoding
/// a known, trusted payload.
pub fn decode_webcast_message(method: &str, payload: &[u8]) -> Result<SchemaMessage, ProtoError> {
    let schema = schema_for_method(method);
    let mut fields_remaining = MAX_SCHEMA_FIELDS;
    let object = decode_object(payload, schema, 0, &mut fields_remaining)?;
    Ok(SchemaMessage {
        method: method.to_owned(),
        schema,
        fields: object.fields,
        truncated: object.truncated,
    })
}

fn decode_object(
    payload: &[u8],
    schema: Option<&'static MessageSchema>,
    depth: usize,
    fields_remaining: &mut usize,
) -> Result<SchemaObject, ProtoError> {
    let mut reader = Reader::new(payload);
    let mut fields = Vec::new();
    while *fields_remaining > 0 {
        let Some(raw_field) = reader.next_field() else {
            break;
        };
        let (number, value) = raw_field?;
        *fields_remaining -= 1;
        fields.push(decode_field(
            RawProtoField {
                number,
                value: value.into(),
            },
            schema,
            depth,
            fields_remaining,
        ));
    }
    Ok(SchemaObject {
        schema,
        fields,
        truncated: reader.has_remaining(),
    })
}

fn decode_field(
    raw_field: RawProtoField,
    schema: Option<&'static MessageSchema>,
    depth: usize,
    fields_remaining: &mut usize,
) -> SchemaField {
    let field_schema = schema.and_then(|message| {
        message
            .fields
            .iter()
            .find(|field| field.number == raw_field.number)
    });
    let name = field_schema.map(|field| field.name);
    let value = match raw_field.value {
        RawProtoValue::Varint(value) => SchemaValue::Varint(value),
        RawProtoValue::Fixed64(value) => SchemaValue::Fixed64(value),
        RawProtoValue::Fixed32(value) => SchemaValue::Fixed32(value),
        RawProtoValue::Bytes(bytes) => decode_bytes(bytes, field_schema, depth, fields_remaining),
    };
    SchemaField {
        number: raw_field.number,
        name,
        value,
    }
}

fn decode_bytes(
    bytes: Vec<u8>,
    field_schema: Option<&FieldSchema>,
    depth: usize,
    fields_remaining: &mut usize,
) -> SchemaValue {
    match field_schema.map(|field| field.kind) {
        Some(FieldKind::String) => match String::from_utf8(bytes) {
            Ok(text) => SchemaValue::Text(text),
            Err(error) => SchemaValue::Bytes(error.into_bytes()),
        },
        Some(FieldKind::Message(message_name))
            if depth < MAX_SCHEMA_DEPTH && *fields_remaining > 0 =>
        {
            let nested_schema = schema_by_name(message_name);
            match decode_object(&bytes, nested_schema, depth + 1, fields_remaining) {
                Ok(object) => SchemaValue::Message(Box::new(object)),
                // An evolving nested payload remains available as opaque bytes instead of
                // failing the parent message.
                Err(_) => SchemaValue::Bytes(bytes),
            }
        }
        Some(FieldKind::Message(_)) => SchemaValue::Truncated(bytes),
        _ => SchemaValue::Bytes(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Writer;

    const CHAT_CONTENT_FIELD: u32 = 3;
    const UNKNOWN_FIELD: u32 = 7;
    const UNKNOWN_VALUE: u64 = 42;

    #[test]
    fn decodes_a_schema_mapped_chat_message() {
        let payload = Writer::new()
            .str_field(CHAT_CONTENT_FIELD, "hello from the schema registry")
            .clone()
            .finish();

        let event = decode_webcast_message("WebcastChatMessage", &payload).unwrap();

        assert_eq!(event.schema_name(), "TikTok.Messages.ChatMessage");
        assert!(matches!(
            event.field_named("content"),
            Some(SchemaField {
                value: SchemaValue::Text(content),
                ..
            }) if content == "hello from the schema registry"
        ));
        assert!(schema_by_name("TikTok.UnknownObjects.RoomNotifyMessage").is_some());
        assert!(schema_for_method("WebcastLinkMicFanTicketMethod").is_some());
    }

    /// Message types live in sibling namespaces too. Searching only `TikTok.Messages`
    /// reported these as unknown while their descriptor sat in the snapshot unused.
    #[test]
    fn methods_defined_outside_the_messages_namespace_resolve() {
        for method in [
            "WebcastRoomNotifyMessage",
            "WebcastHourlyRankMessage",
            "WebcastSystemMessage",
            "WebcastRoomPinMessage",
            "WebcastQuestionNewMessage",
            "WebcastGiftBroadcastMessage",
        ] {
            let schema = schema_for_method(method)
                .unwrap_or_else(|| panic!("{method} has a descriptor in the snapshot"));
            assert!(
                schema.name.starts_with("TikTok."),
                "{method} resolved to {}",
                schema.name
            );
        }
    }

    /// `TikTok.Messages` still wins when a name exists in more than one namespace.
    #[test]
    fn the_messages_namespace_is_preferred() {
        let chat = schema_for_method("WebcastChatMessage").expect("chat is a page message");
        assert_eq!(chat.name, "TikTok.Messages.ChatMessage");
    }

    /// A genuinely newer method has no descriptor, and must stay unknown rather than
    /// matching some unrelated nested type by name.
    #[test]
    fn methods_absent_from_the_snapshot_stay_unknown() {
        assert!(schema_for_method("WebcastAISummaryMessage").is_none());
        assert!(schema_for_method("WebcastGiftPanelUpdateMessage").is_none());
        assert!(schema_for_method("NotEvenAWebcastMethod").is_none());
    }

    #[test]
    fn preserves_unknown_methods_as_raw_schema_fields() {
        let payload = Writer::new()
            .u64_field(UNKNOWN_FIELD, UNKNOWN_VALUE)
            .clone()
            .finish();

        let event = decode_webcast_message("WebcastFutureMessage", &payload).unwrap();

        assert!(event.schema.is_none());
        assert!(matches!(
            event.fields.as_slice(),
            [SchemaField {
                number: UNKNOWN_FIELD,
                name: None,
                value: SchemaValue::Varint(UNKNOWN_VALUE),
            }]
        ));
    }

    /// Any of the snapshot's methods can be read by field name, without a hand-written
    /// struct per message type.
    #[test]
    fn named_accessors_reach_values_and_nested_messages() {
        const USER_FIELD: u32 = 2;
        const CONTENT_FIELD: u32 = 3;
        const USER_ID_FIELD: u32 = 1;
        const NICKNAME_FIELD: u32 = 3;

        let user = Writer::new()
            .u64_field(USER_ID_FIELD, 4242)
            .str_field(NICKNAME_FIELD, "Ada")
            .clone()
            .finish();
        let payload = Writer::new()
            .bytes_field(USER_FIELD, &user)
            .str_field(CONTENT_FIELD, "hello")
            .clone()
            .finish();

        let event = decode_webcast_message("WebcastChatMessage", &payload).unwrap();

        assert!(event.is_known());
        assert_eq!(event.text("content"), Some("hello"));
        // Names are matched case-insensitively: the schema spells it `Content`.
        assert_eq!(event.text("CONTENT"), Some("hello"));

        let sender = event.message("user").expect("chat carries a user");
        assert_eq!(sender.text("nickname"), Some("Ada"));
        assert_eq!(sender.number("id"), Some(4242));

        // Asking for the wrong shape is `None`, never a panic or a coerced value.
        assert_eq!(event.number("content"), None);
        assert_eq!(event.text("user"), None);
        assert_eq!(event.message("content"), None);
        assert_eq!(event.text("no_such_field"), None);
    }

    /// An event newer than the snapshot still arrives with its data intact.
    #[test]
    fn unknown_methods_keep_their_values_without_names() {
        let payload = Writer::new()
            .u64_field(UNKNOWN_FIELD, UNKNOWN_VALUE)
            .clone()
            .finish();

        let event = decode_webcast_message("WebcastAISummaryMessage", &payload).unwrap();

        assert!(!event.is_known(), "not in the snapshot");
        assert_eq!(event.schema_name(), "Unknown");
        assert_eq!(event.text("anything"), None, "no names to match against");
        // The payload is not lost, only unnamed.
        assert_eq!(event.fields.len(), 1);
        assert_eq!(event.fields[0].number, UNKNOWN_FIELD);
        assert_eq!(event.fields[0].value, SchemaValue::Varint(UNKNOWN_VALUE));
    }

    #[test]
    fn limits_field_count_without_parsing_the_remaining_packet() {
        let mut writer = Writer::new();
        for _ in 0..=MAX_SCHEMA_FIELDS {
            writer.u64_field(UNKNOWN_FIELD, UNKNOWN_VALUE);
        }
        let payload = writer.finish();

        let event = decode_webcast_message("WebcastFutureMessage", &payload).unwrap();

        assert_eq!(event.fields.len(), MAX_SCHEMA_FIELDS);
        assert!(event.truncated);
    }
}
