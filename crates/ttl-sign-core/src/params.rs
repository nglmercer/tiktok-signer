//! Query-string construction.
//!
//! Two consumers:
//!
//! - [`FetchParams`] → query for `https://webcast.tiktok.com/webcast/im/fetch/`; it is the
//!   only signed part.
//! - [`WsParams`] → WebSocket query; it is **not** signed because `route_params` are already
//!   signed inside the protobuf.
//!
//! A `Vec<(String, String)>` is intentional instead of a `HashMap`: ordering is stable and
//! the WebSocket query must repeat `version_code` with two different values.

use rand::Rng;

use crate::preset::Preset;

const DEVICE_ID_DIGITS: usize = 19;
const DEVICE_ID_FIRST_DIGIT_MIN: u32 = 1;
const DEVICE_ID_FIRST_DIGIT_MAX: u32 = 9;
const DEVICE_ID_DIGIT_MIN: u32 = 0;
const DEVICE_ID_DIGIT_MAX: u32 = 9;
const LAST_RTT_MIN: u32 = 100;
const LAST_RTT_MAX: u32 = 200;
const APP_ID: &str = "1988";
const LIVE_ID: &str = "12";
const FETCH_RULE: &str = "1";
/// `version_code` for the transport request.
///
/// `180800`, which is a constant in the player's own transport chunk
/// (`static/js/async/9894.*.js`), not the `270000` the rest of the web app sends. Read directly out
/// of that chunk on 2026-08-18.
const FETCH_VERSION_CODE: &str = "180800";
const WS_VERSION_CODE: &str = "180800";
/// The `version_code` a client appends after the signed query, which is not the one in it.
///
/// Distinct from [`FETCH_VERSION_CODE`]: this duplicate is the connector's own value and stays
/// `270000` regardless of what the transport request carries.
const TRAILING_VERSION_CODE: &str = "270000";
const HEARTBEAT_DURATION_MS: &str = "10000";
/// The IM SDK's own version, sent alongside the two `version_code` values.
const UPDATE_VERSION_CODE: &str = "2.0.0";

/// Query string under construction: ordered pairs with duplicates allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query(Vec<(String, String)>);

impl Query {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Add a pair. If the key exists, **overwrite** its value in place so client parameters
    /// override route parameters without reordering.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        let key = key.into();
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
        self
    }

    /// Add a pair **without** deduplication. Required for the duplicated `version_code`.
    pub fn push_raw(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.0.push((key.into(), value.into()));
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Serialize as `k=v&k=v`, percent-encoding values.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.0 {
            if !out.is_empty() {
                out.push('&');
            }
            out.push_str(&percent_encode(k));
            out.push('=');
            out.push_str(&percent_encode(v));
        }
        out
    }

    /// Serialize as `k=v&k=v` with **no** encoding at all.
    ///
    /// The direct-socket query is signed over its own bytes, and the player's serializer does not
    /// percent-encode: `browser_version` keeps its spaces and `tz_name` its slash. Encoding here
    /// would sign different bytes than the ones sent.
    pub fn encode_raw(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.0 {
            if !out.is_empty() {
                out.push('&');
            }
            out.push_str(k);
            out.push('=');
            out.push_str(v);
        }
        out
    }

    /// Parse `k=v&k=v`. No decoding is performed: this is used on normalized test queries.
    pub fn parse(raw: &str) -> Self {
        let mut q = Query::new();
        for pair in raw.trim_start_matches('?').split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            q.push_raw(k, v);
        }
        q
    }

    /// Parse an encoded query into semantic key/value pairs.
    ///
    /// Unlike [`Query::parse`], this decodes each component. Use it when a subsequent
    /// [`Query::encode`] must reproduce `%2F` rather than double-encoding it as `%252F`.
    pub fn parse_encoded(raw: &str) -> Self {
        let mut query = Query::new();
        for pair in raw.trim_start_matches('?').split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            query.push_raw(percent_decode(key), percent_decode(value));
        }
        query
    }
}

/// Percent-encoding for query strings: preserve unreserved characters plus `.`, `-`, `_`,
/// and `~`, and encode everything else. This matches `encodeURIComponent`.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode `%XX` escapes.
///
/// `+` is deliberately left alone: these queries carry signatures whose bytes must survive
/// verbatim, and TikTok percent-encodes a real space rather than using `+`.
pub fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    // Not a valid escape: keep the `%` as an ordinary character.
                    Err(_) => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_string())
}

/// Generate a 19-digit `device_id`, matching the real browser.
pub fn random_device_id() -> String {
    let mut rng = rand::thread_rng();
    let mut s = String::with_capacity(DEVICE_ID_DIGITS);
    s.push(
        char::from_digit(
            rng.gen_range(DEVICE_ID_FIRST_DIGIT_MIN..=DEVICE_ID_FIRST_DIGIT_MAX),
            10,
        )
        .expect("valid digit"),
    );
    for _ in 1..DEVICE_ID_DIGITS {
        s.push(
            char::from_digit(rng.gen_range(DEVICE_ID_DIGIT_MIN..=DEVICE_ID_DIGIT_MAX), 10)
                .expect("valid digit"),
        );
    }
    s
}

/// Keep `last_rtt` plausible, within the range used by the reference client.
pub fn random_last_rtt() -> u32 {
    rand::thread_rng().gen_range(LAST_RTT_MIN..=LAST_RTT_MAX)
}

/// Query for `/webcast/im/fetch/`.
///
/// `X-Bogus` / `X-Gnarly` / `msToken` are **not** added here; `webmssdk.js` adds them.
/// when intercepting `fetch` (`docs/01-architecture.md` §D2).
#[derive(Debug, Clone)]
pub struct FetchParams {
    pub room_id: String,
    pub device_id: String,
    pub cursor: String,
    pub internal_ext: String,
    /// Contact email required by the Euler spec. Empty means it is omitted.
    pub contact_us: String,
    /// `sup_ws_ds_opt`: tells TikTok which WebSocket type we want.
    ///
    /// With `1`, the response includes a `push_server` of the type
    /// `ws_proxy/ws_reuse_supplement/`; the pattern documented by
    /// `docs/00-research.md` §1 describes `webcast<N>-ws-web-<idc>`. Configurable so both
    /// so both variants can be compared against a real room.
    pub sup_ws_ds_opt: u8,
}

impl FetchParams {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            device_id: random_device_id(),
            // The player's *initial* fetch sends `cursor=0` and `internal_ext=0`, not empty ones —
            // and its query builder deletes empty values outright, so an empty `cursor` is a request
            // it can never produce. Read from the transport chunk on 2026-08-18.
            cursor: "0".into(),
            internal_ext: "0".into(),
            contact_us: String::new(),
            sup_ws_ds_opt: 1,
        }
    }

    /// Build the complete query from the preset.
    pub fn build(&self, preset: &Preset) -> Query {
        let d = &preset.device;
        let l = &preset.location;
        let s = &preset.screen;

        let mut q = Query::new();
        q.set("aid", APP_ID)
            .set("app_language", &l.language)
            .set("app_name", "tiktok_web")
            .set("browser_language", &l.browser_language)
            .set("browser_name", &d.browser_name)
            .set("browser_online", "true")
            .set("browser_platform", &d.browser_platform)
            .set("browser_version", &d.browser_version)
            .set("cookie_enabled", "true")
            .set("cursor", &self.cursor)
            .set("debug", "false")
            .set("device_id", &self.device_id)
            .set("device_platform", "web")
            .set("did_rule", "3")
            .set("fetch_rule", FETCH_RULE)
            .set("history_comment_count", "6")
            .set("identity", "audience")
            .set("internal_ext", &self.internal_ext)
            // `-1` on the first fetch, the value the chunk passes when it has no round trip to
            // report yet.
            .set("last_rtt", "-1")
            .set("live_id", LIVE_ID)
            .set("os", &d.os)
            .set("priority_region", &l.region)
            .set("region", &l.region)
            .set("resp_content_type", "protobuf")
            .set("room_id", &self.room_id)
            .set("screen_height", s.height.to_string())
            .set("screen_width", s.width.to_string())
            .set("sup_ws_ds_opt", self.sup_ws_ds_opt.to_string())
            .set("tz_name", &l.tz_name)
            .set("version_code", FETCH_VERSION_CODE)
            .set("webcast_language", &l.language);

        if !self.contact_us.is_empty() {
            q.set("contact_us", &self.contact_us);
        }
        q
    }

    /// Absolute URL ready to be signed.
    pub fn url(&self, preset: &Preset) -> String {
        format!("{}?{}", FETCH_ENDPOINT, self.build(preset).encode())
    }
}

/// Endpoint to sign. The only endpoint on the critical path.
pub const FETCH_ENDPOINT: &str = "https://webcast.tiktok.com/webcast/im/fetch/";

/// WebSocket query: signed `route_params` + our parameters + the trailing `version_code`
/// duplicated at the end (`docs/05-spec-websocket-client.md`).
#[derive(Debug, Clone)]
pub struct WsParams {
    pub room_id: String,
    /// `gzip`, or empty to request no compression.
    pub compress: String,
    pub last_rtt: u32,
    /// Cursor from the `/webcast/im/fetch/` response.
    ///
    /// It belongs in the WebSocket query, not in `route_params`: verified against a
    /// real response, where `route_params` only contains `wrss` and `imprp`. Without the cursor,
    /// handshake is accepted but **no frames arrive**, the hardest failure in the flow to
    /// diagnose.
    pub cursor: String,
    /// `internal_ext` from the same response follows the same reasoning.
    pub internal_ext: String,
}

impl WsParams {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            compress: "gzip".into(),
            // The current web player sends 0 on its first WebSocket request. A random
            // RTT is accepted by some older clients but makes the synthetic URI diverge
            // from the page URL we otherwise reproduce exactly.
            last_rtt: 0,
            cursor: String::new(),
            internal_ext: String::new(),
        }
    }

    /// Client-owned parameters, without `route_params`.
    pub fn client_params(&self, preset: &Preset) -> Query {
        let d = &preset.device;
        let l = &preset.location;
        let s = &preset.screen;

        let mut q = Query::new();
        q.set("aid", APP_ID)
            .set("app_language", &l.language)
            .set("app_name", "tiktok_web")
            .set("browser_language", &l.browser_language)
            .set("browser_name", &d.browser_name)
            .set("browser_online", "true")
            .set("browser_platform", &d.browser_platform)
            .set("browser_version", &d.browser_version)
            .set("client_enter", "1")
            .set("compress", &self.compress)
            .set("cookie_enabled", "true")
            .set("cursor", &self.cursor)
            .set("device_platform", "web")
            .set("did_rule", "3")
            .set("heartbeat_duration", HEARTBEAT_DURATION_MS)
            .set("internal_ext", &self.internal_ext)
            .set("history_comment_count", "6")
            .set("identity", "audience")
            .set("last_rtt", self.last_rtt.to_string())
            .set("live_id", LIVE_ID)
            .set("resp_content_type", "protobuf")
            .set("room_id", &self.room_id)
            .set("screen_height", s.height.to_string())
            .set("screen_width", s.width.to_string())
            .set("sup_ws_ds_opt", "1")
            .set("tz_name", &l.tz_name)
            .set("update_version_code", "2.0.0")
            .set("version_code", WS_VERSION_CODE)
            .set("webcast_language", &l.language)
            .set("ws_direct", "1");
        q
    }

    /// Complete WebSocket URI.
    ///
    /// Three rules must be preserved exactly (`docs/05` URI construction):
    ///
    /// 1. Empty `route_params` values are discarded.
    /// 2. Client parameters are applied afterward and win collisions.
    /// 3. `&version_code=270000` is appended at the end **even when**
    ///    `version_code=180800` is already present. The duplicate is intentional.
    pub fn build_uri(
        &self,
        push_server: &str,
        route_params: &[(String, String)],
        preset: &Preset,
    ) -> String {
        let mut q = Query::new();
        for (k, v) in route_params {
            if v.is_empty() {
                continue;
            }
            q.set(k, v);
        }
        for (k, v) in self.client_params(preset).iter() {
            q.set(k, v);
        }
        q.push_raw("version_code", TRAILING_VERSION_CODE);

        let sep = if push_server.contains('?') { '&' } else { '?' };
        format!("{push_server}{sep}{}", q.encode())
    }
}

/// Query for the socket the current web player opens directly, with no `im/fetch` in front of it.
///
/// The live room page configures its IM SDK with `wsDirect: "1"` and a `socketHost`, and the SDK
/// then builds the socket URL itself instead of waiting for a `push_server`:
///
/// ```text
/// wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?<query>&X-Gnarly=<sig>
/// ```
///
/// The query below reproduces the SDK's serializer parameter for parameter, in its order, because
/// `registerWsSigner` signs the query string verbatim. Two properties of it look like mistakes and
/// are not:
///
/// - **Nothing is percent-encoded.** `browser_version` carries raw spaces and parentheses, and
///   `tz_name` a raw slash. [`Query::encode_raw`] preserves them; the URI is repaired for
///   `http::Uri` afterwards by `sanitize_uri`, which does not disturb the signed bytes.
/// - **`version_code` appears twice**, `180800` then `270000`. The SDK's browser block supplies the
///   first under a snake_case key and the page config the second under a camelCase one, so its
///   serializer emits both.
///
/// Read out of `static/js/async/9894.*.js` on 2026-08-18, and verified against a live room:
/// the socket opens and pushes frames.
#[derive(Debug, Clone)]
pub struct DirectSocketParams {
    pub room_id: String,
    pub device_id: String,
    /// `gzip`, or empty for uncompressed frames.
    pub compress: String,
    /// `audience`, unless connecting as the broadcaster.
    pub identity: String,
    /// Milliseconds between application heartbeats; echoed to the server in the query.
    pub heartbeat_duration: String,
}

impl DirectSocketParams {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            room_id: room_id.into(),
            device_id: random_device_id(),
            compress: "gzip".into(),
            identity: "audience".into(),
            heartbeat_duration: HEARTBEAT_DURATION_MS.into(),
        }
    }

    /// The query, in the SDK's order. Serialize it with [`Query::encode_raw`].
    pub fn build(&self, preset: &Preset) -> Query {
        let d = &preset.device;
        let l = &preset.location;
        let s = &preset.screen;

        let mut q = Query::new();
        // The SDK's browser block comes first, carrying its own `version_code`.
        q.push_raw("version_code", WS_VERSION_CODE);
        q.push_raw("device_platform", "web");
        q.push_raw("cookie_enabled", "true");
        q.push_raw("screen_width", s.width.to_string());
        q.push_raw("screen_height", s.height.to_string());
        q.push_raw("browser_language", &l.browser_language);
        q.push_raw("browser_platform", &d.browser_platform);
        q.push_raw("browser_name", &d.browser_name);
        q.push_raw("browser_version", &d.browser_version);
        q.push_raw("browser_online", "true");
        q.push_raw("tz_name", &l.tz_name);
        // Then the SDK's own fixed fields, then the page's config.
        q.push_raw("app_name", "tiktok_web");
        q.push_raw("sup_ws_ds_opt", "1");
        q.push_raw("update_version_code", UPDATE_VERSION_CODE);
        if !self.compress.is_empty() {
            q.push_raw("compress", &self.compress);
        }
        q.push_raw("webcast_language", &l.language);
        q.push_raw("aid", APP_ID);
        q.push_raw("live_id", LIVE_ID);
        q.push_raw("version_code", TRAILING_VERSION_CODE);
        q.push_raw("app_language", &l.language);
        q.push_raw("ws_direct", "1");
        q.push_raw("client_enter", "1");
        q.push_raw("room_id", &self.room_id);
        q.push_raw("identity", &self.identity);
        // `-1` because no round trip has been measured yet; the SDK sends the same on a cold start.
        q.push_raw("last_rtt", "-1");
        q.push_raw("heartbeat_duration", &self.heartbeat_duration);
        // The tail the SDK appends after the config, in this order.
        q.push_raw("resp_content_type", "protobuf");
        // `did_rule` is 3 only when there is no device id; with one it is 0.
        q.push_raw("did_rule", if self.device_id.is_empty() { "3" } else { "0" });
        q.push_raw("device_id", &self.device_id);
        q
    }

    /// The unsigned socket URL. Append the signature as `&X-Gnarly=<percent-encoded>`.
    pub fn url(&self, preset: &Preset) -> String {
        format!("{DIRECT_SOCKET_HOST}{WS_REUSE_PATH}?{}", self.build(preset).encode_raw())
    }
}

/// Default socket host. The player picks `webcast-ws.us` / `.eu` by cluster region; this is the
/// value it uses everywhere else.
pub const DIRECT_SOCKET_HOST: &str = "wss://webcast-ws.tiktok.com";
/// The path the SDK opens when `wsDirect` is on.
pub const WS_REUSE_PATH: &str = "/webcast/im/ws_proxy/ws_reuse_supplement/";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{DevicePreset, LocationPreset, Preset, ScreenPreset};

    fn preset() -> Preset {
        Preset::new(
            DevicePreset::chrome_windows(),
            LocationPreset::us_east(),
            ScreenPreset::FHD,
        )
    }

    #[test]
    fn fetch_query_carries_the_euler_documented_params() {
        let p = preset();
        let q = FetchParams::new("7300000000000000000").build(&p);

        for key in [
            "aid",
            "app_name",
            "browser_name",
            "browser_version",
            "device_id",
            "device_platform",
            "identity",
            "resp_content_type",
            "room_id",
            "sup_ws_ds_opt",
            "version_code",
        ] {
            assert!(q.get(key).is_some(), "missing parameter {key}");
        }
        assert_eq!(q.get("aid"), Some("1988"));
        assert_eq!(q.get("resp_content_type"), Some("protobuf"));
        assert_eq!(q.get("room_id"), Some("7300000000000000000"));
    }

    /// UA/query consistency: the number-one cause of rejection.
    #[test]
    fn fetch_query_browser_params_match_the_user_agent() {
        for p in Preset::all() {
            let q = FetchParams::new("1").build(&p);
            let ua = p.user_agent();
            assert_eq!(
                ua,
                format!(
                    "{}/{}",
                    q.get("browser_name").unwrap(),
                    q.get("browser_version").unwrap()
                )
            );
            assert_eq!(
                q.get("browser_platform"),
                Some(p.device.browser_platform.as_str())
            );
            assert_eq!(q.get("os"), Some(p.device.os.as_str()));
        }
    }

    #[test]
    fn device_id_is_19_digits() {
        for _ in 0..100 {
            let id = random_device_id();
            assert_eq!(id.len(), 19, "device_id has the wrong length: {id}");
            assert!(id.chars().all(|c| c.is_ascii_digit()));
            assert_ne!(id.as_bytes()[0], b'0', "must not start with zero: {id}");
        }
    }

    /// `cursor` and `internal_ext` travel in the WebSocket query, not in `route_params`.
    #[test]
    fn ws_uri_carries_cursor_and_internal_ext() {
        let mut params = WsParams::new("42");
        params.cursor = "1786_7672_1_1".into();
        params.internal_ext = "internal_src:dim|wss_info:0-1".into();
        let uri = params.build_uri("wss://x/ws/", &[("wrss".into(), "abc".into())], &preset());
        assert!(uri.contains("cursor=1786_7672_1_1"), "{uri}");
        assert!(uri.contains("internal_ext=internal_src"), "{uri}");
        assert!(uri.contains("wrss=abc"), "{uri}");
        assert!(uri.contains("last_rtt=0"), "{uri}");
    }

    #[test]
    fn ws_uri_keeps_version_code_twice() {
        let uri = WsParams::new("42").build_uri(
            "wss://webcast5-ws-web-useast1a.tiktok.com/webcast/im/ws/",
            &[("wss_push_room_id".into(), "42".into())],
            &preset(),
        );
        // Include the leading `&`; otherwise `update_version_code=` is counted too.
        let occurrences = uri.matches("&version_code=").count();
        assert_eq!(occurrences, 2, "version_code must appear twice: {uri}");
        assert!(uri.ends_with("&version_code=270000"), "{uri}");
        assert!(uri.contains("version_code=180800"), "{uri}");
    }

    #[test]
    fn ws_uri_drops_empty_route_params() {
        let uri = WsParams::new("42").build_uri(
            "wss://example.tiktok.com/webcast/im/ws/",
            &[
                ("keep".into(), "yes".into()),
                ("drop".into(), String::new()),
            ],
            &preset(),
        );
        assert!(uri.contains("keep=yes"), "{uri}");
        assert!(!uri.contains("drop="), "{uri}");
    }

    #[test]
    fn client_params_win_over_route_params() {
        let uri = WsParams::new("42").build_uri(
            "wss://example.tiktok.com/webcast/im/ws/",
            &[("identity".into(), "anchor".into())],
            &preset(),
        );
        assert!(uri.contains("identity=audience"), "{uri}");
        assert!(!uri.contains("identity=anchor"), "{uri}");
    }

    #[test]
    fn query_roundtrips_through_parse() {
        let q = Query::parse("a=1&b=2&a=3");
        assert_eq!(q.len(), 3, "parse must not deduplicate");
        assert_eq!(q.encode(), "a=1&b=2&a=3");
    }

    #[test]
    fn encoding_escapes_reserved_characters() {
        let mut q = Query::new();
        q.set("tz_name", "America/New_York");
        assert_eq!(q.encode(), "tz_name=America%2FNew_York");
    }

    #[test]
    fn encoded_parse_avoids_double_encoding() {
        let query = Query::parse_encoded("X-Gnarly=abc%2Fdef%2Bghi%3D&empty=");
        assert_eq!(query.get("X-Gnarly"), Some("abc/def+ghi="));
        assert_eq!(query.encode(), "X-Gnarly=abc%2Fdef%2Bghi%3D&empty=");
    }
    /// The bytes the signature covers, compared against what the real SDK emits.
    ///
    /// `TTL_PRINT_QUERY=1 node scripts/headless/ws-direct.mjs <bundle> <room>` prints exactly this
    /// string for the same preset, room, and device id. If this test starts failing, the player's
    /// serializer changed and the signature will be computed over bytes the server did not receive.
    #[test]
    fn direct_socket_query_matches_the_player() {
        let mut params = DirectSocketParams::new("7675481361573382932");
        params.device_id = "7300000000000000001".into();
        // The JavaScript probe reports a Chrome-on-Linux browser block; compare like for like.
        let preset = Preset::new(
            crate::DevicePreset::chrome_linux(),
            crate::LocationPreset::us_east(),
            crate::ScreenPreset::FHD,
        );

        let expected = concat!(
            "version_code=180800&device_platform=web&cookie_enabled=true",
            "&screen_width=1920&screen_height=1080&browser_language=en-US",
            "&browser_platform=Linux x86_64&browser_name=Mozilla",
            "&browser_version=5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) ",
            "Chrome/131.0.0.0 Safari/537.36",
            "&browser_online=true&tz_name=America/New_York",
            "&app_name=tiktok_web&sup_ws_ds_opt=1&update_version_code=2.0.0&compress=gzip",
            "&webcast_language=en&aid=1988&live_id=12&version_code=270000&app_language=en",
            "&ws_direct=1&client_enter=1&room_id=7675481361573382932&identity=audience",
            "&last_rtt=-1&heartbeat_duration=10000&resp_content_type=protobuf&did_rule=0",
            "&device_id=7300000000000000001",
        );
        assert_eq!(params.build(&preset).encode_raw(), expected);
    }

    #[test]
    fn direct_socket_url_is_the_reuse_supplement_path() {
        let params = DirectSocketParams::new("7000000000000000000");
        let url = params.url(&preset());
        assert!(url.starts_with("wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?"));
        // Unencoded, because the signature covers these bytes.
        assert!(url.contains("tz_name=America/New_York"));
    }

    /// The builder is checked against the player's shipped code, not against itself.
    ///
    /// `scripts/headless/player-audit.mjs` reads these facts back out of the live app's JavaScript
    /// and writes them to the fixture; this asserts the builder still agrees with them. Together
    /// they cover the whole chain — player's chunk → fixture → `DirectSocketParams` → the wire —
    /// so a TikTok deploy that moves any of it fails here instead of producing a socket that opens
    /// and never speaks.
    ///
    /// Offline: the fixture is committed, and only the audit touches the network.
    #[test]
    fn direct_socket_builder_agrees_with_the_audited_player() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/research/player-transport-v1.json"
        ))
        .expect("player-transport fixture; regenerate with scripts/headless/player-audit.mjs");
        let facts: serde_json::Value =
            serde_json::from_str(&raw).expect("player-transport fixture is JSON");
        let facts = &facts["facts"];

        // The player still builds the socket itself. If this flips, the direct path is gone and no
        // amount of query correctness will help.
        assert_eq!(facts["ws_direct"], serde_json::json!(true));
        assert_eq!(facts["ws_signer"], serde_json::json!("registerWsSigner"));
        assert_eq!(facts["ws_sign_output"], serde_json::json!("X-Gnarly"));
        // Our query is signed as raw bytes. An encoding serializer invalidates every signature.
        assert_eq!(
            facts["serializer_percent_encodes"],
            serde_json::json!(false),
            "the player now encodes its query; Query::encode_raw would sign the wrong bytes"
        );

        assert_eq!(facts["direct_socket_path"], serde_json::json!(WS_REUSE_PATH));
        assert_eq!(facts["sdk_version_code"], serde_json::json!(WS_VERSION_CODE));
        assert_eq!(
            facts["config_version_code"],
            serde_json::json!(TRAILING_VERSION_CODE)
        );
        assert_eq!(
            facts["update_version_code"],
            serde_json::json!(UPDATE_VERSION_CODE)
        );
        let hosts = facts["socket_hosts"].as_array().expect("socket_hosts");
        assert!(
            hosts.iter().any(|h| h == DIRECT_SOCKET_HOST),
            "the default socket host is no longer one the player would pick: {hosts:?}"
        );

        // The browser block leads the query, in its order and under its spellings.
        let expected: Vec<&str> = facts["browser_block_keys"]
            .as_array()
            .expect("browser_block_keys")
            .iter()
            .map(|key| key.as_str().expect("key"))
            .collect();
        let params = DirectSocketParams::new("7000000000000000000");
        let query = params.build(&preset());
        let ours: Vec<&str> = query.iter().take(expected.len()).map(|(k, _)| k).collect();
        assert_eq!(ours, expected, "the player's browser block changed shape");
    }

}
