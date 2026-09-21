//! Print the pinned `ws` signature for a fixed socket URL.
//!
//! Exists to record the vector `tests/mobile_sign.rs` asserts against, and to diff this
//! crate's QuickJS output against the Node/V8 reference:
//!
//! ```sh
//! cargo run -p ttl-sign-mobile --example print_pinned > /tmp/mobile-pinned.txt
//! node scripts/headless/tools/sign-pinned.mjs /tmp/webmssdk.js \
//!   "$(sed -n 1p /tmp/mobile-pinned.txt)" ws > /tmp/node-pinned.txt
//! diff <(sed -n 2p /tmp/mobile-pinned.txt) /tmp/node-pinned.txt
//! ```

use ttl_sign_mobile::{direct_socket_url, MobileSigner, Product};

fn main() {
    let bundle = std::fs::read_to_string(bundle_path()).expect("read the signing bundle");
    let url = direct_socket_url("7300000000000000001", Some("1234567890123456789"));
    let signer = MobileSigner::new(&bundle, r#"{"pinned":true}"#).expect("open the signer");
    let signed = signer.sign(&url, Product::Ws).expect("sign the URL");
    println!("{url}");
    println!("{signed}");
}

fn bundle_path() -> std::path::PathBuf {
    std::env::var("TTL_BUNDLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/webmssdk.js"))
}
