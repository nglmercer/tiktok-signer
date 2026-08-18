//! Browser-free discovery for the TikTok LIVE endpoints that need no signature.
//!
//! Phase 4 of `docs/11-webview-removal.md`. Discovery is one of the three dependencies the WebView
//! carries, and unlike signing it does not converge — it *splits*. Some of it is plain HTTP and
//! works natively today; some of it needs a signature; one part needs a renderer and cannot be
//! made native at all.
//!
//! This crate implements the part that is already native, and makes the rest of the boundary
//! explicit rather than leaving a caller to discover it by failing:
//!
//! | Operation | Requirement | Native today |
//! |---|---|---|
//! | `unique_id` → `room_id` | none | **yes** |
//! | `/webcast/room/info/` | a signature | no |
//! | `/webcast/gift/list/` | a signature | no |
//! | who is live now | a rendering engine | no, and not by signing |
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
    /// The data is produced by client-side rendering and is absent from the served HTML. No
    /// amount of signing progress makes this native.
    Renderer,
}

/// What an operation needs. This is the WebView-removal boundary, in one place and testable.
pub fn requirement(operation: DiscoveryOperation) -> Requirement {
    match operation {
        DiscoveryOperation::RoomLookup => Requirement::None,
        DiscoveryOperation::RoomInfo | DiscoveryOperation::GiftList => Requirement::Signature,
        DiscoveryOperation::LiveChannels => Requirement::Renderer,
    }
}

/// Operations this crate can perform without a browser.
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
}

/// Native discovery client.
///
/// Holds one `reqwest::Client` so connections are pooled across lookups.
#[derive(Debug, Clone)]
pub struct DiscoveryClient {
    http: reqwest::Client,
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
        Ok(Self { http })
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
            Requirement::Renderer
        );
        assert_eq!(native_operations(), vec![DiscoveryOperation::RoomLookup]);
    }

    /// Signing progress must never be read as making the renderer-bound operation native.
    #[test]
    fn no_operation_is_both_signature_and_renderer_bound() {
        for operation in DiscoveryOperation::ALL {
            let requirement = requirement(operation);
            if operation == DiscoveryOperation::LiveChannels {
                assert_eq!(
                    requirement,
                    Requirement::Renderer,
                    "live channels need rendering; a signature does not help"
                );
            }
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
}
