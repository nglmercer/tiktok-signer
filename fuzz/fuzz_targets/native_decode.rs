#![no_main]
use libfuzzer_sys::fuzz_target;
use ttl_live::*;
fuzz_target!(|data: &[u8]| unsafe {
    if data.len() > 16 * 1024 * 1024 { return; }
    let len = (data.first().copied().unwrap_or(0) as usize).min(data.len());
    let e = ttl_event_decode(data.as_ptr().cast(),len,u64::MAX,false,data[len..].as_ptr(),data.len()-len);
    if !e.is_null() {
        let copy = ttl_event_clone(e);
        for mode in 0..5 { ttl_string_free(ttl_event_to_json(copy,mode)); }
        ttl_event_free(e);ttl_event_free(copy);
    }
    ttl_batch_free(ttl_batch_decode(data.as_ptr(),data.len()));
});
