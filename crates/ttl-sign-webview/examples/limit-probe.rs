//! Diagnostics: what does TikTok's refusal actually look like?
//!
//! Detection cannot be written against guessed status codes. This captures the real shapes:
//! the challenge infrastructure the live page carries, and the first response that stops
//! being `status_code: 0` when `room/info` is called repeatedly as a guest.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example limit-probe -- [user] [requests]
//! ```

use std::time::Duration;

use ttl_sign_webview::{run, EngineConfig, Signer};

const DEFAULT_REQUESTS: usize = 40;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=warn")
        .init();

    let requested = std::env::args().nth(1);
    let requests: usize = std::env::args()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_REQUESTS);

    // Guest on purpose: this probe provokes refusals, and a disposable identity is the one
    // to spend on that.
    let config = EngineConfig {
        block_page_websockets: false,
        sign_timeout: Duration::from_secs(60),
        ..EngineConfig::default()
    };

    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
        rt.block_on(async move {
            let user = match requested.filter(|u| !u.is_empty() && u != "-") {
                Some(user) => user,
                None => discover(&signer).await,
            };
            println!("user=@{user} requests={requests}");

            let lookup = signer.room_lookup(&user).await.expect("lookup");
            let room_id = lookup.room_id.clone();
            println!("room_id={room_id}\n");

            signer
                .navigate(&ttl_sign_core::room::live_page_url(&user))
                .await
                .expect("navigate");
            tokio::time::sleep(Duration::from_secs(4)).await;

            report_challenge_surface(&signer).await;

            println!("\n=== hammering room/info as a guest ===");
            let url = ttl_sign_core::room::room_info_url(&room_id);
            let mut first_refusal = None;
            for attempt in 1..=requests {
                match signer.fetch_text(&url).await {
                    Ok(body) => {
                        let code = status_code(&body);
                        if code != Some(0) {
                            println!("  #{attempt}: status_code={code:?}");
                            println!("  body: {}", &body[..body.len().min(1200)]);
                            first_refusal = Some(attempt);
                            break;
                        }
                        if attempt % 10 == 0 {
                            println!("  #{attempt}: ok");
                        }
                    }
                    Err(error) => {
                        println!("  #{attempt}: transport/bridge error: {error}");
                        first_refusal = Some(attempt);
                        break;
                    }
                }
            }
            match first_refusal {
                Some(attempt) => println!("\nfirst refusal at request #{attempt}"),
                None => println!("\nno refusal in {requests} requests"),
            }

            println!("\n=== typed refusal path ===");
            match signer.room_info("1").await {
                Ok(info) => println!("  unexpectedly parsed a room: {info:?}"),
                Err(error) => println!(
                    "  room_info(\"1\") -> {error}  (refusal={})",
                    error.is_refusal()
                ),
            }

            println!("\n=== challenge detection ===");
            match signer.challenge().await {
                Ok(challenge) => println!("  {challenge:?}"),
                Err(error) => println!("  ERROR {error}"),
            }

            println!("\n=== challenge surface after hammering ===");
            report_challenge_surface(&signer).await;

            signer.shutdown();
            std::process::exit(0);
        })
    })
}

/// What the page exposes that relates to verification, before anything is provoked.
async fn report_challenge_surface(signer: &Signer) {
    for (label, js) in [
        (
            "captcha-related globals",
            "JSON.stringify(Object.keys(window).filter(k=>/captcha|verify|secsdk|slide/i.test(k)))",
        ),
        (
            "captcha/verify network requests",
            "JSON.stringify(performance.getEntriesByType('resource').map(e=>e.name.split('?')[0])\
             .filter(n=>/captcha|verify|secsdk/i.test(n)).filter((v,i,a)=>a.indexOf(v)===i).slice(0,15))",
        ),
        (
            "captcha DOM markers",
            "JSON.stringify(Array.from(document.querySelectorAll('[id*=captcha],[class*=captcha],[class*=verify],iframe'))\
             .map(e=>e.tagName+'#'+(e.id||'')+'.'+(e.className&&e.className.toString&&e.className.toString().slice(0,60)||'')).slice(0,10))",
        ),
    ] {
        match signer.eval(js).await {
            Ok(value) => println!("  {label}: {value}"),
            Err(error) => println!("  {label}: ERROR {error}"),
        }
    }
}

fn status_code(body: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("status_code")?
        .as_i64()
}

async fn discover(signer: &Signer) -> String {
    for _ in 0..25 {
        if let Ok(channels) = signer.live_channels().await {
            if let Some(first) = channels.first() {
                return first.unique_id.clone();
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("no live channel discovered");
}
