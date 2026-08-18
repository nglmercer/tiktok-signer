//! Making a signed WebSocket URI acceptable to a Rust HTTP client.
//!
//! The transport's socket URI is rebuilt from `push_server` and `route_params` — the fields
//! `/webcast/im/fetch/` answers with — and those values are what a browser put on the wire, not
//! what `http::Uri` accepts. `browser_version=5.0 (X11; Linux x86_64) …` carries raw spaces, which
//! fail `into_client_request` with "invalid uri character" before a single byte is sent.
//!
//! Escaping them does not invalidate the signature: a sanitized URI was accepted and delivered
//! frames.

/// Characters `http::Uri` accepts.
///
/// A browser is more permissive than any Rust HTTP client.
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
/// Idempotent, and safe on a URI that needed nothing.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape a rebuilt socket URI has: raw spaces from `browser_version`, and values that
    /// already carry their own `%XX` escapes.
    const SOCKET_URI: &str = "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/\
?version_code=180800&device_platform=web&browser_version=5.0 (X11; Linux x86_64)\
&compress=gzip&room_id=7672457163312155399&X-Gnarly=abc%2Fdef%2Bghi%3D";

    #[test]
    fn sanitizing_escapes_only_what_an_http_client_rejects() {
        let sanitized = sanitize_uri(SOCKET_URI);
        assert!(!sanitized.contains(' '), "spaces break `http::Uri`");
        // Existing escapes are not encoded a second time.
        assert!(sanitized.contains("X-Gnarly=abc%2Fdef%2Bghi%3D"));
        assert!(sanitized.contains("5.0%20(X11;%20Linux%20x86_64)"));
        // Idempotent, and a clean URI is returned unchanged.
        assert_eq!(sanitize_uri(&sanitized), sanitized);
        let clean = "wss://x.test/ws/?a=1&b=2";
        assert_eq!(sanitize_uri(clean), clean);
    }

    #[test]
    fn non_ascii_is_encoded_per_utf8_byte() {
        assert_eq!(sanitize_uri("wss://x.test/ws/?t=é"), "wss://x.test/ws/?t=%C3%A9");
    }
}
