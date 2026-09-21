//! The mobile signer must produce what the reference produces — byte for byte.
//!
//! Same sandbox (`bootstrap.js`), same pinned profile, two engines: QuickJS in-process
//! here, V8 through `scripts/headless/tools/sign-pinned.mjs`. Any drift between them is a
//! wrong signature.
//!
//! Needs the signing bundle, which is deliberately not vendored. Without it, and without
//! `node`, the parity test skips rather than failing: a fresh clone must be able to run
//! `cargo test` offline.
//!
//! ```sh
//! curl -s -o /tmp/webmssdk.js \
//!   https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js
//! cargo test -p ttl-sign-mobile
//! ```
//!
//! No signed URL is committed here: the reference output is produced at test time, so the
//! repository never holds a reusable signature. See `ttl-fixture-hygiene`.

use std::path::{Path, PathBuf};
use std::process::Command;

use ttl_sign_mobile::{direct_socket_url, MobileSigner, Product};

/// Fixed device id: the input URL must be deterministic for a pinned comparison.
const DEVICE_ID: &str = "1234567890123456789";
const ROOM_ID: &str = "7300000000000000001";

fn bundle_path() -> Option<PathBuf> {
    let path = std::env::var("TTL_BUNDLE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/webmssdk.js"));
    path.is_file().then_some(path)
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn has_node() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn reference_sign(bundle: &Path, url: &str) -> Option<String> {
    let tool = repository_root().join("scripts/headless/tools/sign-pinned.mjs");
    let output = Command::new("node")
        .arg(tool)
        .arg(bundle)
        .arg(url)
        .arg("ws")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn pinned_ws_matches_the_node_reference_byte_for_byte() {
    let Some(bundle) = bundle_path() else {
        eprintln!("skipping: no signing bundle (set TTL_BUNDLE or fetch /tmp/webmssdk.js)");
        return;
    };
    if !has_node() {
        eprintln!("skipping: node is needed for the reference signature");
        return;
    }
    let bundle_source = std::fs::read_to_string(&bundle).expect("read the bundle");
    let url = direct_socket_url(ROOM_ID, Some(DEVICE_ID));
    let signer = MobileSigner::new(&bundle_source, r#"{"pinned":true}"#).expect("open the signer");

    let signed = signer.sign(&url, Product::Ws).expect("sign the URL");
    // Shape first, so a gross failure reads as one rather than as a diff.
    assert!(
        signed.starts_with(&format!("{url}&X-Gnarly=")),
        "the ws product must append X-Gnarly, got: {}",
        &signed[..signed.len().min(120)]
    );
    let gnarly = signed.split("&X-Gnarly=").nth(1).unwrap();
    assert!(!gnarly.is_empty(), "X-Gnarly is empty");

    let reference = reference_sign(&bundle, &url).expect("the reference must sign");
    assert_eq!(
        signed, reference,
        "mobile QuickJS diverged from the Node/V8 reference"
    );
}

#[test]
fn a_pinned_first_signature_is_deterministic_across_signers() {
    // The ws signer object carries a per-call counter, so the second signature of one
    // signer differs from its first even when pinned. Determinism means two fresh
    // signers agree on their first signature — which is also what the reference test
    // above compares, since `sign-pinned.mjs` prepares once and signs once.
    let Some(bundle) = bundle_path() else {
        eprintln!("skipping: no signing bundle (set TTL_BUNDLE or fetch /tmp/webmssdk.js)");
        return;
    };
    let bundle_source = std::fs::read_to_string(&bundle).expect("read the bundle");
    let url = direct_socket_url(ROOM_ID, Some(DEVICE_ID));

    let first = MobileSigner::new(&bundle_source, r#"{"pinned":true}"#)
        .expect("open the first signer")
        .sign(&url, Product::Ws)
        .expect("first signature");
    let second = MobileSigner::new(&bundle_source, r#"{"pinned":true}"#)
        .expect("open the second signer")
        .sign(&url, Product::Ws)
        .expect("second signature");
    assert_eq!(first, second, "pinned first signatures must agree");
}

/// The C ABI signs what the Rust API signs. Exercises the real boundary — raw
/// pointers in, owned strings out — rather than trusting it by inspection.
#[test]
fn the_c_abi_signs_like_the_rust_api() {
    use std::ffi::{CStr, CString};

    let Some(bundle) = bundle_path() else {
        eprintln!("skipping: no signing bundle (set TTL_BUNDLE or fetch /tmp/webmssdk.js)");
        return;
    };
    let bundle_source = std::fs::read_to_string(&bundle).expect("read the bundle");
    let url = direct_socket_url(ROOM_ID, Some(DEVICE_ID));

    let expected = MobileSigner::new(&bundle_source, r#"{"pinned":true}"#)
        .expect("open the signer")
        .sign(&url, Product::Ws)
        .expect("sign the URL");

    let bundle_c = CString::new(bundle_source).unwrap();
    let options_c = CString::new(r#"{"pinned":true}"#).unwrap();
    // SAFETY: the CString ranges are live for the call.
    let handle = unsafe {
        ttl_sign_mobile::ffi::ttl_mobile_open(
            bundle_c.as_ptr(),
            bundle_c.count_bytes(),
            options_c.as_ptr(),
            options_c.count_bytes(),
        )
    };
    assert!(!handle.is_null(), "C ABI open failed");
    let url_c = CString::new(url).unwrap();
    let product_c = CString::new("ws").unwrap();
    // SAFETY: the handle is live and the CString ranges are live for the call.
    let out = unsafe {
        ttl_sign_mobile::ffi::ttl_mobile_sign(
            handle,
            url_c.as_ptr(),
            url_c.count_bytes(),
            product_c.as_ptr(),
            product_c.count_bytes(),
        )
    };
    assert!(!out.is_null(), "C ABI sign failed");
    // SAFETY: `out` came from `ttl_mobile_sign` and is freed once, here.
    let signed = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `out` came from `ttl_mobile_sign` and is freed once, here; the handle
    // is closed once, here.
    unsafe {
        ttl_sign_mobile::ffi::ttl_mobile_free_str(out);
        ttl_sign_mobile::ffi::ttl_mobile_close(handle);
    }

    assert_eq!(
        signed, expected,
        "the C ABI must sign what the Rust API signs"
    );
}
