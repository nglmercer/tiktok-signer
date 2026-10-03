package dev.nglmercer.tiktools.core.network

import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Fetches TikTok's `webmssdk` signing bundle — a public static asset, deliberately not vendored —
 * and caches it for a day. Both signing paths share this: the bundle is the same bytes the server
 * signs with, pinned to the version this project was measured against
 * (`packages/tiktok-live/src/signer.ts` carries the same URL and digest).
 */
object BundleFetch {
    const val BUNDLE_URL =
        "https://sf16-website-login.neutral.ttwstatic.com/obj/tiktok_web_login_static/webmssdk/1.0.0.388/webmssdk.js"
    const val BUNDLE_SHA256 = "dee22566273d398e074df6db40f39cfd827f6b8efd6fc382de03c44c501299ac"
    const val BUNDLE_VERSION = "1.0.0.388"

    private const val CACHE_NAME = "webmssdk-1.0.0.388.js"
    private const val MAX_AGE_MS = 24L * 60 * 60 * 1000

    /** What the settings tab shows about the cached bundle, if any. */
    data class CacheInfo(val sizeBytes: Long, val ageMs: Long, val shaOk: Boolean)

    fun cacheFile(cacheDir: File): File = File(cacheDir, CACHE_NAME)

    /**
     * Describe the cached bundle, or null when nothing is cached. Reads the file to verify the
     * digest — call off the UI thread.
     */
    fun describeCache(cacheDir: File): CacheInfo? {
        val cached = cacheFile(cacheDir)
        if (!cached.isFile) return null
        val bytes = cached.readBytes()
        return CacheInfo(
            sizeBytes = bytes.size.toLong(),
            ageMs = System.currentTimeMillis() - cached.lastModified(),
            shaOk = sha256Hex(bytes.toString(Charsets.UTF_8)) == BUNDLE_SHA256,
        )
    }

    /** Drop the cache and download again. The next signer open parses the result. */
    suspend fun refresh(cacheDir: File): String =
        withContext(Dispatchers.IO) {
            cacheFile(cacheDir).delete()
            load(cacheDir)
        }

    suspend fun load(cacheDir: File): String =
        withContext(Dispatchers.IO) {
            val cached = File(cacheDir, CACHE_NAME)
            if (cached.isFile && System.currentTimeMillis() - cached.lastModified() < MAX_AGE_MS) {
                val text = cached.readText()
                if (sha256Hex(text) == BUNDLE_SHA256) return@withContext text
            }
            val text = download()
            if (sha256Hex(text) != BUNDLE_SHA256) {
                throw IllegalStateException("the signing bundle did not match its expected SHA-256")
            }
            val tmp = File.createTempFile("webmssdk", ".tmp", cacheDir)
            try {
                tmp.writeText(text)
                if (!tmp.renameTo(cached)) cached.writeText(text)
            } finally {
                tmp.delete()
            }
            text
        }

    private fun download(): String {
        val connection = URL(BUNDLE_URL).openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 15_000
            connection.readTimeout = 30_000
            connection.setRequestProperty("User-Agent", "Mozilla/5.0")
            if (connection.responseCode != HttpURLConnection.HTTP_OK) {
                throw IllegalStateException(
                    "could not download the signing bundle: HTTP ${connection.responseCode}"
                )
            }
            return connection.inputStream.bufferedReader().readText()
        } finally {
            connection.disconnect()
        }
    }

    fun sha256Hex(text: String): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(text.toByteArray(Charsets.UTF_8))
        return digest.joinToString("") { "%02x".format(it) }
    }
}
