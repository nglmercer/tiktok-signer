//! The embedded signer must produce what the reference produced — byte for byte.
//!
//! Same sandbox (`bootstrap.js`), same pinned profile, same bundle. The reference is
//! `fixtures/research/pinned-signatures-1.0.0.388.json`: the SHA-256 and length of what V8 signed
//! for each product, recorded once before V8 left the repository. QuickJS is now the only engine,
//! so the recorded bytes are what keeps it honest — any drift is a wrong signature, which is not a
//! portability problem that can be lived with.
//!
//! Needs the signing bundle, which is deliberately not vendored — it is TikTok's, and a public
//! static asset. Without it the test skips rather than failing: a fresh clone must be able to run
//! `cargo test` offline. A bundle with a different digest also skips, because the recorded
//! signatures belong to one bundle version.
//!
//! ```sh
//! curl -s -o /tmp/webmssdk.js \
//!   https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
//! cargo test -p ttl-sign-embedded
//! ```

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use ttl_live_discovery::{SigningProduct, UrlSigner};
use ttl_sign_embedded::{EmbeddedSigner, Profile};

/// A URL for each product. The socket one carries a query because its signature covers those bytes.
const CASES: [(SigningProduct, &str); 3] = [
    (
        SigningProduct::FetchPatch,
        "https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001",
    ),
    (
        SigningProduct::FrontierSign,
        "https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001",
    ),
    (
        SigningProduct::WsDirect,
        "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/\
?room_id=7300000000000000001&aid=1988&version_code=180800",
    ),
];

fn bundle_path() -> Option<PathBuf> {
    let path = std::env::var("TTL_BUNDLE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/webmssdk.js"));
    path.is_file().then_some(path)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn product_name(product: SigningProduct) -> &'static str {
    match product {
        SigningProduct::FetchPatch => "fetch",
        SigningProduct::FrontierSign => "frontier",
        SigningProduct::WsDirect => "ws",
    }
}

fn recorded() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/research/pinned-signatures-1.0.0.388.json");
    let text = std::fs::read_to_string(&path).expect("read the recorded signatures");
    serde_json::from_str(&text).expect("parse the recorded signatures")
}

/// The recorded case must match the URL this test signs, or the comparison is meaningless.
#[test]
fn the_recorded_cases_are_the_cases_signed_here() {
    let recorded = recorded();
    let cases = recorded["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), CASES.len());
    for ((product, url), case) in CASES.iter().zip(cases) {
        assert_eq!(case["product"], product_name(*product));
        assert_eq!(case["url"], *url);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn quickjs_signs_exactly_what_v8_signed() {
    let Some(bundle) = bundle_path() else {
        eprintln!("skipped: no signing bundle at TTL_BUNDLE or /tmp/webmssdk.js");
        return;
    };
    let source = std::fs::read_to_string(&bundle).expect("read the bundle");
    let recorded = recorded();
    if sha256_hex(source.as_bytes()) != recorded["bundle_sha256"] {
        eprintln!("skipped: the bundle is not the one the signatures were recorded from");
        return;
    }

    for ((product, url), case) in CASES.into_iter().zip(recorded["cases"].as_array().unwrap()) {
        // A fresh context per product: the SDK carries per-call state, so the first signature out
        // of a context is the one that was recorded.
        let signer = EmbeddedSigner::with_product(
            source.clone(),
            Profile {
                pinned: true,
                ..Profile::default()
            },
            product,
        )
        .expect("prepare the embedded signer");

        let embedded = signer.sign(url).await.expect("embedded signature");
        assert_eq!(
            embedded.len() as u64,
            case["signed_length"].as_u64().unwrap(),
            "{product:?} signature length differs from the recorded one"
        );
        assert_eq!(
            sha256_hex(embedded.as_bytes()),
            case["signed_sha256"].as_str().unwrap(),
            "{product:?} differs from the signature recorded from V8"
        );
    }
}

/// A pinned profile is reproducible; an unpinned one must not be. Both halves matter: the first is
/// what makes the parity test meaningful, and the second is what makes a signature usable.
#[tokio::test(flavor = "current_thread")]
async fn pinning_decides_whether_a_signature_repeats() {
    let Some(bundle) = bundle_path() else {
        eprintln!("skipped: no signing bundle at TTL_BUNDLE or /tmp/webmssdk.js");
        return;
    };
    let source = std::fs::read_to_string(&bundle).expect("read the bundle");
    let url = CASES[2].1;

    let pinned = EmbeddedSigner::with_product(
        source.clone(),
        Profile {
            pinned: true,
            ..Profile::default()
        },
        SigningProduct::WsDirect,
    )
    .expect("pinned signer");
    // Two signers rather than two calls: the SDK carries per-call state, so even pinned it does not
    // repeat within one context — which is exactly what a browser does.
    let another = EmbeddedSigner::with_product(
        source.clone(),
        Profile {
            pinned: true,
            ..Profile::default()
        },
        SigningProduct::WsDirect,
    )
    .expect("second pinned signer");
    assert_eq!(
        pinned.sign(url).await.unwrap(),
        another.sign(url).await.unwrap(),
        "a pinned profile must be reproducible, or no differential means anything"
    );

    let live = EmbeddedSigner::with_product(source, Profile::default(), SigningProduct::WsDirect)
        .expect("live signer");
    assert_ne!(
        live.sign(url).await.unwrap(),
        pinned.sign(url).await.unwrap(),
        "an unpinned signature must not equal the frozen one"
    );
}
