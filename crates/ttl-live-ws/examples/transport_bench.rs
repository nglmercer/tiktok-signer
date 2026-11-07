//! Offline loopback ACK/decompression benchmark; no discovery, signing or TikTok traffic.
use futures_util::{SinkExt, StreamExt};
use std::{hint::black_box, io::Write, time::Instant};
use ttl_live_ws::{ConnectConfig, LiveConnection};
use ttl_sign_core::{proto::PushFrame, CookieJar};
#[path = "../../../scripts/bench/counting_allocator.rs"]
mod counting_allocator;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let gzip = std::env::args().nth(1).as_deref() == Some("gzip");
    let iterations: usize = std::env::args()
        .nth(2)
        .map(|s| s.parse().expect("iterations"))
        .unwrap_or(1000);
    assert!((100..=10000).contains(&iterations));
    let mut batch = include_bytes!("../../../fixtures/events/batch.pb").to_vec();
    // The original HTTP capture has need_ack=false. This benchmark explicitly requests ACKs.
    batch.extend_from_slice(&[0x48, 1]);
    let payload = if gzip {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&batch)?;
        encoder.finish()?
    } else {
        batch.to_vec()
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut samples = Vec::with_capacity(iterations);
        for log_id in 1..=iterations as u64 {
            let frame = PushFrame {
                log_id,
                payload_type: "msg".into(),
                payload: payload.clone(),
                headers: if gzip {
                    vec![("compress_type".into(), "gzip".into())]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            let started = Instant::now();
            ws.send(tokio_tungstenite::tungstenite::Message::Binary(
                frame.encode().into(),
            ))
            .await
            .unwrap();
            loop {
                let incoming = ws.next().await.unwrap().unwrap();
                if let tokio_tungstenite::tungstenite::Message::Binary(bytes) = incoming {
                    let ack = PushFrame::decode(&bytes).unwrap();
                    if ack.payload_type == "ack" {
                        assert_eq!(ack.log_id, log_id);
                        samples.push(started.elapsed().as_nanos());
                        break;
                    }
                }
            }
        }
        while let Some(Ok(incoming)) = ws.next().await {
            if incoming.is_close() {
                break;
            }
        }
        samples
    });
    let mut connection = LiveConnection::open_uri(
        &format!("ws://{address}/?room_id=7300"),
        &CookieJar::parse("ttwid=offline"),
        "offline",
        "",
        &ConnectConfig::default(),
    )
    .await?;
    counting_allocator::reset();
    let started = Instant::now();
    for _ in 0..iterations {
        let message =
            tokio::time::timeout(std::time::Duration::from_secs(2), connection.next_message())
                .await?
                .expect("message")?;
        assert_eq!(message.payload, batch);
        black_box(message);
    }
    connection.close().await;
    let mut samples = tokio::time::timeout(std::time::Duration::from_secs(2), peer).await??;
    println!("allocation_unit=batch_including_loopback_peer");
    counting_allocator::report(iterations);
    samples.sort_unstable();
    println!("gzip={gzip} batches_per_second={:.0} ack_roundtrip_ns_p50={} ack_roundtrip_ns_p95={} ack_roundtrip_ns_p99={}",
        iterations as f64 / started.elapsed().as_secs_f64(), samples[iterations/2], samples[iterations*95/100], samples[iterations*99/100]);
    Ok(())
}
