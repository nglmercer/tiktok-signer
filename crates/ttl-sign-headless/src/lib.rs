//! A [`SignerBackend`] that needs no browser.
//!
//! This is the production replacement for the WebView signer. It builds the `/webcast/im/fetch/`
//! query with [`ttl_sign_core::params`], has an external signer sign it, issues the request, and
//! decodes the response into the same [`SignedFetch`] every other backend returns — so the sign
//! server, the connector, and the contract tests are unchanged.
//!
//! It does not sign. Signing is a [`UrlSigner`], which today is
//! [`ttl_live_discovery::CommandSigner`] driving `scripts/headless/sign-url.mjs`. Replacing that
//! subprocess with an embedded JavaScript engine changes one constructor argument and nothing
//! else.
//!
//! # What it requires
//!
//! - **The public signing product.** `/webcast/im/fetch/` answers the `frontierSign` product with
//!   an empty 200, and refuses the patched-fetch suffix with a 403 — either `X-Gnarly` or
//!   `X-Dynosaur`, alone, is enough to draw the 403. This backend pins the form that is at least
//!   answered rather than leaving the choice to the caller. "Accepted" would be too strong a word
//!   for it.
//! - **An authenticated session.** Supply the account cookies; [`HeadlessConfig::session`] takes
//!   the same jar the WebView path loaded.
//!
//! # This path does not yet reach a push_server
//!
//! An empty body is reported as [`RejectReason::EmptyBody`], matching the WebView's classification
//! exactly. What that empty body means is unresolved, and the shape of the open question changed on
//! 2026-08-18:
//!
//! - The endpoints this crate's signature *was* verified against — `room/info`, `gift/list`, the
//!   live search — do not verify signatures at all (`scripts/headless/verify-probe.mjs`). Their
//!   success was never evidence that the suffix is correct.
//! - The refusal on `im/fetch` does not respond to any input the signature is known to read:
//!   fifteen variants over the user agent, the canvas fingerprint, three parameter sets, `msToken`,
//!   entropy, client hints and `X-Bogus` all land on the same 403
//!   (`scripts/headless/im-fetch-bisect.mjs`, and `fixtures/research/bisect-ledger.json` for the
//!   dated outcomes).
//!
//! So this is not "one wrong field away". See `docs/12-transport-reverse-engineering.md` for the
//! two levers that remain.

use std::time::Duration;

use ttl_live_discovery::{SigningProduct, UrlSigner};
use ttl_sign_core::params::FetchParams;
use ttl_sign_core::proto::FetchResult;
use ttl_sign_core::{
    BackendFuture, ClientIdentity, CookieJar, Preset, RejectReason, SignError, SignOutcome,
    SignedFetch, SignerBackend, TransportRequest,
};

/// Default ceiling on one signing round trip, including the signer subprocess.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Which `sup_ws_ds_opt` values to try, in order.
///
/// Which one the service honours varies between rooms and over time — both the WebView and this
/// backend see the same variation — so a single value would fail intermittently for no reason the
/// caller could act on.
const WS_DS_OPTIONS: [u8; 2] = [1, 0];

pub struct HeadlessConfig {
    pub preset: Preset,
    /// Account cookies. The transport endpoint refuses guests, so this is effectively required.
    pub session: CookieJar,
    pub timeout: Duration,
}

impl HeadlessConfig {
    pub fn new(preset: Preset, session: CookieJar) -> Self {
        Self {
            preset,
            session,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Signs and issues the transport request without a browser.
pub struct HeadlessBackend {
    http: reqwest::Client,
    signer: Box<dyn UrlSigner>,
    preset: Preset,
    session: CookieJar,
}

impl HeadlessBackend {
    pub fn new(config: HeadlessConfig, signer: Box<dyn UrlSigner>) -> Result<Self, SignError> {
        let http = reqwest::Client::builder()
            .user_agent(config.preset.user_agent())
            .timeout(config.timeout)
            .build()
            .map_err(|error| SignError::BackendUnavailable(error.to_string()))?;
        Ok(Self {
            http,
            signer,
            preset: config.preset,
            session: config.session,
        })
    }

    /// True when the configured session can actually reach the transport endpoint.
    ///
    /// A guest is accepted and answered with nothing, which is indistinguishable from a closed
    /// room unless the caller checks this first.
    pub fn is_authenticated(&self) -> bool {
        self.session
            .get("sessionid")
            .is_some_and(|value| !value.is_empty())
    }

    async fn attempt(&self, room_id: &str, sup_ws_ds_opt: u8) -> SignOutcome {
        let mut params = FetchParams::new(room_id);
        params.sup_ws_ds_opt = sup_ws_ds_opt;
        let unsigned = params.url(&self.preset);

        let signed = match self.signer.sign(&unsigned).await {
            Ok(signed) => signed,
            Err(error) => {
                return SignOutcome::Transport(SignError::BackendUnavailable(error.to_string()))
            }
        };

        let mut request = self
            .http
            .get(&signed)
            .header("referer", "https://www.tiktok.com/")
            .header("origin", "https://www.tiktok.com");
        let cookie = self.session.to_cookie_string();
        if !cookie.is_empty() {
            request = request.header("cookie", cookie);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => return SignOutcome::Transport(SignError::Transport(error.to_string())),
        };
        let status = response.status().as_u16();
        let body = match response.bytes().await {
            Ok(bytes) => bytes.to_vec(),
            Err(error) => return SignOutcome::Transport(SignError::Transport(error.to_string())),
        };

        build_outcome(status, &signed, body, self.session.clone(), &self.preset)
    }
}

/// Classify the response.
///
/// Deliberately identical to the WebView's classification: a 200 with an empty body or an empty
/// `push_server` is a **rejection**, not a transient error, and a caller must not retry it as one.
pub fn build_outcome(
    status: u16,
    signed_url: &str,
    protobuf: Vec<u8>,
    cookies: CookieJar,
    preset: &Preset,
) -> SignOutcome {
    if status != 200 {
        return SignOutcome::Rejected(RejectReason::HttpStatus(status));
    }
    if protobuf.is_empty() {
        return SignOutcome::Rejected(RejectReason::EmptyBody);
    }
    match FetchResult::decode(&protobuf) {
        Ok(result) => {
            if let Some(reason) = result.rejection_reason() {
                return SignOutcome::Rejected(reason);
            }
        }
        Err(error) => return SignOutcome::Transport(SignError::Decode(error.to_string())),
    }
    SignOutcome::Ok(SignedFetch {
        protobuf,
        cookies,
        user_agent: preset.user_agent(),
        signed_url: signed_url.to_string(),
    })
}

impl SignerBackend for HeadlessBackend {
    fn transport(&self, request: TransportRequest) -> BackendFuture<'_> {
        Box::pin(async move {
            let mut last = SignOutcome::Rejected(RejectReason::EmptyBody);
            for option in WS_DS_OPTIONS {
                let outcome = self.attempt(&request.room_id, option).await;
                match outcome {
                    SignOutcome::Ok(_) => return outcome,
                    // Only an empty body is worth retrying under the other option; a transport
                    // error or a real refusal will not change.
                    SignOutcome::Rejected(RejectReason::EmptyBody) => {
                        tracing::debug!(
                            room_id = %request.room_id,
                            sup_ws_ds_opt = option,
                            "empty transport response; trying the other option"
                        );
                        last = SignOutcome::Rejected(RejectReason::EmptyBody);
                    }
                    other => return other,
                }
            }
            last
        })
    }

    fn identity(&self) -> ClientIdentity {
        ClientIdentity::new(self.preset.user_agent())
    }
}

/// The signing product `/webcast/im/fetch/` accepts.
///
/// Exposed so a caller configuring [`ttl_live_discovery::CommandSigner`] cannot pick the other one
/// by accident; the patched-fetch suffix is rejected with 403 on this route.
pub const TRANSPORT_PRODUCT: SigningProduct = SigningProduct::FrontierSign;

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_live_discovery::SignFuture;

    fn preset() -> Preset {
        Preset::new(
            ttl_sign_core::DevicePreset::chrome_linux(),
            ttl_sign_core::LocationPreset::us_east(),
            ttl_sign_core::ScreenPreset::FHD,
        )
    }

    struct FailingSigner;

    impl UrlSigner for FailingSigner {
        fn sign<'a>(&'a self, _url: &'a str) -> SignFuture<'a> {
            Box::pin(async move {
                Err(ttl_live_discovery::DiscoveryError::Signer(
                    "no signer".into(),
                ))
            })
        }
    }

    fn backend(session: CookieJar) -> HeadlessBackend {
        HeadlessBackend::new(
            HeadlessConfig::new(preset(), session),
            Box::new(FailingSigner),
        )
        .unwrap()
    }

    /// The guest case has to be visible up front: the endpoint accepts a guest and answers with
    /// nothing, which reads exactly like a closed room.
    #[test]
    fn an_unauthenticated_session_is_reported() {
        assert!(!backend(CookieJar::new()).is_authenticated());
        assert!(!backend(CookieJar::parse("ttwid=abc")).is_authenticated());
        assert!(backend(CookieJar::parse("sessionid=abc; ttwid=d")).is_authenticated());
        assert!(!backend(CookieJar::parse("sessionid=")).is_authenticated());
    }

    /// A signer that cannot run is a backend problem, never a rejection: a caller must not read it
    /// as "TikTok refused this room".
    #[tokio::test]
    async fn a_signer_failure_is_a_backend_error_not_a_rejection() {
        let outcome = backend(CookieJar::parse("sessionid=x"))
            .transport(TransportRequest::new("7300000000000000001"))
            .await;
        assert!(matches!(
            outcome,
            SignOutcome::Transport(SignError::BackendUnavailable(_))
        ));
    }

    #[test]
    fn a_non_200_is_a_rejection_with_its_status() {
        let outcome = build_outcome(
            429,
            "https://x.invalid/",
            vec![1],
            CookieJar::new(),
            &preset(),
        );
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::HttpStatus(429))
        ));
    }

    /// The classification that matters most: an accepted-but-empty answer is a rejection, and is
    /// what both this backend and the WebView return for the same rooms.
    #[test]
    fn an_empty_body_is_a_rejection_not_a_success() {
        let outcome = build_outcome(
            200,
            "https://x.invalid/",
            Vec::new(),
            CookieJar::new(),
            &preset(),
        );
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyBody)
        ));
    }

    #[test]
    fn an_undecodable_body_is_a_decode_error() {
        // A truncated varint cannot be a valid message.
        let outcome = build_outcome(
            200,
            "https://x.invalid/",
            vec![0xff],
            CookieJar::new(),
            &preset(),
        );
        assert!(matches!(
            outcome,
            SignOutcome::Transport(SignError::Decode(_))
        ));
    }

    #[test]
    fn the_identity_matches_the_preset_user_agent() {
        let backend = backend(CookieJar::new());
        assert_eq!(backend.identity().user_agent, preset().user_agent());
    }

    /// Pinning the product is the point: the transport endpoint refuses the other one with 403.
    #[test]
    fn the_transport_route_uses_the_public_signing_product() {
        assert_eq!(TRANSPORT_PRODUCT, SigningProduct::FrontierSign);
    }
}
