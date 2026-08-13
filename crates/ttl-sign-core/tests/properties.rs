use proptest::prelude::*;
use ttl_sign_core::{sanitize_uri, Query};

proptest! {
    #[test]
    fn encoded_queries_roundtrip_semantically(
        entries in prop::collection::vec((any::<String>(), any::<String>()), 0..32)
    ) {
        let mut query = Query::new();
        for (name, value) in entries {
            query.push_raw(name, value);
        }
        let encoded = query.encode();
        prop_assert_eq!(Query::parse_encoded(&encoded), query);
    }

    #[test]
    fn uri_sanitization_is_idempotent(uri in any::<String>()) {
        let once = sanitize_uri(&uri);
        prop_assert_eq!(sanitize_uri(&once), once);
    }

    #[test]
    fn encoded_query_never_contains_raw_spaces(value in any::<String>()) {
        let mut query = Query::new();
        query.push_raw("value", value);
        prop_assert!(!query.encode().contains(' '));
    }
}
