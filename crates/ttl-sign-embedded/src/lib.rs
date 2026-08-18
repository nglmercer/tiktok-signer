//! Signing without a subprocess: the real bundle, inside this process.
//!
//! [`ttl_live_discovery::CommandSigner`] spawns `node scripts/headless/sign-url.mjs` for every
//! signature, which parses 235 KB of bundle each time and needs Node on the host. That is why the
//! Docker image carries a Node runtime, and it is why nothing here can be packaged as a desktop
//! application.
//!
//! This is the same signer with the process removed. QuickJS runs the same sandbox — literally the
//! same source: `bootstrap.js` is generated from `scripts/headless/shim.mjs` by
//! `scripts/headless/tools/build-bootstrap.mjs`, so there is one sandbox in the repository, not two.
//!
//! Measured against the subprocess on 2026-08-18 (`docs/13-embedded-runtime.md`): 137 ms to load
//! the bundle once, then 76 ms per signature, against 118 ms per signature for a fresh `node`.
//! QuickJS was chosen because it produces **byte-identical** signatures to Node under a pinned
//! profile, in 1.9 MB, without a JIT. Boa fails inside the bundle's own interpreter; V8 is faster
//! but cannot target `wasm32`.
//!
//! # The thread
//!
//! A QuickJS context is not [`Send`], so the engine lives on its own thread and this type is a
//! handle to it. Requests go over a channel and replies come back on a oneshot, which keeps the
//! [`UrlSigner`] signature — an async `sign(&str)` — exactly as it was.

use std::sync::mpsc;
use std::thread;

use rand::RngCore;
use rquickjs::{Context, Function, Runtime};
use ttl_live_discovery::{DiscoveryError, SignFuture, SigningProduct, UrlSigner};

/// The sandbox, flattened into one script. Generated; see the module docs.
const BOOTSTRAP: &str = include_str!("../bootstrap.js");

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

/// A signer holding a warm QuickJS context on its own thread.
pub struct EmbeddedSigner {
    requests: mpsc::Sender<Request>,
    product: SigningProduct,
}

impl EmbeddedSigner {
    /// Load `bundle` into a fresh context and keep it warm.
    ///
    /// Returns once the bundle is loaded and `byted_acrawler.init` has run, so a constructed signer
    /// is a working one — a caller never discovers a broken bundle on its first live request.
    pub fn new(bundle: impl Into<String>, profile: Profile) -> Result<Self, EmbeddedError> {
        Self::with_product(bundle, profile, SigningProduct::WsDirect)
    }

    /// As [`EmbeddedSigner::new`], with the product this signer applies by default.
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
            .name("ttl-sign-embedded".into())
            .spawn(move || worker(bundle, options, ready, incoming))
            .map_err(|error| EmbeddedError::Engine(error.to_string()))?;

        started.recv().map_err(|_| EmbeddedError::Stopped)??;
        Ok(Self { requests, product })
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

impl UrlSigner for EmbeddedSigner {
    fn sign<'a>(&'a self, url: &'a str) -> SignFuture<'a> {
        Box::pin(async move { self.sign_with(url, self.product).await })
    }
}

/// The engine thread. Owns the runtime for its whole life; nothing else touches it.
fn worker(
    bundle: String,
    options: String,
    ready: mpsc::Sender<Result<(), EmbeddedError>>,
    incoming: mpsc::Receiver<Request>,
) {
    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(EmbeddedError::Engine(error.to_string())));
            return;
        }
    };
    let context = match Context::full(&runtime) {
        Ok(context) => context,
        Err(error) => {
            let _ = ready.send(Err(EmbeddedError::Engine(error.to_string())));
            return;
        }
    };

    context.with(|ctx| {
        // The engine has no `crypto`, and the SDK's entropy is not decoration: signatures built
        // from a counter come out short, which was measured. The bootstrap wires its random source
        // to this function when it finds no `crypto`, so it must exist before the script runs.
        let random = Function::new(ctx.clone(), |count: usize| {
            let mut bytes = vec![0u8; count.min(4096)];
            rand::thread_rng().fill_bytes(&mut bytes);
            bytes
        });
        match random {
            Ok(random) => {
                if let Err(error) = ctx.globals().set("__ttl_random_bytes", random) {
                    let _ = ready.send(Err(EmbeddedError::Engine(error.to_string())));
                    return;
                }
            }
            Err(error) => {
                let _ = ready.send(Err(EmbeddedError::Engine(error.to_string())));
                return;
            }
        }

        if let Err(error) = ctx.eval::<(), _>(BOOTSTRAP) {
            let _ = ready.send(Err(EmbeddedError::Engine(describe(&ctx, error))));
            return;
        }

        let prepare: Function = match ctx.globals().get("ttlPrepare") {
            Ok(function) => function,
            Err(error) => {
                let _ = ready.send(Err(EmbeddedError::Engine(describe(&ctx, error))));
                return;
            }
        };
        let prepared: Result<String, _> = prepare.call((bundle.as_str(), options.as_str()));
        match prepared {
            Ok(out) => {
                if let Err(reason) = read_error(&out) {
                    let _ = ready.send(Err(EmbeddedError::Bundle(reason)));
                    return;
                }
            }
            Err(error) => {
                let _ = ready.send(Err(EmbeddedError::Bundle(describe(&ctx, error))));
                return;
            }
        }

        let sign: Function = match ctx.globals().get("ttlSignUrl") {
            Ok(function) => function,
            Err(error) => {
                let _ = ready.send(Err(EmbeddedError::Engine(describe(&ctx, error))));
                return;
            }
        };
        let _ = ready.send(Ok(()));

        // One request at a time, in arrival order. The channel closing ends the thread, which is
        // how a dropped signer shuts its engine down.
        while let Ok(request) = incoming.recv() {
            let out: Result<String, _> = sign.call((request.url.as_str(), product_arg(request.product)));
            let answer = match out {
                Ok(out) => read(&out),
                Err(error) => Err(describe(&ctx, error)),
            };
            let _ = request.reply.send(answer);
        }
    });
}

/// The driver's argument for a product. Same three names `sign-url.mjs` takes.
fn product_arg(product: SigningProduct) -> &'static str {
    match product {
        SigningProduct::FetchPatch => "fetch",
        SigningProduct::FrontierSign => "frontier",
        SigningProduct::WsDirect => "ws",
    }
}

/// QuickJS reports "Exception generated by QuickJS" and leaves the real error on the context.
fn describe(ctx: &rquickjs::Ctx, error: rquickjs::Error) -> String {
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let value = ctx.catch();
    match value.as_exception() {
        Some(exception) => exception.message().unwrap_or_else(|| "unknown error".into()),
        None => "unknown error".into(),
    }
}

/// The driver's replies carry an `error` or they succeeded. `ttlPrepare` answers `{"ok":true}`,
/// which carries nothing else worth reading.
fn read_error(out: &str) -> Result<(), String> {
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
fn read(out: &str) -> Result<String, String> {
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

#[cfg(test)]
mod engine_tests {
    use super::*;

    /// The random source is injected from Rust and wired up by the bootstrap. If that wiring
    /// breaks, the SDK swallows the exception inside its own interpreter and quietly omits parts of
    /// its API — which is a far harder failure to read than an error here.
    #[test]
    fn the_injected_random_source_reaches_the_sandbox() {
        let runtime = Runtime::new().expect("runtime");
        let context = Context::full(&runtime).expect("context");
        context.with(|ctx| {
            let random = Function::new(ctx.clone(), |count: usize| {
                let mut bytes = vec![0u8; count.min(4096)];
                rand::thread_rng().fill_bytes(&mut bytes);
                bytes
            })
            .expect("random function");
            ctx.globals()
                .set("__ttl_random_bytes", random)
                .expect("install");
            ctx.eval::<(), _>(BOOTSTRAP).expect("bootstrap");

            let kind: String = ctx
                .eval("typeof globalThis.TTL_RANDOM_SOURCE")
                .expect("read the source");
            assert_eq!(kind, "function", "the bootstrap did not wire the random source");

            let filled: String = ctx
                .eval(
                    "(function () { const a = new Uint8Array(8); \
                     globalThis.TTL_RANDOM_SOURCE(a); \
                     return Array.from(a).join(','); })()",
                )
                .expect("fill an array");
            assert_eq!(filled.split(',').count(), 8, "got: {filled}");
            assert!(
                filled.split(',').any(|byte| byte != "0"),
                "every byte was zero: {filled}"
            );
        });
    }
}
