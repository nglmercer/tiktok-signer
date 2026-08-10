//! WebView-based signing engine.
//!
//! Implements **Plan A**: it does not reimplement signing; TikTok's page signs and executes
//! the request, and the engine collects bytes over IPC.
//!
//! # Thread model
//!
//! `wry`/`tao` require the event loop on the main thread, and `run()` does not return.
//! [`run`] therefore owns the main thread and gives a [`Signer`] to a worker:
//!
//! ```no_run
//! use ttl_sign_webview::{run, EngineConfig};
//!
//! run(EngineConfig::default(), |signer| {
//!     let rt = tokio::runtime::Runtime::new().unwrap();
//!     rt.block_on(async move {
//!         let outcome = signer.fetch("7300000000000000000").await;
//!         println!("{}", outcome.is_ok());
//!     });
//! });
//! ```
//!
//! `#[tokio::main]` in `main()` **does not work**: the event loop must run on the main
//! thread. The API prevents constructing the engine in another way.

pub mod ipc;
pub mod session;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};
use wry::WebViewBuilder;

use flate2::read::GzDecoder;

use ttl_sign_core::proto::{LiveEvent, PushFrame, WebcastEventBatch};
use ttl_sign_core::room::{self, LiveChannel, RoomLookup};
use ttl_sign_core::{
    CookieJar, FetchParams, FetchResult, Preset, RejectReason, SignError, SignOutcome, SignedFetch,
};

use crate::ipc::{FromPage, PageEnv, ToPage, ToPageText};

/// Login-session polling interval.
const POLL_LOGIN: Duration = Duration::from_secs(2);

/// Cheap TikTok-origin document used to prime cookies before the real page. The first
/// navigation necessarily occurs before a document-start script can run; the second carries
/// the session in its HTTP header.
const SESSION_BOOTSTRAP_URL: &str = "https://www.tiktok.com/robots.txt";

/// Script injected before page scripts.
pub const BRIDGE_JS: &str = include_str!("bridge.js");

/// Engine configuration.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Device preset: generates the WebView UA **and** query parameters.
    pub preset: Preset,
    /// Page used for signing. It must be a TikTok URL that loads `webmssdk.js`; any live
    /// page is sufficient.
    pub landing_url: String,
    /// Maximum signing time. The result expires after roughly 30 seconds, so failing fast
    /// is preferable to waiting.
    pub sign_timeout: Duration,
    /// Deadline for `window.byted_acrawler` to appear.
    pub sdk_ready_timeout: Duration,
    /// Contact email required by the Euler spec (`contact_us`). Optional.
    pub contact_us: String,
    /// Visible window. Used only for login, where a person must see and type into the page;
    /// signing runs invisibly.
    pub visible: bool,
    /// Prevent the player from opening its own WebSocket. Rust opens the socket with the
    /// parameters returned by `/im/fetch/`; two sockets for the same room/session cause the
    /// second one to remain silent.
    pub block_page_websockets: bool,
    /// Cookies from a real account. Empty means anonymous session.
    ///
    /// Verified on 2026-08-10 against real live rooms: **the flow requires it**.
    /// `/webcast/room/enter/` literally returns `User doesn't login`, and
    /// `/webcast/im/fetch/` returns 200 with an empty body, the same silent rejection.
    /// Session-independent endpoints (`room/info`, `room/check_alive`) respond through the
    /// same signing path, so the problem is neither signing nor replay.
    ///
    /// This exposes an account session to the signing component. With `sessionid`, the
    /// WebSocket also **requires** the cookie in the handshake or rejects with
    /// "illegal secret key".
    pub session: CookieJar,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            // On Linux WebView reports `navigator.platform = "Linux x86_64"`, and
            // `with_user_agent` does not change it. A Windows preset would mismatch
            // `browser_platform` and the UA, a leading rejection cause.
            preset: Preset {
                device: default_device_preset(),
                ..Preset::default()
            },
            landing_url: "https://www.tiktok.com/live".into(),
            sign_timeout: Duration::from_secs(15),
            sdk_ready_timeout: Duration::from_secs(30),
            contact_us: String::new(),
            visible: false,
            block_page_websockets: true,
            session: CookieJar::new(),
        }
    }
}

impl EngineConfig {
    /// Configure the session from a standalone `sessionid` cookie.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        let session_id = session_id.into();
        if !session_id.is_empty() {
            self.session.set(session::SESSION_COOKIE, session_id);
        }
        self
    }

    /// Login configuration: visible window, no existing session, and the login page.
    pub fn for_login() -> Self {
        Self {
            visible: true,
            landing_url: "https://www.tiktok.com/login".into(),
            session: CookieJar::new(),
            ..Self::default()
        }
    }

    /// Is a session configured?
    pub fn is_authenticated(&self) -> bool {
        session::is_logged_in(&self.session)
    }
}

/// Preset matching this platform's WebView engine.
fn default_device_preset() -> ttl_sign_core::DevicePreset {
    if cfg!(target_os = "linux") {
        ttl_sign_core::DevicePreset::chrome_linux()
    } else if cfg!(target_os = "macos") {
        ttl_sign_core::DevicePreset::chrome_macos()
    } else {
        ttl_sign_core::DevicePreset::chrome_windows()
    }
}

/// Work injected into the event loop from the Tokio runtime.
enum UserEvent {
    Sign {
        url: String,
        reply: oneshot::Sender<Result<SignedRequest, SignError>>,
    },
    /// Unsigned step 1. `url: None` requests the rendered DOM.
    Text {
        url: Option<String>,
        reply: oneshot::Sender<Result<String, SignError>>,
    },
    /// Current WebView cookies. Does not wait for the readiness gate: the login page may
    /// never load the SDK, while cookies are still readable.
    Cookies {
        reply: oneshot::Sender<CookieJar>,
    },
    /// Navigate to another URL and reset the readiness gate.
    Navigate {
        url: String,
        reply: oneshot::Sender<Result<(), SignError>>,
    },
    /// Subscribe to the WebSocket already owned by TikTok's page.
    SubscribePageWebSocket {
        reply: oneshot::Sender<mpsc::UnboundedReceiver<PageWebSocketEvent>>,
    },
    /// Wake the event loop from the IPC handler.
    ///
    /// The handler runs outside the event-loop closure. When it marks the instance ready,
    /// queued requests need an event to drain; without this they remain until expiry.
    Wake,
    Shutdown {
        code: i32,
    },
}

/// Event mirrored from the WebSocket opened and managed by TikTok's own page.
///
/// This path needs no external sign server: the page owns signing, room entry, and
/// heartbeat behavior; Rust receives a copy of each transport event over IPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageWebSocketEvent {
    Open {
        url: String,
    },
    Binary {
        url: String,
        data: Vec<u8>,
    },
    Text {
        url: String,
        text: String,
    },
    Close {
        url: String,
        code: u16,
        reason: String,
    },
    Error {
        url: String,
        message: String,
    },
}

/// One decoded event from TikTok's page-owned webcast WebSocket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLiveEvent {
    pub socket_url: String,
    pub frame_log_id: u64,
    pub message_id: u64,
    pub method: String,
    pub event: LiveEvent,
}

/// Page result: an already signed URL and the cookies used to sign it. Rust fetches the body
/// afterward by repeating the request.
#[derive(Debug, Clone)]
pub struct SignedRequest {
    /// URL with X-Bogus, X-Gnarly, X-Dynosaur, and msToken added by webmssdk.
    pub url: String,
    pub cookies: CookieJar,
    pub user_agent: String,
    /// Page that produced the signature. Sent as `Referer` when replaying.
    pub page_url: String,
}

/// A `webcast` request observed by the bridge, with its response.
///
/// Recorded by the `fetch`/XHR wrapper installed **before** webmssdk, so the URL is already
/// signed by the SDK and the body is what the page received.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Capture {
    pub url: String,
    #[serde(default)]
    pub status: u16,
    /// `Response.type`: `cors` and `basic` are readable; `opaque` is not.
    #[serde(default, rename = "kind")]
    pub response_type: String,
    /// `fetch` or `xhr`.
    #[serde(default)]
    pub via: String,
    #[serde(default)]
    pub bytes: usize,
    /// Base64 body. Empty when it could not be read.
    #[serde(default)]
    pub body: String,
    /// Plain text body when short and not stored in `body`.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl Capture {
    /// Decoded body. Empty when the bridge could not read it.
    pub fn decoded(&self) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(&self.body)
            .unwrap_or_default()
    }

    pub fn endpoint(&self) -> &str {
        self.url.split('?').next().unwrap_or(&self.url)
    }
}

/// Async engine handle. Cloneable and `Send`; this is what the HTTP server sees.
#[derive(Clone)]
pub struct Signer {
    proxy: EventLoopProxy<UserEvent>,
    config: EngineConfig,
    http: reqwest::Client,
    /// The **live** preset: configuration fixes the device (and its immutable WebView UA),
    /// while the page corrects language, time zone, screen, and region when ready.
    preset: Arc<Mutex<Preset>>,
}

impl Signer {
    /// Sign and execute `/webcast/im/fetch/` for a room.
    ///
    /// Does not return `Result`: the three outcomes are represented by [`SignOutcome`], and
    /// detection rejection is never confused with a transport error.
    pub async fn fetch(&self, room_id: impl Into<String>) -> SignOutcome {
        let mut params = FetchParams::new(room_id);
        params.contact_us = self.config.contact_us.clone();
        self.fetch_with(params).await
    }

    /// Like [`Signer::fetch`], with full control over parameters (`cursor`,
    /// `internal_ext`).
    pub async fn fetch_with(&self, params: FetchParams) -> SignOutcome {
        let preset = self.preset();
        match self.sign_url(&params.url(&preset)).await {
            Ok(signed) => self.replay(signed).await,
            Err(e) => e.into(),
        }
    }

    /// Sign a TikTok LIVE URL and return it signed, **without executing it**.
    ///
    /// This corresponds to Euler's optional `GET /webcast/sign_url` and is useful for
    /// comparing endpoints when one stops responding.
    pub async fn sign_url(&self, url: &str) -> Result<SignedRequest, SignError> {
        let (tx, rx) = oneshot::channel();
        self.proxy
            .send_event(UserEvent::Sign {
                url: url.to_string(),
                reply: tx,
            })
            .map_err(|_| SignError::EngineGone("event loop has closed".into()))?;

        match tokio::time::timeout(self.config.sign_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(SignError::EngineGone("engine discarded the request".into())),
            Err(_) => Err(SignError::Timeout(
                self.config.sign_timeout.as_millis() as u64
            )),
        }
    }

    /// Replay a signed request and return raw status and bytes without interpreting them.
    pub async fn replay_raw(&self, signed: &SignedRequest) -> Result<(u16, Vec<u8>), SignError> {
        let mut signed = signed.clone();

        // The query `msToken` and cookie `msToken` must match. When signing,
        // The browser request receives a new `x-ms-token`, so the WebView cookie has rotated
        // by the time we read it. Otherwise we would send a new cookie with an old signature.
        if let Some(query_token) = query_param(&signed.url, "msToken") {
            signed.cookies.set("msToken", query_token);
        }

        // Browser headers, not generic HTTP-client headers: mirror the real player,
        // including `Accept: */*` (not `application/protobuf`), the live-page `Referer`,
        // and client hints consistent with the User-Agent.
        let referer = if signed.page_url.is_empty() {
            "https://www.tiktok.com/".to_string()
        } else {
            signed.page_url.clone()
        };

        let preset = self.preset();
        let response = self
            .http
            .get(&signed.url)
            .header("User-Agent", &signed.user_agent)
            .header("Cookie", signed.cookies.to_cookie_string())
            .header("Accept", "*/*")
            .header("Accept-Language", &preset.location.browser_language)
            .header("Referer", &referer)
            .header("Origin", "https://www.tiktok.com")
            .header("Sec-Fetch-Site", "same-site")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Dest", "empty")
            .header(
                "sec-ch-ua",
                "\"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\"",
            )
            .header("sec-ch-ua-mobile", "?0")
            .header("sec-ch-ua-platform", platform_hint(&preset))
            .send()
            .await
            .map_err(|e| SignError::Transport(e.to_string()))?;

        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| SignError::Transport(e.to_string()))?
            .to_vec();
        Ok((status, bytes))
    }

    /// Replay a signed request and interpret the result.
    ///
    /// This is Plan B from `docs/01-architecture.md` §D2: the response from
    /// `/webcast/im/fetch/` cannot be read from the page, while the restriction does not
    /// exist outside the browser.
    async fn replay(&self, signed: SignedRequest) -> SignOutcome {
        debug!(
            // Without the query: it exposes msToken and X-Gnarly in clear text.
            endpoint = %signed.url.split('?').next().unwrap_or_default(),
            cookies = ?signed.cookies.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            "replaying signed request from Rust"
        );
        match self.replay_raw(&signed).await {
            Ok((status, body)) => build_outcome(
                status,
                &signed.url,
                body,
                signed.cookies,
                &signed.user_agent,
            ),
            Err(e) => e.into(),
        }
    }

    /// Current WebView cookies, including `HttpOnly` cookies.
    ///
    /// They contain the session; never print them in clear text.
    pub async fn cookies(&self) -> Result<CookieJar, SignError> {
        let (tx, rx) = oneshot::channel();
        self.proxy
            .send_event(UserEvent::Cookies { reply: tx })
            .map_err(|_| SignError::EngineGone("the event loop has closed".into()))?;
        rx.await
            .map_err(|_| SignError::EngineGone("engine discarded the request".into()))
    }

    /// Wait for a person to log in through the window, up to `timeout`.
    ///
    /// Poll WebView cookies for `sessionid`. Returns filtered session cookies ready for
    /// [`session::save`].
    ///
    /// `on_tick` receives remaining time on each poll, allowing callers to report progress
    /// without this crate printing anything.
    pub async fn wait_for_login(
        &self,
        timeout: Duration,
        mut on_tick: impl FnMut(Duration),
    ) -> Result<CookieJar, SignError> {
        let deadline = Instant::now() + timeout;
        loop {
            let jar = self.cookies().await?;
            if session::is_logged_in(&jar) {
                return Ok(session::filter(&jar));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SignError::LoginTimeout(timeout.as_secs()));
            }
            on_tick(remaining);
            tokio::time::sleep(POLL_LOGIN.min(remaining)).await;
        }
    }

    /// Navigate the WebView and wait for the bridge to become ready again.
    ///
    /// Required for signing: `/webcast/im/fetch/` only leaves from the live page where the
    /// real player sends it. From the `/live` landing page the patched `fetch` resolves to
    /// `undefined` and XHR has status 0, so no request is emitted.
    pub async fn navigate(&self, url: &str) -> Result<(), SignError> {
        let (tx, rx) = oneshot::channel();
        if self
            .proxy
            .send_event(UserEvent::Navigate {
                url: url.to_string(),
                reply: tx,
            })
            .is_err()
        {
            return Err(SignError::EngineGone("event loop has closed".into()));
        }
        match tokio::time::timeout(self.config.sdk_ready_timeout, rx).await {
            Ok(Ok(result)) => result?,
            Ok(Err(_)) => return Err(SignError::EngineGone("engine discarded navigation".into())),
            Err(_) => {
                return Err(SignError::Timeout(
                    self.config.sdk_ready_timeout.as_millis() as u64,
                ))
            }
        }
        // Every request waits for the gate; requesting the DOM is equivalent to waiting for
        // `ready`.
        self.dom().await.map(|_| ())
    }

    /// Resolve `unique_id` → `room_id` and live status. **Unsigned**.
    ///
    /// Runs from the page to reuse the real session and User-Agent, although the endpoint
    /// does not require them.
    pub async fn room_lookup(&self, unique_id: &str) -> Result<RoomLookup, SignError> {
        let body = self.fetch_text(&room::room_lookup_url(unique_id)).await?;
        RoomLookup::from_json(&body).ok_or_else(|| {
            SignError::Decode(format!("unexpected lookup response for @{unique_id}"))
        })
    }

    /// Live channels read from the current page's **rendered** DOM.
    ///
    /// The engine must be on a page that lists them (by default `https://www.tiktok.com/live`).
    /// A raw `GET` is insufficient: the client renders the data, so raw HTML is empty.
    ///
    /// Returns the list as-is: empty means the page has not rendered yet, not that nobody is
    /// live.
    pub async fn live_channels(&self) -> Result<Vec<LiveChannel>, SignError> {
        let dom = self.dom().await?;
        Ok(room::extract_live_channels(&dom))
    }

    /// Rendered DOM of the current page.
    pub async fn dom(&self) -> Result<String, SignError> {
        self.text_request(None).await
    }

    /// Sign a WebSocket URI.
    ///
    /// The real player's URI contains `X-Gnarly`. Without it the handshake is accepted
    /// (`101`, `handshake-msg: OK`) but no frames arrive, the hardest failure to diagnose.
    ///
    /// Sign by passing the **same query** through the HTTP request signer with an `https`
    /// scheme, then return `wss` with SDK-added parameters. Three paths, in order:
    ///
    /// 1. URI already built by the player on this page: exact reference and no artificial GET.
    /// 2. The `fetch` signer used by `/webcast/im/fetch/`, which adds `X-Gnarly`.
    /// 3. `byted_acrawler.frontierSign`, retained as a fallback even though it currently
    ///    returns only `X-Bogus`.
    pub async fn sign_ws_uri(&self, uri: &str) -> Result<String, SignError> {
        let (scheme, remainder) = match uri.split_once("://") {
            Some((scheme, remainder)) => (scheme, remainder),
            None => return Err(SignError::Decode(format!("URI has no scheme: {uri}"))),
        };

        // The page's constructor wrapper records the URL before optionally blocking the
        // connection, so this remains available even when Rust owns the only real socket.
        if let Some(page_uri) = self.page_ws_candidate(uri).await {
            debug!(
                endpoint = %uri.split('?').next().unwrap_or(uri),
                "reusing page-signed WebSocket URI"
            );
            return Ok(page_uri);
        }

        let as_https = format!("https://{remainder}");

        // 2. Request signer: it adds X-Gnarly.
        if let Ok(signed) = self.sign_url(&as_https).await {
            if signed.url.contains("X-Gnarly=") {
                let without_scheme = signed
                    .url
                    .split_once("://")
                    .map(|(_, remainder)| remainder)
                    .unwrap_or(&signed.url);
                return Ok(format!("{scheme}://{without_scheme}"));
            }
        }

        // 3. Fallback: the SDK's declared API.
        let url_json = serde_json::to_string(uri).map_err(|e| SignError::Decode(e.to_string()))?;
        let script = format!(
            "JSON.stringify(window.byted_acrawler.frontierSign({{ url: {url_json} }}) || {{}})"
        );
        let raw = self.eval(&script).await?;
        let params: std::collections::BTreeMap<String, String> = serde_json::from_str(&raw)
            .map_err(|e| {
                SignError::Decode(format!("frontierSign returned unexpected data: {e}"))
            })?;
        if params.is_empty() {
            return Err(SignError::Bridge(
                "neither the fetch signer nor frontierSign signed the WebSocket URI".into(),
            ));
        }

        let mut signed = uri.to_string();
        for (key, value) in params {
            signed.push('&');
            signed.push_str(&key);
            signed.push('=');
            signed.push_str(&urlencode(&value));
        }
        Ok(signed)
    }

    /// `webcast` requests recorded by the **page itself**.
    ///
    /// This is the reference truth: the real player signs with correct parameters and the
    /// correct session, separating a malformed request from TikTok rejection.
    pub async fn captures(&self) -> Result<Vec<Capture>, SignError> {
        let raw = self
            .eval("JSON.stringify(window.__ttlCaptures||[])")
            .await?;
        serde_json::from_str(&raw)
            .map_err(|e| SignError::Decode(format!("unreadable captures: {e}")))
    }

    /// WebSocket URIs opened by the page, in order.
    pub async fn page_ws_urls(&self) -> Result<Vec<String>, SignError> {
        let raw = self.eval("JSON.stringify(window.__ttlWsUrls||[])").await?;
        serde_json::from_str(&raw)
            .map_err(|e| SignError::Decode(format!("unreadable WebSocket list: {e}")))
    }

    async fn page_ws_candidate(&self, uri: &str) -> Option<String> {
        let expected_endpoint = uri.split_once('?').map(|(base, _)| base).unwrap_or(uri);
        let expected_room = query_param(uri, "room_id");
        for attempt in 0..10 {
            if let Ok(urls) = self.page_ws_urls().await {
                if let Some(page_uri) = urls.into_iter().rev().find(|page_uri| {
                    page_uri
                        .split_once('?')
                        .map(|(base, query)| {
                            let same_room = match (&expected_room, query_param(page_uri, "room_id"))
                            {
                                (Some(expected), Some(actual)) => {
                                    actual.as_str() == expected.as_str()
                                }
                                (None, _) => true,
                                _ => false,
                            };
                            base == expected_endpoint && same_room && query.contains("X-Gnarly=")
                        })
                        .unwrap_or(false)
                }) {
                    return Some(page_uri);
                }
            }
            if attempt != 9 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        None
    }

    /// Prevent the page from opening its own WebSockets from now on.
    ///
    /// Two connections to the same room/session interfere: the server accepts the second
    /// but sends no events. If Rust opens the WebSocket, the page connection is unnecessary.
    pub async fn block_page_websockets(&self) -> Result<(), SignError> {
        self.eval("(window.__ttlBlockWs=true,'ok')")
            .await
            .map(|_| ())
    }

    /// Evaluate a JavaScript expression in the page and return text.
    ///
    /// Diagnostic tool for page requests and SDK symbols. It does not participate in signing.
    pub async fn eval(&self, js: &str) -> Result<String, SignError> {
        self.text_request(Some(format!("js:{js}"))).await
    }

    /// Text GET from inside the page with its session and User-Agent.
    pub async fn fetch_text(&self, url: &str) -> Result<String, SignError> {
        self.text_request(Some(url.to_string())).await
    }

    async fn text_request(&self, url: Option<String>) -> Result<String, SignError> {
        let (tx, rx) = oneshot::channel();
        if self
            .proxy
            .send_event(UserEvent::Text { url, reply: tx })
            .is_err()
        {
            return Err(SignError::EngineGone("event loop has closed".into()));
        }
        match tokio::time::timeout(self.config.sign_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(SignError::EngineGone("engine discarded the request".into())),
            Err(_) => Err(SignError::Timeout(
                self.config.sign_timeout.as_millis() as u64
            )),
        }
    }

    /// Preset used by this engine. The WebSocket must use the same User-Agent.
    ///
    /// Return a copy because the page can correct language, region, time zone, and screen
    /// when it emits `ready`; holding a Mutex reference would not be a safe worker API.
    pub fn preset(&self) -> Preset {
        self.preset
            .lock()
            .map(|preset| preset.clone())
            .unwrap_or_else(|_| self.config.preset.clone())
    }

    /// Subscribe to the transport opened by TikTok's page.
    ///
    /// Set [`EngineConfig::block_page_websockets`] to `false` before starting the engine.
    /// Subscribe before navigating to a live page so the initial `open` event is retained.
    pub async fn subscribe_page_websocket(
        &self,
    ) -> Result<mpsc::UnboundedReceiver<PageWebSocketEvent>, SignError> {
        let (tx, rx) = oneshot::channel();
        self.proxy
            .send_event(UserEvent::SubscribePageWebSocket { reply: tx })
            .map_err(|_| SignError::EngineGone("event loop has closed".into()))?;
        rx.await
            .map_err(|_| SignError::EngineGone("event loop discarded subscription".into()))
    }

    /// Subscribe to decoded TikTok LIVE events.
    ///
    /// The returned channel stays active across page-owned socket replacements. Unknown
    /// protobuf methods are emitted as [`LiveEvent::Unknown`] instead of being discarded.
    pub async fn subscribe_live_events(
        &self,
    ) -> Result<mpsc::UnboundedReceiver<Result<DecodedLiveEvent, SignError>>, SignError> {
        let mut page_events = self.subscribe_page_websocket().await?;
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut active_socket: Option<String> = None;
            while let Some(page_event) = page_events.recv().await {
                match page_event {
                    PageWebSocketEvent::Open { url } if is_webcast_socket(&url) => {
                        active_socket = Some(url);
                    }
                    PageWebSocketEvent::Binary { url, data }
                        if active_socket.as_deref() == Some(url.as_str()) =>
                    {
                        match decode_live_frame(&url, &data) {
                            Ok(events) => {
                                for event in events {
                                    if tx.send(Ok(event)).is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(error) => {
                                if tx.send(Err(error)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    PageWebSocketEvent::Close { url, code, reason }
                        if active_socket.as_deref() == Some(url.as_str()) =>
                    {
                        active_socket = None;
                        if tx
                            .send(Err(SignError::EngineGone(format!(
                                "page WebSocket closed: code={code} reason={reason}"
                            ))))
                            .is_err()
                        {
                            return;
                        }
                    }
                    PageWebSocketEvent::Error { url, message }
                        if active_socket.as_deref() == Some(url.as_str()) =>
                    {
                        if tx.send(Err(SignError::Transport(message))).is_err() {
                            return;
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(rx)
    }

    /// Request orderly event-loop shutdown.
    pub fn shutdown(&self) {
        self.shutdown_with_code(0);
    }

    /// Request orderly event-loop shutdown and propagate a process exit code.
    pub fn shutdown_with_code(&self, code: i32) {
        let _ = self.proxy.send_event(UserEvent::Shutdown { code });
    }
}

/// A signature waiting for the SDK.
type QueuedSign = (String, oneshot::Sender<Result<SignedRequest, SignError>>);

/// A text request waiting for the page.
type QueuedText = (Option<String>, oneshot::Sender<Result<String, SignError>>);

/// State shared by the event loop and IPC handler. Both run on the main thread, so
/// `Rc<RefCell<_>>` is sufficient and avoids locks.
#[derive(Default)]
struct Shared {
    next_id: u64,
    /// In-flight signing requests by `request_id`.
    pending: HashMap<u64, oneshot::Sender<Result<SignedRequest, SignError>>>,
    /// In-flight text requests (unsigned step 1).
    pending_text: HashMap<u64, oneshot::Sender<Result<String, SignError>>>,
    /// Requests received before the SDK was ready.
    queued: Vec<QueuedSign>,
    /// Likewise for text: `ready` also means the document loaded, which is required to read
    /// its DOM.
    queued_text: Vec<QueuedText>,
    ready: bool,
    /// When the readiness gate started. Reset on each navigation.
    gate_started: Option<Instant>,
    sdk_version: Option<String>,
    signs: u64,
    /// Cookies visible to the document. WebKitGTK may omit them from the cookie manager
    /// even though page requests carry them.
    page_cookies: CookieJar,
    /// Initial `robots.txt` navigation used so the bridge can install cookies at the TikTok
    /// origin. While pending, bootstrap-document `ready` does not authorize signing.
    session_bootstrap_pending: bool,
    session_bootstrap_ready: bool,
    /// Consumers observing the WebSocket managed by TikTok's page.
    page_ws_subscribers: Vec<mpsc::UnboundedSender<PageWebSocketEvent>>,
}

impl Shared {
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn resolve(&mut self, request_id: u64, result: Result<SignedRequest, SignError>) {
        match self.pending.remove(&request_id) {
            Some(tx) => {
                let _ = tx.send(result);
            }
            None => debug!(request_id, "result has no in-flight request; discarded"),
        }
    }

    fn resolve_text(&mut self, request_id: u64, result: Result<String, SignError>) {
        match self.pending_text.remove(&request_id) {
            Some(tx) => {
                let _ = tx.send(result);
            }
            None => debug!(request_id, "text has no in-flight request; discarded"),
        }
    }

    fn publish_page_websocket(&mut self, event: PageWebSocketEvent) {
        self.page_ws_subscribers
            .retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }

    /// Fail everything waiting. Used when the SDK does not start.
    fn fail_all(&mut self, make_error: impl Fn() -> SignError) {
        for (_, tx) in self.pending.drain() {
            let _ = tx.send(Err(make_error()));
        }
        for (_, tx) in self.queued.drain(..) {
            let _ = tx.send(Err(make_error()));
        }
        for (_, tx) in self.pending_text.drain() {
            let _ = tx.send(Err(make_error()));
        }
        for (_, tx) in self.queued_text.drain(..) {
            let _ = tx.send(Err(make_error()));
        }
    }
}

/// Start the engine. **It owns the main thread and does not return.**
///
/// `worker` runs on another thread and receives [`Signer`]; this is where the Tokio runtime
/// lives (HTTP server, custom client, or anything else).
pub fn run<F>(config: EngineConfig, worker: F) -> !
where
    F: FnOnce(Signer) + Send + 'static,
{
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(if config.visible {
            "Log in to TikTok — this window closes after login"
        } else {
            "ttl-sign-webview"
        })
        .with_visible(config.visible)
        .build(&event_loop)
        .expect("could not create window (is a display available? otherwise use Xvfb)");

    let shared = Rc::new(RefCell::new(Shared::default()));
    let user_agent = config.preset.user_agent();
    let preset = Arc::new(Mutex::new(config.preset.clone()));
    shared.borrow_mut().session_bootstrap_pending = !config.session.is_empty();

    let ipc_shared = Rc::clone(&shared);
    let ipc_ua = user_agent.clone();
    let ipc_preset = Arc::clone(&preset);
    let ipc_proxy = proxy.clone();
    // The WebView does not exist when the handler is built, but the handler needs to read
    // the cookie manager; share it through a cell filled immediately afterward.
    let webview_slot: Rc<RefCell<Option<Rc<wry::WebView>>>> = Rc::new(RefCell::new(None));
    let ipc_slot = Rc::clone(&webview_slot);

    // With a session, the first document is only a cookie bootstrap. Its response can be
    // anonymous, but its document-start script runs on the correct TikTok origin; after it
    // reports `session`, the event loop navigates to the real landing URL.
    let session_script =
        session_initialization_script(&config.session, config.block_page_websockets);
    let initial_url = if config.session.is_empty() {
        config.landing_url.as_str()
    } else {
        SESSION_BOOTSTRAP_URL
    };
    let builder = WebViewBuilder::new()
        .with_url(initial_url)
        .with_user_agent(&user_agent)
        .with_initialization_script(session_script)
        .with_initialization_script(BRIDGE_JS)
        .with_ipc_handler(move |req| {
            let body = req.body().as_str();
            let msg: FromPage = match serde_json::from_str(body) {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "unreadable IPC message");
                    return;
                }
            };
            let jar_from_webview = ipc_slot
                .borrow()
                .as_ref()
                .map(|wv| cookies_from_webview(wv))
                .unwrap_or_default();
            handle_page_message(&ipc_shared, msg, &ipc_ua, jar_from_webview, &ipc_preset);
            // The handler is not the event loop; wake it so it can drain the queue.
            let _ = ipc_proxy.send_event(UserEvent::Wake);
        });

    let webview = build_webview(builder, &window).expect("could not create WebView");

    let webview = Rc::new(webview);
    *webview_slot.borrow_mut() = Some(Rc::clone(&webview));

    if !config.session.is_empty() {
        info!(
            cookies = config.session.len(),
            "session prepared for bootstrap document"
        );
    }

    std::thread::spawn({
        let signer = Signer {
            proxy,
            config: config.clone(),
            http: http_client(&user_agent),
            preset: Arc::clone(&preset),
        };
        move || worker(signer)
    });

    shared.borrow_mut().gate_started = Some(Instant::now());
    let sdk_ready_timeout = config.sdk_ready_timeout;
    let landing_url = config.landing_url.clone();

    event_loop.run(move |event, _target, control_flow| {
        // Wait wakes on every event; check the SDK deadline on every pass.
        *control_flow = ControlFlow::Wait;

        {
            let mut state = shared.borrow_mut();
            let waiting = !state.queued.is_empty() || !state.queued_text.is_empty();
            let expired = state
                .gate_started
                .is_some_and(|t| t.elapsed() > sdk_ready_timeout);
            if !state.ready && waiting && expired {
                error!("SDK did not appear within {sdk_ready_timeout:?}");
                state.fail_all(|| SignError::SdkNotReady);
            }
        }

        match event {
            Event::UserEvent(UserEvent::Sign { url, reply }) => {
                let mut state = shared.borrow_mut();
                if !state.ready {
                    debug!("request queued: SDK is not ready");
                    state.queued.push((url, reply));
                    return;
                }
                let id = state.next_id();
                state.pending.insert(id, reply);
                state.signs += 1;
                drop(state);
                dispatch(&webview, &shared, id, url);
            }
            Event::UserEvent(UserEvent::Text { url, reply }) => {
                let mut state = shared.borrow_mut();
                if !state.ready {
                    debug!("text request queued: page is not ready");
                    state.queued_text.push((url, reply));
                    return;
                }
                let id = state.next_id();
                state.pending_text.insert(id, reply);
                drop(state);
                dispatch_text(&webview, &shared, id, url);
            }
            Event::UserEvent(UserEvent::Cookies { reply }) => {
                let mut jar = cookies_from_webview(&webview);
                jar.merge(&shared.borrow().page_cookies);
                let _ = reply.send(jar);
            }
            Event::UserEvent(UserEvent::Navigate { url, reply }) => {
                {
                    let mut state = shared.borrow_mut();
                    state.ready = false;
                    state.gate_started = Some(Instant::now());
                }
                info!(%url, "navigating");
                let result = webview
                    .load_url(&url)
                    .map_err(|e| SignError::EngineGone(e.to_string()));
                let _ = reply.send(result);
            }
            Event::UserEvent(UserEvent::SubscribePageWebSocket { reply }) => {
                let (tx, rx) = mpsc::unbounded_channel();
                shared.borrow_mut().page_ws_subscribers.push(tx);
                let _ = reply.send(rx);
            }
            // Only used to reach the queue-draining logic below.
            Event::UserEvent(UserEvent::Wake) => {}
            Event::UserEvent(UserEvent::Shutdown { code }) => {
                shared
                    .borrow_mut()
                    .fail_all(|| SignError::EngineGone("shutdown requested".into()));
                *control_flow = ControlFlow::ExitWithCode(code);
            }
            _ => {}
        }

        // Do this on the event-loop thread, never from the IPC callback. The callback has
        // already observed the cookies, while this navigation is the first request that
        // can carry them in its HTTP Cookie header.
        let navigate_after_bootstrap = {
            let mut state = shared.borrow_mut();
            if state.session_bootstrap_pending && state.session_bootstrap_ready {
                state.session_bootstrap_pending = false;
                state.session_bootstrap_ready = false;
                state.ready = false;
                state.gate_started = Some(Instant::now());
                true
            } else {
                false
            }
        };
        if navigate_after_bootstrap {
            info!(url = %landing_url, "session installed; navigating to real page");
            if let Err(e) = webview.load_url(&landing_url) {
                error!(error = %e, "could not load authenticated page");
                shared
                    .borrow_mut()
                    .fail_all(|| SignError::EngineGone(e.to_string()));
            }
        }

        // The bridge may have emitted `ready` from the IPC handler while requests were
        // queued; drain them here.
        let to_dispatch: Vec<(u64, String)> = {
            let mut state = shared.borrow_mut();
            if !state.ready || state.queued.is_empty() {
                Vec::new()
            } else {
                let queued: Vec<_> = state.queued.drain(..).collect();
                let mut out = Vec::with_capacity(queued.len());
                for (url, reply) in queued {
                    let id = state.next_id();
                    state.pending.insert(id, reply);
                    state.signs += 1;
                    out.push((id, url));
                }
                out
            }
        };
        for (id, url) in to_dispatch {
            dispatch(&webview, &shared, id, url);
        }

        let text_to_dispatch: Vec<(u64, Option<String>)> = {
            let mut state = shared.borrow_mut();
            if !state.ready || state.queued_text.is_empty() {
                Vec::new()
            } else {
                let queued: Vec<_> = state.queued_text.drain(..).collect();
                let mut out = Vec::with_capacity(queued.len());
                for (url, reply) in queued {
                    let id = state.next_id();
                    state.pending_text.insert(id, reply);
                    out.push((id, url));
                }
                out
            }
        };
        for (id, url) in text_to_dispatch {
            dispatch_text(&webview, &shared, id, url);
        }
    })
}

/// Prepare cookie pairs for the bridge to install at the TikTok origin.
///
/// With WebKitGTK, `WebView::set_cookie` may remain in the cookie manager without affecting
/// the first navigation. The initialization script assigns them at the same origin before
/// TikTok's bootstrap, so cookies travel in SSR and later XHR requests.
fn session_initialization_script(jar: &CookieJar, block_page_websockets: bool) -> String {
    let pairs: Vec<(&str, &str)> = jar.iter().collect();
    let json = serde_json::to_string(&pairs).expect("cookies always serialize");
    format!("window.__ttlSession = {json}; window.__ttlBlockWs = {block_page_websockets};")
}

/// On Linux the window is invisible, so GTK never realizes it and `build()` has no window
/// handle, returning `WindowHandleError(Unavailable)`. Put the WebView directly in the
/// window's GTK container instead.
#[cfg(target_os = "linux")]
fn build_webview(
    builder: WebViewBuilder<'_>,
    window: &tao::window::Window,
) -> wry::Result<wry::WebView> {
    use tao::platform::unix::WindowExtUnix;
    use wry::WebViewBuilderExtUnix;

    let vbox = window
        .default_vbox()
        .expect("tao always creates a GTK vbox");
    builder.build_gtk(vbox)
}

#[cfg(not(target_os = "linux"))]
fn build_webview(
    builder: WebViewBuilder<'_>,
    window: &tao::window::Window,
) -> wry::Result<wry::WebView> {
    builder.build(window)
}

/// Dispatch a signature inside the page.
fn dispatch(webview: &wry::WebView, shared: &Rc<RefCell<Shared>>, request_id: u64, url: String) {
    let msg = ToPage { request_id, url };
    debug!(request_id, "signature requested from page");
    if let Err(e) = webview.evaluate_script(&msg.to_script()) {
        error!(request_id, error = %e, "could not inject signature");
        shared
            .borrow_mut()
            .resolve(request_id, Err(SignError::EngineGone(e.to_string())));
    }
}

/// Dispatch a text request (or DOM request when `url` is `None`) inside the page.
fn dispatch_text(
    webview: &wry::WebView,
    shared: &Rc<RefCell<Shared>>,
    request_id: u64,
    url: Option<String>,
) {
    let msg = ToPageText { request_id, url };
    if let Err(e) = webview.evaluate_script(&msg.to_script()) {
        error!(request_id, error = %e, "could not inject text request");
        shared
            .borrow_mut()
            .resolve_text(request_id, Err(SignError::EngineGone(e.to_string())));
    }
}

/// Cookies from WebKit's cookie manager, which **does** include `HttpOnly`; `document.cookie`
/// cannot see them.
fn cookies_from_webview(webview: &wry::WebView) -> CookieJar {
    match webview.cookies() {
        Ok(cookies) => cookies
            .iter()
            .map(|c| (c.name().to_string(), c.value().to_string()))
            .collect(),
        Err(e) => {
            warn!(error = %e, "could not read WebView cookies");
            CookieJar::new()
        }
    }
}

/// Translate a bridge message into [`SignOutcome`] and resolve its waiter.
fn handle_page_message(
    shared: &Rc<RefCell<Shared>>,
    msg: FromPage,
    user_agent: &str,
    jar_from_webview: CookieJar,
    preset: &Arc<Mutex<Preset>>,
) {
    match msg {
        FromPage::Ready { sdk_version, env } => {
            if let Some(env) = env {
                apply_page_env(preset, &env);
            }
            let mut state = shared.borrow_mut();
            // `robots.txt` has no SDK of its own, but keep this guard in case the bootstrap
            // document happens to expose one through a cached page.
            state.ready = !state.session_bootstrap_pending;
            state.sdk_version = sdk_version.clone();
            info!(sdk_version = ?sdk_version, "bridge ready");
        }
        FromPage::Session {
            installed,
            host,
            cookie,
        } => {
            let mut state = shared.borrow_mut();
            let document_cookies = CookieJar::parse(&cookie);
            let has_session = session::is_logged_in(&document_cookies);
            state.page_cookies.merge(&document_cookies);
            if state.session_bootstrap_pending {
                state.session_bootstrap_ready =
                    installed != 0 && has_session && host.ends_with("tiktok.com");
            }
            debug!(installed, %host, "session cookies offered to document");
        }
        FromPage::Text {
            request_id,
            status,
            body,
        } => {
            let result = if status == 200 {
                Ok(body)
            } else {
                // Without signing, a non-200 here is a normal step-1 failure, not detection;
                // do not map it to `SignOutcome::Rejected`.
                Err(SignError::Transport(format!(
                    "unsigned step returned HTTP {status}"
                )))
            };
            shared.borrow_mut().resolve_text(request_id, result);
        }
        FromPage::WsOpen { url } => {
            let endpoint = url.split('?').next().unwrap_or(&url);
            info!(%endpoint, "page WebSocket opened");
            shared
                .borrow_mut()
                .publish_page_websocket(PageWebSocketEvent::Open { url });
        }
        FromPage::WsFrame {
            url,
            data_b64,
            text,
        } => {
            let event = if !data_b64.is_empty() {
                use base64::Engine;
                match base64::engine::general_purpose::STANDARD.decode(data_b64) {
                    Ok(data) => PageWebSocketEvent::Binary { url, data },
                    Err(error) => PageWebSocketEvent::Error {
                        url,
                        message: format!("invalid base64 WebSocket frame: {error}"),
                    },
                }
            } else {
                PageWebSocketEvent::Text { url, text }
            };
            shared.borrow_mut().publish_page_websocket(event);
        }
        FromPage::WsClose { url, code, reason } => {
            let endpoint = url.split('?').next().unwrap_or(&url);
            info!(%endpoint, code, %reason, "page WebSocket closed");
            shared
                .borrow_mut()
                .publish_page_websocket(PageWebSocketEvent::Close { url, code, reason });
        }
        FromPage::WsError { url, message } => {
            let endpoint = url.split('?').next().unwrap_or(&url);
            warn!(%endpoint, %message, "page WebSocket error");
            shared
                .borrow_mut()
                .publish_page_websocket(PageWebSocketEvent::Error { url, message });
        }
        FromPage::Error {
            request_id,
            message,
        } => {
            if request_id == 0 {
                // Not a signing response: this is the readiness gate.
                error!(%message, "bridge failed before becoming ready");
                let mut state = shared.borrow_mut();
                state.ready = false;
                state.fail_all(|| SignError::SdkNotReady);
            } else {
                // It may belong to a signature or text request: only one of the two
                // Only one pending map can own this request ID.
                let mut state = shared.borrow_mut();
                if state.pending_text.contains_key(&request_id) {
                    state.resolve_text(request_id, Err(SignError::Bridge(message)));
                } else {
                    state.resolve(request_id, Err(SignError::Bridge(message)));
                }
            }
        }
        FromPage::Signed {
            request_id,
            url,
            cookie,
            page,
        } => {
            // The manager supplies HttpOnly cookies; document cookies are merged afterward
            // because they reflect the latest `msToken` and session rotation.
            // The document cookie string reflects values injected into WebKitGTK.
            let mut cookies = jar_from_webview;
            cookies.merge(&CookieJar::parse(&cookie));
            let signed = SignedRequest {
                url,
                cookies,
                user_agent: user_agent.to_string(),
                page_url: page,
            };
            shared.borrow_mut().resolve(request_id, Ok(signed));
        }
    }
}

fn is_webcast_socket(url: &str) -> bool {
    url.starts_with("wss://") && (url.contains("/webcast/im/") || url.contains("webcast-ws"))
}

fn decode_live_frame(url: &str, data: &[u8]) -> Result<Vec<DecodedLiveEvent>, SignError> {
    let frame = PushFrame::decode(data).map_err(|error| SignError::Decode(error.to_string()))?;
    if !frame.is_message() {
        return Ok(Vec::new());
    }

    let payload =
        if frame.compress_type() == Some("gzip") || frame.payload.starts_with(&[0x1f, 0x8b]) {
            let mut decoder = GzDecoder::new(frame.payload.as_slice());
            let mut decoded = Vec::new();
            decoder
                .read_to_end(&mut decoded)
                .map_err(|error| SignError::Decode(format!("gzip event payload: {error}")))?;
            decoded
        } else {
            frame.payload
        };

    let batch = WebcastEventBatch::decode(&payload)
        .map_err(|error| SignError::Decode(format!("event batch: {error}")))?;
    batch
        .messages
        .into_iter()
        .map(|message| {
            let event = message
                .decode_event()
                .map_err(|error| SignError::Decode(format!("{}: {error}", message.method)))?;
            Ok(DecodedLiveEvent {
                socket_url: url.to_owned(),
                frame_log_id: frame.log_id,
                message_id: message.message_id,
                method: message.method,
                event,
            })
        })
        .collect()
}

/// Align parameters signed by Rust with the environment seen by the real WebView.
fn apply_page_env(preset: &Arc<Mutex<Preset>>, env: &PageEnv) {
    let Ok(mut current) = preset.lock() else {
        warn!("could not update preset from page environment");
        return;
    };

    if !env.language.is_empty() {
        current.location.language = env.language.clone();
    }
    if !env.browser_language.is_empty() {
        current.location.browser_language = env.browser_language.clone();
    }
    if !env.tz_name.is_empty() {
        current.location.tz_name = env.tz_name.clone();
    }
    if !env.region.is_empty() {
        current.location.region = env.region.clone();
    }
    if env.screen_width != 0 {
        current.screen.width = env.screen_width;
    }
    if env.screen_height != 0 {
        current.screen.height = env.screen_height;
    }
}

/// Classify the response replayed by Rust.
///
/// The critical distinction: a 200 with an empty body or empty `push_server` is **rejection**,
/// not a transient error.
fn build_outcome(
    status: u16,
    url: &str,
    protobuf: Vec<u8>,
    cookies: CookieJar,
    user_agent: &str,
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
        Err(e) => return SignError::Decode(e.to_string()).into(),
    }

    SignOutcome::Ok(SignedFetch {
        protobuf,
        cookies,
        user_agent: user_agent.to_string(),
        signed_url: url.to_string(),
    })
}

/// Query parameter value, decoded as needed.
fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| {
            v.replace("%2B", "+")
                .replace("%2F", "/")
                .replace("%3D", "=")
        })
}

/// Percent-encode a query value. Signature values contain `/`, `+`, and `=`.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `sec-ch-ua-platform` value consistent with the preset. A mismatch with the UA is another
/// inconsistency TikTok checks.
fn platform_hint(preset: &Preset) -> &'static str {
    match preset.device.os.as_str() {
        "windows" => "\"Windows\"",
        "mac" => "\"macOS\"",
        _ => "\"Linux\"",
    }
}

/// HTTP client for replaying signed requests.
///
/// No automatic redirects: a 3xx would mean TikTok sent us elsewhere, and following it
/// would mask rejection.
fn http_client(user_agent: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(user_agent.to_string())
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .expect("HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_sign_core::proto::Writer;

    fn valid_protobuf() -> Vec<u8> {
        let mut w = Writer::new();
        w.str_field(2, "cursor")
            .map_entry(7, "wss_push_room_id", "1")
            .str_field(
                10,
                "wss://webcast5-ws-web-useast1a.tiktok.com/webcast/im/ws/",
            );
        w.finish()
    }

    #[test]
    fn query_param_is_extracted_and_decoded() {
        let url = "https://x/?a=1&msToken=ab%2Bcd%3D&z=2";
        assert_eq!(query_param(url, "msToken").as_deref(), Some("ab+cd="));
        assert_eq!(query_param(url, "a").as_deref(), Some("1"));
        assert_eq!(query_param(url, "missing"), None);
        assert_eq!(query_param("https://x/no-query", "a"), None);
    }

    #[test]
    fn non_200_is_a_rejection() {
        let outcome = build_outcome(403, "", vec![1], CookieJar::new(), "UA");
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::HttpStatus(403))
        ));
    }

    #[test]
    fn empty_body_on_200_is_a_rejection_not_a_transport_error() {
        let outcome = build_outcome(200, "", Vec::new(), CookieJar::new(), "UA");
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyBody)
        ));
        assert!(!outcome.is_retryable());
    }

    #[test]
    fn empty_push_server_on_200_is_a_rejection() {
        let body = Writer::new().str_field(2, "cursor").clone().finish();
        let outcome = build_outcome(200, "", body, CookieJar::new(), "UA");
        assert!(matches!(
            outcome,
            SignOutcome::Rejected(RejectReason::EmptyPushServer)
        ));
    }

    #[test]
    fn a_valid_response_carries_context_for_the_websocket() {
        let outcome = build_outcome(
            200,
            "https://webcast.tiktok.com/webcast/im/fetch/?X-Gnarly=K",
            valid_protobuf(),
            CookieJar::parse("msToken=abc; ttwid=xyz"),
            "test-UA",
        );
        let signed = outcome.ok().expect("expected a valid signature");
        // The WebSocket needs exactly these cookies and this UA, not different ones.
        assert_eq!(signed.cookies.get("ttwid"), Some("xyz"));
        assert_eq!(signed.user_agent, "test-UA");
        assert!(signed.signed_url.contains("X-Gnarly"));
    }

    #[test]
    fn page_websocket_events_are_published_to_subscribers() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut shared = Shared::default();
        shared.page_ws_subscribers.push(tx);
        let event = PageWebSocketEvent::Binary {
            url: "wss://example.test/webcast/im/".into(),
            data: vec![1, 2, 3],
        };

        shared.publish_page_websocket(event.clone());

        assert_eq!(rx.try_recv().unwrap(), event);
    }

    #[test]
    fn decodes_events_from_a_page_websocket_frame() {
        let user = Writer::new()
            .u64_field(1, 7)
            .str_field(3, "Grace")
            .str_field(38, "grace_live")
            .clone()
            .finish();
        let chat = Writer::new()
            .bytes_field(2, &user)
            .str_field(3, "hello from live")
            .clone()
            .finish();
        let message = Writer::new()
            .str_field(1, "WebcastChatMessage")
            .bytes_field(2, &chat)
            .u64_field(3, 11)
            .clone()
            .finish();
        let batch = Writer::new().bytes_field(1, &message).clone().finish();
        let frame = PushFrame {
            log_id: 5,
            payload_encoding: "pb".into(),
            payload_type: "msg".into(),
            payload: batch,
            ..Default::default()
        };

        let events = decode_live_frame("wss://example.test/webcast/im/", &frame.encode()).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].frame_log_id, 5);
        assert_eq!(events[0].message_id, 11);
        assert!(matches!(
            &events[0].event,
            LiveEvent::Chat { user, content }
                if user.display_id == "grace_live" && content == "hello from live"
        ));
    }
}
