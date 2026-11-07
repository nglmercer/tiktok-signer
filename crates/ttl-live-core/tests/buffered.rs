use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
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
    gate: Arc<(Mutex<bool>, Condvar)>,
    delivered: Arc<Mutex<usize>>,
    errors: Arc<Mutex<Vec<Error>>>,
    total: usize,
    stop: watch::Sender<bool>,
}
impl LiveSink for Sink {
    fn on_state(&self, _: State, _: &str) {}
    fn on_error(&self, error: Error, _: bool) {
        self.errors.lock().unwrap().push(error);
    }
    fn on_batch(&self, _: ttl_live_events::EventBatch) {
        let (lock, ready) = &*self.gate;
        let opened = lock.lock().unwrap();
        let (opened, timeout) = ready
            .wait_timeout_while(opened, Duration::from_secs(5), |opened| !*opened)
            .unwrap();
        assert!(
            *opened && !timeout.timed_out(),
            "transport stalled behind callback"
        );
        let mut delivered = self.delivered.lock().unwrap();
        *delivered += 1;
        if *delivered == self.total {
            self.stop.send_replace(true);
        }
    }
}
fn preset() -> Preset {
    Preset::new(
        DevicePreset::chrome_linux(),
        LocationPreset::us_east(),
        ScreenPreset::FHD,
    )
}

async fn scenario(saturate: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let server_gate = gate.clone();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        ws.next().await.unwrap().unwrap();
        let count = if saturate { 100 } else { 4 };
        let mut acks = 0;
        'frames: for index in 1..=count {
            let batch = ttl_live_proto::ProtoMessageFetchResult {
                need_ack: true,
                messages: vec![ttl_live_proto::BaseProtoMessage {
                    method: "WebcastGiftMessage".into(),
                    payload: include_bytes!("../../../fixtures/events/deep-gift.pb").to_vec(),
                    ..Default::default()
                }],
                ..Default::default()
            };
            let frame = ttl_sign_core::proto::PushFrame {
                log_id: index,
                payload_type: "msg".into(),
                payload: batch.encode_to_vec(),
                ..Default::default()
            };
            if ws
                .send(tokio_tungstenite::tungstenite::Message::Binary(
                    frame.encode().into(),
                ))
                .await
                .is_err()
            {
                break;
            }
            loop {
                match ws.next().await {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) => {
                        let ack = ttl_sign_core::proto::PushFrame::decode(&bytes).unwrap();
                        if ack.payload_type == "ack" {
                            assert_eq!(ack.log_id, index);
                            acks += 1;
                            break;
                        }
                    }
                    Some(Ok(message)) if !message.is_close() => (),
                    _ => break 'frames,
                }
            }
        }
        if saturate {
            assert!((2..100).contains(&acks));
        } else {
            assert_eq!(acks, 4);
        }
        *server_gate.0.lock().unwrap() = true;
        server_gate.1.notify_all();
        while let Some(Ok(message)) = ws.next().await {
            if message.is_close() {
                break;
            }
        }
    });
    let (stop, rx) = watch::channel(false);
    let delivered = Arc::new(Mutex::new(0));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = Sink {
        gate,
        delivered: delivered.clone(),
        errors: errors.clone(),
        total: 4,
        stop,
    };
    tokio::time::timeout(
        Duration::from_secs(8),
        ttl_live_core::run_buffered(
            &Signer(format!("ws://{address}/?room_id=1")),
            &CookieJar::parse("ttwid=offline"),
            &preset(),
            "1",
            0,
            sink,
            rx,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .unwrap()
        .unwrap();
    if saturate {
        assert!(errors.lock().unwrap().contains(&Error::Backpressure));
    } else {
        assert!(errors.lock().unwrap().is_empty());
        assert_eq!(*delivered.lock().unwrap(), 4);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn blocked_callback_does_not_block_socket_or_ack_on_current_thread_runtime() {
    scenario(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn saturation_cancels_transport_and_reports_error_after_draining() {
    scenario(true).await;
}

#[tokio::test]
async fn callback_panic_interrupts_pending_signer() {
    struct Pending;
    impl SocketSigner for Pending {
        fn sign<'a>(&'a self, _: &'a str) -> SignFuture<'a> {
            Box::pin(std::future::pending())
        }
    }
    struct Panic;
    impl LiveSink for Panic {
        fn on_state(&self, _: State, _: &str) {
            panic!("test callback failure");
        }
        fn on_error(&self, _: Error, _: bool) {}
        fn on_batch(&self, _: ttl_live_events::EventBatch) {}
    }
    let (_stop, rx) = watch::channel(false);
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        ttl_live_core::run_buffered(
            &Pending,
            &CookieJar::parse("ttwid=offline"),
            &preset(),
            "1",
            0,
            Panic,
            rx,
        ),
    )
    .await
    .unwrap();
    assert!(result.is_err());
}
