//! The embedded signer must produce what the reference produces — byte for byte.
//!
//! Same sandbox (`bootstrap.js`), same pinned profile, two engines: QuickJS in-process here, V8
//! through `scripts/headless/tools/sign-pinned.mjs`. Any drift between them is a wrong signature,
//! which is not a portability problem that can be lived with.
//!
//! Needs the signing bundle, which is deliberately not vendored — it is TikTok's, and a public
//! static asset. Without it, and without `node`, the test skips rather than failing: a fresh clone
//! must be able to run `cargo test` offline.
//!
//! ```sh
//! curl -s -o /tmp/webmssdk.js \
//!   https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
//! cargo test -p ttl-sign-embedded
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

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

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repository root")
}

fn has_node() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
    }

/// V8's answer for the same URL, product and profile.
fn reference(bundle: &Path, url: &str, product: SigningProduct) -> String {
    let root = repository_root();
    let script = root.join("scripts/headless/tools/sign-pinned.mjs");
    let name = match product {
        SigningProduct::FetchPatch => "fetch",
        SigningProduct::FrontierSign => "frontier",
        SigningProduct::WsDirect => "ws",
    };
    let out = Command::new("node")
        .arg(&script)
        .arg(bundle)
        .arg(url)
        .arg(name)
        .output()
        .expect("run the reference signer");
    assert!(
        out.status.success(),
        "reference signer failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("reference output is UTF-8")
}

#[tokio::test(flavor = "current_thread")]
async fn quickjs_signs_exactly_what_v8_signs() {
    let Some(bundle) = bundle_path() else {
        eprintln!("skipped: no signing bundle at TTL_BUNDLE or /tmp/webmssdk.js");
        return;
    };
    if !has_node() {
        eprintln!("skipped: node is not available to produce the reference");
        return;
    }
    let source = std::fs::read_to_string(&bundle).expect("read the bundle");

    for (product, url) in CASES {
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
            embedded,
            reference(&bundle, url, product),
            "{product:?} differs between QuickJS and V8"
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

    let live = EmbeddedSigner::with_product(
        source,
        Profile::default(),
        SigningProduct::WsDirect,
    )
    .expect("live signer");
    assert_ne!(
        live.sign(url).await.unwrap(),
        pinned.sign(url).await.unwrap(),
        "an unpinned signature must not equal the frozen one"
    );
}

/// The two embedded engines against each other, with no Node in the picture.
///
/// This is the check that keeps a second engine honest: it needs nothing installed, so it runs in
/// CI on a clone with a cached bundle, and it fails on the difference that matters — the bytes.
#[cfg(all(feature = "quickjs", feature = "v8"))]
#[tokio::test(flavor = "current_thread")]
async fn both_embedded_engines_sign_the_same_bytes() {
    use ttl_sign_embedded::{QuickJsSigner, V8Signer};

    let Some(bundle) = bundle_path() else {
        eprintln!("skipped: no signing bundle at TTL_BUNDLE or /tmp/webmssdk.js");
        return;
    };
    let source = std::fs::read_to_string(&bundle).expect("read the bundle");
    let pinned = || Profile {
        pinned: true,
        ..Profile::default()
    };

    for (product, url) in CASES {
        // A fresh context per engine per product: the SDK carries per-call state, so the first
        // signature out of each is the only one the two engines can be asked to agree on.
        let quickjs = QuickJsSigner::with_product(source.clone(), pinned(), product)
            .expect("prepare QuickJS");
        let v8 = V8Signer::with_product(source.clone(), pinned(), product).expect("prepare V8");
        assert_eq!(
            quickjs.sign(url).await.expect("QuickJS signature"),
            v8.sign(url).await.expect("V8 signature"),
            "{product:?} differs between the embedded engines"
        );
    }
}
