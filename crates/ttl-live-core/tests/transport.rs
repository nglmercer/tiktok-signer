use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
use ttl_live_core::{Error, LiveSink, SignFuture, SocketSigner, State};
use ttl_sign_core::{CookieJar, DevicePreset, LocationPreset, Preset, ScreenPreset};
struct Signer(String);
impl SocketSigner for Signer {
    fn sign<'a>(&'a self, _: &'a str) -> SignFuture<'a> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}
struct Sink {
    metrics: Arc<Mutex<(u64, u64)>>,
    events: Arc<Mutex<Vec<String>>>,
    stop: watch::Sender<bool>,
}
impl LiveSink for Sink {
    fn on_decode_duration(&self, _: Duration) {
        self.metrics.lock().unwrap().0 += 1;
    }
    fn on_ack_duration(&self, _: Duration) {
        self.metrics.lock().unwrap().1 += 1;
    }
    fn on_state(&self, _: State, _: &str) {}
    fn on_error(&self, _: Error, _: bool) {}
    fn on_batch(&self, b: ttl_live_events::EventBatch) {
        self.events
            .lock()
            .unwrap()
            .extend(b.events.iter().map(|e| e.method().to_owned()));
        self.stop.send_replace(true);
    }
}
fn preset() -> Preset {
    Preset::new(
        DevicePreset::chrome_linux(),
        LocationPreset::us_east(),
        ScreenPreset::FHD,
    )
}
#[tokio::test]
async fn actual_websocket_delivers_all_methods_and_stops_without_callbacks_after_join() {
    use futures_util::{SinkExt, StreamExt};
    use prost::Message;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap(); // room-entry handshake
        let batch = ttl_live_proto::ProtoMessageFetchResult {
            need_ack: true,
            messages: vec![
                ttl_live_proto::BaseProtoMessage {
                    method: "WebcastChatMessage".into(),
                    payload: vec![26, 2, b'h', b'i'],
                    ..Default::default()
                },
                ttl_live_proto::BaseProtoMessage {
                    method: "WebcastLiveIntroMessage".into(),
                    payload: vec![],
                    ..Default::default()
                },
                ttl_live_proto::BaseProtoMessage {
                    method: "WebcastFutureMessage".into(),
                    payload: vec![8, 42],
                    ..Default::default()
                },
                ttl_live_proto::BaseProtoMessage {
                    method: "WebcastChatMessage".into(),
                    payload: vec![255],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let frame = ttl_sign_core::proto::PushFrame {
            payload_type: "msg".into(),
            payload: batch.encode_to_vec(),
            ..Default::default()
        };
        ws.send(tokio_tungstenite::tungstenite::Message::Binary(
            frame.encode().into(),
        ))
        .await
        .unwrap();
        while let Some(Ok(m)) = ws.next().await {
            if m.is_close() {
                break;
            }
        }
    });
    let (stop, rx) = watch::channel(false);
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Sink {
        metrics: Arc::new(Mutex::new((0, 0))),
        events: events.clone(),
        stop,
    };
    tokio::time::timeout(
        Duration::from_secs(2),
        ttl_live_core::run(
            &Signer(format!("ws://127.0.0.1:{port}/?room_id=1")),
            &CookieJar::parse("ttwid=test"),
            &preset(),
            "1",
            0,
            &sink,
            rx,
        ),
    )
    .await
    .unwrap();
    assert_eq!(*sink.metrics.lock().unwrap(), (1, 1));
    assert_eq!(
        *events.lock().unwrap(),
        [
            "WebcastChatMessage",
            "WebcastLiveIntroMessage",
            "WebcastFutureMessage",
            "WebcastChatMessage"
        ]
    );
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn cancellation_interrupts_pending_signer() {
    struct Pending;
    impl SocketSigner for Pending {
        fn sign<'a>(&'a self, _: &'a str) -> SignFuture<'a> {
            Box::pin(std::future::pending())
        }
    }
    let (stop, rx) = watch::channel(false);
    let sink = Sink {
        metrics: Arc::new(Mutex::new((0, 0))),
        events: Arc::new(Mutex::new(Vec::new())),
        stop: stop.clone(),
    };
    let cancel = tokio::spawn(async move {
        tokio::task::yield_now().await;
        stop.send_replace(true);
    });
    tokio::time::timeout(
        Duration::from_millis(100),
        ttl_live_core::run(
            &Pending,
            &CookieJar::parse("ttwid=test"),
            &preset(),
            "1",
            5,
            &sink,
            rx,
        ),
    )
    .await
    .unwrap();
    cancel.await.unwrap();
}
#[tokio::test]
async fn cancellation_interrupts_reconnect_backoff() {
    struct StopSink(watch::Sender<bool>);
    impl LiveSink for StopSink {
        fn on_state(&self, state: State, _: &str) {
            if state == State::Reconnecting {
                self.0.send_replace(true);
            }
        }
        fn on_error(&self, _: Error, _: bool) {}
        fn on_batch(&self, _: ttl_live_events::EventBatch) {}
    }
    let (stop, rx) = watch::channel(false);
    tokio::time::timeout(
        Duration::from_millis(100),
        ttl_live_core::run(
            &Signer("invalid URI".into()),
            &CookieJar::parse("ttwid=test"),
            &preset(),
            "1",
            5,
            &StopSink(stop),
            rx,
        ),
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn refused_handshake_is_structured_and_never_retried() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    struct Errors(Arc<Mutex<Vec<(Error, bool)>>>);
    impl LiveSink for Errors {
        fn on_state(&self, _: State, _: &str) {}
        fn on_batch(&self, _: ttl_live_events::EventBatch) {}
        fn on_error(&self, e: Error, retryable: bool) {
            self.0.lock().unwrap().push((e, retryable));
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
            assert!(request.len() < 8192);
        }
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let (_stop, rx) = watch::channel(false);
    let errors = Arc::new(Mutex::new(Vec::new()));
    tokio::time::timeout(
        Duration::from_secs(1),
        ttl_live_core::run(
            &Signer(format!("ws://127.0.0.1:{port}/?room_id=1")),
            &CookieJar::parse("ttwid=test"),
            &preset(),
            "1",
            5,
            &Errors(errors.clone()),
            rx,
        ),
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert_eq!(*errors.lock().unwrap(), [(Error::Auth(403), false)]);
}

#[tokio::test]
async fn cancellation_interrupts_blocked_handshake_and_socket_read() {
    for handshake in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            if handshake {
                entered.send(()).unwrap();
                let _stream = stream;
                std::future::pending::<()>().await;
            } else {
                use futures_util::StreamExt;
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                ws.next().await.unwrap().unwrap();
                entered.send(()).unwrap();
                while ws.next().await.is_some() {}
            }
        });
        let (stop, rx) = watch::channel(false);
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Sink {
            metrics: Arc::new(Mutex::new((0, 0))),
            events: events.clone(),
            stop: stop.clone(),
        };
        let worker = tokio::spawn(async move {
            ttl_live_core::run(
                &Signer(format!("ws://{address}/?room_id=1")),
                &CookieJar::parse("ttwid=test"),
                &preset(),
                "1",
                5,
                &sink,
                rx,
            )
            .await;
        });
        tokio::time::timeout(Duration::from_secs(2), ready)
            .await
            .unwrap()
            .unwrap();
        stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(events.lock().unwrap().is_empty());
        peer.abort();
        let _ = peer.await;
    }
}
