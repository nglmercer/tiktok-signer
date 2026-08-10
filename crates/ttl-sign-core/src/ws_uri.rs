//! Turning the player's signed WebSocket URI into a `ProtoMessageFetchResult`.
//!
//! TikTok answers `/webcast/im/fetch/` with an empty body for this signer, so the protobuf
//! that clients expect cannot be obtained from the endpoint that normally produces it. It
//! does not have to be: the player builds and signs its own WebSocket URI, `bridge.js`
//! records it before the connection is attempted, and everything a client needs to open that
//! socket is in the URI already.
//!
//! Verified against a real room on 2026-08-10: a URI captured this way, connected from Rust
//! with the page's cookies and **no** `/webcast/im/fetch/` call, received `msg` frames.

use crate::params::percent_decode;
use crate::proto::FetchResult;

/// `cursor` a synthesized result reports.
///
/// The player's URI carries no cursor — the `ws_reuse_supplement` transport does not use one
/// — but clients reject a result whose cursor is empty, treating it as a malformed response.
/// A zero cursor means "from the beginning", which is what a fresh listener wants anyway.
const SYNTHETIC_CURSOR: &str = "0";

/// Characters `http::Uri` accepts.
///
/// A browser is more permissive than any Rust HTTP client. The player's URI genuinely
/// contains raw spaces (`browser_version=5.0 (X11; Linux x86_64) …`), which make
/// `into_client_request` fail with "invalid uri character" before a single byte is sent.
fn is_uri_safe(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '-' | '.'
                | '_'
                | '~'
                | ':'
                | '/'
                | '?'
                | '#'
                | '['
                | ']'
                | '@'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | '%'
        )
}

/// Percent-encode only what an HTTP client cannot parse, leaving existing `%XX` escapes
/// intact.
///
/// Idempotent, and safe on a URI that needed nothing. Encoding the spaces does not
/// invalidate `X-Gnarly`: a sanitized URI was accepted and delivered frames.
pub fn sanitize_uri(uri: &str) -> String {
    if uri.chars().all(is_uri_safe) {
        return uri.to_string();
    }
    let mut out = String::with_capacity(uri.len());
    for character in uri.chars() {
        if is_uri_safe(character) {
            out.push(character);
        } else {
            let mut buffer = [0u8; 4];
            for byte in character.encode_utf8(&mut buffer).as_bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// Build a `ProtoMessageFetchResult` from a signed WebSocket URI.
///
/// Values are **decoded**, because clients re-encode `route_params` when they rebuild the
/// query. Storing `%2F` verbatim would be encoded again into `%252F` and break the
/// signature; storing `/` round-trips back to the original.
///
/// Returns `None` when the URI has no query, which means it carries no signature and is not
/// a usable transport.
pub fn fetch_result_from_ws_uri(uri: &str) -> Option<FetchResult> {
    let (push_server, query) = uri.split_once('?')?;
    if push_server.is_empty() || query.is_empty() {
        return None;
    }

    let mut route_params = Vec::new();
    let mut internal_ext = String::new();
    let mut cursor = String::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(key);
        let value = percent_decode(value);
        match key.as_str() {
            // Carried separately in the protobuf, so clients read them from their own fields.
            "cursor" => cursor = value,
            "internal_ext" => internal_ext = value,
            _ => route_params.push((key, value)),
        }
    }

    if route_params.is_empty() {
        return None;
    }
    if cursor.is_empty() {
        cursor = SYNTHETIC_CURSOR.to_string();
    }

    Some(FetchResult {
        push_server: push_server.to_string(),
        route_params,
        cursor,
        internal_ext,
        // The player's own heartbeat cadence; clients that manage their own use this only as
        // a hint, and `ttl-live-ws` ignores it in favour of `ConnectConfig`.
        heartbeat_duration: 0,
        need_ack: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape captured from a real player URI on 2026-08-10, shortened.
    const PAGE_URI: &str = "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/\
?version_code=180800&device_platform=web&browser_version=5.0 (X11; Linux x86_64)\
&compress=gzip&room_id=7672457163312155399&X-Gnarly=abc%2Fdef%2Bghi%3D";

    #[test]
    fn builds_a_fetch_result_from_a_player_uri() {
        let result = fetch_result_from_ws_uri(PAGE_URI).expect("a signed URI");

        assert_eq!(
            result.push_server,
            "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/"
        );
        assert_eq!(result.rejection_reason(), None, "this is not a rejection");

        let param = |name: &str| {
            result
                .route_params
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(param("room_id"), Some("7672457163312155399"));
        assert_eq!(param("compress"), Some("gzip"));
        // Decoded, so a client that re-encodes reproduces the original rather than `%252F`.
        assert_eq!(param("X-Gnarly"), Some("abc/def+ghi="));
        assert_eq!(param("browser_version"), Some("5.0 (X11; Linux x86_64)"));
    }

    /// The player's URI has no cursor, and clients reject an empty one as malformed.
    #[test]
    fn supplies_a_cursor_when_the_uri_has_none() {
        let result = fetch_result_from_ws_uri(PAGE_URI).unwrap();
        assert_eq!(result.cursor, SYNTHETIC_CURSOR);
        assert!(result.internal_ext.is_empty());
        assert!(
            !result.route_params.iter().any(|(key, _)| key == "cursor"),
            "cursor belongs to its own protobuf field, not to route_params"
        );
    }

    #[test]
    fn keeps_a_real_cursor_and_internal_ext_when_present() {
        let uri = "wss://x.test/ws/?room_id=1&cursor=17_9&internal_ext=src%3Adim";
        let result = fetch_result_from_ws_uri(uri).unwrap();
        assert_eq!(result.cursor, "17_9");
        assert_eq!(result.internal_ext, "src:dim");
        assert_eq!(result.route_params, vec![("room_id".into(), "1".into())]);
    }

    #[test]
    fn a_uri_without_a_query_is_not_a_transport() {
        assert!(fetch_result_from_ws_uri("wss://x.test/ws/").is_none());
        assert!(fetch_result_from_ws_uri("wss://x.test/ws/?").is_none());
        assert!(fetch_result_from_ws_uri("not a uri").is_none());
    }

    #[test]
    fn round_trips_through_the_protobuf_encoder() {
        let result = fetch_result_from_ws_uri(PAGE_URI).unwrap();
        let decoded = FetchResult::decode(&result.encode()).expect("valid protobuf");
        assert_eq!(decoded, result);
    }

    #[test]
    fn sanitizing_escapes_only_what_an_http_client_rejects() {
        let sanitized = sanitize_uri(PAGE_URI);
        assert!(!sanitized.contains(' '), "spaces break `http::Uri`");
        // Existing escapes are not encoded a second time.
        assert!(sanitized.contains("X-Gnarly=abc%2Fdef%2Bghi%3D"));
        assert!(sanitized.contains("5.0%20(X11;%20Linux%20x86_64)"));
        // Idempotent, and a clean URI is returned unchanged.
        assert_eq!(sanitize_uri(&sanitized), sanitized);
        let clean = "wss://x.test/ws/?a=1&b=2";
        assert_eq!(sanitize_uri(clean), clean);
    }
}
