#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(uri) = std::str::from_utf8(data) {
        let _ = ttl_sign_core::sanitize_uri(uri);
        if let Some(result) = ttl_sign_core::fetch_result_from_ws_uri(uri) {
            let encoded = result.encode();
            let _ = ttl_sign_core::FetchResult::decode(&encoded);
        }
    }
});
