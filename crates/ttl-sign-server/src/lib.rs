//! Servidor HTTP compatible con la spec de sign server de Euler Stream
//! (`docs/03-spec-sign-server.md`).
//!
//! Capa fina a propósito: traduce HTTP ↔ llamadas al `Signer` y no tiene lógica propia.
//! Existe sobre todo para poder validar la implementación contra clientes que no hemos
//! escrito nosotros — un TikTokLive de Python apuntando aquí es la validación cruzada más
//! barata que hay.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use ttl_sign_core::{RejectReason, SignError, SignOutcome};
use ttl_sign_webview::Signer;

/// Cabecera que exige el cliente Python: sin ella aborta con `EMPTY_COOKIES`.
const X_SET_TT_COOKIE: &str = "X-Set-TT-Cookie";
/// Extensión propia: el UA realmente usado, para que el cliente lo replique en el WS.
const X_SET_TT_USER_AGENT: &str = "X-Set-TT-User-Agent";
const X_REQUEST_ID: &str = "X-Request-Id";

pub struct AppState {
    signer: Signer,
    /// Firmas simultáneas permitidas. Encolar hace caducar firmas ya emitidas
    /// (~30 s de vida útil), así que por encima de esto se responde 429.
    slots: tokio::sync::Semaphore,
    max_concurrent: usize,
    started: Instant,
    requests: AtomicU64,
    signs_ok: AtomicU64,
    rejects: AtomicU64,
}

impl AppState {
    pub fn new(signer: Signer, max_concurrent: usize) -> Arc<Self> {
        Arc::new(Self {
            signer,
            slots: tokio::sync::Semaphore::new(max_concurrent),
            max_concurrent,
            started: Instant::now(),
            requests: AtomicU64::new(0),
            signs_ok: AtomicU64::new(0),
            rejects: AtomicU64::new(0),
        })
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/webcast/fetch", get(webcast_fetch))
        .route("/healthz", get(healthz))
        .with_state(state)
}

/// Params que envían los clientes. Se ignora casi todo: los parámetros de navegador los
/// regenera el servidor desde su propio preset, y el UA usado se devuelve en
/// `X-Set-TT-User-Agent` para que el cliente abra el WS con el mismo
/// (`docs/03-spec-sign-server.md` §Regla).
#[derive(Debug, Deserialize)]
pub struct FetchQuery {
    room_id: Option<String>,
    /// Solo para logs.
    client: Option<String>,
}

async fn webcast_fetch(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FetchQuery>,
) -> Response {
    let request_id = state.requests.fetch_add(1, Ordering::Relaxed) + 1;

    let room_id = match query.room_id.as_deref() {
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) => id.to_string(),
        Some(id) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("room_id no numérico: {id}"),
                request_id,
                None,
            )
        }
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "falta el parámetro room_id".into(),
                request_id,
                None,
            )
        }
    };

    // Rechazar antes que encolar: una firma que espera es una firma que caduca.
    let _slot = match state.slots.try_acquire() {
        Ok(slot) => slot,
        Err(_) => {
            warn!(request_id, "límite de concurrencia alcanzado");
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "límite de firmas simultáneas alcanzado".into(),
                request_id,
                Some("concurrent_signs"),
            );
        }
    };

    let started = Instant::now();
    let outcome = state.signer.fetch(&room_id).await;
    let latency_ms = started.elapsed().as_millis();

    match outcome {
        SignOutcome::Ok(signed) => {
            state.signs_ok.fetch_add(1, Ordering::Relaxed);
            info!(
                request_id,
                room_id,
                client = query.client.as_deref().unwrap_or("-"),
                latency_ms,
                bytes = signed.protobuf.len(),
                "firma emitida"
            );

            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/protobuf"),
            );
            // Sin esta cabecera el cliente Python aborta antes de intentar el WS.
            if let Ok(v) = HeaderValue::from_str(&signed.cookies.to_cookie_string()) {
                headers.insert(X_SET_TT_COOKIE, v);
            }
            if let Ok(v) = HeaderValue::from_str(&signed.user_agent) {
                headers.insert(X_SET_TT_USER_AGENT, v);
            }
            headers.insert(X_REQUEST_ID, HeaderValue::from(request_id));

            (StatusCode::OK, headers, signed.protobuf).into_response()
        }

        // Rechazo: TikTok nos ha detectado. 502, y el cliente no debe reintentar.
        SignOutcome::Rejected(reason) => {
            state.rejects.fetch_add(1, Ordering::Relaxed);
            warn!(request_id, room_id, latency_ms, %reason, "firma rechazada");
            error_response(
                StatusCode::BAD_GATEWAY,
                format!("TikTok rechazó la petición: {reason}"),
                request_id,
                match reason {
                    RejectReason::HttpStatus(_) => Some("upstream_status"),
                    _ => Some("detected"),
                },
            )
        }

        SignOutcome::Transport(err) => {
            warn!(request_id, room_id, latency_ms, %err, "fallo de transporte");
            let status = match err {
                // El pool todavía no está listo: es reintentable, y 503 lo dice.
                SignError::SdkNotReady
                | SignError::NoInstanceAvailable
                | SignError::LoginTimeout(_) => StatusCode::SERVICE_UNAVAILABLE,
                SignError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
                _ => StatusCode::BAD_GATEWAY,
            };
            error_response(status, err.to_string(), request_id, None)
        }
    }
}

async fn healthz(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let available = state.slots.available_permits();
    Json(json!({
        "ready": true,
        "uptime_s": state.started.elapsed().as_secs(),
        "user_agent": state.signer.preset().user_agent(),
        "in_flight": state.max_concurrent - available,
        "max_concurrent": state.max_concurrent,
        "requests": state.requests.load(Ordering::Relaxed),
        "signs_ok": state.signs_ok.load(Ordering::Relaxed),
        "rejects": state.rejects.load(Ordering::Relaxed),
    }))
}

/// Cuerpo de error en JSON. **Nunca** un 200 con cuerpo vacío: el cliente lo
/// interpretaría como "detectado" y el mensaje apuntaría al sitio equivocado.
fn error_response(
    status: StatusCode,
    message: String,
    request_id: u64,
    limit_label: Option<&str>,
) -> Response {
    let mut body = json!({ "message": message });
    if let Some(label) = limit_label {
        body["limit_label"] = json!(label);
    }
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(X_REQUEST_ID, HeaderValue::from(request_id));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_id_validation_rejects_non_numeric() {
        // Refleja la tabla de errores de docs/03: 400 solo por room_id ausente o no numérico.
        let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        assert!(valid("7300000000000000000"));
        assert!(!valid(""));
        assert!(!valid("7300abc"));
        assert!(!valid("@usuario"));
    }

    #[test]
    fn error_body_carries_a_message() {
        let response = error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "límite alcanzado".into(),
            7,
            Some("concurrent_signs"),
        );
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get(X_REQUEST_ID).unwrap(), "7");
    }
}
