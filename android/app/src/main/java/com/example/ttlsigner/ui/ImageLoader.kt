package com.example.ttlsigner.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import android.widget.ImageView
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Tiny URL → [ImageView] loader: an in-memory LRU, one shared IO scope, and a
 * view-tag guard so recycled rows never show another row's download. No disk
 * cache, no transforms — enough for CDN avatars, covers, and gift art without
 * a third-party image library.
 */
object ImageLoader {

    private const val MAX_BYTES = 2 * 1024 * 1024
    private const val TARGET_PX = 160

    private val cache = object : LruCache<String, Bitmap>(
        (Runtime.getRuntime().maxMemory() / 8 / 1024).toInt().coerceAtLeast(4 * 1024),
    ) {
        override fun sizeOf(key: String, value: Bitmap): Int =
            (value.byteCount / 1024).coerceAtLeast(1)
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /**
     * Load [url] into [view]. Empty URLs bind [fallback] immediately; cached
     * bitmaps bind synchronously. While fetching, [placeholder] shows (when
     * non-zero); a failed fetch falls back to [fallback] (when non-zero).
     */
    fun load(view: ImageView, url: String, placeholder: Int = 0, fallback: Int = 0) {
        view.tag = url
        if (url.isEmpty()) {
            if (fallback != 0) view.setImageResource(fallback) else view.setImageDrawable(null)
            return
        }
        cache.get(url)?.let {
            view.setImageBitmap(it)
            return
        }
        if (placeholder != 0) view.setImageResource(placeholder)
        scope.launch {
            val bitmap = fetch(url)
            withContext(Dispatchers.Main) {
                // The row was recycled mid-flight: someone else owns the view now.
                if (view.tag != url) return@withContext
                if (bitmap != null) {
                    cache.put(url, bitmap)
                    view.setImageBitmap(bitmap)
                } else if (fallback != 0) {
                    view.setImageResource(fallback)
                }
            }
        }
    }

    private fun fetch(url: String): Bitmap? {
        return try {
            val connection = URL(url).openConnection() as HttpURLConnection
            try {
                connection.connectTimeout = 10_000
                connection.readTimeout = 10_000
                connection.instanceFollowRedirects = true
                if (connection.responseCode !in 200..299) return null
                val bytes = connection.inputStream.use { stream ->
                    val out = java.io.ByteArrayOutputStream()
                    val buf = ByteArray(8192)
                    var left = MAX_BYTES
                    while (left > 0) {
                        val n = stream.read(buf, 0, minOf(buf.size, left))
                        if (n <= 0) break
                        out.write(buf, 0, n)
                        left -= n
                    }
                    out.toByteArray()
                }
                if (bytes.isEmpty()) return null
                decodeSampled(bytes)
            } finally {
                connection.disconnect()
            }
        } catch (e: Exception) {
            null
        }
    }

    /** Decode downsampled to roughly [TARGET_PX]: rows need thumbs, not posters. */
    private fun decodeSampled(bytes: ByteArray): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= TARGET_PX &&
            bounds.outHeight / (sample * 2) >= TARGET_PX
        ) {
            sample *= 2
        }
        val opts = BitmapFactory.Options().apply { inSampleSize = sample }
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)
    }
}
