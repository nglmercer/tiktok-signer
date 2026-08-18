//! Resolve a creator to a live room, then read its metadata and gift table — with no browser.
//!
//! ```sh
//! cargo run -p ttl-live-discovery --example discover -- <unique_id>
//! ```
//!
//! Every step here is unsigned, and needs no bundle, no signer subprocess and no browser. The room
//! lookup never needed one; `room/info` and `gift/list` were signed until it was measured that they
//! do not verify a signature — see `scripts/headless/verify-probe.mjs`.
//!
//! AUTHORIZED USE ONLY: this sends real requests.

use ttl_live_discovery::{DiscoveryClient, DiscoveryError};
use ttl_sign_core::{DevicePreset, LocationPreset, Preset, ScreenPreset};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let Some(user) = std::env::args().nth(1) else {
        eprintln!("usage: discover <unique_id>");
        std::process::exit(2);
    };

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

    // Step 1 — the room lookup. No signer, no browser, no cookies.
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
        println!("room is not broadcasting; skipping the room reads");
        return;
    }

    // Steps 2 and 3 — unsigned. Neither endpoint verifies a signature.
    match client.room_info(&lookup.room_id).await {
        Ok(info) => println!(
            "room/info  title={:?} viewers={} likes={}",
            info.title, info.viewer_count, info.like_count
        ),
        Err(error) => println!("room/info  failed: {error}"),
    }

    match client.gift_list(&lookup.room_id).await {
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
