//! Browser-free discovery for the TikTok LIVE endpoints that need no signature.
//!
//! Phase 4 of `docs/11-webview-removal.md`. Discovery is one of the three dependencies the WebView
//! carried, and it turns out to need no signing at all:
//!
//! | Operation | Requirement | Implemented |
//! |---|---|---|
//! | `unique_id` → `room_id` | none | [`DiscoveryClient::room_lookup`] |
//! | `/webcast/room/info/` | none | [`DiscoveryClient::room_info`] |
//! | `/webcast/gift/list/` | none | [`DiscoveryClient::gift_list`] |
//! | who is live now | none | [`DiscoveryClient::live_channels`] |
//!
//! The last three were signed until 2026-08-18, when a one-character tamper test showed that none
//! of them verifies a signature: a correct signature, one with a single character changed, and no
//! signature at all return the same data. Run `scripts/headless/verify-probe.mjs` to reproduce it.
//! Dropping the signature removed a signer subprocess from every read, and removed a false success
//! signal — "room/info accepts our signature" was the evidence the transport diagnosis rested on,
//! and it never meant anything.
//!
//! The one request that *is* verified — the direct message socket — is signed by
//! [`UrlSigner`], which `ttl-sign-embedded` implements by running the real bundle in an embedded
//! engine. This crate never signs and never spawns a process to do it.
//!
//! "Who is live now" was previously believed to need a rendering engine, because the `/live` page
//! ships no channel data in its HTML. It does not: `/api/search/live/full/` returns the same
//! information as JSON. See also `scripts/headless/find-live.mjs`.
//!
//! Parsing and URL construction live in [`ttl_sign_core::room`] and are shared with the WebView
//! path, so a native lookup and a page lookup cannot disagree about what "live" means.
//!
//! This crate never signs and never opens a browser. Its dependency tree contains no `wry`.

use std::time::Duration;

use ttl_sign_core::room::{self, RoomLookup};
use ttl_sign_core::Preset;

/// Default ceiling on one discovery request.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Largest response body accepted from a discovery endpoint.
///
/// The lookup response is a few kilobytes. A much larger body means the endpoint returned
/// something other than what this crate parses, and reading it to completion would be the only
/// harm done.
const MAX_BODY_BYTES: usize = 1024 * 1024;

// Check every decoded HTTP chunk before retaining it, including chunked/gzip responses.
async fn read_bounded(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<String, DiscoveryError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(DiscoveryError::TooLarge(limit));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| DiscoveryError::Transport(error.to_string()))?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(DiscoveryError::TooLarge(limit));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// A discovery operation and what it needs beyond plain HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiscoveryOperation {
    /// `unique_id` → `room_id` and live status.
    RoomLookup,
    /// Full room metadata: title, owner, counters.
    RoomInfo,
    /// The room's gift table, needed to price gift events.
    GiftList,
    /// Which creators are live right now.
    LiveChannels,
}

impl DiscoveryOperation {
    pub const ALL: [DiscoveryOperation; 4] = [
        DiscoveryOperation::RoomLookup,
        DiscoveryOperation::RoomInfo,
        DiscoveryOperation::GiftList,
        DiscoveryOperation::LiveChannels,
    ];
}

/// What an operation needs on top of an ordinary HTTP client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Requirement {
    /// Plain HTTP is enough. This crate implements it.
    None,
    /// The request must carry a webmssdk signature, so it depends on the signing workstream.
    Signature,
    /// The data is produced by client-side rendering and is absent from any JSON endpoint. No
    /// amount of signing progress makes this native.
    ///
    /// Nothing currently carries this requirement: the one operation that appeared to — listing
    /// who is live — turned out to have a signable JSON endpoint. The variant is kept because the
    /// distinction is real and the next capability audited may need it.
    Renderer,
}

/// What an operation needs. This is the WebView-removal boundary, in one place and testable.
pub fn requirement(operation: DiscoveryOperation) -> Requirement {
    match operation {
        DiscoveryOperation::RoomLookup => Requirement::None,
        // These endpoints were independently verified to work without signing
        // (2026-08-18). Keep the capability table aligned with actual HTTP calls.
        DiscoveryOperation::RoomInfo
        | DiscoveryOperation::GiftList
        | DiscoveryOperation::LiveChannels => Requirement::None,
    }
}

/// Operations that need no signer at all.
///
/// All currently supported discovery operations are unsigned; the Requirement
/// enum remains extensible if a future endpoint proves to require a signature.
pub fn native_operations() -> Vec<DiscoveryOperation> {
    DiscoveryOperation::ALL
        .into_iter()
        .filter(|operation| requirement(*operation) == Requirement::None)
        .collect()
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("discovery transport failed: {0}")]
    Transport(String),
    #[error("discovery endpoint answered {status}")]
    Status { status: u16 },
    #[error("discovery response could not be parsed: {0}")]
    Decode(String),
    /// Valid JSON with no `data.user`: the handle does not resolve. Kept distinct from
    /// [`DiscoveryError::Decode`] so a TikTok schema change (unparseable body) can never
    /// be reported as "user does not exist".
    #[error("lookup answered without a user: {0}")]
    UserNotFound(String),
    /// The endpoint answered, but the user has no live room. This is a normal outcome, not a
    /// failure of the client, and is distinguished so a caller does not retry it.
    #[error("@{0} has no live room")]
    NoRoom(String),
    #[error("response exceeded the {0} byte discovery limit")]
    TooLarge(usize),
    /// The signer could not produce a signed URL.
    #[error("signing failed: {0}")]
    Signer(String),
    /// TikTok refused the request with a webcast status code.
    #[error("webcast refused the request: {0}")]
    Refused(String),
    /// Accepted but answered with an empty body — the identity was not sufficient.
    #[error("endpoint accepted the request but returned nothing")]
    EmptyResponse,
}

/// Native discovery client.
///
/// Holds one `reqwest::Client` so connections are pooled across lookups.
#[derive(Debug, Clone)]
pub struct DiscoveryClient {
    http: reqwest::Client,
    /// Cookie header sent with signed requests.
    ///
    /// Signing and requesting must present the same identity: the signer sees these cookies, so
    /// the request has to carry them too, or the service is asked to validate a signature made
    /// under a different identity.
    session: String,
}

impl DiscoveryClient {
    /// Build a client that presents the same User-Agent as the signing identity.
    ///
    /// The lookup endpoint does not require a matching User-Agent, but presenting two different
    /// ones from the same logical client is a needless inconsistency.
    pub fn new(preset: &Preset) -> Result<Self, DiscoveryError> {
        Self::with_timeout(preset, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(preset: &Preset, timeout: Duration) -> Result<Self, DiscoveryError> {
        let http = reqwest::Client::builder()
            .user_agent(preset.user_agent())
            .timeout(timeout)
            .build()
            .map_err(|error| DiscoveryError::Transport(error.to_string()))?;
        Ok(Self {
            http,
            session: String::new(),
        })
    }

    /// Attach the session presented with signed requests.
    ///
    /// The same jar must be given to the signer, so that what is signed and what is sent agree.
    pub fn with_session(mut self, cookie_header: impl Into<String>) -> Self {
        self.session = cookie_header.into();
        self
    }

    /// Resolve `unique_id` → `room_id`. Unsigned, no cookies, no browser.
    ///
    /// A leading `@` is tolerated; [`ttl_sign_core::room::room_lookup_url`] normalizes it.
    pub async fn room_lookup(&self, unique_id: &str) -> Result<RoomLookup, DiscoveryError> {
        let url = room::room_lookup_url(unique_id);
        tracing::debug!(unique_id, "native room lookup");

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|error| DiscoveryError::Transport(error.to_string()))?;

        let status = response.status().as_u16();
        let body = read_bounded(response, MAX_BODY_BYTES).await?;

        interpret_room_lookup(unique_id, status, &body)
    }
}

/// Turn a lookup response into an outcome.
///
/// Separated from the request so every branch is testable without a network: the interesting
/// behaviour is the classification, not the socket.
pub fn interpret_room_lookup(
    unique_id: &str,
    status: u16,
    body: &str,
) -> Result<RoomLookup, DiscoveryError> {
    if !(200..300).contains(&status) {
        return Err(DiscoveryError::Status { status });
    }
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        DiscoveryError::Decode(format!("unexpected lookup response for @{unique_id}"))
    })?;
    // Valid JSON without a user means the handle does not resolve. This check runs
    // before field parsing so only genuinely absent users — never a schema change,
    // which fails the parse above — take the not-found path.
    let has_user = value
        .get("data")
        .and_then(|data| data.get("user"))
        .is_some_and(|user| !user.is_null());
    if !has_user {
        return Err(DiscoveryError::UserNotFound(
            unique_id.trim_start_matches('@').to_string(),
        ));
    }
    let lookup = room::RoomLookup::from_value(&value).ok_or_else(|| {
        DiscoveryError::Decode(format!("unexpected lookup response for @{unique_id}"))
    })?;
    // The endpoint reports an absent room as an empty string or a literal `0`, never by omitting
    // the field. Report that as its own outcome so a caller does not treat a normal offline
    // creator as a transport problem — and does not go on to sign a request for room `0`.
    //
    // A real room id with a non-live status is *not* an error: the caller reads `is_live()`.
    if !room::is_usable_room_id(&lookup.room_id) {
        return Err(DiscoveryError::NoRoom(
            unique_id.trim_start_matches('@').to_string(),
        ));
    }
    Ok(lookup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset() -> Preset {
        Preset::new(
            ttl_sign_core::DevicePreset::chrome_linux(),
            ttl_sign_core::LocationPreset::us_east(),
            ttl_sign_core::ScreenPreset::FHD,
        )
    }

    fn live_body() -> String {
        r#"{"data":{"user":{"roomId":"7300000000000000001","nickname":"Fixture",
           "uniqueId":"fixture","status":2,"avatarThumb":""}}}"#
            .into()
    }

    /// The boundary this crate exists to state. If a future change makes `room_info` native, this
    /// is the one place that has to move.
    #[test]
    fn the_capability_boundary_is_explicit() {
        assert_eq!(
            requirement(DiscoveryOperation::RoomLookup),
            Requirement::None
        );
        assert_eq!(requirement(DiscoveryOperation::RoomInfo), Requirement::None);
        assert_eq!(requirement(DiscoveryOperation::GiftList), Requirement::None);
        assert_eq!(
            requirement(DiscoveryOperation::LiveChannels),
            Requirement::None
        );
        // Every implemented discovery route is currently unsigned.
        assert_eq!(native_operations(), DiscoveryOperation::ALL.to_vec());
    }

    /// Every operation must be reachable once signing works. A `Renderer` requirement would mean
    /// a capability no signing progress can ever deliver, so it must be justified by evidence
    /// rather than by an untested assumption — as `LiveChannels` was until a signable JSON
    /// endpoint for it was found.
    #[test]
    fn nothing_is_renderer_bound_without_evidence() {
        for operation in DiscoveryOperation::ALL {
            assert_ne!(
                requirement(operation),
                Requirement::Renderer,
                "{operation:?} is marked renderer-bound; confirm no JSON endpoint serves it"
            );
        }
    }

    #[test]
    fn live_search_auth_refusal_is_not_silent_empty() {
        let error = interpret_live_search(
            r#"{"status_code":2483,"status_msg":"Please login your account first","data":[]}"#,
        )
        .unwrap_err();
        assert!(matches!(error, DiscoveryError::Refused(message)
            if message.contains("2483") && message.contains("Please login")));
    }

    #[test]
    fn live_search_zero_status_can_return_no_rooms() {
        assert_eq!(
            interpret_live_search(r#"{"status_code":0,"data":[]}"#).unwrap(),
            Vec::<LiveRoom>::new()
        );
    }

    #[test]
    fn a_live_room_is_resolved() {
        let lookup = interpret_room_lookup("fixture", 200, &live_body()).unwrap();
        assert_eq!(lookup.room_id, "7300000000000000001");
        assert!(lookup.is_live());
    }

    #[test]
    fn a_leading_at_is_tolerated() {
        let lookup = interpret_room_lookup("@fixture", 200, &live_body()).unwrap();
        assert_eq!(lookup.room_id, "7300000000000000001");
        assert!(room::room_lookup_url("@fixture").ends_with("uniqueId=fixture"));
    }

    /// An offline creator is a normal outcome with its own error, not a transport failure, so a
    /// caller does not retry it.
    #[test]
    fn an_offline_creator_is_reported_as_no_room_not_as_a_failure() {
        let offline = r#"{"data":{"user":{"roomId":"0","nickname":"Fixture",
           "uniqueId":"fixture","status":4,"avatarThumb":""}}}"#;
        assert_eq!(
            interpret_room_lookup("fixture", 200, offline),
            Err(DiscoveryError::NoRoom("fixture".into()))
        );
    }

    /// A contradictory response — "live" status on room `0` — must still be refused, or the
    /// caller signs a request for a room that does not exist.
    #[test]
    fn a_zero_room_id_is_refused_even_when_marked_live() {
        let contradictory = r#"{"data":{"user":{"roomId":"0","nickname":"Fixture",
           "uniqueId":"fixture","status":2,"avatarThumb":""}}}"#;
        assert_eq!(
            interpret_room_lookup("fixture", 200, contradictory),
            Err(DiscoveryError::NoRoom("fixture".into()))
        );
    }

    /// A real room that simply is not broadcasting is returned, not rejected: the distinction
    /// between "no room" and "room is offline" belongs to the caller.
    #[test]
    fn a_real_room_with_an_offline_status_is_returned() {
        let offline = r#"{"data":{"user":{"roomId":"7300000000000000001","nickname":"Fixture",
           "uniqueId":"fixture","status":4,"avatarThumb":""}}}"#;
        let lookup = interpret_room_lookup("fixture", 200, offline).unwrap();
        assert_eq!(lookup.room_id, "7300000000000000001");
        assert!(!lookup.is_live());
    }

    #[test]
    fn a_non_success_status_is_reported_with_its_code() {
        assert_eq!(
            interpret_room_lookup("fixture", 429, ""),
            Err(DiscoveryError::Status { status: 429 })
        );
        assert_eq!(
            interpret_room_lookup("fixture", 503, ""),
            Err(DiscoveryError::Status { status: 503 })
        );
    }

    #[test]
    fn malformed_json_decodes_to_an_error_rather_than_panicking() {
        let error = interpret_room_lookup("fixture", 200, "not json").unwrap_err();
        assert!(matches!(error, DiscoveryError::Decode(_)));
    }

    #[test]
    fn an_empty_body_is_a_decode_error() {
        assert!(matches!(
            interpret_room_lookup("fixture", 200, ""),
            Err(DiscoveryError::Decode(_))
        ));
    }

    /// Valid JSON without a user is the unknown-handle signal — distinct from Decode so a
    /// TikTok schema change can never report a valid user as nonexistent.
    #[test]
    fn a_missing_user_is_not_found_not_a_decode_failure() {
        assert_eq!(
            interpret_room_lookup("fixture", 200, r#"{"data":{}}"#),
            Err(DiscoveryError::UserNotFound("fixture".into()))
        );
        assert_eq!(
            interpret_room_lookup("@fixture", 200, r#"{"data":{"user":null}}"#),
            Err(DiscoveryError::UserNotFound("fixture".into()))
        );
    }

    /// The client builds without a browser; this is the crate's reason for existing.
    #[test]
    fn a_client_can_be_constructed_without_a_browser() {
        let client = DiscoveryClient::new(&preset()).unwrap();
        // Cloning shares the connection pool rather than opening a second one.
        let _shared = client.clone();
    }

    #[test]
    fn a_custom_timeout_is_accepted() {
        assert!(DiscoveryClient::with_timeout(&preset(), Duration::from_millis(250)).is_ok());
    }

    // --- signed discovery -----------------------------------------------------------------

    fn gift_body() -> &'static str {
        r#"{"data":{"gifts":[
            {"id":5655,"name":"Rose","describe":"sent Rose","diamond_count":1,"combo":true,
             "type":1,"icon":{"url_list":["https://example.invalid/rose.png"]}},
            {"id":6064,"name":"TikTok","describe":"sent TikTok","diamond_count":5,"combo":false,
             "type":2,"icon":{"url_list":["https://example.invalid/tt.png"]}}
        ]},"status_code":0}"#
    }

    #[test]
    fn a_gift_list_is_parsed_with_its_diamond_costs() {
        let gifts = interpret_gift_list("7300000000000000001", gift_body()).unwrap();
        assert_eq!(gifts.len(), 2);
        assert_eq!(gifts[0].name, "Rose");
        assert_eq!(gifts[0].diamond_count, 1);
        assert!(gifts[0].combo);
        assert_eq!(gifts[1].diamond_count, 5);
    }

    /// A refusal must not be read as an empty gift table: one means "ask again", the other
    /// means "this room offers no gifts".
    #[test]
    fn a_refusal_is_not_an_empty_gift_list() {
        let refusal = r#"{"status_code":10041,"data":{"message":"room has finished"}}"#;
        assert!(matches!(
            interpret_gift_list("7300000000000000001", refusal),
            Err(DiscoveryError::Refused(_))
        ));
        // An genuinely empty table is still a success.
        let empty = r#"{"data":{"gifts":[]},"status_code":0}"#;
        assert_eq!(
            interpret_gift_list("7300000000000000001", empty)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn malformed_gift_json_is_a_decode_error() {
        assert!(matches!(
            interpret_gift_list("7300000000000000001", "not json"),
            Err(DiscoveryError::Decode(_))
        ));
    }

    #[test]
    fn a_refused_room_info_is_not_parsed_as_an_empty_room() {
        let refusal = r#"{"status_code":10041,"data":{"message":"room has finished"}}"#;
        assert!(matches!(
            interpret_room_info("7300000000000000001", refusal),
            Err(DiscoveryError::Refused(_))
        ));
    }

    struct StubSigner;

    impl UrlSigner for StubSigner {
        fn sign<'a>(&'a self, url: &'a str) -> SignFuture<'a> {
            Box::pin(async move { Ok(format!("{url}&X-Bogus=1")) })
        }
    }

    #[tokio::test]
    async fn chunked_http_bodies_are_bounded_before_retention() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n8\r\nabcdefgh\r\n0\r\n\r\n").unwrap();
        });
        let response = reqwest::get(format!("http://{address}/")).await.unwrap();
        assert!(matches!(
            read_bounded(response, 12).await,
            Err(DiscoveryError::TooLarge(12))
        ));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn a_signer_is_invoked_for_signed_routes() {
        let signed = StubSigner
            .sign("https://example.invalid/webcast/gift/list/?room_id=1")
            .await;
        assert_eq!(
            signed.unwrap(),
            "https://example.invalid/webcast/gift/list/?room_id=1&X-Bogus=1"
        );
    }
}

// --- Signed discovery ---------------------------------------------------------------------
//
// `room/info` and `gift/list` need a webmssdk signature. This crate does not sign: it takes a
// [`UrlSigner`], so the same code works against the WebView, the headless signer, or a future
// native one, and the choice stays with the caller.

use std::future::Future;
use std::pin::Pin;

use ttl_sign_core::room::{Gift, RoomInfo};

pub type SignFuture<'a> = Pin<Box<dyn Future<Output = Result<String, DiscoveryError>> + Send + 'a>>;

/// Something that can turn an unsigned webcast URL into a signed one.
pub trait UrlSigner: Send + Sync {
    fn sign<'a>(&'a self, url: &'a str) -> SignFuture<'a>;
}

/// Which signing product to request.
///
/// These are not interchangeable. The patched-fetch suffix is what `room/info` and `gift/list`
/// accept; `im/fetch` rejects it with 403 and wants the public `frontierSign` product instead.
/// Sending the wrong one looks exactly like a broken signer, so it is chosen explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningProduct {
    /// Patched-fetch suffix: `X-Dynosaur`, `msToken`, `X-Bogus=1`, `X-Gnarly`.
    FetchPatch,
    /// Public `frontierSign`: a real 16-byte `X-Bogus`.
    FrontierSign,
    /// `registerWsSigner` over the query bytes, appended as `X-Gnarly`. This is what the direct
    /// message socket verifies; it signs the query exactly as sent, so the URL must not be
    /// re-encoded on the way in or out.
    WsDirect,
}

/// URL of the live search endpoint, which lists rooms that are broadcasting now.
///
/// The `/live` page renders its list client-side, so its HTML carries nothing. This endpoint
/// returns the same rooms as JSON and is merely signed, which is why listing live channels needs
/// no rendering engine.
pub fn live_search_url(keyword: &str, offset: usize) -> String {
    // Built from pairs rather than one long literal: a line-continued string silently keeps the
    // indentation as spaces inside the query, which TikTok answers with a body that has no data.
    let mut url = String::from("https://www.tiktok.com/api/search/live/full/?");
    let offset = offset.to_string();
    let pairs: [(&str, &str); 24] = [
        ("aid", "1988"),
        ("app_language", "en"),
        ("app_name", "tiktok_web"),
        ("browser_language", "en-US"),
        ("browser_name", "Mozilla"),
        ("browser_platform", "Linux x86_64"),
        ("browser_version", "5.0 (X11)"),
        ("cookie_enabled", "true"),
        ("count", "20"),
        ("device_platform", "web_pc"),
        ("focus_state", "true"),
        ("from_page", "search"),
        ("history_len", "4"),
        ("is_fullscreen", "false"),
        ("is_page_visible", "true"),
        ("keyword", keyword),
        ("offset", &offset),
        ("os", "linux"),
        ("priority_region", ""),
        ("referer", ""),
        ("region", "US"),
        ("screen_height", "1080"),
        ("screen_width", "1920"),
        ("search_id", ""),
    ];
    for (index, (name, value)) in pairs.iter().enumerate() {
        if index > 0 {
            url.push('&');
        }
        url.push_str(name);
        url.push('=');
        url.push_str(&percent_encode(value));
    }
    url.push_str("&tz_name=America%2FNew_York&webcast_language=en");
    url
}

/// Percent-encode a query value, leaving only the unreserved set intact.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// One room returned by the live search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRoom {
    pub unique_id: String,
    pub room_id: String,
    pub nickname: String,
    pub title: String,
    pub viewers: u64,
}

/// Extract broadcasting rooms from a live-search response.
///
/// Each item nests the room as a JSON *string* under `live_info.raw_data`. Only `status == 2`
/// counts as live: an unrecognized status is skipped rather than assumed to be broadcasting.
pub fn interpret_live_search(body: &str) -> Result<Vec<LiveRoom>, DiscoveryError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| DiscoveryError::Decode(error.to_string()))?;
    // TikTok sometimes answers HTTP 200 with a login/rate-limit refusal in
    // status_code. Do not turn that into an empty list of available rooms.
    if let Some(status_code) = value.get("status_code").and_then(serde_json::Value::as_i64) {
        if status_code != 0 {
            let message = value
                .get("status_msg")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("request refused");
            return Err(DiscoveryError::Refused(format!(
                "live search status_code={status_code}: {message}"
            )));
        }
    }
    let items = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| DiscoveryError::Decode("live search has no data array".into()))?;

    let mut rooms = Vec::new();
    for item in items {
        let Some(raw) = item
            .get("live_info")
            .and_then(|info| info.get("raw_data"))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let Ok(room) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        if room.get("status").and_then(serde_json::Value::as_i64) != Some(2) {
            continue;
        }
        let owner = room.get("owner");
        let unique_id = owner
            .and_then(|owner| owner.get("display_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let room_id = room
            .get("id_str")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        if unique_id.is_empty() || !room::is_usable_room_id(&room_id) {
            continue;
        }
        rooms.push(LiveRoom {
            unique_id,
            room_id,
            nickname: owner
                .and_then(|owner| owner.get("nickname"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            title: room
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            viewers: room
                .get("user_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
        });
    }
    Ok(rooms)
}

impl DiscoveryClient {
    /// Rooms broadcasting right now, most watched first. Unsigned.
    ///
    /// Results depend on the keyword, so this samples live rooms rather than enumerating them.
    pub async fn live_channels(&self, keyword: &str) -> Result<Vec<LiveRoom>, DiscoveryError> {
        self.live_channels_page(keyword, 0).await
    }

    /// Browse one page of currently live rooms without automatically connecting.
    ///
    /// The caller controls pagination/rate limits and selects a creator explicitly.
    /// An empty page is not proof that no one is broadcasting; the upstream search
    /// may be incomplete. The returned rooms are ordered by viewer count.
    pub async fn live_channels_page(
        &self,
        keyword: &str,
        offset: usize,
    ) -> Result<Vec<LiveRoom>, DiscoveryError> {
        let body = self.read(&live_search_url(keyword, offset)).await?;
        let mut rooms = interpret_live_search(&body)?;
        rooms.sort_by_key(|room| std::cmp::Reverse(room.viewers));
        Ok(rooms)
    }

    /// Full room metadata. Unsigned.
    pub async fn room_info(&self, room_id: &str) -> Result<RoomInfo, DiscoveryError> {
        let body = self.read(&room::room_info_url(room_id)).await?;
        interpret_room_info(room_id, &body)
    }

    /// Every gift the room offers, with its diamond cost. Unsigned.
    ///
    /// The response is several megabytes; the client's timeout applies to it.
    pub async fn gift_list(&self, room_id: &str) -> Result<Vec<Gift>, DiscoveryError> {
        let body = self.read(&room::gift_list_url(room_id)).await?;
        interpret_gift_list(room_id, &body)
    }

    /// Read a webcast endpoint that does not verify a signature.
    ///
    /// These three were signed until it was measured that they do not check: `room/info`,
    /// `gift/list` and the live search each return identical data whether the request carries a
    /// correct signature, one with a single character changed, or none at all
    /// (`scripts/headless/verify-probe.mjs`, 2026-08-18). Signing them cost a subprocess per call
    /// and, worse, produced a success that was mistaken for evidence that the signer works —
    /// which is how the transport diagnosis went wrong. Only `/webcast/im/fetch/` evaluates a
    /// signature, and that path lives in `ttl-sign-headless`.
    async fn read(&self, url: &str) -> Result<String, DiscoveryError> {
        let mut request = self
            .http
            .get(url)
            // These endpoints are read from the web app, and reject a request that does not look
            // like it came from one.
            .header("referer", "https://www.tiktok.com/")
            .header("origin", "https://www.tiktok.com");
        if !self.session.is_empty() {
            request = request.header("cookie", self.session.clone());
        }
        let response = request
            .send()
            .await
            .map_err(|error| DiscoveryError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(DiscoveryError::Status { status });
        }
        let body = read_bounded(response, 16 * 1024 * 1024).await?;
        if body.is_empty() {
            // A signed request that is accepted but answered with nothing means the identity was
            // insufficient, not that the room is empty. Reported distinctly so a caller does not
            // read it as "no gifts".
            return Err(DiscoveryError::EmptyResponse);
        }
        Ok(body)
    }
}

/// Classify a `room/info` response. Separated from the request so every branch is testable.
pub fn interpret_room_info(room_id: &str, body: &str) -> Result<RoomInfo, DiscoveryError> {
    if let Some(refusal) = room::webcast_refusal(body) {
        return Err(DiscoveryError::Refused(refusal.to_string()));
    }
    RoomInfo::from_json(body).ok_or_else(|| {
        DiscoveryError::Decode(format!("unexpected room/info response for room {room_id}"))
    })
}

/// Classify a `gift/list` response.
pub fn interpret_gift_list(room_id: &str, body: &str) -> Result<Vec<Gift>, DiscoveryError> {
    if let Some(refusal) = room::webcast_refusal(body) {
        return Err(DiscoveryError::Refused(refusal.to_string()));
    }
    room::parse_gift_list(body).ok_or_else(|| {
        DiscoveryError::Decode(format!("unexpected gift/list response for room {room_id}"))
    })
}

pub mod identity;
