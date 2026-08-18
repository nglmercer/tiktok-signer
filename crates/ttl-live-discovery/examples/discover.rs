//! Resolve a creator to a live room, then read its metadata and gift table — with no browser.
//!
//! ```sh
//! # the bundle is a public static asset; see scripts/headless/README.md
//! curl -s -o /tmp/webmssdk.js \
//!   https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
//!
//! cargo run -p ttl-live-discovery --example discover -- <unique_id> [/tmp/webmssdk.js]
//! ```
//!
//! The room lookup is unsigned and needs nothing. `room/info` and `gift/list` are signed by
//! driving `scripts/headless/sign-url.mjs` as a subprocess, so Rust reaches the signer without
//! embedding a JavaScript engine and without a WebView.
//!
//! AUTHORIZED USE ONLY: this sends real signed requests.

use ttl_live_discovery::{CommandSigner, DiscoveryClient, DiscoveryError};
use ttl_sign_core::{DevicePreset, LocationPreset, Preset, ScreenPreset};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(user) = args.next() else {
        eprintln!("usage: discover <unique_id> [webmssdk.js] [sign-url.mjs]");
        std::process::exit(2);
    };
    let bundle = args.next().unwrap_or_else(|| "/tmp/webmssdk.js".into());
    let script = args
        .next()
        .unwrap_or_else(|| "scripts/headless/sign-url.mjs".into());

    let preset = Preset::new(
        DevicePreset::chrome_linux(),
        LocationPreset::us_east(),
        ScreenPreset::FHD,
    );
    let client = match DiscoveryClient::new(&preset) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("could not build the discovery client: {error}");
            std::process::exit(1);
        }
    };

    // Step 1 — unsigned. No signer, no browser, no cookies.
    let lookup = match client.room_lookup(&user).await {
        Ok(lookup) => lookup,
        Err(DiscoveryError::NoRoom(user)) => {
            println!("@{user} is not live");
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("lookup failed: {error}");
            std::process::exit(1);
        }
    };
    println!(
        "@{} room_id={} live={}",
        lookup.unique_id,
        lookup.room_id,
        lookup.is_live()
    );
    if !lookup.is_live() {
        println!("room is not broadcasting; skipping the signed calls");
        return;
    }

    // Steps 2 and 3 — signed, via the headless signer subprocess.
    let signer = CommandSigner::node(script, bundle).with_user_agent(preset.user_agent());

    match client.room_info(&lookup.room_id, &signer).await {
        Ok(info) => println!(
            "room/info  title={:?} viewers={} likes={}",
            info.title, info.viewer_count, info.like_count
        ),
        Err(error) => println!("room/info  failed: {error}"),
    }

    match client.gift_list(&lookup.room_id, &signer).await {
        Ok(gifts) => {
            println!("gift/list  {} gifts", gifts.len());
            let mut cheapest: Vec<_> = gifts.iter().collect();
            cheapest.sort_by_key(|gift| gift.diamond_count);
            for gift in cheapest.iter().take(3) {
                println!(
                    "           {} ({} diamonds, id={})",
                    gift.name, gift.diamond_count, gift.id
                );
            }
        }
        Err(error) => println!("gift/list  failed: {error}"),
    }
}
