//! Mobile compatibility frontend over the shared cancellable LIVE worker.
use crate::{mobile_preset, MobileError, MobileSigner, Product};
use std::{sync::Arc, thread};
use tokio::sync::watch;
use ttl_live_events::LiveEvent;
use ttl_sign_core::CookieJar;

/// Callbacks are serialized on the LIVE worker. Legacy JSON is unchanged.
pub trait LiveSink: Send + 'static {
    fn on_state(&self, kind: &str, detail: &str);
    fn on_event(&self, json: &str);
}
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

pub struct LiveClient {
    stop: watch::Sender<bool>,
    worker: Option<thread::JoinHandle<()>>,
}
struct Signer(Arc<MobileSigner>);
impl ttl_live_core::SocketSigner for Signer {
    fn sign<'a>(&'a self, url: &'a str) -> ttl_live_core::SignFuture<'a> {
        Box::pin(async move { self.0.sign_async(url, Product::Ws).await.map_err(|_| ()) })
    }
}
struct Sink(Box<dyn LiveSink>);
impl ttl_live_core::LiveSink for Sink {
    fn on_state(&self, state: ttl_live_core::State, detail: &str) {
        use ttl_live_core::State::*;
        self.0.on_state(
            match state {
                Disconnected => "closed",
                Connecting => "connecting",
                Connected => "open",
                Reconnecting => "reconnecting",
                Offline => "offline",
                Error => "error",
            },
            detail,
        );
    }
    fn on_error(&self, error: ttl_live_core::Error, _retryable: bool) {
        self.0.on_state(
            if error == ttl_live_core::Error::Decode {
                "decode_error"
            } else {
                "error"
            },
            "LIVE operation failed",
        );
    }
    fn on_batch(&self, batch: ttl_live_events::EventBatch) {
        for event in &batch.events {
            self.0.on_event(&event_json(&event.event));
        }
    }
}
impl LiveClient {
    pub fn connect(
        bundle: &str,
        options_json: &str,
        room_id: &str,
        cookie_header: &str,
        sink: impl LiveSink,
    ) -> Result<Self, MobileError> {
        Self::connect_with_retry(bundle, options_json, room_id, cookie_header, 5, sink)
    }
    pub fn connect_with_retry(
        bundle: &str,
        options_json: &str,
        room_id: &str,
        cookie_header: &str,
        max_attempts: u32,
        sink: impl LiveSink,
    ) -> Result<Self, MobileError> {
        let cookies = CookieJar::parse(cookie_header);
        if cookies.is_empty() {
            return Err(MobileError::Sign(
                "the handshake needs guest cookies; an empty jar is refused".into(),
            ));
        }
        let signer = Signer(Arc::new(MobileSigner::new(bundle, options_json)?));
        let (stop, rx) = watch::channel(false);
        let room = room_id.to_owned();
        let worker = thread::Builder::new()
            .name("ttl-live-mobile".into())
            .spawn(move || {
                let sink = Sink(Box::new(sink));
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt.block_on(ttl_live_core::run(
                        &signer,
                        &cookies,
                        &mobile_preset(),
                        &room,
                        max_attempts.saturating_sub(1),
                        &sink,
                        rx,
                    )),
                    Err(_) => sink.0.on_state("error", "no async runtime"),
                }
            })
            .map_err(|e| MobileError::Engine(e.to_string()))?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
    pub fn disconnect(mut self) {
        self.stop.send_replace(true);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
impl Drop for LiveClient {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        if let Some(w) = self.worker.take() {
            if w.thread().id() != thread::current().id() {
                let _ = w.join();
            }
        }
    }
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
