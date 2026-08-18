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
//! Every step is signed by `scripts/headless/sign-url.mjs` running the real bundle under a
//! synthetic environment. Step 4 needs an account session, because `/webcast/im/fetch/` answers a
//! guest with an empty body; the earlier steps work as a guest.
//!
//! AUTHORIZED USE ONLY: this sends real signed requests.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ttl_live_events::{decode_batch, LiveEvent};
use ttl_live_ws::{ConnectConfig, LiveConnection};

use ttl_live_discovery::{CommandSigner, DiscoveryClient, DiscoveryError};
use ttl_sign_core::{
    CookieJar, DevicePreset, LocationPreset, Preset, RejectReason, ScreenPreset, SignOutcome,
    SignerBackend, TransportRequest,
};
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
    let script =
        std::env::var("TTL_SIGN_SCRIPT").unwrap_or_else(|_| "scripts/headless/sign-url.mjs".into());

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
            "guest — discovery works; the transport step needs an account"
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

    // [4/5] transport bootstrap.
    println!("\n[4/5] bootstrapping the transport…");
    if !authenticated {
        println!("      skipped: /webcast/im/fetch/ refuses guests. Provide a session to try it.");
        return;
    }
    let transport_signer = CommandSigner::node(script, bundle)
        .with_product(TRANSPORT_PRODUCT)
        .with_user_agent(preset.user_agent())
        .with_cookie(jar.to_cookie_string());
    let backend = HeadlessBackend::new(
        HeadlessConfig::new(preset.clone(), jar),
        Box::new(transport_signer),
    )
    .expect("headless backend");

    let signed = match backend
        .transport(TransportRequest::new(&lookup.room_id))
        .await
    {
        SignOutcome::Ok(signed) => {
            println!(
                "      push_server obtained ({} bytes)",
                signed.protobuf.len()
            );
            signed
        }
        SignOutcome::Rejected(RejectReason::EmptyBody) => {
            println!("      rejected: empty body (silent rejection)");
            println!();
            println!("      The request itself is now the one a browser makes. Two things were");
            println!("      wrong until 2026-08-18, and both are fixed:");
            println!();
            println!("      1. This request was signed. The page's own signing allowlist covers 7");
            println!("         GET and 22 POST webcast paths — wallet, KYC, room/chat, room/enter —");
            println!("         and /webcast/im/fetch/ is on neither, so a browser sends it");
            println!("         unsigned. Signed it answers 403 whatever the signature contains;");
            println!("         unsigned it answers 200. It now goes out unsigned.");
            println!("      2. The query was ours, not the player's. version_code is 180800, and");
            println!("         the first fetch sends cursor=0, internal_ext=0, last_rtt=-1 — the");
            println!("         chunk deletes empty values, so cursor= is a request no player");
            println!("         makes. FetchParams builds the chunk's query now.");
            println!();
            println!("      The signature was never wrong. /webcast/room/enter/ is on the POST");
            println!("      allowlist and does verify one: unsigned 403, X-Bogus alone 403, our");
            println!("      full computed suffix 200. Check it yourself:");
            println!();
            println!("        node scripts/headless/enter-then-fetch.mjs /tmp/webmssdk.js {}", user);
            println!();
            println!("      What remains is this empty body, and it is not a signing problem. A");
            println!("      zero-byte 200 carries no status_code, so it is an edge answering");
            println!("      nothing rather than the service objecting. Ruled out: signature");
            println!("      present or absent, three parameter sets, protobuf versus JSON, the");
            println!("      hosts webcast.us and webcast.tiktokv.com, the x-tt-target-idc header,");
            println!("      identity from a full session down to no cookies, and the room.");
            println!();
            println!("      Standing hypothesis: cross-data-centre routing. This account is");
            println!("      pinned to one idc and the rooms tested report another. A session in a");
            println!("      different location would settle it in one request. See docs/12.");
            return;
        }
        SignOutcome::Rejected(reason) => {
            println!("      rejected: {reason}");
            return;
        }
        SignOutcome::Transport(error) => {
            println!("      transport error: {error}");
            return;
        }
    };

    listen(
        LiveConnection::open(&signed, &preset, &lookup.room_id, &ConnectConfig::default()).await,
    )
    .await;
}

/// Connect, decode [`LISTEN_SECONDS`] of events, and report what arrived.
///
/// Shared by both transport routes: the signed `im/fetch` result and a URI captured from a
/// browser. Once a `FetchResult` exists, nothing downstream cares where it came from.
async fn listen(opened: Result<LiveConnection, ttl_live_ws::WsError>) {
    println!("\n[5/5] listening for {LISTEN_SECONDS}s of live events…");
    let mut connection = match opened {
        Ok(connection) => connection,
        Err(error) => {
            println!("      could not open the WebSocket: {error}");
            std::process::exit(1);
        }
    };
    println!(
        "      connected to {}",
        connection.uri().split('?').next().unwrap_or_default()
    );

    let deadline = tokio::time::sleep(Duration::from_secs(LISTEN_SECONDS));
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
    connection.close().await;

    println!("      {frames} frames decoded");
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
