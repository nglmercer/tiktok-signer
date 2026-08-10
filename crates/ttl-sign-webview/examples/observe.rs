//! Observe what the real player does without asking the page to perform extra work.
//!
//! Loads `https://www.tiktok.com/@<user>/live` with the saved session and records which
//! `webcast` requests leave, whether their responses are readable, and which WebSocket URI
//! the player opens. This is the reference for comparing our implementation.
//!
//! It does not sign anything itself: it only loads the page, as a viewer would.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example observe -- <live user>
//! ```

use std::time::Duration;

use ttl_sign_core::room::live_page_url;
use ttl_sign_core::FetchResult;
use ttl_sign_webview::{run, session, EngineConfig, Signer};

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=info")
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: observe <live user>");
        std::process::exit(2);
    });

    let session = session::configured_path()
        .and_then(|p| session::load(&p).ok().flatten())
        .unwrap_or_default();
    if !session::is_logged_in(&session) {
        eprintln!("session required: cargo run -p ttl-sign-webview --example login");
        std::process::exit(1);
    }

    let config = EngineConfig {
        landing_url: live_page_url(&user),
        sign_timeout: Duration::from_secs(30),
        // This tool must see the URI opened by the real player and must not open its own
        // Rust socket.
        block_page_websockets: false,
        session,
        ..EngineConfig::default()
    };

    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime");
        rt.block_on(async move {
            // First verify that the session reached the page; otherwise the remaining
            // observations are explained and there is no point continuing.
            match signer.cookies().await {
                Ok(jar) => {
                    let mut names: Vec<&str> = jar.iter().map(|(k, _)| k).collect();
                    names.sort_unstable();
                    println!("WebView cookies: {}", names.join(" "));
                    println!(
                        "browser session: {}",
                        if session::is_logged_in(&jar) {
                            "yes"
                        } else {
                            "NO — the page will browse anonymously"
                        }
                    );
                }
                Err(e) => println!("could not read cookies: {e}"),
            }
            match signer
                .eval("String(!!(document.cookie||'').match(/sid_guard|sessionid/))")
                .await
            {
                Ok(v) => println!("page sees itself as logged in: {v}"),
                Err(e) => println!("could not query page: {e}"),
            }

            // The player takes time to start. Observe it several times because both the
            // requests and their order matter.
            for t in [8u64, 8, 14] {
                tokio::time::sleep(Duration::from_secs(t)).await;
                println!("\n=== t+{t}s ===");
                dump(&signer).await;
            }
            signer.shutdown();
            std::process::exit(0);
        })
    })
}

async fn dump(signer: &Signer) {
    match signer.captures().await {
        Ok(captures) if captures.is_empty() => {
            println!("  (page has not requested anything from im/)")
        }
        Ok(captures) => {
            for c in &captures {
                println!(
                    "  {} via {} status={} type={:?} {} bytes{}",
                    c.endpoint(),
                    c.via,
                    c.status,
                    c.response_type,
                    c.bytes,
                    c.error
                        .as_ref()
                        .map(|e| format!("  ERROR {e}"))
                        .unwrap_or_default()
                );
                if !c.text.is_empty() {
                    println!("    body: {}", c.text.replace('\n', " "));
                }
                // Compare these parameters with ours.
                if let Some(query) = c.url.split_once('?').map(|(_, q)| q) {
                    println!("    params: {}", summarize(query));
                }
                let body = c.decoded();
                if !body.is_empty() {
                    match FetchResult::decode(&body) {
                        Ok(r) => println!(
                            "    protobuf: push_server={} route_params={:?} cursor={} internal_ext={} chars",
                            r.push_server,
                            r.route_params.iter().map(|(k, _)| k).collect::<Vec<_>>(),
                            r.cursor,
                            r.internal_ext.len()
                        ),
                        Err(e) => println!("    unreadable protobuf: {e}"),
                    }
                }
            }
        }
        Err(e) => println!("  could not read captures: {e}"),
    }

    match signer.page_ws_urls().await {
        Ok(urls) if urls.is_empty() => println!("  (page has not opened any WebSocket)"),
        Ok(urls) => {
            for url in &urls {
                let (base, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
                println!("  WS {base}");
                println!("    params: {}", summarize(query));
            }
        }
        Err(e) => println!("  could not read WebSocket list: {e}"),
    }
}

/// `k=v` pairs separated by spaces, with long or sensitive values shortened: full
/// signatures and tokens add no value and clutter the terminal.
fn summarize(query: &str) -> String {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if v.len() > 24 || matches!(k, "msToken" | "X-Gnarly" | "X-Dynosaur" | "X-Bogus") {
                format!("{k}=<{} chars>", v.len())
            } else {
                format!("{k}={v}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
