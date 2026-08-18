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
//! # The transport request is not signed
//!
//! It was, until 2026-08-18, and that was the defect. The live page's own signing allowlist —
//! read out of `static/js/main.*.js`, where the app calls `byted_acrawler.init` — covers seven GET
//! paths and twenty-two POST paths on `webcast.tiktok.com`, all wallet, KYC, `room/chat` and
//! `room/enter`. **`/webcast/im/fetch/` is on neither list**, so a browser sends it with no
//! signature at all, and the service refuses one that carries a signature it never expects:
//!
//! | Form | Result |
//! |---|---|
//! | unsigned | **200** |
//! | `X-Bogus` alone | 200, empty |
//! | with `X-Gnarly` or `X-Dynosaur`, any content | **403** |
//!
//! That is why the 403 was insensitive to every input the signature reads: it was never about the
//! value. `scripts/headless/im-fetch-bisect.mjs` has the dated rows.
//!
//! The query matters as much as the absence of a signature, and the same chunk supplies it —
//! `version_code` is `180800`, the initial fetch sends `cursor=0`, `internal_ext=0` and
//! `last_rtt=-1`, and its builder deletes empty values, so `cursor=` is a request no player makes.
//! [`ttl_sign_core::params::FetchParams`] now builds that query.
//!
//! # The signer is still needed, for `room/enter`
//!
//! `/webcast/room/enter/` *is* on the POST allowlist, and it is the one endpoint measured here that
//! verifies a signature — unsigned and `X-Bogus`-only are both refused with 403, while the full
//! computed suffix is accepted with 200. So the signature this project produces is **correct**, and
//! `room/enter` is the oracle that proves it. `scripts/headless/enter-then-fetch.mjs` runs that
//! sequence.
//!
//! # This is no longer the transport
//!
//! `im/fetch` answers 200 with **zero bytes**, and nothing about the request explains it: ruled out
//! are the signature in either direction, three parameter sets, `resp_content_type`, five hosts, the
//! `x-tt-target-idc` header, identity down to no cookies, and the room. The player's own code
//! explains it instead. Its IM SDK is configured with `wsDirect: "1"` and a `socketHost`, and then
//! builds and signs the message socket URI itself; `im/fetch` still runs under
//! `fetchBeforeWsSuccess`, but only as a best-effort first page of messages, so nothing depends on
//! its answer.
//!
//! The live path is therefore [`ttl_sign_core::DirectSocketParams`] plus
//! [`ttl_live_discovery::SigningProduct::WsDirect`], which is what
//! `cargo run -p ttl-live-discovery --example live-check` runs. This backend is kept because it is
//! the one place the `im/fetch` request shape is written down correctly, and because the sign
//! server and the contract tests are built on its [`SignedFetch`]. An empty body is reported as
//! [`RejectReason::EmptyBody`].
//!
//! # What it requires
//!
//! - **An authenticated session.** [`HeadlessConfig::session`] takes the same jar the WebView
//!   path loaded.
//! - **A signer**, for `room/enter`. Not for the transport request.
//!
use std::time::Duration;

use ttl_live_discovery::{SigningProduct, UrlSigner};
use ttl_sign_core::params::{DirectSocketParams, FetchParams};
use ttl_sign_core::proto::FetchResult;
use ttl_sign_core::{
    BackendFuture, ClientIdentity, CookieJar, Preset, RejectReason, SignError, SignOutcome,
    SignedFetch, SignerBackend, TransportRequest, WS_REUSE_PATH,
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

    /// Issue the legacy `/webcast/im/fetch/` request, trying both `sup_ws_ds_opt` values.
    ///
    /// **Not the transport.** It is kept because it is the only Rust statement of that request's
    /// shape, and because the shape is not settled history: on 2026-08-18 one signed attempt was
    /// answered with 120,290 bytes and a real `push_server`, and the next two with 403
    /// (`fixtures/research/bisect-ledger.json`). A caller wanting to reproduce that must configure
    /// its signer with a product this route accepts — [`TRANSPORT_PRODUCT`] is the socket's, and
    /// signs the wrong thing here.
    pub async fn im_fetch(&self, room_id: &str) -> SignOutcome {
        let mut last = SignOutcome::Rejected(RejectReason::EmptyBody);
        for option in WS_DS_OPTIONS {
            match self.attempt(room_id, option).await {
                outcome @ SignOutcome::Ok(_) => return outcome,
                // Only an empty body is worth retrying under the other option; a transport error or
                // a real refusal will not change.
                SignOutcome::Rejected(RejectReason::EmptyBody) => {
                    tracing::debug!(
                        room_id,
                        sup_ws_ds_opt = option,
                        "empty transport response; trying the other option"
                    );
                    last = SignOutcome::Rejected(RejectReason::EmptyBody);
                }
                other => return other,
            }
        }
        last
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

/// Heartbeat the synthesized result advertises, matching the query it was built from.
const HEARTBEAT_DURATION_MS: u64 = 10_000;

/// A cursor a client can start from.
///
/// `tiktok-live-connector` refuses a result whose cursor is empty, and the socket does not need a
/// real one: the server sends the current state after `im_enter_room`. The player's own cold start
/// sends `0` for the same reason.
const COLD_START_CURSOR: &str = "0";

impl HeadlessBackend {
    /// Build and sign the socket URL, and describe it the way a client expects.
    ///
    /// This is the transport. Nothing is sent here: the URL *is* the product, and the client opens
    /// it. That is what the player does — with `wsDirect` on it builds the socket URL itself and
    /// signs the query with `registerWsSigner`, rather than asking `/webcast/im/fetch/` for a
    /// `push_server`.
    ///
    /// The result is packed into TikTok's own `ProtoMessageFetchResult` shape because that is the
    /// contract every client of this server already speaks: `push_server` carries the host and path,
    /// and `route_params` every query parameter including the signature.
    ///
    /// Clients rebuild the query from that map rather than sending our bytes — `URLSearchParams`
    /// reorders it, re-encodes the spaces, and collapses the duplicated `version_code`. Measured on
    /// 2026-08-18: the socket accepts the rebuilt query and pushes frames, so the verifier is not
    /// byte-exact. That is what makes this shape safe to hand out.
    async fn direct_socket(&self, room_id: &str) -> SignOutcome {
        let mut params = DirectSocketParams::new(room_id);
        if let Some(webid) = self
            .session
            .get("tt_webid_v2")
            .filter(|value| !value.is_empty())
        {
            params.device_id = webid.to_string();
        }
        let unsigned = params.url(&self.preset);

        let signed = match self.signer.sign(&unsigned).await {
            Ok(signed) => signed,
            Err(error) => {
                return SignOutcome::Transport(SignError::BackendUnavailable(error.to_string()))
            }
        };

        let Some((host, query)) = signed.split_once('?') else {
            return SignOutcome::Transport(SignError::BackendUnavailable(
                "the signer returned a socket URL with no query".into(),
            ));
        };
        if !query.contains("X-Gnarly=") {
            // Without the signature the handshake is refused, and a client cannot tell that from a
            // dead room. Fail here, where the reason is still legible.
            return SignOutcome::Transport(SignError::BackendUnavailable(
                "the signer returned no X-Gnarly; the socket product was not used".into(),
            ));
        }

        let result = FetchResult {
            cursor: COLD_START_CURSOR.into(),
            internal_ext: String::new(),
            route_params: route_params(query),
            heartbeat_duration: HEARTBEAT_DURATION_MS,
            need_ack: true,
            push_server: host.to_string(),
        };

        SignOutcome::Ok(SignedFetch {
            protobuf: result.encode(),
            cookies: self.session.clone(),
            user_agent: self.preset.user_agent(),
            signed_url: signed,
        })
    }
}

/// Split a signed query into the `route_params` map a client rebuilds its URL from.
///
/// Values are percent-decoded, because the client percent-encodes them again on the way out and
/// would otherwise send `%253D` where the signature covered `=`.
fn route_params(query: &str) -> Vec<(String, String)> {
    let mut params: Vec<(String, String)> = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = ttl_sign_core::params::percent_decode(value);
        // The query carries `version_code` twice and a map cannot; last wins, which is the same
        // collapse the client would perform. Measured to still be accepted.
        match params.iter_mut().find(|(name, _)| name == key) {
            Some(slot) => slot.1 = value,
            None => params.push((key.to_string(), value)),
        }
    }
    params
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
        Box::pin(async move { self.direct_socket(&request.room_id).await })
    }

    fn identity(&self) -> ClientIdentity {
        ClientIdentity::new(self.preset.user_agent())
    }
}

/// The signing product the transport requires.
///
/// Exposed so a caller configuring [`ttl_live_discovery::CommandSigner`] cannot pick another by
/// accident. The socket is signed by `registerWsSigner` over the query bytes; the patched-fetch
/// suffix and `frontierSign` produce parameters this endpoint ignores, and the handshake is then
/// refused with a bare 1006 that looks like a dead room.
pub const TRANSPORT_PRODUCT: SigningProduct = SigningProduct::WsDirect;

/// The socket path every signed result points at.
pub const TRANSPORT_PATH: &str = WS_REUSE_PATH;

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

    /// Pinning the product is the point: the socket ignores the parameters the other two products
    /// add, and then refuses the handshake with a bare 1006 that reads like a dead room.
    #[test]
    fn the_transport_route_uses_the_socket_signing_product() {
        assert_eq!(TRANSPORT_PRODUCT, SigningProduct::WsDirect);
    }

    /// A client rebuilds its URL from `route_params`, so what goes in there has to survive that.
    #[test]
    fn route_params_decode_values_and_collapse_the_duplicate_version_code() {
        let params = route_params(
            "version_code=180800&room_id=7&tz_name=America%2FNew_York&version_code=270000\
&X-Gnarly=ab%2Fcd",
        );
        let by_name = |name: &str| {
            params
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(params.iter().filter(|(k, _)| k == "version_code").count(), 1);
        // Last wins, which is the same collapse the client performs on its own map.
        assert_eq!(by_name("version_code").as_deref(), Some("270000"));
        // Decoded, because the client encodes them again on the way out.
        assert_eq!(by_name("tz_name").as_deref(), Some("America/New_York"));
        assert_eq!(by_name("X-Gnarly").as_deref(), Some("ab/cd"));
    }
}
