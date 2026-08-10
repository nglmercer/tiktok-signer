//! Verify the generated TikTok Webcast schema against a real page-owned WebSocket.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example schema-check -- <live-user>
//! ```

use std::time::Duration;

use ttl_sign_core::SchemaValue;
use ttl_sign_webview::{run, session, DecodedSchemaEvent, EngineConfig, Signer};

const EVENT_TIMEOUT: Duration = Duration::from_secs(45);
const PAGE_READY_TIMEOUT: Duration = Duration::from_secs(45);
const REQUIRED_SCHEMA_EVENTS: usize = 3;
const MAX_CONTENT_CHARS: usize = 240;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_webview=info".into()),
        )
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: schema-check <live-user>");
        std::process::exit(2);
    });
    let (config, source) = configured_engine();
    println!("session: {source}");

    run(config, move |signer| {
        let runtime = tokio::runtime::Runtime::new().expect("Tokio runtime");
        let exit_code = match runtime.block_on(check(signer.clone(), user)) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("\nFAILED: {error}");
                1
            }
        };
        signer.shutdown_with_code(exit_code);
    })
}

fn configured_engine() -> (EngineConfig, String) {
    let mut config = EngineConfig {
        block_page_websockets: false,
        sign_timeout: PAGE_READY_TIMEOUT,
        sdk_ready_timeout: PAGE_READY_TIMEOUT,
        ..EngineConfig::default()
    };

    if let Ok(session_id) = std::env::var("TTL_SESSION_ID") {
        if !session_id.is_empty() {
            config = config.with_session_id(session_id);
            return (config, "authenticated (TTL_SESSION_ID)".into());
        }
    }
    if let Some(path) = session::configured_path() {
        if let Ok(Some(cookies)) = session::load(&path) {
            if session::is_logged_in(&cookies) {
                config.session = cookies;
                return (config, format!("authenticated ({})", path.display()));
            }
        }
    }
    (
        config,
        "anonymous — log in with: cargo run -p ttl-sign-webview --example login".into(),
    )
}

async fn check(signer: Signer, user: String) -> Result<(), String> {
    let room = signer
        .room_lookup(&user)
        .await
        .map_err(|error| format!("room lookup failed: {error}"))?;
    println!(
        "room: @{} id={} status={} title={:?}",
        room.unique_id, room.room_id, room.status, room.title
    );
    if !room.is_live() {
        return Err(format!("@{user} is not live (status={})", room.status));
    }

    let mut events = signer
        .subscribe_schema_events()
        .await
        .map_err(|error| format!("could not subscribe to schema events: {error}"))?;
    let url = ttl_sign_core::room::live_page_url(&user);
    println!("navigating to {url}");
    signer
        .navigate(&url)
        .await
        .map_err(|error| format!("could not navigate: {error}"))?;

    println!("waiting for schema-registry events…");
    let deadline = tokio::time::sleep(EVENT_TIMEOUT);
    tokio::pin!(deadline);
    let mut received = 0usize;
    let mut decode_errors = 0usize;

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = events.recv() => {
                let Some(event) = event else {
                    return Err("schema event relay closed before the WebView".into());
                };
                match event {
                    Ok(event) => {
                        received += 1;
                        print_event(&event);
                        if received >= REQUIRED_SCHEMA_EVENTS {
                            println!(
                                "\nOK: received {received} schema-registry events from @{user}; \
                                 TikTok's page owns the WebSocket and signing."
                            );
                            return Ok(());
                        }
                    }
                    Err(error) => {
                        decode_errors += 1;
                        println!("[schema-decode-error] {error}");
                    }
                }
            }
        }
    }

    Err(format!(
        "received {received} generated-schema events within {EVENT_TIMEOUT:?} \
         ({decode_errors} decode failures)"
    ))
}

fn print_event(event: &DecodedSchemaEvent) {
    println!(
        "[schema] frame={} id={} method={} type={}",
        event.frame_log_id,
        event.message_id,
        event.method,
        event.event.schema_name()
    );
    if let Some(field) = event.event.field_named("content") {
        if let SchemaValue::Text(content) = &field.value {
            println!("         chat={}", single_line(content));
        }
    }
}

fn single_line(text: &str) -> String {
    let mut output: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX_CONTENT_CHARS)
        .collect();
    if text.chars().count() > MAX_CONTENT_CHARS {
        output.push('…');
    }
    output
}
