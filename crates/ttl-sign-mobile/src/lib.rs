//! On-device signing for Android: the real bundle in QuickJS, behind a blocking API.
//!
//! This crate answers one question — can the desktop signing stack run on a phone? — by being
//! the listing of what survives the move:
//!
//! | Kept | Left behind | Why |
//! |---|---|---|
//! | `ttl-sign-core` (pure query/URL construction) | `ttl-live-discovery` (`reqwest` + `tokio`) | HTTP on Android is `HttpURLConnection`/OkHttp; shipping a second TLS stack in the `.so` buys nothing |
//! | `bootstrap.js` (the sandbox, by relative include) | `ttl-sign-embedded` (async signer) | JNI calls are blocking, so the `tokio` oneshot becomes a `std` channel |
//! | QuickJS via `rquickjs` | prebuilt C bindings | Android is not among them; the `bindgen` feature generates them from the NDK headers |
//!
//! The sandbox is literally the same source — [`BOOTSTRAP`] includes
//! `../ttl-sign-embedded/bootstrap.js` — so a signature produced here and one produced by the
//! server are comparable byte for byte under a pinned profile. `tests/mobile_sign.rs` asserts
//! that against a recorded vector.
//!
//! Three surfaces, narrow on purpose:
//!
//! - [`MobileSigner`] — blocking Rust API: `new(bundle, options)` once, `sign(url, product)` per
//!   request. The engine lives on its own thread, as in `ttl-sign-embedded`.
//! - [`ffi`] — C ABI (`ttl_mobile_*`) for callers that cannot speak JNI.
//! - `jni` (feature) — `TtlNative` static natives, the path the demo app uses.
//!
//! URL construction stays in Rust too ([`direct_socket_url`], [`room_lookup_url`]), so Kotlin
//! never reimplements the parameter order `registerWsSigner` signs over.

use std::str::FromStr;
use std::sync::mpsc;
use std::thread;

use ttl_sign_core::{DevicePreset, LocationPreset, Preset, ScreenPreset};

pub mod ffi;

#[cfg(feature = "jni")]
pub mod jni;
#[cfg(all(feature = "jni", feature = "live"))]
pub mod jni_live;
#[cfg(feature = "live")]
pub mod live;

/// The sandbox, flattened into one script. Not a copy: this is
/// `crates/ttl-sign-embedded/bootstrap.js` itself, which is generated from
/// `scripts/headless/shim.mjs`. One sandbox in the repository, not two.
pub(crate) const BOOTSTRAP: &str = include_str!("../../ttl-sign-embedded/bootstrap.js");

/// The most entropy an engine hands the sandbox in one draw. Same ceiling as
/// `ttl-sign-embedded`: bundle 1.0.0.388 draws nothing, so this bounds a request
/// that should never arrive.
pub(crate) const MAX_RANDOM_BYTES: usize = 8192;

mod engine;

/// Which signature to produce. The socket verifies `ws` and ignores the other two —
/// same three products as `scripts/headless/sign-url.mjs`, minus the dependency on
/// `ttl-live-discovery` (whose `SigningProduct` would drag `reqwest` into the `.so`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    /// The patched-fetch suffix (`X-Dynosaur`, `msToken`, `X-Bogus`, `X-Gnarly`).
    Fetch,
    /// The public `frontierSign` product, a real 16-byte `X-Bogus`.
    Frontier,
    /// `registerWsSigner` over the query bytes, appended as `X-Gnarly`.
    Ws,
}

impl Product {
    /// The driver's argument. Same three names the reference driver takes.
    pub fn as_driver_arg(self) -> &'static str {
        match self {
            Product::Fetch => "fetch",
            Product::Frontier => "frontier",
            Product::Ws => "ws",
        }
    }
}

impl FromStr for Product {
    type Err = MobileError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "fetch" => Ok(Product::Fetch),
            "frontier" => Ok(Product::Frontier),
            "ws" => Ok(Product::Ws),
            other => Err(MobileError::Product(other.to_string())),
        }
    }
}

/// Environment the sandbox reports while signing. Deserialized from the JSON object
/// Kotlin passes to [`MobileSigner::new`]; unknown fields are ignored so older apps
/// keep working when a field is added.
#[derive(Debug, Clone, Default)]
pub struct SignOptions {
    pub user_agent: Option<String>,
    pub cookie: Option<String>,
    /// The stored `xmst` token. `msToken` is a verbatim passthrough of it.
    pub stored_token: Option<String>,
    /// Freeze the clock, `performance`, `Math.random` and the entropy sequence.
    /// For tests and differentials only: a repeating signature is not one a browser
    /// would produce.
    pub pinned: bool,
    /// The longest one call into the engine may run — loading the bundle, or one signature —
    /// before it is interrupted. Defaults to [`DEFAULT_TIMEOUT`]. Not passed to the driver.
    pub timeout_ms: Option<u64>,
}

/// How long one engine call may run when the options name no `timeoutMs`.
pub const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl SignOptions {
    /// Parse the options JSON. Accepts the same shape `Profile::to_json` writes
    /// (`pinned`, `userAgent`, `cookie`, `xmst`) plus `storedToken` as an alias.
    pub fn from_json(json: &str) -> Result<Self, MobileError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| MobileError::Options(e.to_string()))?;
        let text = |keys: &[&str]| {
            keys.iter()
                .filter_map(|k| value.get(*k).and_then(|v| v.as_str()))
                .next()
                .map(str::to_string)
        };
        Ok(Self {
            user_agent: text(&["userAgent", "user_agent"]),
            cookie: text(&["cookie"]),
            stored_token: text(&["xmst", "storedToken", "stored_token"]),
            pinned: value
                .get("pinned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            timeout_ms: match ["timeoutMs", "timeout_ms"]
                .iter()
                .find_map(|k| value.get(*k))
            {
                None => None,
                Some(v) => match v.as_u64() {
                    Some(ms @ 1..=600_000) => Some(ms),
                    _ => {
                        return Err(MobileError::Options(
                            "timeoutMs must be an integer in 1..=600000".into(),
                        ))
                    }
                },
            },
        })
    }

    /// The per-call engine deadline these options ask for.
    pub fn timeout(&self) -> std::time::Duration {
        self.timeout_ms
            .map_or(DEFAULT_TIMEOUT, std::time::Duration::from_millis)
    }

    /// Re-encode for the driver. The driver reads `userAgent`/`cookie`/`xmst`/`pinned`.
    fn to_driver_json(&self) -> String {
        let field = |name: &str, value: &Option<String>| match value {
            Some(value) => format!(",\"{name}\":\"{}\"", escape(value)),
            None => String::new(),
        };
        format!(
            "{{\"pinned\":{}{}{}{}}}",
            self.pinned,
            field("userAgent", &self.user_agent),
            field("cookie", &self.cookie),
            field("xmst", &self.stored_token),
        )
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MobileError {
    #[error("the engine could not start: {0}")]
    Engine(String),
    #[error("the bundle could not be prepared: {0}")]
    Bundle(String),
    #[error("signing failed: {0}")]
    Sign(String),
    #[error("unknown product {0:?}: expected fetch, frontier, or ws")]
    Product(String),
    #[error("options are not valid JSON: {0}")]
    Options(String),
    #[error("the signer thread stopped")]
    Stopped,
}

/// A signature request for the worker thread.
struct Request {
    deadline: std::time::Instant,
    url: String,
    product: Product,
    reply: Reply,
}

enum Reply {
    Blocking(mpsc::SyncSender<Result<String, MobileError>>),
    #[cfg(feature = "live")]
    Async(tokio::sync::oneshot::Sender<Result<String, MobileError>>),
}

/// A signer holding a warm QuickJS context on its own thread.
///
/// Blocking by design: JNI calls block, so there is no async runtime here at all —
/// `ttl-sign-embedded`'s `tokio::sync::oneshot` becomes a `std::sync::mpsc` channel.
pub struct MobileSigner {
    requests: Option<mpsc::SyncSender<Request>>,
    worker: Option<thread::JoinHandle<()>>,
    stopping: std::sync::Arc<std::sync::atomic::AtomicBool>,
    timeout: std::time::Duration,
}

impl MobileSigner {
    /// Load `bundle` into a fresh QuickJS context and keep it warm.
    ///
    /// Returns once the bundle is loaded and `byted_acrawler.init` has run, so a
    /// constructed signer is a working one. `options_json` is a [`SignOptions`] object.
    pub fn new(bundle: &str, options_json: &str) -> Result<Self, MobileError> {
        Self::with_capacity(bundle, options_json, 64)
    }
    /// Bound waiting work. Submissions reject overload; neither API waits for queue space.
    pub fn with_capacity(
        bundle: &str,
        options_json: &str,
        capacity: usize,
    ) -> Result<Self, MobileError> {
        if capacity == 0 || capacity > 65_536 {
            return Err(MobileError::Options(
                "queue capacity must be 1..65536".into(),
            ));
        }
        let options = SignOptions::from_json(options_json)?;
        let timeout = options.timeout();
        let options = options.to_driver_json();
        let bundle = bundle.to_string();
        let (requests, incoming) = mpsc::sync_channel::<Request>(capacity);
        let (ready, started) = mpsc::sync_channel::<Result<(), MobileError>>(1);

        let stopping = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stopping = stopping.clone();
        let worker = thread::Builder::new()
            .name("ttl-sign-mobile".into())
            .spawn(move || worker(bundle, options, timeout, ready, incoming, worker_stopping))
            .map_err(|e| MobileError::Engine(e.to_string()))?;

        if let Err(error) = started
            .recv()
            .map_err(|_| MobileError::Stopped)
            .and_then(|result| result)
        {
            let _ = worker.join();
            return Err(error);
        }
        Ok(Self {
            requests: Some(requests),
            worker: Some(worker),
            stopping,
            timeout,
        })
    }

    /// Stop the engine: interrupt the call it is running, if any, and refuse further work.
    ///
    /// Safe to call from any thread while another is blocked in [`MobileSigner::sign`], which
    /// then returns an error promptly. Dropping the signer still releases it.
    pub fn interrupt(&self) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Sign one URL. Blocks until the worker answers.
    pub fn sign(&self, url: &str, product: Product) -> Result<String, MobileError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .as_ref()
            .ok_or(MobileError::Stopped)?
            .try_send(Request {
                deadline: std::time::Instant::now() + self.timeout,
                url: url.to_string(),
                product,
                reply: Reply::Blocking(reply),
            })
            .map_err(queue_error)?;
        answer.recv_timeout(self.timeout).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => {
                MobileError::Sign("signing request timed out".into())
            }
            mpsc::RecvTimeoutError::Disconnected => MobileError::Stopped,
        })?
    }
    /// Live worker variant: cancelling the wait never blocks runtime shutdown.
    #[cfg(feature = "live")]
    pub async fn sign_async(&self, url: &str, product: Product) -> Result<String, MobileError> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.requests
            .as_ref()
            .ok_or(MobileError::Stopped)?
            .try_send(Request {
                deadline: std::time::Instant::now() + self.timeout,
                url: url.to_owned(),
                product,
                reply: Reply::Async(reply),
            })
            .map_err(queue_error)?;
        tokio::time::timeout(self.timeout, answer)
            .await
            .map_err(|_| MobileError::Sign("signing request timed out".into()))?
            .map_err(|_| MobileError::Stopped)?
    }
}

fn queue_error(error: mpsc::TrySendError<Request>) -> MobileError {
    match error {
        mpsc::TrySendError::Full(_) => MobileError::Sign("signer request queue is full".into()),
        mpsc::TrySendError::Disconnected(_) => MobileError::Stopped,
    }
}
impl Drop for MobileSigner {
    fn drop(&mut self) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Release);
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The engine thread. Owns the QuickJS context for its whole life; nothing else touches it.
fn worker(
    bundle: String,
    options: String,
    timeout: std::time::Duration,
    ready: mpsc::SyncSender<Result<(), MobileError>>,
    incoming: mpsc::Receiver<Request>,
    stopping: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let interrupt = engine::Interrupt::new(stopping.clone());
    interrupt.arm(std::time::Instant::now() + timeout);
    let started = engine::QuickJs::start(&bundle, &options, &interrupt);
    interrupt.disarm();
    let mut engine = match started {
        Ok(engine) => {
            let _ = ready.send(Ok(()));
            engine
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };

    // One request at a time, in arrival order. The channel closing ends the thread,
    // which is how a dropped signer shuts its engine down.
    while let Ok(request) = incoming.recv() {
        if stopping.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        if std::time::Instant::now() >= request.deadline {
            continue;
        }
        #[cfg(feature = "live")]
        if matches!(&request.reply, Reply::Async(reply) if reply.is_closed()) {
            continue;
        }
        interrupt.arm(request.deadline);
        let answer = engine.sign(&request.url, request.product.as_driver_arg());
        interrupt.disarm();
        match request.reply {
            Reply::Blocking(reply) => {
                let _ = reply.send(answer);
            }
            #[cfg(feature = "live")]
            Reply::Async(reply) => {
                let _ = reply.send(answer);
            }
        }
    }
}

// --- URL construction -----------------------------------------------------------
//
// Kotlin could build these queries itself — they are string concatenation — but the order
// is load-bearing (`registerWsSigner` signs the bytes verbatim) and already tested in
// `ttl-sign-core`. Reusing rather than porting keeps one serializer in the project.

/// Chrome Mobile on Android. The declared identity for on-device signing: the UA, the
/// `browser_*` query values, and the sandbox profile must all describe the same client.
pub fn android_device_preset() -> DevicePreset {
    DevicePreset {
        browser_name: "Mozilla".into(),
        browser_version: "5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36".into(),
        browser_platform: "Linux armv81".into(),
        os: "android".into(),
    }
}

/// Default on-device preset: Android Chrome, US East, a phone-class screen.
pub fn mobile_preset() -> Preset {
    Preset::new(
        android_device_preset(),
        LocationPreset::us_east(),
        ScreenPreset {
            width: 412,
            height: 915,
        },
    )
}

/// The unsigned direct-socket URL for `room_id`, built from [`mobile_preset`].
///
/// `device_id` is the caller's 19-digit id; `None` mints one. Sign the result with
/// [`Product::Ws`] and open it as a WebSocket.
pub fn direct_socket_url(room_id: &str, device_id: Option<&str>) -> String {
    let mut params = ttl_sign_core::DirectSocketParams::new(room_id);
    if let Some(id) = device_id {
        params.device_id = id.to_string();
    }
    params.url(&mobile_preset())
}

/// `unique_id` → lookup URL. Unsigned; no signer needed. Re-exported so Kotlin has one
/// native entry point for every URL in the flow.
pub fn room_lookup_url(unique_id: &str) -> String {
    ttl_sign_core::room_lookup_url(unique_id)
}

/// The User-Agent of [`mobile_preset`]. Present this on every HTTP request in the flow:
/// signing and requesting must present the same identity.
pub fn user_agent() -> String {
    mobile_preset().user_agent()
}

/// Which engine this build signs with, for logs and the demo app's about line.
pub const ENGINE: &str = "QuickJS";

/// Crate version, for the demo app's about line.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- driver reply parsing -------------------------------------------------------
//
// Same two shapes as `ttl-sign-embedded`, parsed by hand for the same reason: the
// signature must not be re-encoded on the way through.

pub(crate) fn read_error(out: &str) -> Result<(), MobileError> {
    match out.find("\"error\":\"") {
        Some(at) => {
            let rest = &out[at + 9..];
            let end = rest.find('"').unwrap_or(rest.len());
            Err(MobileError::Bundle(unescape(&rest[..end])))
        }
        None => Ok(()),
    }
}

pub(crate) fn read_signed(out: &str) -> Result<String, MobileError> {
    if let Some(at) = out.find("\"error\":\"") {
        let rest = &out[at + 9..];
        let end = rest.find('"').unwrap_or(rest.len());
        return Err(MobileError::Sign(unescape(&rest[..end])));
    }
    let at = out
        .find("\"signed\":\"")
        .ok_or_else(|| MobileError::Sign(format!("unexpected signer reply: {out}")))?;
    let rest = &out[at + 10..];
    let end = rest
        .find('"')
        .ok_or(MobileError::Sign("unterminated signed URL".into()))?;
    let signed = unescape(&rest[..end]);
    if signed.is_empty() || signed == "null" {
        return Err(MobileError::Sign("the signer produced no URL".into()));
    }
    Ok(signed)
}

/// JSON-escape a string value.
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// Reverse of [`escape`], for the two sequences `JSON.stringify` produces in a URL.
fn unescape(value: &str) -> String {
    value
        .replace("\\/", "/")
        .replace("\\u0026", "&")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUEUE_BUNDLE: &str = "window.byted_acrawler={init:function(){},registerWsSigner:function(){let calls=0; return function(){return {'X-Gnarly':'fixture-'+(++calls)}}}};";
    #[test]
    fn capacity_validation_and_repeated_drop_do_not_leave_workers() {
        for capacity in [0, 65_537] {
            assert!(MobileSigner::with_capacity(QUEUE_BUNDLE, "{}", capacity).is_err());
        }
        for _ in 0..10 {
            let signer = MobileSigner::with_capacity(QUEUE_BUNDLE, "{}", 1).unwrap();
            assert!(signer
                .sign("wss://example.test/?a=1", Product::Ws)
                .unwrap()
                .contains("fixture-1"));
        }
    }
    #[cfg(feature = "live")]
    #[test]
    fn worker_skips_expired_and_cancelled_queued_requests() {
        let (requests, incoming) = mpsc::sync_channel(3);
        let (expired, expired_answer) = mpsc::sync_channel(1);
        requests
            .send(Request {
                deadline: std::time::Instant::now(),
                url: "wss://expired".into(),
                product: Product::Ws,
                reply: Reply::Blocking(expired),
            })
            .unwrap();
        let (cancelled, cancelled_answer) = tokio::sync::oneshot::channel();
        drop(cancelled_answer);
        requests
            .send(Request {
                deadline: std::time::Instant::now() + DEFAULT_TIMEOUT,
                url: "wss://cancelled".into(),
                product: Product::Ws,
                reply: Reply::Async(cancelled),
            })
            .unwrap();
        let (reply, answer) = mpsc::sync_channel(1);
        requests
            .send(Request {
                deadline: std::time::Instant::now() + DEFAULT_TIMEOUT,
                url: "wss://example.test/?a=1".into(),
                product: Product::Ws,
                reply: Reply::Blocking(reply),
            })
            .unwrap();
        assert!(matches!(
            requests.try_send(Request {
                deadline: std::time::Instant::now(),
                url: String::new(),
                product: Product::Ws,
                reply: Reply::Blocking(mpsc::sync_channel(1).0)
            }),
            Err(mpsc::TrySendError::Full(_))
        ));
        drop(requests);
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            super::worker(
                QUEUE_BUNDLE.into(),
                SignOptions::default().to_driver_json(),
                DEFAULT_TIMEOUT,
                ready,
                incoming,
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
        });
        started.recv().unwrap().unwrap();
        assert!(expired_answer
            .recv_timeout(std::time::Duration::from_secs(2))
            .is_err());
        assert!(answer
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .contains("fixture-1"));
        worker.join().unwrap();
    }

    #[test]
    fn products_round_trip_through_the_driver_names() {
        assert_eq!("fetch".parse::<Product>().unwrap(), Product::Fetch);
        assert_eq!("frontier".parse::<Product>().unwrap(), Product::Frontier);
        assert_eq!("ws".parse::<Product>().unwrap(), Product::Ws);
        assert_eq!(Product::Ws.as_driver_arg(), "ws");
        assert!("bogus".parse::<Product>().is_err());
    }

    #[test]
    fn options_accept_both_key_styles_and_default_unpinned() {
        let opts =
            SignOptions::from_json(r#"{"pinned":true,"userAgent":"UA","xmst":"t"}"#).unwrap();
        assert!(opts.pinned);
        assert_eq!(opts.user_agent.as_deref(), Some("UA"));
        assert_eq!(opts.stored_token.as_deref(), Some("t"));
        assert_eq!(opts.cookie, None);

        let opts = SignOptions::from_json(r#"{"stored_token":"t","cookie":"c=1"}"#).unwrap();
        assert!(!opts.pinned);
        assert_eq!(opts.stored_token.as_deref(), Some("t"));

        assert!(SignOptions::from_json("not json").is_err());
        // Unknown fields are ignored: forward compatibility for older apps.
        assert!(SignOptions::from_json(r#"{"future":1}"#).is_ok());
    }

    #[test]
    fn the_timeout_option_is_validated_and_kept_from_the_driver() {
        let opts = SignOptions::from_json(r#"{"timeoutMs":250}"#).unwrap();
        assert_eq!(opts.timeout(), std::time::Duration::from_millis(250));
        assert_eq!(SignOptions::default().timeout(), DEFAULT_TIMEOUT);
        for bad in [
            r#"{"timeoutMs":0}"#,
            r#"{"timeoutMs":-1}"#,
            r#"{"timeoutMs":"5"}"#,
        ] {
            assert!(SignOptions::from_json(bad).is_err(), "{bad}");
        }
        assert!(!opts.to_driver_json().contains("timeout"));
    }

    /// A bundle that never returns must not hold the engine thread: loading it is cut off at the
    /// deadline, so the open fails instead of hanging.
    #[test]
    fn a_wedged_bundle_is_interrupted_while_loading() {
        let started = std::time::Instant::now();
        let error = MobileSigner::new("while (true) {}", r#"{"timeoutMs":200}"#)
            .err()
            .expect("a wedged bundle must not open");
        assert!(matches!(error, MobileError::Bundle(_)), "{error:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    /// A signature that never returns is cut off at its deadline instead of hanging the caller.
    #[test]
    fn a_wedged_signature_is_interrupted_at_its_deadline() {
        let bundle = "window.byted_acrawler={init:function(){},registerWsSigner:function(){\
                      let calls = 0; return function(){ if (++calls > 1) { while (true) {} } \
                      return {'X-Gnarly':'ok'} }}};";
        let signer = MobileSigner::new(bundle, r#"{"timeoutMs":300}"#).unwrap();
        assert!(signer
            .sign("wss://example.test/?a=1", Product::Ws)
            .unwrap()
            .contains("X-Gnarly=ok"));
        let started = std::time::Instant::now();
        assert!(signer.sign("wss://example.test/?a=1", Product::Ws).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        // Dropping joins the engine thread, which must already be free.
        let started = std::time::Instant::now();
        drop(signer);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    /// Interrupting from another thread stops a running call promptly, and later calls fail.
    #[test]
    fn interrupt_stops_a_running_signature_from_another_thread() {
        let bundle = "window.byted_acrawler={init:function(){},registerWsSigner:function(){\
                      return function(){ while (true) {} }}};";
        let signer =
            std::sync::Arc::new(MobileSigner::new(bundle, r#"{"timeoutMs":60000}"#).unwrap());
        let busy = std::sync::Arc::clone(&signer);
        let started = std::time::Instant::now();
        let call = thread::spawn(move || busy.sign("wss://example.test/?a=1", Product::Ws));
        thread::sleep(std::time::Duration::from_millis(150));
        signer.interrupt();
        assert!(call.join().unwrap().is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(signer.sign("wss://example.test/?a=1", Product::Ws).is_err());
        drop(signer);
    }

    #[test]
    fn options_re_encode_for_the_driver() {
        let opts = SignOptions {
            user_agent: Some("Mozilla/5.0 \"q\"".into()),
            cookie: Some("a\\b".into()),
            stored_token: None,
            pinned: true,
            timeout_ms: Some(5),
        };
        assert_eq!(
            opts.to_driver_json(),
            r#"{"pinned":true,"userAgent":"Mozilla/5.0 \"q\"","cookie":"a\\b"}"#
        );
    }

    #[test]
    fn the_mobile_preset_is_self_consistent() {
        let preset = mobile_preset();
        let ua = preset.user_agent();
        assert!(ua.starts_with("Mozilla/"), "unexpected UA: {ua}");
        assert!(ua.contains("Android"), "not an Android UA: {ua}");
        assert!(ua.contains("Chrome/"), "not a Chrome UA: {ua}");
        assert_eq!(
            ua,
            format!(
                "{}/{}",
                preset.device.browser_name, preset.device.browser_version
            )
        );
    }

    #[test]
    fn the_socket_url_carries_the_room_and_a_device_id() {
        let url = direct_socket_url("7300000000000000001", Some("1234567890123456789"));
        assert!(
            url.starts_with(
                "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?"
            ),
            "unexpected URL: {url}"
        );
        assert!(
            url.contains("room_id=7300000000000000001"),
            "no room: {url}"
        );
        assert!(
            url.contains("device_id=1234567890123456789"),
            "no device id: {url}"
        );
        assert!(url.contains("aid=1988"), "no aid: {url}");
        assert!(!url.contains("X-Gnarly"), "must be unsigned: {url}");
    }

    #[test]
    fn a_missing_device_id_is_minted() {
        let url = direct_socket_url("1", None);
        let id = url
            .split("device_id=")
            .nth(1)
            .and_then(|tail| tail.split('&').next())
            .unwrap();
        assert_eq!(id.len(), 19, "device_id has the wrong length: {id}");
        assert!(id.chars().all(|c| c.is_ascii_digit()));
    }

    /// The sandbox has to be the one in `scripts/headless`, not a copy that drifted from it.
    #[test]
    fn the_bootstrap_is_the_shared_sandbox() {
        assert!(BOOTSTRAP.contains("GENERATED by scripts/headless/tools/build-bootstrap.mjs"));
        assert!(BOOTSTRAP.contains("globalThis.ttlPrepare"));
        assert!(BOOTSTRAP.contains("globalThis.ttlSignUrl"));
    }
}
