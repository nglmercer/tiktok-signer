//! End-to-end verification against a real channel.
//!
//! Runs the complete flow and reports the step at which it fails:
//!
//! ```text
//! 1. discover who is live (rendered /live DOM, unsigned)
//! 2. unique_id → room_id + status (unsigned JSON lookup)
//! 3. navigate to the live page and let TikTok open its own WebSocket
//! 4. read room metadata and the gift table (signed by the page, read as JSON)
//! 5. relay and decode the page-owned WebSocket frames over IPC
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

use std::io::Read;
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use ttl_live_events::{decode_webcast_message, LiveEvent, SchemaMessage, SchemaValue};
use ttl_sign_core::proto::{PushFrame, WebcastEventBatch};
use ttl_sign_core::{Gift, GiftStreaks, RoomInfo};
use ttl_sign_webview::{is_webcast_socket, run, session, EngineConfig, PageWebSocketEvent, Signer};

const PAGE_TRANSPORT_TIMEOUT: Duration = Duration::from_secs(45);
const REQUIRED_MESSAGE_FRAMES: usize = 3;
const MAX_EVENT_TEXT_CHARS: usize = 80;
/// Fields shown per schema event; enough to identify it without flooding the log.
const MAX_SUMMARY_FIELDS: usize = 6;
/// The gift table is a few megabytes of JSON crossing the IPC bridge.
const REST_TIMEOUT: Duration = Duration::from_secs(60);

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "live_check=info,ttl_sign_webview=info,ttl_live_ws=debug".into()
            }),
        )
        .init();

    let requested_user = std::env::args().nth(1);
    // A session is optional: the whole flow was verified anonymously. One is used when
    // available only because it is the closer match to a real viewer.
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

/// Guest by default; an account is opt-in.
///
/// Listening needs no identity, and `sessionid` *is* the account: attaching one makes TikTok
/// attribute everything this automated browser does to a real user. A stored session is
/// therefore used only when explicitly asked for, via `TTL_SESSION_ID` or `TTL_USE_SESSION=1`.
fn configure_session() -> (EngineConfig, String) {
    if let Ok(id) = std::env::var("TTL_SESSION_ID") {
        if !id.is_empty() {
            return (
                page_transport_config(EngineConfig::default().with_session_id(id)),
                "authenticated (TTL_SESSION_ID)".into(),
            );
        }
    }
    if std::env::var("TTL_USE_SESSION").is_ok_and(|value| value == "1") {
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
        return (
            page_transport_config(EngineConfig::default()),
            "guest (TTL_USE_SESSION=1 was set, but no stored session was found)".into(),
        );
    }
    (
        page_transport_config(EngineConfig::default()),
        "guest — no account needed to listen (TTL_USE_SESSION=1 to use a stored one)".into(),
    )
}

fn page_transport_config(mut config: EngineConfig) -> EngineConfig {
    config.block_page_websockets = false;
    config.sign_timeout = REST_TIMEOUT;
    config
}

async fn check(signer: Signer, requested_user: Option<String>) -> Result<(), String> {
    // --- 1. Who is live? -------------------------------------------------------------
    let user = match requested_user {
        Some(user) => {
            println!("[1/5] manually selected channel: @{user}");
            user
        }
        None => {
            println!("[1/5] waiting for /live to render channels…");
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
    println!("\n[2/5] resolving @{user} → room_id…");
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
    // One subscription, not two. Subscribing twice relays and decodes every frame twice and
    // lets the two streams interleave, so schema lines drift away from the events they
    // describe. Each frame is decoded both ways in place below instead.
    let mut page_events = signer
        .subscribe_page_websocket()
        .await
        .map_err(|e| format!("could not subscribe to page WebSocket: {e}"))?;

    let room_page = ttl_sign_core::room::live_page_url(&user);
    println!("\n[3/5] navigating to {room_page} …");
    signer
        .navigate(&room_page)
        .await
        .map_err(|e| format!("could not load live page: {e}"))?;

    // --- 4. Room state ---------------------------------------------------------------
    //
    // The event stream reports *changes*; this reports the room as it already is. TikTok
    // signs these requests through the page's patched `fetch`, and unlike
    // `/webcast/im/fetch/` they answer with CORS headers, so the body is readable.
    println!("\n[4/5] reading room metadata…");
    let room_info = signer
        .room_info(&lookup.room_id)
        .await
        .map_err(|e| format!("room/info failed: {e}"))?;
    print_room_info(&room_info);

    // A missing gift table degrades gift events to bare IDs; it does not invalidate the
    // transport this check is about.
    let gifts = match signer.gift_list(&lookup.room_id).await {
        Ok(gifts) => {
            println!("      gifts: {} available", gifts.len());
            if let Some(cheapest) = gifts.iter().min_by_key(|gift| gift.diamond_count) {
                println!(
                    "        cheapest: {} ({} diamonds, id={})",
                    cheapest.name, cheapest.diamond_count, cheapest.id
                );
            }
            gifts
        }
        Err(error) => {
            println!("      gifts: unavailable ({error})");
            Vec::new()
        }
    };

    println!("\n[5/5] waiting for TikTok's page WebSocket and relayed frames…");
    let deadline = tokio::time::sleep(PAGE_TRANSPORT_TIMEOUT);
    tokio::pin!(deadline);

    let mut active_url: Option<String> = None;
    let mut binary_frames = 0usize;
    let mut message_frames = 0usize;
    let mut decoded_events = 0usize;
    let mut decode_errors = 0usize;
    let mut schema_event_count = 0usize;
    let mut schema_decode_errors = 0usize;
    // Collapses gift bursts so a held send button is one gift, not a dozen.
    let mut streaks = GiftStreaks::new();

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = page_events.recv() => {
                let Some(event) = event else {
                    return Err("page WebSocket relay closed before the WebView".into());
                };
                match event {
                    PageWebSocketEvent::Open { url } if is_webcast_socket(&url) => {
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
                                    match decode_event_batch(&frame) {
                                        Ok(batch) => {
                                            println!(
                                                "        batch: {} events cursor={:?}",
                                                batch.messages.len(),
                                                batch.cursor
                                            );
                                            for message in batch.messages {
                                                // Normalisation never fails: an unreadable
                                                // payload arrives as `Unknown` with its bytes.
                                                let event = ttl_live_events::decode_event(
                                                    &message.method,
                                                    &message.payload,
                                                );
                                                decoded_events += 1;
                                                print_event(message.message_id, &event, &gifts, &mut streaks);
                                                // Decode the same payload against the pinned
                                                // v3 schema here, so each schema line sits
                                                // directly under its own event.
                                                match decode_webcast_message(
                                                    &message.method,
                                                    &message.payload,
                                                ) {
                                                    Ok(event) => {
                                                        schema_event_count += 1;
                                                        print_schema_event(
                                                            message.message_id,
                                                            &message.method,
                                                            &event,
                                                        );
                                                    }
                                                    Err(error) => {
                                                        schema_decode_errors += 1;
                                                        println!(
                                                            "        [schema-decode-error] method={} error={error}",
                                                            message.method
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        Err(error) => {
                                            decode_errors += 1;
                                            println!("        event batch decode failed: {error}");
                                        }
                                    }
                                    if message_frames >= REQUIRED_MESSAGE_FRAMES
                                        && decoded_events > 0
                                        && schema_event_count > 0
                                    {
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
                            || (active_url.is_none() && is_webcast_socket(&url)) =>
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
    if decoded_events == 0 {
        return Err(format!(
            "received {message_frames} msg frames but decoded no embedded events \
             ({decode_errors} decode failures)"
        ));
    }
    if schema_event_count == 0 {
        return Err(format!(
            "decoded {decoded_events} reduced events but no full-schema event \
             ({schema_decode_errors} schema decode failures)"
        ));
    }

    println!(
        "\nOK: decoded {decoded_events} reduced events and {schema_event_count} schema events \
         from {message_frames} message frames for @{user}; no Euler or custom signing endpoint \
         was used."
    );
    Ok(())
}

fn decode_event_batch(frame: &PushFrame) -> Result<WebcastEventBatch, String> {
    let payload =
        if frame.compress_type() == Some("gzip") || frame.payload.starts_with(&[0x1f, 0x8b]) {
            let mut decoder = GzDecoder::new(frame.payload.as_slice());
            let mut decoded = Vec::new();
            decoder
                .read_to_end(&mut decoded)
                .map_err(|error| format!("gzip decompression failed: {error}"))?;
            decoded
        } else {
            frame.payload.clone()
        };
    WebcastEventBatch::decode(&payload).map_err(|error| error.to_string())
}

/// Room state as it stands before the first event arrives.
fn print_room_info(info: &RoomInfo) {
    println!(
        "      @{} ({}) — {} followers",
        info.owner.unique_id, info.owner.nickname, info.owner.follower_count
    );
    println!(
        "      title={:?} status={} live={}",
        info.title,
        info.status,
        info.is_live()
    );
    println!(
        "      viewers={} total_viewers={} likes={} comments={} shares={} new_follows={}",
        info.viewer_count,
        info.total_viewers,
        info.like_count,
        info.comment_count,
        info.share_count,
        info.follow_count
    );
    println!("      started_at={} (unix)", info.create_time);
}

fn print_event(message_id: u64, event: &LiveEvent, gifts: &[Gift], streaks: &mut GiftStreaks) {
    match event {
        LiveEvent::Chat(chat) => println!(
            "        [chat] id={message_id} @{}: {}",
            chat.user.label(),
            one_line(&chat.comment)
        ),
        LiveEvent::Gift(gift_event) => {
            let (user, gift_id, repeat_count, repeat_end) = (
                &gift_event.user,
                &gift_event.gift_id,
                &gift_event.repeat_count,
                &gift_event.repeat_end,
            );
            // A gift event carries only an id; the gift table turns it into a name and a
            // diamond value. A streakable gift is reported once, when TikTok ends the
            // streak, so a held send button is one gift rather than a dozen.
            let gift = gifts.iter().find(|gift| gift.id == *gift_id);
            let streakable = gift.is_some_and(Gift::is_streakable);
            let Some(completed) =
                streaks.observe(user.id, *gift_id, *repeat_count, *repeat_end, streakable)
            else {
                return;
            };
            let named = match gift {
                Some(gift) => format!(
                    "{} ×{} = {} diamonds",
                    gift.name,
                    completed.count,
                    completed.diamonds(gift.diamond_count)
                ),
                None => format!("gift_id={gift_id} ×{}", completed.count),
            };
            println!("        [gift] id={message_id} @{} {named}", user.label());
        }
        LiveEvent::Like(like) => println!(
            "        [like] id={message_id} @{} count={} total={}",
            like.user.label(),
            like.count,
            like.total
        ),
        LiveEvent::Member(member) => println!(
            "        [member] id={message_id} @{} viewers={} action={}",
            member.user.label(),
            member.member_count,
            member.action
        ),
        LiveEvent::Social(social) => println!(
            "        [social] id={message_id} @{} action={} follows={} shares={}",
            social.user.label(),
            social.action,
            social.follow_count,
            social.share_count
        ),
        LiveEvent::RoomUser(room) => println!(
            "        [room-user] id={message_id} total={} popularity={} total_users={}",
            room.total, room.popularity, room.total_user
        ),
        // "Unknown" here means only that this method is outside the six-method reduced
        // enum, not that anything was dropped. The schema line below carries its fields.
        LiveEvent::Unknown { method, payload } => println!(
            "        [other] id={message_id} method={method} payload={} bytes",
            payload.len()
        ),
    }
}

/// Show that every event carries data, named when the pinned schema knows the method and by
/// wire number when it does not. Nothing is discarded either way.
fn print_schema_event(message_id: u64, method: &str, event: &SchemaMessage) {
    println!(
        "          [schema] id={message_id} method={method} type={}{}",
        event.schema_name(),
        if event.truncated { " (truncated)" } else { "" }
    );
    let summary: Vec<String> = event
        .fields
        .iter()
        .take(MAX_SUMMARY_FIELDS)
        .map(describe_field)
        .collect();
    if !summary.is_empty() {
        println!("            {}", summary.join(" "));
    }
}

/// One field as `name=value`, falling back to its wire number when unnamed.
fn describe_field(field: &ttl_live_events::SchemaField) -> String {
    let label = field
        .name
        .map(str::to_owned)
        .unwrap_or_else(|| format!("#{}", field.number));
    let value = match &field.value {
        SchemaValue::Varint(value) | SchemaValue::Fixed64(value) => value.to_string(),
        SchemaValue::Fixed32(value) => value.to_string(),
        SchemaValue::Text(text) => format!("{:?}", one_line(text)),
        SchemaValue::Bytes(bytes) => format!("<{} bytes>", bytes.len()),
        SchemaValue::Message(object) => format!("{{{} fields}}", object.fields.len()),
        SchemaValue::Truncated(bytes) => format!("<truncated {} bytes>", bytes.len()),
    };
    format!("{label}={value}")
}

fn one_line(text: &str) -> String {
    let mut output: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX_EVENT_TEXT_CHARS)
        .collect();
    if text.chars().count() > MAX_EVENT_TEXT_CHARS {
        output.push('…');
    }
    output
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
