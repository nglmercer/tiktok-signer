#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = ttl_sign_core::FetchResult::decode(data);
    let _ = ttl_sign_core::proto::PushFrame::decode(data);
    let _ = ttl_sign_core::proto::WebcastEventBatch::decode(data);
});
