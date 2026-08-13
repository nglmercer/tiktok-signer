//! Offline/headless sign-server backed by a sanitized replay corpus.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::info;
use ttl_sign_replay::ReplayBackend;
use ttl_sign_server::{router, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ttl_sign_server=info".into()),
        )
        .init();

    let bind = std::env::var("TTL_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let max_concurrent = std::env::var("TTL_MAX_CONCURRENT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(16);
    let fixtures = std::env::var_os("TTL_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("fixtures/signing"));
    let backend = ReplayBackend::load(&fixtures)
        .with_context(|| format!("could not load fixture corpus {}", fixtures.display()))?;

    info!(
        %bind,
        max_concurrent,
        cases = backend.len(),
        fixtures = %fixtures.display(),
        "starting offline replay server"
    );
    let state = AppState::new(backend, max_concurrent);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("could not listen on {bind}"))?;
    info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("shutdown requested");
        })
        .await
        .context("HTTP server failed")
}
