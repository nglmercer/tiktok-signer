//! End-to-end LIVE check with **no browser**.
//!
//! This is the replacement for the old WebView `live-check`: same flow, no Wry, no display.
//!
//! ```sh
//! curl -s -o /tmp/webmssdk.js \
//!   https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
//!
//! cargo run -p ttl-live-discovery --example live-check              # pick a live room
//! cargo run -p ttl-live-discovery --example live-check -- <user>    # or name one
//! ```
//!
//! Discovery is unsigned. The last step signs the socket query in-process — the real bundle in an
//! embedded engine, under a synthetic environment — and then opens the message socket directly,
//! the same thing the web player does, with no `/webcast/im/fetch/` in front of it. Nothing here
//! needs Node; `--features v8` swaps QuickJS for V8.
//!
//! AUTHORIZED USE ONLY: this sends real signed requests.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ttl_live_events::{decode_batch, LiveEvent};
use ttl_live_ws::{ConnectConfig, ReconnectPolicy, ReconnectingConnection};

use ttl_live_discovery::{DiscoveryClient, DiscoveryError, UrlSigner};
use ttl_sign_core::{
    CookieJar, DevicePreset, LocationPreset, Preset, ScreenPreset, DIRECT_SOCKET_HOST as SOCKET_HOST,
    WS_REUSE_PATH,
};
use ttl_sign_embedded::{EmbeddedSigner, Profile};
use ttl_sign_headless::{HeadlessBackend, HeadlessConfig, TRANSPORT_PRODUCT};

fn session() -> CookieJar {
    let path = std::env::var_os("TTL_SESSION_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
            Some(base.join("ttl-signer").join("session"))
        });
    path.and_then(|path| std::fs::read_to_string(path).ok())
        .map(|raw| CookieJar::parse(raw.trim()))
        .unwrap_or_default()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let requested = std::env::args().skip(1).find(|arg| !arg.starts_with("--"));
    let bundle = std::env::var("TTL_BUNDLE").unwrap_or_else(|_| "/tmp/webmssdk.js".into());
    if !PathBuf::from(&bundle).is_file() {
        eprintln!("signing bundle not found at {bundle}; see scripts/headless/README.md");
        std::process::exit(2);
    }

    let preset = Preset::new(
        DevicePreset::chrome_linux(),
        LocationPreset::us_east(),
        ScreenPreset::FHD,
    );
    let jar = session();
    let authenticated = jar.get("sessionid").is_some_and(|v| !v.is_empty());
    println!(
        "session: {}",
        if authenticated {
            "account — transport available"
        } else {
            "guest — discovery works; the socket needs an account"
        }
    );

    let client = DiscoveryClient::new(&preset)
        .expect("discovery client")
        .with_session(jar.to_cookie_string());
    // [1/5] who is live now — no signature, no rendering engine.
    let user = match requested {
        Some(user) => {
            println!("\n[1/5] using the requested channel: @{user}");
            user.trim_start_matches('@').to_string()
        }
        None => {
            println!("\n[1/5] listing channels that are live now…");
            match client.live_channels("live").await {
                Ok(rooms) if !rooms.is_empty() => {
                    for room in rooms.iter().take(8) {
                        println!("      @{} ({} viewers)", room.unique_id, room.viewers);
                    }
                    let first = &rooms[0];
                    println!("      selected: @{}", first.unique_id);
                    first.unique_id.clone()
                }
                Ok(_) => {
                    eprintln!("\nFAILED: no live rooms found");
                    std::process::exit(1);
                }
                Err(error) => {
                    eprintln!("\nFAILED: live search: {error}");
                    std::process::exit(1);
                }
            }
        }
    };

    // [2/5] unique_id → room_id — unsigned.
    println!("\n[2/5] resolving @{user} → room_id…");
    let lookup = match client.room_lookup(&user).await {
        Ok(lookup) => lookup,
        Err(DiscoveryError::NoRoom(user)) => {
            println!("      @{user} is not live");
            return;
        }
        Err(error) => {
            eprintln!("\nFAILED: lookup: {error}");
            std::process::exit(1);
        }
    };
    println!(
        "      room_id={} status={} live={}",
        lookup.room_id,
        lookup.status,
        lookup.is_live()
    );

    // [3/5] room metadata and gift table. Unsigned: these endpoints do not verify a signature,
    // measurable with `scripts/headless/verify-probe.mjs`.
    println!("\n[3/5] reading room metadata…");
    match client.room_info(&lookup.room_id).await {
        Ok(info) => println!(
            "      title={:?} viewers={} likes={}",
            info.title, info.viewer_count, info.like_count
        ),
        Err(error) => println!("      room/info unavailable: {error}"),
    }
    match client.gift_list(&lookup.room_id).await {
        Ok(gifts) => {
            println!("      gifts: {} available", gifts.len());
            if let Some(cheapest) = gifts.iter().min_by_key(|gift| gift.diamond_count) {
                println!(
                    "        cheapest: {} ({} diamonds, id={})",
                    cheapest.name, cheapest.diamond_count, cheapest.id
                );
            }
        }
        Err(error) => println!("      gifts: unavailable ({error})"),
    }

    // [4/5] the transport. The socket is opened directly — there is no `im/fetch` on this path.
    //
    // The live room page configures its IM SDK with `wsDirect: "1"` and a `socketHost`, and the SDK
    // then builds and signs the socket URL itself rather than asking `im/fetch` for a
    // `push_server`. Reading that out of the player's transport chunk is what ended a long search
    // for a request shape `im/fetch` would answer: it answers 200 with zero bytes because nothing
    // depends on its answer any more.
    println!("\n[4/5] building and signing the socket URL…");
    if !authenticated {
        println!("      skipped: the message socket refuses a jar-less handshake — measured as an");
        println!("      immediate 1006, before a frame is exchanged. Store a session and re-run.");
        return;
    }
    // Through the same backend the sign server uses, so this example exercises the production path
    // rather than a parallel copy of it. It builds the socket URL, has it signed, and describes it.
    // The bundle runs in-process; which engine that is comes from the build (`--features v8`).
    let source = std::fs::read_to_string(&bundle).unwrap_or_else(|error| {
        eprintln!("\nFAILED: could not read {bundle}: {error}");
        std::process::exit(1);
    });
    println!("      signing in-process with {}", ttl_sign_embedded::ENGINE);
    let signer: Box<dyn UrlSigner> = Box::new(
        EmbeddedSigner::with_product(
            source,
            Profile {
                user_agent: Some(preset.user_agent()),
                cookie: Some(jar.to_cookie_string()),
                ..Profile::default()
            },
            TRANSPORT_PRODUCT,
        )
        .unwrap_or_else(|error| {
            eprintln!("\nFAILED: could not start the embedded signer: {error}");
            std::process::exit(1);
        }),
    );
    let backend = match HeadlessBackend::new(
        HeadlessConfig::new(preset.clone(), jar.clone()),
        signer,
    ) {
        Ok(backend) => backend,
        Err(error) => {
            eprintln!("\nFAILED: could not build the signing backend: {error}");
            std::process::exit(1);
        }
    };

    // A reconnecting stream, because a signature ages out and a room can restart its push server.
    // Five attempts with a doubling backoff; a refusal is reported rather than retried.
    let opened = ReconnectingConnection::open(
        Arc::new(backend),
        &lookup.room_id,
        ConnectConfig::default(),
        ReconnectPolicy::default(),
    )
    .await;
    listen(opened).await;
}

/// Decode [`LISTEN_SECONDS`] of events, and report what arrived.
async fn listen(opened: Result<ReconnectingConnection, ttl_live_ws::StreamError>) {
    let seconds = std::env::var("TTL_LISTEN_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(LISTEN_SECONDS);
    println!("\n[5/5] listening for {seconds}s of live events…");
    let mut connection = match opened {
        Ok(connection) => connection,
        Err(error) => {
            println!("      could not open the WebSocket: {error}");
            std::process::exit(1);
        }
    };
    println!("      connected to {SOCKET_HOST}{WS_REUSE_PATH}");

    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let mut frames = 0usize;
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            message = connection.next_message() => match message {
                Some(Ok(message)) => {
                    frames += 1;
                    match decode_batch(&message.payload) {
                        Ok(batch) => {
                            for decoded in &batch.events {
                                let (label, sample) = describe(&decoded.event);
                                *counts.entry(label).or_default() += 1;
                                if let Some(sample) = sample {
                                    if samples.len() < 5 {
                                        samples.push(sample);
                                    }
                                }
                            }
                        }
                        Err(error) => println!("      batch decode failed: {error}"),
                    }
                }
                Some(Err(error)) => {
                    println!("      stream error: {error}");
                    break;
                }
                None => {
                    println!("      stream closed by the server");
                    break;
                }
            },
        }
    }
    let reconnects = connection.reconnects();
    connection.close().await;

    println!("      {frames} frames decoded");
    if reconnects > 0 {
        println!("      {reconnects} reconnect(s) along the way");
    }
    if counts.is_empty() {
        println!("      no events in this window — a quiet room still proves the transport");
    } else {
        for (label, count) in &counts {
            println!("        {label}: {count}");
        }
        for sample in &samples {
            println!("        · {sample}");
        }
    }
}

/// Listening window. Long enough for a busy room to produce chat, short enough to stay a check.
/// Default listening window. `TTL_LISTEN_SECONDS` overrides it, which is how a soak run long
/// enough to see the stream reconnect is driven.
const LISTEN_SECONDS: u64 = 5;

/// Event label plus a short human-readable sample, for the few kinds worth showing.
fn describe(event: &LiveEvent) -> (&'static str, Option<String>) {
    match event {
        LiveEvent::Chat(chat) => (
            "chat",
            Some(format!("{}: {}", chat.user.nickname, chat.comment)),
        ),
        LiveEvent::Gift(gift) => (
            "gift",
            Some(format!("{} sent gift {}", gift.user.nickname, gift.gift_id)),
        ),
        LiveEvent::Like(like) => ("like", Some(format!("{} liked", like.user.nickname))),
        LiveEvent::Member(member) => ("member", Some(format!("{} joined", member.user.nickname))),
        LiveEvent::Social(_) => ("social", None),
        LiveEvent::RoomUser(room) => ("room_user", Some(format!("{} viewers", room.total))),
        LiveEvent::Unknown { .. } => ("unknown", None),
    }
}
