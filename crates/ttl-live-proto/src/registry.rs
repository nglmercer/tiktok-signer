//! Descriptor registry for the pinned schema.
//!
//! Generated structs answer "decode these bytes as `WebcastChatMessage`". This
//! registry answers the different question the page-owned WebSocket path asks:
//! "what is field 7 of whatever this method is called?" — including for methods
//! TikTok shipped after the pin, where no generated struct exists.
//!
//! It is plain data with no dependencies. The decoder that walks it lives in
//! `ttl-live-events`.

/// Protobuf wire representation expected for a schema field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Varint,
    Fixed64,
    Fixed32,
    String,
    Bytes,
    /// Fully qualified name of the nested message type.
    Message(&'static str),
}

/// Descriptor for one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSchema {
    pub number: u32,
    pub name: &'static str,
    pub kind: FieldKind,
}

/// Descriptor for one protobuf message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageSchema {
    /// Fully qualified protobuf name, e.g. `webcast.model.message.WebcastChatMessage`.
    pub name: &'static str,
    pub fields: &'static [FieldSchema],
}

impl MessageSchema {
    /// Descriptor for a field number, if the schema declares one.
    pub fn field(&self, number: u32) -> Option<&'static FieldSchema> {
        self.fields.iter().find(|field| field.number == number)
    }
}

include!(concat!(env!("OUT_DIR"), "/schema_registry.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_methods_we_normalise() {
        for method in [
            "WebcastChatMessage",
            "WebcastGiftMessage",
            "WebcastLikeMessage",
            "WebcastMemberMessage",
            "WebcastSocialMessage",
            "WebcastRoomUserSeqMessage",
        ] {
            let schema = schema_for_method(method)
                .unwrap_or_else(|| panic!("{method} has a descriptor in the pinned schema"));
            assert_eq!(
                schema.name,
                format!("webcast.model.message.{method}"),
                "{method} resolved to the wrong package"
            );
        }
    }

    /// Methods the old snapshot could not describe. The whole point of re-pinning
    /// to v3 was that these stop coming back as unknown.
    #[test]
    fn resolves_methods_absent_from_the_retired_snapshot() {
        for method in [
            "WebcastGiftPanelUpdateMessage",
            "WebcastLinkMicFanTicketMethod",
        ] {
            assert!(
                schema_for_method(method).is_some(),
                "{method} should resolve in v3"
            );
        }
    }

    /// A genuinely newer method must stay unknown rather than matching some
    /// unrelated type, and a non-method name must never resolve at all.
    #[test]
    fn unknown_and_non_method_names_do_not_resolve() {
        assert!(schema_for_method("WebcastDefinitelyNotARealMessage").is_none());
        assert!(schema_for_method("NotEvenAWebcastMethod").is_none());
        // `User` is a real message but not a method; it must not resolve.
        assert!(schema_for_method("User").is_none());
    }

    #[test]
    fn nested_types_are_addressable_by_qualified_name() {
        let user = schema_by_name("webcast.model.base.user.User").expect("User descriptor");
        assert_eq!(user.field(3).map(|field| field.name), Some("nickname"));
        assert_eq!(user.field(38).map(|field| field.name), Some("display_id"));
    }

    #[test]
    fn chat_content_is_a_string_and_user_is_a_message() {
        let chat = schema_for_method("WebcastChatMessage").expect("chat descriptor");
        assert_eq!(
            chat.field(3).map(|field| field.kind),
            Some(FieldKind::String)
        );
        assert_eq!(
            chat.field(2).map(|field| field.kind),
            Some(FieldKind::Message("webcast.model.base.user.User"))
        );
    }

    /// Guards against a schema update silently gutting the registry. The bounds
    /// are deliberately loose — they catch "the descriptor set came back empty",
    /// not ordinary upstream churn.
    #[test]
    fn the_registry_is_populated() {
        let all = schemas();
        let methods = all
            .iter()
            .filter(|schema| {
                schema
                    .name
                    .rsplit('.')
                    .next()
                    .is_some_and(|short| short.starts_with("Webcast"))
            })
            .count();

        assert!(all.len() > 500, "only {} descriptors", all.len());
        assert!(methods > 40, "only {methods} Webcast* descriptors");

        // The generated counts must describe the generated data.
        assert_eq!(all.len(), GENERATED_SCHEMA_MESSAGE_COUNT);
        assert_eq!(methods, GENERATED_WEBCAST_METHOD_COUNT);
    }

    /// Every method observed in the committed capture must resolve.
    ///
    /// Three of these (`GiftPanelUpdate`, `GiftDynamicRestriction`,
    /// `LinkMicLayoutState`) had no descriptor in the retired January-2025
    /// snapshot and decoded without field names. Re-pinning to v3 is what fixed
    /// them, so this test is the regression guard for that gain.
    #[test]
    fn every_method_in_the_capture_resolves() {
        for method in [
            "WebcastChatMessage",
            "WebcastLiveIntroMessage",
            "WebcastGiftPanelUpdateMessage",
            "WebcastGiftDynamicRestrictionMessage",
            "WebcastLinkMicLayoutStateMessage",
            "WebcastLinkMicFanTicketMethod",
        ] {
            assert!(
                schema_for_method(method).is_some(),
                "{method} appears in fixtures/events but has no descriptor"
            );
        }
    }

    /// Two methods in the capture resolve in *no* schema — neither v3 nor the
    /// retired snapshot ever described them. That is not a regression, and it is
    /// the case the dynamic decoder exists for: they still decode, with wire
    /// numbers and values but no field names.
    #[test]
    fn methods_absent_from_every_schema_stay_unresolved() {
        assert!(schema_for_method("WebcastUpdateShareRevenueNoticeMessage").is_none());
        // Not a `Webcast*` name at all, so it cannot be a method lookup.
        assert!(schema_for_method("RoomMessage").is_none());
    }
}
