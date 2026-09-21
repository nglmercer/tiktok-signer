package com.example.ttlsigner

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * On-device proof: the reused Rust core signs `ws` on a real (emulated) phone.
 * Needs network for the one-time bundle download; signing itself is offline.
 * Unpinned signatures differ by design, so this asserts shape — presence and length
 * of `X-Gnarly` — while byte-parity with the reference lives host-side in
 * `crates/ttl-sign-mobile/tests/mobile_sign.rs`.
 */
@RunWith(AndroidJUnit4::class)
class OnDeviceSignTest {

    private val roomId = "7300000000000000001"

    @Test
    fun nativeLibraryLoadsAndReportsItsVersion() {
        val version = RustSigner.version()
        assertTrue("unexpected version: $version", version.startsWith("ttl-sign-mobile "))
        assertTrue("unexpected version: $version", version.contains("QuickJS"))
    }

    @Test
    fun nativeSocketUrlIsWellFormedAndUnsigned() {
        val url = RustSigner.socketUrl(roomId, "1234567890123456789")
        assertTrue("unexpected URL: $url", url.startsWith(
            "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?"))
        assertTrue("no room: $url", url.contains("room_id=$roomId"))
        assertTrue("must be unsigned: $url", !url.contains("X-Gnarly"))
        val ua = RustSigner.userAgent()
        assertTrue("not an Android UA: $ua", ua.contains("Android"))
    }

    @Test
    fun rustSignsWsOnDevice(): Unit = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val signer = RustSigner.open(context, "{}")
        try {
            val url = RustSigner.socketUrl(roomId)
            val signed = signer.sign(url, "ws")
            assertTrue(
                "must append X-Gnarly, got ${signed.take(120)}",
                signed.startsWith("$url&X-Gnarly="),
            )
            val gnarly = signed.substringAfter("&X-Gnarly=", "")
            // A real X-Gnarly is hundreds of chars; anything under 100 is a stub.
            assertTrue("X-Gnarly too short (${gnarly.length})", gnarly.length > 100)
        } finally {
            signer.close()
        }
    }
}
