//! Tail a room's live events through `LiveClient`, printing a compact line per event.
//!
//! Manual verification for the mobile live worker — the same flow the Android app's
//! Connect button drives, minus JNI:
//!
//! ```sh
//! curl -s -c /tmp/live-jar.txt -A "Mozilla/5.0" https://www.tiktok.com/live -o /dev/null
//! TTL_BUNDLE=/tmp/webmssdk.js TTL_COOKIES="ttwid=..." \
//!   cargo run -p ttl-sign-mobile --features live --example live_tail -- <room_id> [seconds]
//! ```
//!
//! Prints states plus one line per event (type, user, and a short detail — never a
//! full payload). AUTHORIZED USE ONLY: this opens a real signed socket.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use ttl_sign_mobile::live::{LiveClient, LiveSink};

struct Print {
    events: mpsc::Sender<String>,
    states: mpsc::Sender<(String, String)>,
}

impl LiveSink for Print {
    fn on_state(&self, kind: &str, detail: &str) {
        let _ = self.states.send((kind.to_string(), detail.to_string()));
    }

    fn on_event(&self, json: &str) {
        let _ = self.events.send(json.to_string());
    }
}

/// One compact line per event: type, user, and a short detail.
fn summarize(json: &str) -> String {
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(_) => return format!("unparsable event ({} bytes)", json.len()),
    };
    let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("?");
    let user = value
        .get("user")
        .and_then(|u| u.get("unique_id"))
        .and_then(|u| u.as_str())
        .unwrap_or("-");
    let detail = match kind {
        "chat" => value
            .get("comment")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .chars()
            .take(60)
            .collect(),
        "gift" => format!(
            "{} x{}",
            value
                .get("gift_name")
                .and_then(|v| v.as_str())
                .unwrap_or("?"),
            value
                .get("repeat_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        ),
        "like" => format!(
            "x{}",
            value.get("count").and_then(|v| v.as_u64()).unwrap_or(0)
        ),
        "member" => format!(
            "action={} in room",
            value.get("action").and_then(|v| v.as_i64()).unwrap_or(-1)
        ),
        "social" => "follow/share".to_string(),
        "room_user" => format!(
            "total={}",
            value.get("total").and_then(|v| v.as_u64()).unwrap_or(0)
        ),
        _ => format!(
            "method={}",
            value.get("method").and_then(|v| v.as_str()).unwrap_or("?")
        ),
    };
    format!("{kind:9} @{user:20} {detail}")
}

fn main() {
    let room_id = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: live_tail <room_id> [seconds]");
        std::process::exit(2);
    });
    let seconds: u64 = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let bundle_path = std::env::var("TTL_BUNDLE").unwrap_or_else(|_| "/tmp/webmssdk.js".into());
    let bundle = std::fs::read_to_string(&bundle_path).unwrap_or_else(|error| {
        eprintln!("could not read {bundle_path}: {error}");
        std::process::exit(2);
    });
    let cookies = std::env::var("TTL_COOKIES").unwrap_or_else(|_| {
        eprintln!("set TTL_COOKIES to a guest cookie header (ttwid=...)");
        std::process::exit(2);
    });

    let (events_tx, events) = mpsc::channel();
    let (states_tx, states) = mpsc::channel();
    let client = LiveClient::connect(
        &bundle,
        "{}",
        &room_id,
        &cookies,
        Print {
            events: events_tx,
            states: states_tx,
        },
    )
    .unwrap_or_else(|error| {
        eprintln!("could not start: {error}");
        std::process::exit(1);
    });

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut count = 0usize;
    while Instant::now() < deadline {
        for (kind, detail) in states.try_iter() {
            println!("[state] {kind}: {detail}");
        }
        for json in events.try_iter() {
            count += 1;
            if count <= 40 {
                println!("[event] {}", summarize(&json));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("--- disconnecting after {count} events ---");
    client.disconnect();
}
