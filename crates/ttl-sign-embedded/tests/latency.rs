//! What each engine costs, so `docs/13-embedded-runtime.md` can be re-derived rather than believed.
//!
//! Ignored by default: it is a measurement, not a check, and a number that varies with the machine
//! must never be allowed to fail a build.
//!
//! ```sh
//! cargo test --release -p ttl-sign-embedded --features v8 --test latency -- --ignored --nocapture
//! ```

use std::time::Instant;

use ttl_live_discovery::{SigningProduct, UrlSigner};
use ttl_sign_embedded::Profile;

const URL: &str = "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/\
?room_id=7300000000000000001&aid=1988&version_code=180800";

const ROUNDS: u32 = 20;

fn bundle() -> Option<String> {
    let path = std::env::var("TTL_BUNDLE").unwrap_or_else(|_| "/tmp/webmssdk.js".into());
    std::fs::read_to_string(path).ok()
}

/// Load the bundle once, then sign [`ROUNDS`] times off the warm context.
macro_rules! measure {
    ($name:ident, $signer:ty, $label:literal) => {
        #[tokio::test(flavor = "current_thread")]
        #[ignore = "a measurement, not a check"]
        async fn $name() {
            let Some(source) = bundle() else {
                eprintln!("skipped: no signing bundle at TTL_BUNDLE or /tmp/webmssdk.js");
                return;
            };

            let started = Instant::now();
            let signer = <$signer>::with_product(
                source,
                Profile::default(),
                SigningProduct::WsDirect,
            )
            .expect("prepare the signer");
            let prepare = started.elapsed();

            let started = Instant::now();
            for _ in 0..ROUNDS {
                signer.sign(URL).await.expect("signature");
            }
            let each = started.elapsed() / ROUNDS;

            println!(
                "{}: {} ms to load the bundle, {} ms per signature over {ROUNDS}",
                $label,
                prepare.as_millis(),
                each.as_millis(),
            );
        }
    };
}

#[cfg(feature = "quickjs")]
measure!(quickjs_latency, ttl_sign_embedded::QuickJsSigner, "QuickJS");
#[cfg(feature = "v8")]
measure!(v8_latency, ttl_sign_embedded::V8Signer, "V8");
