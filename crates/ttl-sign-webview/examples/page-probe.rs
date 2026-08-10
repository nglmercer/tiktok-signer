//! Diagnostics: what a live page actually does.
//!
//! Loads `https://www.tiktok.com/@<user>/live`, waits for the player, and shows which
//! `webcast` requests the page made. This reveals whether `/webcast/im/fetch/` is sent
//! automatically and which parameters it uses.
//!
//! ```sh
//! cargo run -p ttl-sign-webview --example page-probe -- user
//! ```

use std::time::Duration;

use ttl_sign_core::room::live_page_url;
use ttl_sign_webview::{run, session, EngineConfig, Signer};

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter("ttl_sign_webview=info")
        .init();

    let user = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: page-probe <user> [JavaScript expression]");
        std::process::exit(2);
    });
    // Evaluate everything after the user in sequence, with pauses to install hooks, let the
    // page work, and then read the result.
    let scripts: Vec<String> = std::env::args().skip(2).collect();

    let config = EngineConfig {
        landing_url: live_page_url(&user),
        sign_timeout: Duration::from_secs(30),
        block_page_websockets: false,
        // Without a session the player does not fully start, leaving nothing to inspect.
        session: session::configured_path()
            .and_then(|p| session::load(&p).ok().flatten())
            .unwrap_or_default(),
        ..EngineConfig::default()
    };

    run(config, move |signer: Signer| {
        let rt = tokio::runtime::Runtime::new().expect("runtime de tokio");
        rt.block_on(async move {
            // The player takes time to start; give it time before inspecting it.
            if !scripts.is_empty() {
                for (i, js) in scripts.iter().enumerate() {
                    tokio::time::sleep(Duration::from_secs(if i == 0 { 6 } else { 12 })).await;
                    println!("--- script {}", i + 1);
                    match signer.eval(js).await {
                        Ok(value) => println!("{value}"),
                        Err(e) => println!("ERROR {e}"),
                    }
                }
                signer.shutdown();
                std::process::exit(0);
            }

            for wait in [5u64, 10, 15] {
                tokio::time::sleep(Duration::from_secs(wait)).await;
                println!("\n=== t+{wait}s ===");

                for (label, js) in [
                    ("URL", "location.href"),
                    ("SDK", "typeof window.byted_acrawler"),
                    ("fetch nativo", "String(window.fetch).indexOf('[native code]') !== -1"),
                    (
                        "webcast requests",
                        "performance.getEntriesByType('resource').map(e=>e.name).filter(n=>n.indexOf('webcast')!==-1).slice(0,10)",
                    ),
                    (
                        "already-open WebSockets",
                        "performance.getEntriesByType('resource').map(e=>e.name).filter(n=>n.indexOf('ws')!==-1).slice(0,5)",
                    ),
                ] {
                    match signer.eval(js).await {
                        Ok(value) => println!("{label}: {value}"),
                        Err(e) => println!("{label}: ERROR {e}"),
                    }
                }
            }
            signer.shutdown();
            std::process::exit(0);
        })
    })
}
