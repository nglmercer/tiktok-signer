//! Flow step 1, without a browser or display: `unique_id` → `room_id` + status.
//!
//! Obtain a `room_id` **from a room that is actually live** before trying anything else.
//! Signing an offline room returns a protobuf without `push_server`, indistinguishable from
//! a rejection and likely to send debugging in the wrong direction.
//!
//! ```sh
//! cargo run -p ttl-live-ws --example rooms -- usuario1 usuario2
//! ```
//!
//! To discover *who* is live, use the rendered DOM from
//! `https://www.tiktok.com/live`, which requires the WebView:
//! `cargo run -p ttl-live-discovery --example live-check`.

use anyhow::{Context, Result};
use ttl_sign_core::room::{room_lookup_url, RoomLookup};
use ttl_sign_core::Preset;

#[tokio::main]
async fn main() -> Result<()> {
    let users: Vec<String> = std::env::args().skip(1).collect();
    if users.is_empty() {
        anyhow::bail!("usage: rooms <user> [user…]  (initial @ is optional)");
    }

    let preset = Preset::default();
    let client = reqwest::Client::builder()
        .user_agent(preset.user_agent())
        .build()?;

    println!("{:<24} {:<22} {:<8} TITLE", "USER", "ROOM_ID", "STATUS");

    let mut live = 0usize;
    for user in &users {
        let url = room_lookup_url(user);
        let response = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("lookup for @{user} failed"))?;
        let status = response.status();
        let body = response.text().await?;

        let Some(lookup) = RoomLookup::from_json(&body) else {
            println!(
                "{:<24} {:<22} {:<8} unexpected response (HTTP {status})",
                user, "-", "?"
            );
            continue;
        };

        let state = if lookup.is_live() { "LIVE" } else { "OFFLINE" };
        if lookup.is_live() {
            live += 1;
        }
        println!(
            "{:<24} {:<22} {:<8} {}",
            format!("@{}", lookup.unique_id),
            if lookup.room_id.is_empty() {
                "-"
            } else {
                &lookup.room_id
            },
            state,
            lookup.title
        );
    }

    println!("\n{live} of {} users are live.", users.len());
    if live == 0 {
        println!(
            "No live rooms were found, so nothing can be validated: the protobuf would lack \
             push_server and look like a rejection."
        );
    }
    Ok(())
}
