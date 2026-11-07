//! Explicit loopback load test: real sockets, decoder and ACKs; no TikTok traffic.
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{watch, Barrier};
use ttl_live_core::{Error, LiveSink, SignFuture, SocketSigner, State};
use ttl_sign_core::{CookieJar, DevicePreset, LocationPreset, Preset, ScreenPreset};

struct Signer(String);
impl SocketSigner for Signer {
    fn sign<'a>(&'a self, _: &'a str) -> SignFuture<'a> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}
#[derive(Default)]
struct Metrics {
    delivered: usize,
    ack_write_us: Vec<u64>,
    decode_us: Vec<u64>,
    callback_us: Vec<u64>,
}
struct Sink {
    metrics: Arc<Mutex<Metrics>>,
    total: usize,
    stop: watch::Sender<bool>,
    expected: Arc<Vec<ttl_live_events::EventBatch>>,
    callback_delay: Duration,
}
impl LiveSink for Sink {
    fn on_state(&self, _: State, _: &str) {}
    fn on_error(&self, e: Error, _: bool) {
        panic!("transport error: {e:?}");
    }
    fn on_ack_duration(&self, duration: Duration) {
        self.metrics
            .lock()
            .unwrap()
            .ack_write_us
            .push(duration.as_micros() as u64);
    }
    fn on_decode_duration(&self, duration: Duration) {
        self.metrics
            .lock()
            .unwrap()
            .decode_us
            .push(duration.as_micros() as u64);
    }
    fn on_batch(&self, batch: ttl_live_events::EventBatch) {
        let started = Instant::now();
        let mut metrics = self.metrics.lock().unwrap();
        let index = metrics.delivered;
        assert_eq!(
            batch.events,
            self.expected[index % self.expected.len()].events
        );
        assert_eq!(batch.cursor, index.to_string());
        if !self.callback_delay.is_zero() {
            std::thread::sleep(self.callback_delay);
        }
        metrics
            .callback_us
            .push(started.elapsed().as_micros() as u64);
        metrics.delivered += 1;
        if metrics.delivered == self.total {
            self.stop.send_replace(true);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit load test; STRESS_CONNECTIONS and STRESS_BATCHES configure load"]
async fn concurrent_connections_preserve_delivery_ack_and_shutdown() {
    let setting = |name, default| {
        std::env::var(name)
            .map(|s| s.parse::<usize>().unwrap())
            .unwrap_or(default)
    };
    let connections = setting("STRESS_CONNECTIONS", 32);
    let batches = setting("STRESS_BATCHES", 100);
    assert!((1..=1024).contains(&connections) && (1..=10000).contains(&batches));
    let mut corpus = Vec::new();
    if let Ok(dir) = std::env::var("STRESS_CAPTURE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let dir = if dir.is_absolute() {
            dir
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(dir)
        };
        let mut paths = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "pb"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut bytes = 0;
        for path in paths.into_iter().take(256) {
            assert!(path.metadata().unwrap().len() <= 16 * 1024 * 1024);
            let payload = std::fs::read(path).unwrap();
            bytes += payload.len();
            assert!(bytes <= 64 * 1024 * 1024);
            ttl_live_events::decode_batch(&payload).unwrap();
            corpus.push(payload);
        }
        assert!(!corpus.is_empty(), "empty capture corpus");
    } else {
        corpus.push(
            ttl_live_proto::ProtoMessageFetchResult {
                messages: [
                    (
                        "WebcastGiftMessage",
                        include_bytes!("../../../fixtures/events/deep-gift.pb").to_vec(),
                    ),
                    ("WebcastFutureMessage", vec![8, 42]),
                    ("WebcastChatMessage", vec![255]),
                ]
                .into_iter()
                .map(|(method, payload)| ttl_live_proto::BaseProtoMessage {
                    method: method.into(),
                    payload,
                    ..Default::default()
                })
                .collect(),
                ..Default::default()
            }
            .encode_to_vec(),
        );
    }
    let corpus = Arc::new(corpus);
    let gzip = std::env::var_os("STRESS_GZIP").is_some();
    let buffered = std::env::var_os("STRESS_BUFFERED").is_some();
    let callback_delay = Duration::from_millis(setting("STRESS_CALLBACK_DELAY_MS", 0) as u64);
    assert!(callback_delay <= Duration::from_secs(1));
    // Prepare references and wire/compression once, before timing. Event bytes stay unchanged.
    let mut expected = Vec::new();
    let mut retained = 0usize;
    for payload in corpus.iter() {
        let batch = ttl_live_events::decode_batch(payload).unwrap();
        retained = retained
            .checked_add(batch.estimated_retained_bytes())
            .unwrap();
        assert!(
            retained <= 256 * 1024 * 1024,
            "reference corpus exceeds estimated memory budget"
        );
        expected.push(batch);
    }
    let expected = Arc::new(expected);
    let events_per_connection: usize = (0..batches)
        .map(|i| expected[i % expected.len()].events.len())
        .sum();
    let mut frames = Vec::with_capacity(batches);
    let mut frame_bytes = 0usize;
    for index in 0..batches {
        let mut payload = corpus[index % corpus.len()].clone();
        let mut metadata = ttl_sign_core::proto::Writer::new();
        metadata
            .str_field(2, &index.to_string())
            .bool_field(9, true);
        payload.extend_from_slice(&metadata.finish());
        if gzip {
            use std::io::Write;
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&payload).unwrap();
            payload = encoder.finish().unwrap();
        }
        let frame = ttl_sign_core::proto::PushFrame {
            log_id: index as u64 + 1,
            payload_type: "msg".into(),
            payload,
            headers: if gzip {
                vec![("compress_type".into(), "gzip".into())]
            } else {
                vec![]
            },
            ..Default::default()
        }
        .encode();
        frame_bytes += frame.len();
        assert!(
            frame_bytes <= 256 * 1024 * 1024,
            "prepared frames exceed memory budget; reduce STRESS_BATCHES"
        );
        frames.push(frame);
    }
    let frames = Arc::new(frames);
    let started = Instant::now();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let barrier = std::sync::Arc::new(Barrier::new(connections));
    let server = tokio::spawn(async move {
        let mut peers = tokio::task::JoinSet::new();
        for _ in 0..connections {
            let (stream, _) = listener.accept().await.unwrap();
            let barrier = barrier.clone();
            let frames = frames.clone();
            peers.spawn(async move {
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                ws.next().await.unwrap().unwrap();
                barrier.wait().await;
                let mut samples = Vec::with_capacity(batches);
                for (index, frame) in frames.iter().enumerate() {
                    let ack_started = Instant::now();
                    ws.send(tokio_tungstenite::tungstenite::Message::Binary(
                        frame.clone().into(),
                    ))
                    .await
                    .unwrap();
                    loop {
                        let incoming = ws.next().await.unwrap().unwrap();
                        if let tokio_tungstenite::tungstenite::Message::Binary(bytes) = incoming {
                            let ack = ttl_sign_core::proto::PushFrame::decode(&bytes).unwrap();
                            if ack.payload_type == "ack" {
                                assert_eq!(ack.log_id, index as u64 + 1);
                                samples.push(ack_started.elapsed().as_micros() as u64);
                                break;
                            }
                        }
                    }
                }
                while let Some(Ok(message)) = ws.next().await {
                    if message.is_close() {
                        break;
                    }
                }
                samples
            });
        }
        let mut samples = Vec::new();
        while let Some(result) = peers.join_next().await {
            samples.extend(result.unwrap());
        }
        samples
    });
    let mut clients = tokio::task::JoinSet::new();
    for _ in 0..connections {
        let expected = expected.clone();
        clients.spawn(async move {
            let (stop, rx) = watch::channel(false);
            let metrics = Arc::new(Mutex::new(Metrics::default()));
            let sink = Sink {
                metrics: metrics.clone(),
                total: batches,
                stop,
                expected,
                callback_delay,
            };
            let preset = Preset::new(
                DevicePreset::chrome_linux(),
                LocationPreset::us_east(),
                ScreenPreset::FHD,
            );
            let signer = Signer(format!("ws://{address}/?room_id=1"));
            let cookies = CookieJar::parse("ttwid=offline");
            if buffered {
                ttl_live_core::run_buffered(&signer, &cookies, &preset, "1", 0, sink, rx)
                    .await
                    .unwrap();
            } else {
                ttl_live_core::run(&signer, &cookies, &preset, "1", 0, &sink, rx).await;
            }
            let mut metrics = metrics.lock().unwrap();
            assert_eq!(metrics.delivered, batches);
            assert_eq!(metrics.ack_write_us.len(), batches);
            assert_eq!(metrics.decode_us.len(), batches);
            assert_eq!(metrics.callback_us.len(), batches);
            std::mem::take(&mut *metrics)
        });
    }
    let (mut samples, mut metrics) = tokio::time::timeout(Duration::from_secs(120), async {
        let mut metrics = Metrics::default();
        while let Some(result) = clients.join_next().await {
            let client = result.unwrap();
            metrics.ack_write_us.extend(client.ack_write_us);
            metrics.decode_us.extend(client.decode_us);
            metrics.callback_us.extend(client.callback_us);
        }
        (server.await.unwrap(), metrics)
    })
    .await
    .expect("load test stalled");
    fn report(name: &str, samples: &mut [u64]) {
        samples.sort_unstable();
        eprintln!(
            "{name}_us_p50={} p95={} p99={}",
            samples[samples.len() / 2],
            samples[samples.len() * 95 / 100],
            samples[samples.len() * 99 / 100]
        );
    }
    report("ack_write", &mut metrics.ack_write_us);
    report("decode", &mut metrics.decode_us);
    report("callback_including_verification", &mut metrics.callback_us);
    samples.sort_unstable();
    assert_eq!(samples.len(), connections * batches);
    eprintln!(
        "ack_roundtrip_us_p50={} p95={} p99={}",
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100],
        samples[samples.len() * 99 / 100]
    );
    eprintln!(
        "connections={connections} gzip={gzip} buffered={buffered} batches={} events={} elapsed={:?} batches_per_second={:.0}",
        connections * batches,
        connections * events_per_connection,
        started.elapsed(),
        (connections * batches) as f64 / started.elapsed().as_secs_f64()
    );
}
