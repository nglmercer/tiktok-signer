//! HTTP server compatible with the Euler Stream sign-server specification.
//!
//! Deliberately thin layer: translates HTTP ↔ [`ttl_sign_core::SignerBackend`] calls and has
//! no signing logic or browser dependency of its own.
//!
//! The body it returns is a `ProtoMessageFetchResult`, which is what every client of this
//! specification decodes. Since 2026-08-18 the headless backend assembles that locally from a
//! signed socket URL rather than relaying one from `/webcast/im/fetch/`: `push_server` carries the
//! host and path, `route_params` every query parameter including the signature. Clients rebuild the
//! query from that map, and the socket accepts the result.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use ttl_sign_core::{RejectReason, SignError, SignOutcome, SignerBackend, TransportRequest};

/// Header required by the Python client: without it, the client aborts with `EMPTY_COOKIES`.
const X_SET_TT_COOKIE: &str = "X-Set-TT-Cookie";
/// Custom extension: the actual User-Agent so clients can reuse it on the WebSocket.
const X_SET_TT_USER_AGENT: &str = "X-Set-TT-User-Agent";
/// Room the signature belongs to. `tiktok-live-connector` reads this back.
const X_ROOM_ID: &str = "X-Room-Id";
const X_REQUEST_ID: &str = "X-Request-Id";

pub struct AppState {
    backend: Arc<dyn SignerBackend>,
    /// Allowed concurrent signatures. Queuing makes signatures expire, so requests above
    /// this limit receive 429.
    slots: tokio::sync::Semaphore,
    max_concurrent: usize,
    started: Instant,
    requests: AtomicU64,
    signs_ok: AtomicU64,
    rejects: AtomicU64,
}

impl AppState {
    pub fn new(backend: impl SignerBackend + 'static, max_concurrent: usize) -> Arc<Self> {
        Arc::new(Self {
            backend: Arc::new(backend),
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
        // Shape `tiktok-live-connector` (Node) asks for. It expects the same protobuf body
        // as the Python client, but takes the room from the path and reads `x-room-id` off
        // the response, so the two clients differ only in routing.
        .route("/webcast/rooms/{room_id}/connect", get(webcast_connect))
        .route("/healthz", get(healthz))
        .with_state(state)
}

/// Parameters sent by clients. Most are ignored: browser parameters are
/// regenerates the server-side preset, and the UA used is returned in
/// `X-Set-TT-User-Agent` so the client opens the WebSocket with the same
/// (`docs/03-spec-sign-server.md` §Regla).
#[derive(Debug, Deserialize)]
pub struct FetchQuery {
    room_id: Option<String>,
    /// Used only for logs.
    client: Option<String>,
}

/// `GET /webcast/rooms/{room_id}/connect` — the route `tiktok-live-connector` calls.
///
/// Same work as [`webcast_fetch`]; only the room's location differs.
async fn webcast_connect(
    State(state): State<Arc<AppState>>,
    Path(room_id): Path<String>,
    Query(query): Query<FetchQuery>,
) -> Response {
    sign_room(state, Some(room_id), query.client.as_deref()).await
}

async fn webcast_fetch(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FetchQuery>,
) -> Response {
    sign_room(state, query.room_id.clone(), query.client.as_deref()).await
}

async fn sign_room(
    state: Arc<AppState>,
    requested_room_id: Option<String>,
    client: Option<&str>,
) -> Response {
    let request_id = state.requests.fetch_add(1, Ordering::Relaxed) + 1;

    let room_id = match requested_room_id.as_deref() {
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) => id.to_string(),
        Some(id) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("room_id is not numeric: {id}"),
                request_id,
                None,
            )
        }
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "room_id parameter is missing".into(),
                request_id,
                None,
            )
        }
    };

    // Reject before queuing: a queued signature is an expired signature.
    let _slot = match state.slots.try_acquire() {
        Ok(slot) => slot,
        Err(_) => {
            warn!(request_id, "concurrency limit reached");
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "concurrent signature limit reached".into(),
                request_id,
                Some("concurrent_signs"),
            );
        }
    };

    let started = Instant::now();
    let outcome = state
        .backend
        .transport(TransportRequest::new(&room_id))
        .await;
    let latency_ms = started.elapsed().as_millis();

    match outcome {
        SignOutcome::Ok(signed) => {
            state.signs_ok.fetch_add(1, Ordering::Relaxed);
            info!(
                request_id,
                room_id,
                client = client.unwrap_or("-"),
                latency_ms,
                bytes = signed.protobuf.len(),
                "signature issued"
            );

            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/protobuf"),
            );
            // Without this header the Python client aborts before trying the WebSocket.
            if let Ok(v) = HeaderValue::from_str(&signed.cookies.to_cookie_string()) {
                headers.insert(X_SET_TT_COOKIE, v);
            }
            if let Ok(v) = HeaderValue::from_str(&signed.user_agent) {
                headers.insert(X_SET_TT_USER_AGENT, v);
            }
            // `tiktok-live-connector` reads the room back off the response.
            if let Ok(v) = HeaderValue::from_str(&room_id) {
                headers.insert(X_ROOM_ID, v);
            }
            headers.insert(X_REQUEST_ID, HeaderValue::from(request_id));

            (StatusCode::OK, headers, signed.protobuf).into_response()
        }

        // Rejection: TikTok detected the request. Return 502; clients must not retry.
        SignOutcome::Rejected(reason) => {
            state.rejects.fetch_add(1, Ordering::Relaxed);
            warn!(request_id, room_id, latency_ms, %reason, "signature rejected");
            error_response(
                StatusCode::BAD_GATEWAY,
                format!("TikTok rejected the request: {reason}"),
                request_id,
                match reason {
                    RejectReason::HttpStatus(_) => Some("upstream_status"),
                    _ => Some("detected"),
                },
            )
        }

        SignOutcome::Transport(err) => {
            warn!(request_id, room_id, latency_ms, %err, "transport failure");
            let status = match err {
                // The pool is not ready yet: this is retryable, and 503 communicates that.
                SignError::SdkNotReady
                | SignError::NoInstanceAvailable
                | SignError::BackendUnavailable(_)
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
    let identity = state.backend.identity();
    Json(json!({
        "ready": true,
        "uptime_s": state.started.elapsed().as_secs(),
        "user_agent": identity.user_agent,
        "in_flight": state.max_concurrent - available,
        "max_concurrent": state.max_concurrent,
        "requests": state.requests.load(Ordering::Relaxed),
        "signs_ok": state.signs_ok.load(Ordering::Relaxed),
        "rejects": state.rejects.load(Ordering::Relaxed),
    }))
}

/// JSON error body. **Never** return a 200 with an empty body: clients would interpret it as
/// detection and point diagnostics at the wrong component.
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
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;
    use ttl_sign_core::{
        BackendFuture, ClientIdentity, CookieJar, MockBackend, SignedFetch, SignerBackend,
        TransportRequest,
    };

    const ROOM: &str = "7300000000000000000";
    const USER_AGENT: &str = "fixture-agent/1";

    fn mock(outcome: SignOutcome) -> MockBackend {
        MockBackend::new(ClientIdentity::new(USER_AGENT)).with_response(ROOM, outcome)
    }

    async fn request(app: Router, uri: &str) -> Response {
        app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[test]
    fn room_id_validation_rejects_non_numeric() {
        // 400 is reserved for missing or non-numeric room_id.
        let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        assert!(valid("7300000000000000000"));
        assert!(!valid(""));
        assert!(!valid("7300abc"));
        assert!(!valid("@user"));
    }

    #[test]
    fn error_body_carries_a_message() {
        let response = error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "limit reached".into(),
            7,
            Some("concurrent_signs"),
        );
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get(X_REQUEST_ID).unwrap(), "7");
    }

    #[tokio::test]
    async fn success_contract_is_fully_headless() {
        let protobuf = vec![0x12, 0x03, b'a', b'b', b'c'];
        let outcome = SignOutcome::Ok(SignedFetch {
            protobuf: protobuf.clone(),
            cookies: CookieJar::parse("msToken=fixture-token; ttwid=fixture-id"),
            user_agent: USER_AGENT.into(),
            signed_url: "wss://fixture.invalid/webcast".into(),
        });
        let response = request(
            router(AppState::new(mock(outcome), 1)),
            &format!("/webcast/fetch?room_id={ROOM}&client=test"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/protobuf"
        );
        assert_eq!(
            response.headers()[X_SET_TT_COOKIE],
            "msToken=fixture-token; ttwid=fixture-id"
        );
        assert_eq!(response.headers()[X_SET_TT_USER_AGENT], USER_AGENT);
        assert_eq!(response.headers()[X_ROOM_ID], ROOM);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
            protobuf
        );
    }

    #[tokio::test]
    async fn path_route_uses_the_same_backend_contract() {
        let outcome = SignOutcome::Ok(SignedFetch {
            protobuf: vec![1, 2, 3],
            cookies: CookieJar::parse("msToken=fixture-token"),
            user_agent: USER_AGENT.into(),
            signed_url: "wss://fixture.invalid/webcast".into(),
        });
        let response = request(
            router(AppState::new(mock(outcome), 1)),
            &format!("/webcast/rooms/{ROOM}/connect"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[X_ROOM_ID], ROOM);
    }

    #[tokio::test]
    async fn backend_outcomes_map_to_stable_http_errors() {
        let cases = [
            (
                SignOutcome::Rejected(RejectReason::EmptyBody),
                StatusCode::BAD_GATEWAY,
            ),
            (
                SignOutcome::Transport(SignError::Timeout(250)),
                StatusCode::GATEWAY_TIMEOUT,
            ),
            (
                SignOutcome::Transport(SignError::BackendUnavailable("offline".into())),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                SignOutcome::Transport(SignError::Transport("reset".into())),
                StatusCode::BAD_GATEWAY,
            ),
        ];

        for (outcome, expected) in cases {
            let response = request(
                router(AppState::new(mock(outcome), 1)),
                &format!("/webcast/fetch?room_id={ROOM}"),
            )
            .await;
            assert_eq!(response.status(), expected);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(json["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()));
        }
    }

    #[tokio::test]
    async fn invalid_requests_are_rejected_before_the_backend() {
        let state = AppState::new(MockBackend::new(ClientIdentity::new(USER_AGENT)), 1);
        for uri in ["/webcast/fetch", "/webcast/fetch?room_id=@user"] {
            let response = request(router(state.clone()), uri).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn health_reports_backend_identity() {
        let state = AppState::new(MockBackend::new(ClientIdentity::new(USER_AGENT)), 2);
        let response = request(router(state), "/healthz").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["user_agent"], USER_AGENT);
        assert_eq!(json["max_concurrent"], 2);
    }

    #[derive(Clone)]
    struct BlockingBackend {
        entered: Arc<tokio::sync::Barrier>,
        release: Arc<tokio::sync::Notify>,
    }

    impl SignerBackend for BlockingBackend {
        fn transport(&self, _request: TransportRequest) -> BackendFuture<'_> {
            Box::pin(async move {
                self.entered.wait().await;
                self.release.notified().await;
                SignOutcome::Transport(SignError::BackendUnavailable("released".into()))
            })
        }

        fn identity(&self) -> ClientIdentity {
            ClientIdentity::new(USER_AGENT)
        }
    }

    #[tokio::test]
    async fn concurrent_requests_are_rejected_instead_of_queued() {
        let backend = BlockingBackend {
            entered: Arc::new(tokio::sync::Barrier::new(2)),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        let entered = backend.entered.clone();
        let release = backend.release.clone();
        let app = router(AppState::new(backend, 1));
        let uri = format!("/webcast/fetch?room_id={ROOM}");

        let first_app = app.clone();
        let first_uri = uri.clone();
        let first = tokio::spawn(async move { request(first_app, &first_uri).await });
        entered.wait().await;
        let second = request(app, &uri).await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["limit_label"], "concurrent_signs");

        release.notify_one();
        assert_eq!(
            first.await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
