//! Print the pinned `ws` signature for a fixed socket URL.
//!
//! Exists to inspect the vector `tests/mobile_sign.rs` asserts against. Its digest is recorded
//! in `fixtures/research/pinned-signatures-1.0.0.388.json` (`mobile_cases`):
//!
//! ```sh
//! cargo run -p ttl-sign-mobile --example print_pinned | sed -n 2p | tr -d '\n' | sha256sum
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
