//! A warm signer must keep working past its first signature.
//!
//! This exists because of a real failure: `byted_acrawler.registerWsSigner` is a **one-shot** — it
//! returns the signer and removes itself, so the second call finds nothing. A subprocess never
//! noticed, because it signs once and exits. A warm context signs thousands of times, and the
//! second one failed with "this bundle exposes no registerWsSigner" until the driver started
//! keeping what it was handed, which is what the player does too.

use ttl_live_discovery::{SigningProduct, UrlSigner};
use ttl_sign_embedded::{EmbeddedSigner, Profile};

const SOCKET_URL: &str = "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/\
?room_id=7300000000000000001&aid=1988&version_code=180800";

#[tokio::test(flavor = "current_thread")]
async fn a_warm_signer_keeps_signing() {
    let path = std::env::var("TTL_BUNDLE").unwrap_or_else(|_| "/tmp/webmssdk.js".into());
    let Ok(source) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no signing bundle at {path}");
        return;
    };

    let signer = EmbeddedSigner::new(source, Profile::default()).expect("prepare");

    let mut seen = std::collections::HashSet::new();
    for attempt in 1..=25 {
        let signed = signer
            .sign(SOCKET_URL)
            .await
            .unwrap_or_else(|error| panic!("signature {attempt} failed: {error}"));
        assert!(
            signed.contains("&X-Gnarly="),
            "signature {attempt} carried no X-Gnarly"
        );
        seen.insert(signed);
    }
    // Each one differs: the SDK carries per-call state, exactly as it does in a browser. Identical
    // signatures would mean the context is stuck, not that it is fast.
    assert_eq!(seen.len(), 25, "a warm context repeated a signature");
}

/// Every product has to survive the same treatment, not just the socket one.
#[tokio::test(flavor = "current_thread")]
async fn every_product_signs_twice() {
    let path = std::env::var("TTL_BUNDLE").unwrap_or_else(|_| "/tmp/webmssdk.js".into());
    let Ok(source) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no signing bundle at {path}");
        return;
    };
    let signer = EmbeddedSigner::new(source, Profile::default()).expect("prepare");
    let fetch_url = "https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001";

    for product in [
        SigningProduct::FetchPatch,
        SigningProduct::FrontierSign,
        SigningProduct::WsDirect,
    ] {
        let url = if product == SigningProduct::WsDirect {
            SOCKET_URL
        } else {
            fetch_url
        };
        for attempt in 1..=2 {
            signer
                .sign_with(url, product)
                .await
                .unwrap_or_else(|error| panic!("{product:?} attempt {attempt}: {error}"));
        }
    }
}
