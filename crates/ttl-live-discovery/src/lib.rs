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
//! [`CommandSigner`] remains for the one request that *is* verified, `/webcast/im/fetch/`, which
//! `ttl-sign-headless` owns. It drives `scripts/headless/sign-url.mjs` running the real bundle
//! under a synthetic environment, so a browser-free build works today.
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
        // `LiveChannels` reads `/api/search/live/full/`, which is signed like any webcast call.
        // The `/live` DOM is one source of this list, not the only one.
        DiscoveryOperation::RoomInfo
        | DiscoveryOperation::GiftList
        | DiscoveryOperation::LiveChannels => Requirement::Signature,
    }
}

/// Operations that need no signer at all.
///
/// The rest are implemented too, but require a [`UrlSigner`]; see the crate documentation.
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
        if let Some(length) = response.content_length() {
            if length > MAX_BODY_BYTES as u64 {
                return Err(DiscoveryError::TooLarge(MAX_BODY_BYTES));
            }
        }
        let body = response
            .text()
            .await
            .map_err(|error| DiscoveryError::Transport(error.to_string()))?;
        if body.len() > MAX_BODY_BYTES {
            return Err(DiscoveryError::TooLarge(MAX_BODY_BYTES));
        }

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
    let lookup = room::RoomLookup::from_json(body).ok_or_else(|| {
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
        assert_eq!(
            requirement(DiscoveryOperation::RoomInfo),
            Requirement::Signature
        );
        assert_eq!(
            requirement(DiscoveryOperation::GiftList),
            Requirement::Signature
        );
        assert_eq!(
            requirement(DiscoveryOperation::LiveChannels),
            Requirement::Signature
        );
        // Only the lookup needs no signer; the others are implemented but require one.
        assert_eq!(native_operations(), vec![DiscoveryOperation::RoomLookup]);
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

    /// Records the per-route rule as an executable fact, since sending the wrong product yields a
    /// 403 indistinguishable from a broken signer.
    #[test]
    fn signing_products_map_to_the_signer_argument() {
        assert_eq!(SigningProduct::FetchPatch.as_arg(), "fetch");
        assert_eq!(SigningProduct::FrontierSign.as_arg(), "frontier");
    }

    struct StubSigner;

    impl UrlSigner for StubSigner {
        fn sign<'a>(&'a self, url: &'a str) -> SignFuture<'a> {
            Box::pin(async move { Ok(format!("{url}&X-Bogus=1")) })
        }
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

    /// A signer that cannot run must surface as a signing failure, never as a discovery result.
    #[tokio::test]
    async fn a_missing_signer_binary_is_a_signing_error() {
        let signer = CommandSigner::node("no-such-script.mjs", "no-such-bundle.js");
        let outcome = signer
            .sign("https://example.invalid/webcast/gift/list/")
            .await;
        assert!(matches!(outcome, Err(DiscoveryError::Signer(_))));
    }

    /// Cookies must travel in the environment, not in the argument list where the process table
    /// would expose them.
    #[test]
    fn signer_credentials_are_not_command_arguments() {
        let signer = CommandSigner::node("script.mjs", "bundle.js")
            .with_cookie("sessionid=secret-value")
            .with_stored_token("stored-secret");
        assert!(!signer.args.iter().any(|a| a.contains("secret")));
        assert_eq!(signer.cookie.as_deref(), Some("sessionid=secret-value"));
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
}

impl SigningProduct {
    fn as_arg(self) -> &'static str {
        match self {
            SigningProduct::FetchPatch => "fetch",
            SigningProduct::FrontierSign => "frontier",
        }
    }
}

/// A [`UrlSigner`] that shells out to an external signer process.
///
/// This is how a browser-free build reaches the signer today: `scripts/headless/sign-url.mjs`
/// runs the real bundle under a synthetic environment and prints the signed URL. Cookies travel
/// in the environment rather than in the argument list, so they do not appear in the process
/// table.
#[derive(Debug, Clone)]
pub struct CommandSigner {
    program: String,
    args: Vec<String>,
    product: SigningProduct,
    cookie: Option<String>,
    stored_token: Option<String>,
    user_agent: Option<String>,
}

impl CommandSigner {
    /// Drive `node <script> <bundle> <url> <product>`.
    pub fn node(script: impl Into<String>, bundle: impl Into<String>) -> Self {
        Self {
            program: "node".into(),
            args: vec![script.into(), bundle.into()],
            product: SigningProduct::FetchPatch,
            cookie: None,
            stored_token: None,
            user_agent: None,
        }
    }

    pub fn with_product(mut self, product: SigningProduct) -> Self {
        self.product = product;
        self
    }

    /// Cookie header presented to the signer. Passed through the environment, never as an
    /// argument.
    pub fn with_cookie(mut self, cookie: impl Into<String>) -> Self {
        self.cookie = Some(cookie.into());
        self
    }

    /// The stored `xmst` token. `msToken` is a verbatim passthrough of it.
    pub fn with_stored_token(mut self, token: impl Into<String>) -> Self {
        self.stored_token = Some(token.into());
        self
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }
}

impl UrlSigner for CommandSigner {
    fn sign<'a>(&'a self, url: &'a str) -> SignFuture<'a> {
        Box::pin(async move {
            let mut command = tokio::process::Command::new(&self.program);
            command.args(&self.args).arg(url).arg(self.product.as_arg());
            if let Some(cookie) = &self.cookie {
                command.env("TTL_COOKIE", cookie);
            }
            if let Some(token) = &self.stored_token {
                command.env("TTL_XMST", token);
            }
            if let Some(agent) = &self.user_agent {
                command.env("TTL_USER_AGENT", agent);
            }
            let output = command
                .output()
                .await
                .map_err(|error| DiscoveryError::Signer(error.to_string()))?;
            if !output.status.success() {
                // The signer prints diagnostics on stderr and the URL on stdout, so a failure
                // message never carries a signed URL.
                let reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(DiscoveryError::Signer(if reason.is_empty() {
                    format!("signer exited with {}", output.status)
                } else {
                    reason
                }));
            }
            // The signed URL is the last non-empty line: a signer that leaks stray output on
            // stdout should not corrupt the result, and the bundle is known to print while it
            // loads.
            let stdout = String::from_utf8_lossy(&output.stdout);
            let signed = stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .next_back()
                .unwrap_or_default()
                .to_string();
            if !signed.starts_with("http") {
                return Err(DiscoveryError::Signer(
                    "signer produced no URL on stdout".into(),
                ));
            }
            Ok(signed)
        })
    }
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
        let body = self.read(&live_search_url(keyword, 0)).await?;
        let mut rooms = interpret_live_search(&body)?;
        rooms.sort_by(|a, b| b.viewers.cmp(&a.viewers));
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
        let body = response
            .text()
            .await
            .map_err(|error| DiscoveryError::Transport(error.to_string()))?;
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
