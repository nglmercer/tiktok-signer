//! Live event streaming: sign, open the socket, decode batches, push JSON.
//!
//! This is why signing exists: a signed URL nobody opens produces no events. The worker
//! reuses the production pieces rather than reimplementing them:
//!
//! - [`crate::MobileSigner`] signs a fresh socket URL per attempt (a signature ages out,
//!   so reconnects re-sign — the same rule `ReconnectingConnection` exists for).
//! - [`ttl_live_ws::LiveConnection`] owns the handshake, room entry, heartbeats, and acks.
//! - [`ttl_live_events::decode_batch`] normalises each payload into [`LiveEvent`]s.
//!
//! What is *not* reused is `ReconnectingConnection`'s loop: its backoff sleeps inside
//! `next_message`, so a disconnect during a 60 s backoff would hang the caller. This
//! worker runs its own open/drain/backoff loop with shutdown checks between every step
//! (disconnect lands within ~1 s), borrowing only the backoff *timing* from
//! [`ttl_live_ws::ReconnectPolicy`].
//!
//! Events cross to the host as JSON through [`LiveSink`]: `LiveEvent` already serializes
//! with `{"type": …}`, except [`LiveEvent::Unknown`], whose raw payload would serialize
//! as kilobytes of byte values — those cross slim (`method` + byte count) instead.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use ttl_live_events::LiveEvent;
use ttl_live_ws::{ConnectConfig, LiveConnection, ReconnectPolicy};
use ttl_sign_core::CookieJar;

use crate::{mobile_preset, MobileError, MobileSigner, Product};

/// How long one `open_uri` may take before the attempt fails.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Shutdown is checked this often while draining; see the cancel-safety note below.
const DRAIN_POLL: Duration = Duration::from_millis(500);
/// Shutdown is checked this often while backing off.
const BACKOFF_SLICE: Duration = Duration::from_millis(100);

/// Receives connection states and event JSON from the worker thread.
///
/// States (`kind`, with a human `detail`): `connecting` (attempt `n`), `open`
/// (room id), `reconnecting` (reason), `decode_error` (non-fatal detail), `closed`
/// (reason, terminal), `error` (terminal failure detail).
pub trait LiveSink: Send + 'static {
    fn on_state(&self, kind: &str, detail: &str);
    fn on_event(&self, json: &str);
}

/// Serialize one event for the host. Unknown events cross slim: their payload can be
/// kilobytes, and a byte array in JSON helps no renderer.
pub fn event_json(event: &LiveEvent) -> String {
    match event {
        LiveEvent::Unknown { method, payload } => {
            serde_json::json!({"type": "unknown", "method": method, "bytes": payload.len()})
                .to_string()
        }
        _ => serde_json::to_string(event).unwrap_or_else(|_| {
            serde_json::json!({"type": "unknown", "method": event.method(), "bytes": 0}).to_string()
        }),
    }
}

/// An open live connection. Owns its worker thread; [`LiveClient::disconnect`] stops it.
pub struct LiveClient {
    shutdown: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl LiveClient {
    /// Open `room_id`'s event stream: sign, connect, decode, push.
    ///
    /// `bundle`/`options_json` are the signer's inputs (as in [`MobileSigner::new`]);
    /// `cookie_header` is the guest session (`k=v; k=v`) the handshake presents — an
    /// empty jar is refused before a frame is exchanged. Fails fast on a bad bundle,
    /// before any thread or socket exists; everything after that arrives via `sink`.
    pub fn connect(
        bundle: &str,
        options_json: &str,
        room_id: &str,
        cookie_header: &str,
        sink: impl LiveSink,
    ) -> Result<Self, MobileError> {
        Self::connect_with_retry(bundle, options_json, room_id, cookie_header, 5, sink)
    }

    /// As [`LiveClient::connect`], with the reconnect budget made explicit.
    pub fn connect_with_retry(
        bundle: &str,
        options_json: &str,
        room_id: &str,
        cookie_header: &str,
        max_attempts: u32,
        sink: impl LiveSink,
    ) -> Result<Self, MobileError> {
        // Fail fast, cheap checks first: no thread or socket exists on any error here.
        let cookies = CookieJar::parse(cookie_header);
        if cookies.is_empty() {
            return Err(MobileError::Sign(
                "the handshake needs guest cookies; an empty jar is refused".into(),
            ));
        }
        let signer = MobileSigner::new(bundle, options_json)?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let room_id = room_id.to_string();
        let worker = thread::Builder::new()
            .name("ttl-live-mobile".into())
            .spawn(move || {
                run(
                    signer,
                    cookies,
                    room_id,
                    max_attempts,
                    Box::new(sink),
                    worker_shutdown,
                );
            })
            .map_err(|e| MobileError::Engine(e.to_string()))?;
        Ok(Self {
            shutdown,
            worker: Some(worker),
        })
    }

    /// Stop the stream and wait for the worker. Lands within ~1 s: shutdown is
    /// checked between every step, including inside backoff sleeps.
    pub fn disconnect(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker thread. Owns the signer, the socket, and a current-thread runtime.
fn run(
    signer: MobileSigner,
    cookies: CookieJar,
    room_id: String,
    max_attempts: u32,
    sink: Box<dyn LiveSink>,
    shutdown: Arc<AtomicBool>,
) {
    ensure_crypto_provider();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            sink.on_state("error", &format!("no async runtime: {error}"));
            return;
        }
    };
    let user_agent = crate::user_agent();
    let preset = mobile_preset();
    let policy = ReconnectPolicy::default();
    let mut attempt = 0u32;

    runtime.block_on(async {
        loop {
            if shutdown.load(Ordering::SeqCst) {
                sink.on_state("closed", "disconnect requested");
                return;
            }
            if attempt >= max_attempts {
                sink.on_state("closed", &format!("gave up after {attempt} attempt(s)"));
                return;
            }
            attempt += 1;
            sink.on_state("connecting", &format!("attempt {attempt}"));

            // A fresh URL per attempt: device id minted anew, like a cold SDK boot.
            let url = ttl_sign_core::DirectSocketParams::new(&room_id).url(&preset);
            // Blocking the runtime here is safe: nothing else is scheduled yet — the
            // socket, timers, and heartbeat all come after this returns.
            let signed = match signer.sign(&url, Product::Ws) {
                Ok(signed) => signed,
                Err(error) => {
                    sink.on_state("error", &format!("signing failed: {error}"));
                    return;
                }
            };
            let opened = tokio::time::timeout(
                CONNECT_TIMEOUT,
                LiveConnection::open_uri(
                    &signed,
                    &cookies,
                    &user_agent,
                    "",
                    &ConnectConfig::default(),
                ),
            )
            .await;
            let mut connection = match opened {
                Ok(Ok(connection)) => connection,
                Ok(Err(error)) => {
                    sink.on_state("reconnecting", &error.to_string());
                    if !sleep_checked(policy.backoff(attempt), &shutdown).await {
                        sink.on_state("closed", "disconnect requested");
                        return;
                    }
                    continue;
                }
                Err(_) => {
                    sink.on_state("reconnecting", "connect timed out");
                    if !sleep_checked(policy.backoff(attempt), &shutdown).await {
                        sink.on_state("closed", "disconnect requested");
                        return;
                    }
                    continue;
                }
            };

            attempt = 0; // an open socket resets the budget
            sink.on_state("open", &room_id);
            // Drain with shutdown checks. Dropping a `next_message` poll on timeout is
            // cancel-safe: at worst a heartbeat tick is consumed and resent on the next
            // poll, or an ack is skipped and the server resends the frame.
            loop {
                if shutdown.load(Ordering::SeqCst) {
                    connection.close().await;
                    sink.on_state("closed", "disconnect requested");
                    return;
                }
                match tokio::time::timeout(DRAIN_POLL, connection.next_message()).await {
                    Ok(Some(Ok(message))) => {
                        match ttl_live_events::decode_batch(&message.payload) {
                            Ok(batch) => {
                                for decoded in &batch.events {
                                    sink.on_event(&event_json(&decoded.event));
                                }
                            }
                            Err(error) => {
                                sink.on_state("decode_error", &error.to_string());
                            }
                        }
                    }
                    Ok(Some(Err(error))) => {
                        sink.on_state("reconnecting", &error.to_string());
                        break;
                    }
                    Ok(None) => {
                        sink.on_state("reconnecting", "server closed the stream");
                        break;
                    }
                    Err(_) => continue, // poll slice elapsed; re-check shutdown
                }
            }
            if !sleep_checked(policy.backoff(attempt + 1), &shutdown).await {
                sink.on_state("closed", "disconnect requested");
                return;
            }
        }
    });
}

/// Install the process TLS crypto provider. rustls 0.23 panics on first use without
/// exactly one; tungstenite brings none, and this crate has no reqwest to install one
/// as a side effect. Another loader may have installed one first — that wins, and the
/// error is ignored.
fn ensure_crypto_provider() {
    let _ =
        rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());
}

/// Sleep `total`, returning false early when shutdown is requested.
async fn sleep_checked(total: Duration, shutdown: &AtomicBool) -> bool {
    let mut left = total;
    while left > Duration::ZERO {
        if shutdown.load(Ordering::SeqCst) {
            return false;
        }
        let slice = left.min(BACKOFF_SLICE);
        tokio::time::sleep(slice).await;
        left = left.saturating_sub(slice);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttl_live_events::{ChatEvent, EventUser};

    fn user() -> EventUser {
        EventUser {
            id: 7,
            nickname: "Ay".into(),
            unique_id: "ay".into(),
            sec_uid: String::new(),
            avatar_url: None,
        }
    }

    #[test]
    fn events_serialize_with_a_type_tag() {
        let json = event_json(&LiveEvent::Chat(ChatEvent {
            user: user(),
            comment: "hello".into(),
        }));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "chat");
        assert_eq!(value["comment"], "hello");
        assert_eq!(value["user"]["unique_id"], "ay");
        assert_eq!(value["user"]["nickname"], "Ay");
    }

    #[test]
    fn unknown_events_cross_slim_not_as_byte_arrays() {
        let json = event_json(&LiveEvent::Unknown {
            method: "WebcastNewMethod".into(),
            payload: vec![0u8; 4096],
        });
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "unknown");
        assert_eq!(value["method"], "WebcastNewMethod");
        assert_eq!(value["bytes"], 4096);
        // The 4 KB payload must not ride along as 16 KB of JSON numbers.
        assert!(json.len() < 200, "unknown event too large: {json}");
    }

    #[test]
    fn connect_fails_fast_on_a_bad_bundle_without_a_network() {
        struct Sink;
        impl LiveSink for Sink {
            fn on_state(&self, _kind: &str, _detail: &str) {}
            fn on_event(&self, _json: &str) {}
        }
        let outcome = LiveClient::connect("not javascript", "{}", "1", "ttwid=x", Sink);
        assert!(matches!(outcome, Err(MobileError::Bundle(_))));
    }

    #[test]
    fn connect_refuses_an_empty_cookie_jar_before_touching_the_bundle() {
        struct Sink;
        impl LiveSink for Sink {
            fn on_state(&self, _kind: &str, _detail: &str) {}
            fn on_event(&self, _json: &str) {}
        }
        // The bundle is garbage, but the jar check runs first and wins.
        let outcome = LiveClient::connect("not javascript", "{}", "1", "", Sink);
        assert!(
            matches!(outcome, Err(MobileError::Sign(_))),
            "an empty jar must fail before the bundle is read"
        );
    }
}
