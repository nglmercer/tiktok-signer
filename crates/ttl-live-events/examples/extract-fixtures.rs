//! Extracts per-method event fixtures from a captured batch.
//!
//! The captured `.pb` is a full `ProtoMessageFetchResult`; the golden tests want
//! one file per event type holding just that event's `BaseProtoMessage.payload`.
//!
//! ```sh
//! cargo run -p ttl-live-events --example extract-fixtures -- \
//!     fixtures/f0/im_fetch.pb fixtures/events
//! ```
//!
//! Fixtures are captured from real traffic and committed, so unit tests never
//! need a live room.

use std::collections::HashMap;
use std::path::PathBuf;
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: extract-fixtures <batch.pb> <out-dir>")?,
    );
    let out_dir = PathBuf::from(
        args.next()
            .ok_or("usage: extract-fixtures <batch.pb> <out-dir>")?,
    );

    let batch = ttl_live_proto::decode_event_batch(&fs::read(&input)?)?;
    fs::create_dir_all(&out_dir)?;

    // One fixture per method: the first occurrence wins, so re-running against
    // the same capture is deterministic.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for message in &batch.messages {
        let count = seen.entry(message.method.as_str()).or_default();
        *count += 1;
        if *count > 1 {
            continue;
        }
        let name = format!("{}.pb", slug(&message.method));
        fs::write(out_dir.join(&name), &message.payload)?;
        println!("{name}  ({} bytes)", message.payload.len());
    }

    let mut summary: Vec<_> = seen.into_iter().collect();
    summary.sort();
    println!("\n{} methods in {}:", summary.len(), input.display());
    for (method, count) in summary {
        println!("  {count:3}  {method}");
    }
    Ok(())
}

/// `WebcastChatMessage` -> `chat`, so fixture names read as event names.
fn slug(method: &str) -> String {
    let trimmed = method
        .strip_prefix("Webcast")
        .unwrap_or(method)
        .strip_suffix("Message")
        .unwrap_or(method);

    let mut out = String::new();
    for (index, ch) in trimmed.char_indices() {
        if ch.is_uppercase() && index > 0 {
            out.push('-');
        }
        out.extend(ch.to_lowercase());
    }
    out
}
