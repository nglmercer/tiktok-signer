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
        let rt = tokio::runtime::Runtime::new().expect("runtime de tokio");
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
                        Err(e) => println!("sup_ws_ds_opt={opt} → protobuf ilegible: {e}"),
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
            println!("{} bytes de protobuf\n", signed.protobuf.len());

            if let Some(path) = &out_path {
                std::fs::write(path, &signed.protobuf).expect("could not write protobuf");
                println!("guardado en {path}\n");
            }

            // --- Estructura cruda: campo por campo, sin interpretar ---
            println!("top-level fields:");
            let mut messages = Vec::new();
            for (number, kind, size) in describe(&signed.protobuf).expect("protobuf ilegible") {
                let nota = match number {
                    1 => " (messages?)",
                    2 => " (cursor?)",
                    5 => " (internal_ext?)",
                    7 => " (route_params?)",
                    10 => " (push_server?)",
                    _ => "",
                };
                println!("  campo {number:<5} {kind:<8} {size:>7} bytes{nota}");
                if number == 1 {
                    messages.push(size);
                }
            }
            println!("  → {} entradas en el campo 1", messages.len());

            // --- Lo que sacamos hoy ---
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
                println!("      {k} = {}", recorta(v));
            }

            // --- Los mensajes que ya vienen en esta respuesta ---
            println!("\nmessages embedded in the response:");
            let mut reader = Reader::new(&signed.protobuf);
            let mut vistos = 0usize;
            while let Some(Ok((number, wire))) = reader.next_field() {
                if number != 1 {
                    continue;
                }
                if let WireValue::Bytes(bytes) = wire {
                    if let Ok(frame) = PushFrame::decode(bytes) {
                        vistos += 1;
                        if vistos <= 8 {
                            println!(
                                "  #{vistos} payload_type={:?} encoding={:?} payload={} bytes",
                                frame.payload_type,
                                frame.payload_encoding,
                                frame.payload.len()
                            );
                        }
                    }
                }
            }
            println!("  total: {vistos}");

            signer.shutdown();
            std::process::exit(0);
        })
    })
}

fn recorta(s: &str) -> String {
    if s.chars().count() <= 70 {
        return s.to_string();
    }
    format!("{}…", s.chars().take(70).collect::<String>())
}
