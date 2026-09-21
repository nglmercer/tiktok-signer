package com.example.ttlsigner

/**
 * Raw JNI boundary to `libttl_sign_mobile.so`. Method names and signatures must match
 * `crates/ttl-sign-mobile/src/jni.rs`; every failure arrives as a `RuntimeException`.
 *
 * Callers use [RustSigner], never this object directly.
 */
object TtlNative {
    init {
        System.loadLibrary("ttl_sign_mobile")
    }

    /** Open a signer over [bundle] with [optionsJson]; returns the native handle. */
    @JvmStatic
    external fun nOpen(bundle: String, optionsJson: String): Long

    /** Sign [url] under [product] (`fetch`, `frontier`, or `ws`). Blocks. */
    @JvmStatic
    external fun nSign(handle: Long, url: String, product: String): String?

    @JvmStatic
    external fun nClose(handle: Long)

    /** Unsigned direct-socket URL for [roomId]; empty [deviceId] mints one. */
    @JvmStatic
    external fun nSocketUrl(roomId: String, deviceId: String): String?

    /** The User-Agent every request in the flow must present. */
    @JvmStatic
    external fun nUserAgent(): String?

    /** `ttl-sign-mobile <version> (<engine>)`. */
    @JvmStatic
    external fun nVersion(): String?
}
