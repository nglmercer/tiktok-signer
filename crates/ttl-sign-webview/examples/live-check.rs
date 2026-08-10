//! End-to-end verification against a real channel.
//!
//! Runs the complete flow and reports the step at which it fails:
//!
//! ```text
//! 1. discover who is live (rendered /live DOM, unsigned)
//! 2. unique_id → room_id + status (unsigned JSON lookup)
//! 3. navigate to the live page and let TikTok open its own WebSocket
//! 4. relay and decode the page-owned WebSocket frames over IPC
//! ```
//!
//! This is the F2 acceptance criterion (and F4 if frames arrive).
//!
//! ```sh
//! # discover a channel automatically
//! cargo run -p ttl-sign-webview --example live-check
//!
//! # or force a specific channel when you know it is live
//! cargo run -p ttl-sign-webview --example live-check -- user
//! ```
//!
//! Requires a display (X11/Wayland). Without one: `xvfb-run -a cargo run …`.

use std::time::{Duration, Instant};

use ttl_sign_core::proto::PushFrame;
use ttl_sign_webview::{run, session, EngineConfig, PageWebSocketEvent, Signer};

const PAGE_TRANSPORT_TIMEOUT: Duration = Duration::from_secs(45);
const REQUIRED_MESSAGE_FRAMES: usize = 3;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "live_check=info,ttl_sign_webview=info,ttl_live_ws=debug".into()
            }),
        )
        .init();

    let requested_user = std::env::args().nth(1);
    // The flow currently requires authentication; see step 3.
    let (config, session_source) = configure_session();
    println!("session: {session_source}");

    run(config, move |signer| {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
        let code = match rt.block_on(check(signer.clone(), requested_user)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("\nFAILED: {e}");
                1
            }
        };
        signer.shutdown_with_code(code);
    })
}

/// `TTL_SESSION_ID` takes precedence; otherwise use the session saved by `login`.
fn configure_session() -> (EngineConfig, String) {
    if let Ok(id) = std::env::var("TTL_SESSION_ID") {
        if !id.is_empty() {
            return (
                page_transport_config(EngineConfig::default().with_session_id(id)),
                "authenticated (TTL_SESSION_ID)".into(),
            );
        }
    }
    if let Some(path) = session::configured_path() {
        if let Ok(Some(jar)) = session::load(&path) {
            if session::is_logged_in(&jar) {
                let source = format!("authenticated ({})", path.display());
                return (
                    page_transport_config(EngineConfig {
                        session: jar,
                        ..EngineConfig::default()
                    }),
                    source,
                );
            }
        }
    }
    (
        page_transport_config(EngineConfig::default()),
        "anonymous — log in with: cargo run -p ttl-sign-webview --example login".into(),
    )
}

fn page_transport_config(mut config: EngineConfig) -> EngineConfig {
    config.block_page_websockets = false;
    config
}

async fn check(signer: Signer, requested_user: Option<String>) -> Result<(), String> {
    // --- 1. Who is live? -------------------------------------------------------------
    let user = match requested_user {
        Some(user) => {
            println!("[1/4] manually selected channel: @{user}");
            user
        }
        None => {
            println!("[1/4] waiting for /live to render channels…");
            let channels = poll_channels(&signer).await?;
            for channel in channels.iter().take(10) {
                println!(
                    "      @{}{}",
                    channel.unique_id,
                    if channel.room_id.is_empty() {
                        String::new()
                    } else {
                        format!("  room_id={}", channel.room_id)
                    }
                );
            }
            let first = channels
                .first()
                .ok_or("/live listed no channels (did it finish loading?)")?;
            println!("      selected: @{}", first.unique_id);
            first.unique_id.clone()
        }
    };

    // --- 2. unique_id → room_id ------------------------------------------------------
    println!("\n[2/4] resolving @{user} → room_id…");
    let lookup = signer
        .room_lookup(&user)
        .await
        .map_err(|e| format!("lookup failed: {e}"))?;
    println!(
        "      room_id={} status={} title={:?}",
        lookup.room_id, lookup.status, lookup.title
    );
    if !lookup.is_live() {
        return Err(format!(
            "@{user} is not live (status={}). Signing an offline room returns a protobuf \
             without push_server and looks like a rejection; try another channel.",
            lookup.status
        ));
    }

    // --- 3. Page-owned WebSocket -----------------------------------------------------
    //
    // This is intentionally independent of /webcast/im/fetch/. TikTok's own page signs
    // and opens the socket, enters the room, and sends heartbeats. The initialization
    // bridge mirrors transport frames to Rust without changing the page connection.
    let mut page_events = signer
        .subscribe_page_websocket()
        .await
        .map_err(|e| format!("could not subscribe to page WebSocket: {e}"))?;

    let room_page = ttl_sign_core::room::live_page_url(&user);
    println!("\n[3/4] navigating to {room_page} …");
    signer
        .navigate(&room_page)
        .await
        .map_err(|e| format!("could not load live page: {e}"))?;

    println!("[4/4] waiting for TikTok's page WebSocket and relayed frames…");
    let deadline = tokio::time::sleep(PAGE_TRANSPORT_TIMEOUT);
    tokio::pin!(deadline);

    let mut active_url: Option<String> = None;
    let mut binary_frames = 0usize;
    let mut message_frames = 0usize;
    let mut decode_errors = 0usize;

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = page_events.recv() => {
                let Some(event) = event else {
                    return Err("page WebSocket relay closed before the WebView".into());
                };
                match event {
                    PageWebSocketEvent::Open { url } if is_live_transport(&url) => {
                        println!(
                            "      page socket opened: {}",
                            url.split('?').next().unwrap_or(&url)
                        );
                        active_url = Some(url);
                    }
                    PageWebSocketEvent::Binary { url, data }
                        if active_url.as_deref() == Some(url.as_str()) =>
                    {
                        binary_frames += 1;
                        match PushFrame::decode(&data) {
                            Ok(frame) => {
                                println!(
                                    "      page frame #{binary_frames}: payload_type={:?} log_id={} {} bytes",
                                    frame.payload_type,
                                    frame.log_id,
                                    frame.payload.len()
                                );
                                if frame.payload_type == "msg" {
                                    message_frames += 1;
                                    if message_frames >= REQUIRED_MESSAGE_FRAMES {
                                        break;
                                    }
                                }
                            }
                            Err(error) => {
                                decode_errors += 1;
                                println!(
                                    "      page frame #{binary_frames}: {} raw bytes (decode failed: {error})",
                                    data.len()
                                );
                            }
                        }
                    }
                    PageWebSocketEvent::Text { url, text }
                        if active_url.as_deref() == Some(url.as_str()) =>
                    {
                        println!("      page text frame: {} bytes", text.len());
                    }
                    PageWebSocketEvent::Close { url, code, reason }
                        if active_url.as_deref() == Some(url.as_str()) =>
                    {
                        return Err(format!(
                            "TikTok's page WebSocket closed before enough frames: code={code} reason={reason:?}"
                        ));
                    }
                    PageWebSocketEvent::Error { url, message }
                        if active_url.as_deref() == Some(url.as_str())
                            || (active_url.is_none() && is_live_transport(&url)) =>
                    {
                        return Err(format!("TikTok's page WebSocket failed: {message}"));
                    }
                    _ => {}
                }
            }
        }
    }

    if active_url.is_none() {
        return Err(format!(
            "TikTok's page did not open a webcast WebSocket within {PAGE_TRANSPORT_TIMEOUT:?}. \
             The page may require visible playback or a fresh authenticated session."
        ));
    }
    if binary_frames == 0 {
        return Err(format!(
            "TikTok's page opened the WebSocket but relayed no binary frames within {PAGE_TRANSPORT_TIMEOUT:?}"
        ));
    }
    if message_frames == 0 {
        return Err(format!(
            "received {binary_frames} binary page frames, but none decoded as payload_type=msg \
             ({decode_errors} decode failures)"
        ));
    }

    println!(
        "\nOK: relayed {message_frames} message frames from @{user} through TikTok's own \
         page WebSocket; no Euler or custom signing endpoint was used."
    );
    Ok(())
}

fn is_live_transport(url: &str) -> bool {
    url.starts_with("wss://") && (url.contains("/webcast/im/") || url.contains("webcast-ws"))
}

/// The page takes time to render channels, so poll the DOM instead of reading it once.
async fn poll_channels(signer: &Signer) -> Result<Vec<ttl_sign_core::LiveChannel>, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match signer.live_channels().await {
            Ok(channels) if !channels.is_empty() => return Ok(channels),
            Ok(_) => {}
            Err(e) => return Err(format!("could not read DOM: {e}")),
        }
        if Instant::now() >= deadline {
            return Err("/live still listed no channels after 30 s".into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
