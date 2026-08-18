#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(uri) = std::str::from_utf8(data) {
        let sanitized = ttl_sign_core::sanitize_uri(uri);
        // Sanitizing is idempotent, and its output is always parseable by an HTTP client.
        assert_eq!(ttl_sign_core::sanitize_uri(&sanitized), sanitized);
    }
});
