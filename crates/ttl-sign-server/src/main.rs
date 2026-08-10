//! Binario del sign server.
//!
//! El event loop del webview se queda con el hilo principal y no retorna, así que el
//! runtime de tokio se construye a mano en un hilo aparte. `#[tokio::main]` aquí **no**
//! sirve (`docs/01-architecture.md` §D3).
//!
//! ```sh
//! TTL_BIND=0.0.0.0:8080 cargo run -p ttl-sign-server
//! ```
//!
//! Apuntar un cliente TikTokLive de Python a este servidor es el criterio de aceptación
//! de F3:
//!
//! ```python
//! WebDefaults.tiktok_sign_url = "http://localhost:8080"
//! ```

use std::time::Duration;

use anyhow::{Context, Result};
use tracing::info;
use ttl_sign_server::{router, AppState};
use ttl_sign_webview::{run, session, EngineConfig};

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
        sign_timeout: Duration::from_secs(15),
        // Sin sesión, TikTok responde vacío hoy: ver EngineConfig::session.
        session: load_session(),
        ..EngineConfig::default()
    };

    info!(
        %bind,
        max_concurrent,
        landing_url = %config.landing_url,
        autenticado = config.is_authenticated(),
        "arrancando"
    );
    if !config.is_authenticated() {
        tracing::warn!(
            "sin sesión: hoy TikTok devuelve cuerpo vacío en /webcast/im/fetch/ para \
             sesiones anónimas. Inicia sesión con: cargo run -p ttl-sign-webview --example login"
        );
    }

    // `run` se queda con el hilo principal; el servidor HTTP vive en el worker.
    run(config, move |signer| {
        let rt = tokio::runtime::Runtime::new().expect("no se pudo crear el runtime de tokio");
        if let Err(e) = rt.block_on(serve(signer, bind, max_concurrent)) {
            tracing::error!(error = %e, "el servidor HTTP terminó con error");
            std::process::exit(1);
        }
        std::process::exit(0);
    })
}

/// Sesión: `TTL_SESSION_ID` manda; si no, la guardada por el ejemplo `login`.
fn load_session() -> ttl_sign_core::CookieJar {
    if let Ok(id) = std::env::var("TTL_SESSION_ID") {
        if !id.is_empty() {
            return EngineConfig::default().with_session_id(id).session;
        }
    }
    match session::configured_path().map(|path| session::load(&path)) {
        Some(Ok(Some(jar))) => jar,
        Some(Err(e)) => {
            tracing::warn!(error = %e, "no se pudo leer la sesión guardada");
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
        .with_context(|| format!("no se pudo escuchar en {bind}"))?;
    info!(addr = %listener.local_addr()?, "escuchando");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            info!("cierre solicitado");
            signer.shutdown();
        })
        .await
        .context("el servidor HTTP falló")
}
