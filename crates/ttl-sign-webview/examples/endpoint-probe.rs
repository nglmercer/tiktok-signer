//! Compare TikTok LIVE endpoints using exactly the same signing path.
//!
//! Sign each URL with the page SDK and repeat it from Rust, showing status and bytes. This
//! separates two commonly confused cases: our replay is wrong, or the endpoint is gone.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example endpoint-probe -- user
//! ```

use std::time::Duration;

use ttl_sign_core::room::live_page_url;
use ttl_sign_core::{FetchParams, Preset};
use ttl_sign_webview::{run, EngineConfig, Signer};

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=info")
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: endpoint-probe <live user>");
        std::process::exit(2);
    });

    let config = EngineConfig {
        landing_url: live_page_url(&user),
        sign_timeout: Duration::from_secs(30),
        ..EngineConfig::default()
    };
    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
        rt.block_on(async move {
            let lookup = signer.room_lookup(&user).await.expect("lookup");
            println!(
                "@{} room_id={} live={}",
                lookup.unique_id,
                lookup.room_id,
                lookup.is_live()
            );

            let preset = signer.preset();
            for (label, url) in candidates(&lookup.room_id, &preset) {
                match signer.sign_url(&url).await {
                    Ok(signed) => {
                        let signed_by_sdk = signed.url.contains("X-Gnarly=");
                        match signer.replay_raw(&signed).await {
                            Ok((status, body)) => {
                                let text = String::from_utf8_lossy(&body);
                                let head = text.chars().take(90).collect::<String>();
                                println!(
                                    "{label:<22} signed={signed_by_sdk} status={status} bytes={} {}",
                                    body.len(),
                                    head.replace('\n', " ")
                                );
                                // Does it contain what is needed to open the WebSocket?
                                let hints: Vec<&str> = [
                                    "push_server",
                                    "route_params",
                                    "internal_ext",
                                    "cursor",
                                    "wss://",
                                ]
                                .into_iter()
                                .filter(|k| text.contains(k))
                                .collect();
                                if !hints.is_empty() {
                                    println!("{:<22} → WebSocket hints: {hints:?}", "");
                                    if let Some(i) = text.find("wss://") {
                                        println!(
                                            "{:<22} → {}",
                                            "",
                                            text[i..].chars().take(120).collect::<String>()
                                        );
                                    }
                                }
                            }
                            Err(e) => println!("{label:<22} signed={signed_by_sdk} ERROR {e}"),
                        }
                    }
                    Err(e) => println!("{label:<22} signing failed: {e}"),
                }
            }

            signer.shutdown();
            std::process::exit(0);
        })
    })
}

/// Endpoints to compare: the critical path and endpoints the page currently uses.
fn candidates(room_id: &str, preset: &Preset) -> Vec<(String, String)> {
    let common = format!(
        "aid=1988&app_language={lang}&app_name=tiktok_web&browser_language={blang}\
         &browser_name=Mozilla&browser_online=true&browser_platform={plat}\
         &cookie_enabled=true&device_platform=web&identity=audience&live_id=12\
         &webcast_language={lang}",
        lang = preset.location.language,
        blang = preset.location.browser_language,
        plat = preset.device.browser_platform,
    );

    vec![
        (
            "im/fetch (F1)".into(),
            FetchParams::new(room_id).url(preset),
        ),
        (
            "im/fetch (minimal)".into(),
            format!(
                "https://webcast.tiktok.com/webcast/im/fetch/?{common}&room_id={room_id}\
                 &resp_content_type=protobuf&cursor=&internal_ext=&sup_ws_ds_opt=1&did_rule=3&fetch_rule=1"
            ),
        ),
        (
            "room/enter".into(),
            format!("https://webcast.tiktok.com/webcast/room/enter/?{common}&room_id={room_id}"),
        ),
        (
            "room/check_alive".into(),
            format!("https://webcast.tiktok.com/webcast/room/check_alive/?{common}&room_ids={room_id}"),
        ),
        (
            "room/info".into(),
            format!("https://webcast.tiktok.com/webcast/room/info/?{common}&room_id={room_id}"),
        ),
    ]
}
