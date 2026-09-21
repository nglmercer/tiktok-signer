package com.example.ttlsigner

/**
 * Live stream callbacks from the Rust worker thread. Method names and signatures
 * must match `crates/ttl-sign-mobile/src/jni_live.rs`; implementations must be
 * thread-safe (calls arrive off the UI thread).
 */
interface TtlEvents {
    /** One event as JSON (`{"type": "chat"|"gift"|"like"|…, …}`). */
    fun onEvent(json: String)

    /** Stream state: `connecting`, `open`, `reconnecting`, `decode_error`, `closed`, `error`. */
    fun onState(kind: String, detail: String)
}
