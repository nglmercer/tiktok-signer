//! Signing without a subprocess: the real bundle, inside this process.
//!
//! [`ttl_live_discovery::CommandSigner`] spawns `node scripts/headless/sign-url.mjs` for every
//! signature, which parses 235 KB of bundle each time and needs Node on the host. That is why the
//! Docker image carries a Node runtime, and it is why nothing here can be packaged as a desktop
//! application.
//!
//! This is the same signer with the process removed. The engine runs the same sandbox — literally
//! the same source: `bootstrap.js` is generated from `scripts/headless/shim.mjs` by
//! `scripts/headless/tools/build-bootstrap.mjs`, so there is one sandbox in the repository, not two.
//!
//! # Two engines
//!
//! Both pass the same acceptance test — a **byte-identical** signature under a pinned profile — and
//! they trade off against each other rather than one being better (`docs/13-embedded-runtime.md`):
//!
//! | Feature | Engine | Per signature | Binary cost |
//! |---|---|---|---|
//! | `quickjs` (default) | QuickJS via `rquickjs` | 28 ms | 3.1 MB |
//! | `v8` | V8 via `deno_core` | 3 ms | 69.9 MB |
//!
//! QuickJS is the default because it is small and needs no JIT, and it is already several times
//! faster than the 118 ms a fresh `node` costs. Turn on `v8` where binary size does not matter and
//! throughput does — it costs about 67 MB of binary and returns roughly nine times the throughput.
//! Both are exposed as [`QuickJsSigner`] and [`V8Signer`]; `EmbeddedSigner` names the one this
//! build signs with, which is V8 when the `v8` feature is on and QuickJS otherwise.
//!
//! # The thread
//!
//! Neither engine's context is [`Send`], so it lives on its own thread and the signer is a handle
//! to it. Requests go over a channel and replies come back on a oneshot, which keeps the
//! [`UrlSigner`] signature — an async `sign(&str)` — exactly as it was.

use std::marker::PhantomData;
use std::sync::mpsc;
use std::thread;

use ttl_live_discovery::{DiscoveryError, SignFuture, SigningProduct, UrlSigner};

#[cfg(feature = "quickjs")]
mod quickjs;
#[cfg(feature = "v8")]
mod v8;

#[cfg(feature = "quickjs")]
pub use quickjs::QuickJs;
#[cfg(feature = "v8")]
pub use v8::V8;

/// The sandbox, flattened into one script. Generated; see the module docs.
pub(crate) const BOOTSTRAP: &str = include_str!("../bootstrap.js");

/// The most entropy an engine hands the sandbox in one draw.
///
/// Bundle 1.0.0.388 draws none at all — `tests/entropy.rs` measures it, and the V8 pool is sized
/// from that number — so this is a ceiling on a request that should never arrive, not a budget.
pub(crate) const MAX_RANDOM_BYTES: usize = 8192;

/// A JavaScript engine that can hold a prepared bundle and sign URLs with it.
///
/// Deliberately narrow. Everything an engine could differ about — how the sandbox is built, which
/// products exist, how a reply is read — is above this line and shared, so a second engine cannot
/// quietly grow a second behaviour. What is below it is engine-specific and nothing else.
pub trait Engine: Sized {
    /// For error messages, so a failure names the engine that produced it.
    const NAME: &'static str;

    /// Start an engine, install the sandbox, and load `bundle` under `options` (a [`Profile`] as
    /// JSON). Returns only once the bundle is ready to sign.
    fn start(bundle: &str, options: &str) -> Result<Self, EmbeddedError>;

    /// Sign one URL. `product` is the driver's name for it — see [`product_arg`].
    fn sign(&mut self, url: &str, product: &str) -> Result<String, String>;
}

/// Environment the sandbox reports while signing.
///
/// The defaults are a Chrome-on-Linux guest with no session. `pinned` is for tests and
/// differentials only: it freezes the clock and the entropy so two runs agree, which is exactly
/// what a real signature must not do.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub user_agent: Option<String>,
    pub cookie: Option<String>,
    /// The stored `xmst` token. `msToken` is a verbatim passthrough of it.
    pub stored_token: Option<String>,
    /// Freeze the clock, `performance`, `Math.random` and the entropy sequence.
    pub pinned: bool,
}

impl Profile {
    fn to_json(&self) -> String {
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

#[derive(Debug, thiserror::Error)]
pub enum EmbeddedError {
    #[error("the engine could not start: {0}")]
    Engine(String),
    #[error("the bundle could not be prepared: {0}")]
    Bundle(String),
    #[error("the signer thread stopped")]
    Stopped,
}

/// A signature request for the worker thread.
struct Request {
    url: String,
    product: SigningProduct,
    reply: tokio::sync::oneshot::Sender<Result<String, String>>,
}

/// A signer holding a warm engine on its own thread.
pub struct Signer<E: Engine> {
    requests: mpsc::Sender<Request>,
    product: SigningProduct,
    // The engine never crosses the thread, so this is a type tag and nothing more.
    engine: PhantomData<fn() -> E>,
}

/// QuickJS: small, interpreted, byte-identical to V8.
#[cfg(feature = "quickjs")]
pub type QuickJsSigner = Signer<QuickJs>;

/// V8: an order of magnitude faster per signature, at tens of megabytes.
#[cfg(feature = "v8")]
pub type V8Signer = Signer<V8>;

/// The engine this build signs with by default.
///
/// QuickJS unless `v8` was asked for. `quickjs` is a default feature and arrives without anyone
/// choosing it; `v8` only ever arrives because someone did, so it wins when both are present —
/// which is what lets a dependent turn on `ttl-sign-embedded/v8` and get V8, without having to
/// reach in and switch the default feature off.
#[cfg(feature = "v8")]
pub type EmbeddedSigner = Signer<V8>;
/// The engine this build signs with by default.
#[cfg(all(feature = "quickjs", not(feature = "v8")))]
pub type EmbeddedSigner = Signer<QuickJs>;

/// The engine [`EmbeddedSigner`] runs in, for logs and error messages.
#[cfg(feature = "v8")]
pub const ENGINE: &str = V8::NAME;
/// The engine [`EmbeddedSigner`] runs in, for logs and error messages.
#[cfg(all(feature = "quickjs", not(feature = "v8")))]
pub const ENGINE: &str = QuickJs::NAME;

#[cfg(not(any(feature = "quickjs", feature = "v8")))]
compile_error!("ttl-sign-embedded needs an engine: enable the `quickjs` or `v8` feature");

impl<E: Engine + 'static> Signer<E> {
    /// Load `bundle` into a fresh context and keep it warm.
    ///
    /// Returns once the bundle is loaded and `byted_acrawler.init` has run, so a constructed signer
    /// is a working one — a caller never discovers a broken bundle on its first live request.
    pub fn new(bundle: impl Into<String>, profile: Profile) -> Result<Self, EmbeddedError> {
        Self::with_product(bundle, profile, SigningProduct::WsDirect)
    }

    /// As [`Signer::new`], with the product this signer applies by default.
    ///
    /// The products are not interchangeable: the socket verifies `registerWsSigner`'s `X-Gnarly`,
    /// and the patched-fetch suffix or `frontierSign` produce parameters it ignores.
    pub fn with_product(
        bundle: impl Into<String>,
        profile: Profile,
        product: SigningProduct,
    ) -> Result<Self, EmbeddedError> {
        let bundle = bundle.into();
        let options = profile.to_json();
        let (requests, incoming) = mpsc::channel::<Request>();
        let (ready, started) = mpsc::channel::<Result<(), EmbeddedError>>();

        thread::Builder::new()
            .name(format!("ttl-sign-{}", E::NAME.to_lowercase()))
            .spawn(move || worker::<E>(bundle, options, ready, incoming))
            .map_err(|error| EmbeddedError::Engine(error.to_string()))?;

        started.recv().map_err(|_| EmbeddedError::Stopped)??;
        Ok(Self {
            requests,
            product,
            engine: PhantomData,
        })
    }

    /// Sign under a product other than this signer's default.
    pub async fn sign_with(
        &self,
        url: &str,
        product: SigningProduct,
    ) -> Result<String, DiscoveryError> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.requests
            .send(Request {
                url: url.to_string(),
                product,
                reply,
            })
            .map_err(|_| DiscoveryError::Signer("the signer thread stopped".into()))?;
        answer
            .await
            .map_err(|_| DiscoveryError::Signer("the signer thread dropped the request".into()))?
            .map_err(DiscoveryError::Signer)
    }
}

impl<E: Engine + 'static> UrlSigner for Signer<E> {
    fn sign<'a>(&'a self, url: &'a str) -> SignFuture<'a> {
        Box::pin(async move { self.sign_with(url, self.product).await })
    }
}

/// The engine thread. Owns the engine for its whole life; nothing else touches it.
fn worker<E: Engine>(
    bundle: String,
    options: String,
    ready: mpsc::Sender<Result<(), EmbeddedError>>,
    incoming: mpsc::Receiver<Request>,
) {
    let mut engine = match E::start(&bundle, &options) {
        Ok(engine) => {
            let _ = ready.send(Ok(()));
            engine
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };

    // One request at a time, in arrival order. The channel closing ends the thread, which is how a
    // dropped signer shuts its engine down.
    while let Ok(request) = incoming.recv() {
        let answer = engine.sign(&request.url, product_arg(request.product));
        let _ = request.reply.send(answer);
    }
}

/// The driver's argument for a product. Same three names `sign-url.mjs` takes.
fn product_arg(product: SigningProduct) -> &'static str {
    match product {
        SigningProduct::FetchPatch => "fetch",
        SigningProduct::FrontierSign => "frontier",
        SigningProduct::WsDirect => "ws",
    }
}

/// The driver's replies carry an `error` or they succeeded. `ttlPrepare` answers `{"ok":true}`,
/// which carries nothing else worth reading.
pub(crate) fn read_error(out: &str) -> Result<(), String> {
    match out.find("\"error\":\"") {
        Some(at) => {
            let rest = &out[at + 9..];
            let end = rest.find('"').unwrap_or(rest.len());
            Err(unescape(&rest[..end]))
        }
        None => Ok(()),
    }
}

/// Read the driver's `{"signed": …}` / `{"error": …}` reply.
///
/// Parsed by hand rather than with `serde_json`: the shapes are two, they are ours, and the
/// signature must not be re-encoded on the way through — a URL that survives a JSON round trip with
/// its escapes rewritten is a different URL.
pub(crate) fn read(out: &str) -> Result<String, String> {
    if let Some(at) = out.find("\"error\":\"") {
        let rest = &out[at + 9..];
        let end = rest.find('"').unwrap_or(rest.len());
        return Err(unescape(&rest[..end]));
    }
    let at = out
        .find("\"signed\":\"")
        .ok_or_else(|| format!("unexpected signer reply: {out}"))?;
    let rest = &out[at + 10..];
    let end = rest.find('"').ok_or("unterminated signed URL")?;
    let signed = unescape(&rest[..end]);
    if signed.is_empty() || signed == "null" {
        return Err("the signer produced no URL".into());
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

    /// The profile is passed to JavaScript as JSON, so its escaping is not cosmetic: a cookie with
    /// a quote in it would otherwise end the string and change what the sandbox reports.
    #[test]
    fn a_profile_serialises_as_json() {
        let profile = Profile {
            user_agent: Some("Mozilla/5.0 \"test\"".into()),
            cookie: Some("sessionid=a\\b".into()),
            stored_token: None,
            pinned: true,
        };
        assert_eq!(
            profile.to_json(),
            r#"{"pinned":true,"userAgent":"Mozilla/5.0 \"test\"","cookie":"sessionid=a\\b"}"#
        );
        assert_eq!(Profile::default().to_json(), r#"{"pinned":false}"#);
    }

    #[test]
    fn a_prepare_reply_is_only_checked_for_an_error() {
        assert!(read_error(r#"{"ok":true}"#).is_ok());
        assert_eq!(
            read_error(r#"{"error":"bundle failed to load: boom"}"#).unwrap_err(),
            "bundle failed to load: boom"
        );
    }

    #[test]
    fn the_driver_reply_is_read_without_re_encoding_the_url() {
        assert_eq!(
            read(r#"{"signed":"https://x.test/?a=1&b=2\/3"}"#).unwrap(),
            "https://x.test/?a=1&b=2/3"
        );
        assert_eq!(
            read(r#"{"error":"registerWsSigner produced no X-Gnarly"}"#).unwrap_err(),
            "registerWsSigner produced no X-Gnarly"
        );
    }

    /// Every product must reach the driver under the name it expects; a wrong one signs the wrong
    /// thing and the failure appears only against a live endpoint.
    #[test]
    fn products_map_to_the_driver_argument() {
        assert_eq!(product_arg(SigningProduct::FetchPatch), "fetch");
        assert_eq!(product_arg(SigningProduct::FrontierSign), "frontier");
        assert_eq!(product_arg(SigningProduct::WsDirect), "ws");
    }

    /// The sandbox has to be the one in `scripts/headless`, not a copy that drifted from it.
    #[test]
    fn the_bootstrap_carries_the_generated_sandbox() {
        assert!(BOOTSTRAP.contains("GENERATED by scripts/headless/tools/build-bootstrap.mjs"));
        assert!(BOOTSTRAP.contains("function createSandbox()"));
        assert!(BOOTSTRAP.contains("globalThis.ttlPrepare"));
        assert!(BOOTSTRAP.contains("globalThis.ttlSignUrl"));
    }
}
