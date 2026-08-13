//! Offline integration boundary: fixture corpus -> backend -> HTTP response.

use std::path::Path;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use ttl_sign_core::FetchResult;
use ttl_sign_replay::ReplayBackend;
use ttl_sign_server::{router, AppState};

const SUCCESS_ROOM: &str = "7300000000000000001";

#[tokio::test]
async fn replay_fixture_serves_a_decodable_transport_without_webview() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/signing");
    let backend = ReplayBackend::load(corpus).unwrap();
    let app = router(AppState::new(backend, 1));

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/webcast/fetch?room_id={SUCCESS_ROOM}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-room-id"], SUCCESS_ROOM);
    assert_eq!(response.headers()["x-set-tt-user-agent"], "fixture-agent/1");
    assert_eq!(
        response.headers()["x-set-tt-cookie"],
        "msToken=fixture-ms-token; ttwid=fixture-ttwid"
    );

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let transport = FetchResult::decode(&body).unwrap();
    assert_eq!(transport.cursor, "fixture-cursor");
    assert_eq!(transport.route_params.len(), 2);
}

#[tokio::test]
async fn replay_error_corpus_exercises_http_compatibility_offline() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/signing");
    let app = router(AppState::new(ReplayBackend::load(corpus).unwrap(), 1));
    let cases = [
        ("7300000000000000002", StatusCode::BAD_GATEWAY),
        ("7300000000000000003", StatusCode::GATEWAY_TIMEOUT),
        ("7300000000000000004", StatusCode::BAD_GATEWAY),
        ("7300000000000000005", StatusCode::SERVICE_UNAVAILABLE),
        ("7300000000000000006", StatusCode::BAD_GATEWAY),
        ("7300000000000000007", StatusCode::BAD_GATEWAY),
        ("7300000000000000008", StatusCode::BAD_GATEWAY),
        ("7300000000000000009", StatusCode::BAD_GATEWAY),
    ];

    for (room_id, expected) in cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/webcast/fetch?room_id={room_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "room {room_id}");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()));
    }
}
