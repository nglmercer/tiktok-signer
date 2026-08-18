//! Live sign-server with no browser.
//!
//! Same HTTP surface as the WebView server it replaced; the difference is entirely in the backend.
//! Signing
//! runs through an external signer process rather than a page, so this binary's dependency tree
//! contains no `wry`.
//!
//! ```sh
//! curl -s -o /tmp/webmssdk.js \
//!   https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
//!
//! TTL_BIND=127.0.0.1:8080 \
//!   cargo run -p ttl-sign-server --bin ttl-sign-headless-server --features headless
//! ```
//!
//! The message socket refuses a jar-less handshake, so an account session is required. It is read
//! from
//! `TTL_SESSION_FILE`, else `$XDG_CONFIG_HOME/ttl-signer/session` — the same file the WebView
//! path uses. The server refuses to start without one rather than serving requests that would all
//! come back empty.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{info, warn};
use ttl_live_discovery::CommandSigner;
use ttl_sign_core::{CookieJar, DevicePreset, LocationPreset, Preset, ScreenPreset};
use ttl_sign_headless::{HeadlessBackend, HeadlessConfig, TRANSPORT_PRODUCT};
use ttl_sign_server::{router, AppState};

fn session_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("TTL_SESSION_FILE") {
        return Some(PathBuf::from(explicit));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("ttl-signer").join("session"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_server=info,ttl_sign_headless=info".into()),
        )
        .init();

    let bind = std::env::var("TTL_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let max_concurrent = std::env::var("TTL_MAX_CONCURRENT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4);
    let bundle = std::env::var("TTL_BUNDLE").unwrap_or_else(|_| "/tmp/webmssdk.js".into());
    let script =
        std::env::var("TTL_SIGN_SCRIPT").unwrap_or_else(|_| "scripts/headless/sign-url.mjs".into());

    anyhow::ensure!(
        PathBuf::from(&bundle).is_file(),
        "signing bundle not found at {bundle}; set TTL_BUNDLE (see scripts/headless/README.md)"
    );
    anyhow::ensure!(
        PathBuf::from(&script).is_file(),
        "signer script not found at {script}; set TTL_SIGN_SCRIPT"
    );

    let session = session_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|raw| CookieJar::parse(raw.trim()))
        .unwrap_or_default();
    anyhow::ensure!(
        session.get("sessionid").is_some_and(|v| !v.is_empty()),
        "no account session found. The message socket refuses guests, so every request would \
         be refused at the handshake. Create it as a cookie header (name=value; ...) containing at \
         least sessionid, exported from a browser where you are logged in."
    );

    let preset = Preset::new(
        DevicePreset::chrome_linux(),
        LocationPreset::us_east(),
        ScreenPreset::FHD,
    );
    let signer = CommandSigner::node(script.clone(), bundle.clone())
        .with_product(TRANSPORT_PRODUCT)
        .with_user_agent(preset.user_agent())
        .with_cookie(session.to_cookie_string());
    let backend = HeadlessBackend::new(HeadlessConfig::new(preset, session), Box::new(signer))
        .context("could not build the headless backend")?;

    if !backend.is_authenticated() {
        warn!("session has no sessionid; transport requests will be refused");
    }
    info!(%bind, max_concurrent, %bundle, %script, "starting headless live sign server");

    let state = AppState::new(backend, max_concurrent);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("could not listen on {bind}"))?;
    info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("server error")
}
