//! Shared behavioral contract exercised against every headless backend.

use std::path::Path;

use ttl_sign_core::{
    ClientIdentity, CookieJar, MockBackend, SignError, SignOutcome, SignedFetch, SignerBackend,
    TransportRequest,
};
use ttl_sign_native::{
    FixedClock, NativeBackend, NativeConfig, SignatureMaterial, StaticAlgorithm,
};

fn native_preset() -> ttl_sign_core::Preset {
    ttl_sign_core::Preset::new(
        ttl_sign_core::DevicePreset::chrome_linux(),
        ttl_sign_core::LocationPreset::us_east(),
        ttl_sign_core::ScreenPreset::FHD,
    )
}
use ttl_sign_replay::ReplayBackend;

const ROOM: &str = "7300000000000000001";
const MISSING_ROOM: &str = "9999999999999999999";

async fn backend_contract(backend: &dyn SignerBackend) {
    assert!(!backend.identity().user_agent.is_empty());

    let success = backend.transport(TransportRequest::new(ROOM)).await;
    let signed = success.ok().expect("known case must succeed");
    assert!(!signed.protobuf.is_empty());
    assert!(!signed.user_agent.is_empty());
    assert!(!signed.signed_url.is_empty());

    let missing = backend.transport(TransportRequest::new(MISSING_ROOM)).await;
    assert!(matches!(
        missing,
        SignOutcome::Transport(SignError::BackendUnavailable(_))
    ));
}

fn signed() -> SignedFetch {
    SignedFetch {
        protobuf: ttl_sign_core::FetchResult {
            push_server: "wss://fixture.invalid/ws/".into(),
            route_params: vec![("wrss".into(), "fixture-route".into())],
            cursor: "fixture-cursor".into(),
            internal_ext: "fixture-internal".into(),
            heartbeat_duration: 10_000,
            need_ack: true,
        }
        .encode(),
        cookies: CookieJar::parse("msToken=fixture-token"),
        user_agent: "fixture-agent/1".into(),
        signed_url: "wss://fixture.invalid/ws/".into(),
    }
}

#[tokio::test]
async fn mock_backend_contract() {
    let backend = MockBackend::new(ClientIdentity::new("fixture-agent/1"))
        .with_response(ROOM, SignOutcome::Ok(signed()));
    backend_contract(&backend).await;
}

#[tokio::test]
async fn replay_backend_contract() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/signing");
    let backend = ReplayBackend::load(corpus).unwrap();
    backend_contract(&backend).await;
}

#[tokio::test]
async fn native_backend_contract() {
    let preset = native_preset();
    let material = SignatureMaterial {
        push_server: "wss://fixture.invalid/ws/".into(),
        route_params: vec![("wrss".into(), "fixture-route".into())],
        cursor: "fixture-cursor".into(),
        internal_ext: "fixture-internal".into(),
        heartbeat_duration: 10_000,
        need_ack: true,
        signed_url: "wss://fixture.invalid/ws/".into(),
    };
    let backend = NativeBackend::new(
        NativeConfig::new(
            preset,
            "7123456789012345678",
            CookieJar::parse("msToken=fixture-token"),
            FixedClock(1_700_000_000_000),
        ),
        StaticAlgorithm::new().with_response(ROOM, Ok(material)),
    );
    backend_contract(&backend).await;
}

/// Guard the production boundary: the default (unsupported) native backend must not be
/// presentable as a live signer. If someone wires `NativeBackend::unsupported` into the
/// server, every signing request fails explicitly instead of returning a fabricated or
/// silently-rejected transport.
#[tokio::test]
async fn unsupported_native_backend_is_not_live_compatible() {
    let backend = NativeBackend::unsupported(NativeConfig::new(
        native_preset(),
        "7123456789012345678",
        CookieJar::parse("msToken=fixture-token"),
        FixedClock(1_700_000_000_000),
    ));

    let outcome = backend.transport(TransportRequest::new(ROOM)).await;
    assert!(
        matches!(
            outcome,
            SignOutcome::Transport(SignError::BackendUnavailable(_))
        ),
        "unsupported native backend must fail loudly, not sign or reject"
    );
}
