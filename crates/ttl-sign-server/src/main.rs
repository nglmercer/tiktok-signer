//! Sign-server binary.
//!
//! The WebView event loop owns the main thread and does not return, so the Tokio runtime is
//! built manually on a separate thread. `#[tokio::main]` **does not** work here.
//!
//! ```sh
//! TTL_BIND=0.0.0.0:8080 cargo run -p ttl-sign-server
//! ```
//!
//! Pointing a TikTokLive Python client at this server is the F3 acceptance criterion:
//!
//! ```python
//! WebDefaults.tiktok_sign_url = "http://localhost:8080"
//! ```

use std::time::Duration;

use anyhow::{Context, Result};
use tracing::info;
use ttl_sign_server::{router, AppState};
use ttl_sign_webview::{run, session, EngineConfig};

const DEFAULT_SIGN_TIMEOUT_SECONDS: u64 = 15;

fn main() -> ! {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_server=info,ttl_sign_webview=info".into()),
        )
        .init();

    let bind = std::env::var("TTL_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let max_concurrent = std::env::var("TTL_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let config = EngineConfig {
        landing_url: std::env::var("TTL_LANDING_URL")
            .unwrap_or_else(|_| "https://www.tiktok.com/live".into()),
        contact_us: std::env::var("TTL_CONTACT_US").unwrap_or_default(),
        sign_timeout: Duration::from_secs(DEFAULT_SIGN_TIMEOUT_SECONDS),
        // Without a session TikTok currently returns an empty body.
        session: load_session(),
        ..EngineConfig::default()
    };

    info!(
        %bind,
        max_concurrent,
        landing_url = %config.landing_url,
        authenticated = config.is_authenticated(),
        "starting"
    );
    if !config.is_authenticated() {
        tracing::warn!(
            "no session: TikTok currently returns an empty /webcast/im/fetch/ body for \
             anonymous sessions. Log in with: cargo run -p ttl-sign-webview --example login"
        );
    }

    // `run` owns the main thread; the HTTP server lives in the worker.
    run(config, move |signer| {
        let rt = tokio::runtime::Runtime::new().expect("could not create the Tokio runtime");
        if let Err(e) = rt.block_on(serve(signer, bind, max_concurrent)) {
            tracing::error!(error = %e, "HTTP server failed");
            std::process::exit(1);
        }
        std::process::exit(0);
    })
}

/// Session: `TTL_SESSION_ID` takes precedence; otherwise use the session saved by `login`.
fn load_session() -> ttl_sign_core::CookieJar {
    if let Ok(id) = std::env::var("TTL_SESSION_ID") {
        if !id.is_empty() {
            return EngineConfig::default().with_session_id(id).session;
        }
    }
    match session::configured_path().map(|path| session::load(&path)) {
        Some(Ok(Some(jar))) => jar,
        Some(Err(e)) => {
            tracing::warn!(error = %e, "could not read saved session");
            ttl_sign_core::CookieJar::new()
        }
        _ => ttl_sign_core::CookieJar::new(),
    }
}

async fn serve(
    signer: ttl_sign_webview::Signer,
    bind: String,
    max_concurrent: usize,
) -> Result<()> {
    let state = AppState::new(signer.clone(), max_concurrent);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("could not listen on {bind}"))?;
    info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown requested");
            signer.shutdown();
        })
        .await
        .context("HTTP server failed")
}
