package dev.nglmercer.tiktools.core.nativebridge

import android.content.Context
import com.example.ttlsigner.TtlNative
import dev.nglmercer.tiktools.core.network.BundleFetch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * On-device signing through the Rust core: QuickJS in `libttl_sign_mobile.so`, reached over JNI.
 * The engine stays warm across calls — opening parses the 235 KB bundle once, signing then costs
 * one engine round trip.
 *
 * Blocking calls run on `Dispatchers.IO`; never call [sign] on the UI thread.
 */
class RustSigner private constructor(private var handle: Long) {

    /** Sign [url] under [product] (`fetch`, `frontier`, or `ws`). */
    suspend fun sign(url: String, product: String = "ws"): String =
        withContext(Dispatchers.IO) {
            val live = handle
            check(live != 0L) { "sign on a closed signer" }
            TtlNative.nSign(live, url, product)
                ?: throw IllegalStateException("the native signer returned null")
        }

    fun close() {
        val live = handle
        handle = 0L
        if (live != 0L) TtlNative.nClose(live)
    }

    companion object {
        /**
         * Download (or reuse the cached) bundle and open a warm signer. [optionsJson] is a
         * `SignOptions` object: `userAgent`, `cookie`, `xmst`, `pinned`.
         */
        suspend fun open(context: Context, optionsJson: String = "{}"): RustSigner =
            withContext(Dispatchers.IO) {
                val bundle = BundleFetch.load(context.cacheDir)
                val handle = TtlNative.nOpen(bundle, optionsJson)
                check(handle != 0L) { "the native signer failed to open" }
                RustSigner(handle)
            }

        /** Unsigned direct-socket URL for [roomId]; empty [deviceId] mints one. */
        fun socketUrl(roomId: String, deviceId: String = ""): String =
            TtlNative.nSocketUrl(roomId, deviceId)
                ?: throw IllegalStateException("the native URL builder returned null")

        fun userAgent(): String =
            TtlNative.nUserAgent() ?: throw IllegalStateException("no native User-Agent")

        fun version(): String =
            TtlNative.nVersion() ?: throw IllegalStateException("no native version")
    }
}
