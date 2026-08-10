//! Dump the `/webcast/im/fetch/` response and its protobuf structure.
//!
//! This supports the F0/F1 workflow in `docs/02-roadmap.md`: keep the real response visible
//! and **confirm the field numbers** accepted by `ttl-sign-core::proto`, instead of
//! instead of relying on reference-client schemas.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example fetch-dump -- <live-user>
//! cargo run -p ttl-sign-webview --example fetch-dump -- <user> fixtures/f0/im_fetch.pb
//! ```
//!
//! The `.pb` contains session data; `fixtures/` is ignored by git for that reason.

use std::time::Duration;

use ttl_sign_core::proto::{describe, PushFrame, Reader, WireValue};
use ttl_sign_core::room::live_page_url;
use ttl_sign_core::{FetchResult, SignOutcome};
use ttl_sign_webview::{run, session, EngineConfig, Signer};

const FIELD_MESSAGES: u32 = 1;
const FIELD_CURSOR: u32 = 2;
const FIELD_INTERNAL_EXT: u32 = 5;
const FIELD_ROUTE_PARAMS: u32 = 7;
const FIELD_PUSH_SERVER: u32 = 10;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=info")
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: fetch-dump <live-user> [file.pb]");
        std::process::exit(2);
    });
    let out_path = std::env::args().nth(2);

    let session = session::configured_path()
        .and_then(|p| session::load(&p).ok().flatten())
        .unwrap_or_default();
    if !session::is_logged_in(&session) {
        eprintln!("session required: cargo run -p ttl-sign-webview --example login");
        std::process::exit(1);
    }

    let config = EngineConfig {
        landing_url: live_page_url(&user),
        sign_timeout: Duration::from_secs(30),
        session,
        ..EngineConfig::default()
    };

    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
        rt.block_on(async move {
            let lookup = signer.room_lookup(&user).await.expect("lookup");
            println!("@{} room_id={}", lookup.unique_id, lookup.room_id);

            // Compare which push_server each sup_ws_ds_opt variant returns.
            for opt in [1u8, 0] {
                let mut params = ttl_sign_core::FetchParams::new(&lookup.room_id);
                params.sup_ws_ds_opt = opt;
                match signer.fetch_with(params).await {
                    SignOutcome::Ok(s) => match FetchResult::decode(&s.protobuf) {
                        Ok(r) => println!(
                            "sup_ws_ds_opt={opt} → {} ({} route_params, {} bytes)",
                            r.push_server,
                            r.route_params.len(),
                            s.protobuf.len()
                        ),
                        Err(e) => println!("sup_ws_ds_opt={opt} → unreadable protobuf: {e}"),
                    },
                    other => println!("sup_ws_ds_opt={opt} → {other:?}"),
                }
            }
            println!();

            let signed = match signer.fetch(&lookup.room_id).await {
                SignOutcome::Ok(signed) => signed,
                other => {
                    eprintln!("signing failed: {other:?}");
                    std::process::exit(1);
                }
            };
            println!("{} protobuf bytes\n", signed.protobuf.len());

            if let Some(path) = &out_path {
                std::fs::write(path, &signed.protobuf).expect("could not write protobuf");
                println!("saved to {path}\n");
            }

            // --- Raw structure: field by field, without interpretation ---
            println!("top-level fields:");
            let mut messages = Vec::new();
            for (number, kind, size) in describe(&signed.protobuf).expect("unreadable protobuf") {
                let note = match number {
                    FIELD_MESSAGES => " (messages?)",
                    FIELD_CURSOR => " (cursor?)",
                    FIELD_INTERNAL_EXT => " (internal_ext?)",
                    FIELD_ROUTE_PARAMS => " (route_params?)",
                    FIELD_PUSH_SERVER => " (push_server?)",
                    _ => "",
                };
                println!("  field {number:<5} {kind:<8} {size:>7} bytes{note}");
                if number == FIELD_MESSAGES {
                    messages.push(size);
                }
            }
            println!("  → {} entries in field {FIELD_MESSAGES}", messages.len());

            // --- What we extract today ---
            let result = FetchResult::decode(&signed.protobuf).expect("decode");
            println!("\nFetchResult:");
            println!("  push_server ......... {}", result.push_server);
            println!("  cursor .............. {:?}", result.cursor);
            println!(
                "  internal_ext ........ {} chars",
                result.internal_ext.len()
            );
            println!("  heartbeat_duration .. {}", result.heartbeat_duration);
            println!("  need_ack ............ {}", result.need_ack);
            println!("  route_params ({}):", result.route_params.len());
            for (k, v) in &result.route_params {
                println!("      {k} = {}", shorten(v));
            }

            // --- Messages already present in this response ---
            println!("\nmessages embedded in the response:");
            let mut reader = Reader::new(&signed.protobuf);
            let mut seen = 0usize;
            while let Some(Ok((number, wire))) = reader.next_field() {
                if number != FIELD_MESSAGES {
                    continue;
                }
                if let WireValue::Bytes(bytes) = wire {
                    if let Ok(frame) = PushFrame::decode(bytes) {
                        seen += 1;
                        if seen <= 8 {
                            println!(
                                "  #{seen} payload_type={:?} encoding={:?} payload={} bytes",
                                frame.payload_type,
                                frame.payload_encoding,
                                frame.payload.len()
                            );
                        }
                    }
                }
            }
            println!("  total: {seen}");

            signer.shutdown();
            std::process::exit(0);
        })
    })
}

fn shorten(s: &str) -> String {
    if s.chars().count() <= 70 {
        return s.to_string();
    }
    format!("{}…", s.chars().take(70).collect::<String>())
}
