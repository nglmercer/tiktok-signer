package com.example.ttlsigner

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Timestamped, secret-safe log lines for the on-screen console and logcat.
 *
 * The on-screen console may show a full signed URL (that is the demo's point, on the
 * user's own device), but logcat only ever gets [summarizeSignedUrl]: a signed URL is
 * a replayable capability and must not leak into persistent logs.
 */
object Logger {
    const val TAG = "TtlSigner"

    private val clock = SimpleDateFormat("HH:mm:ss.SSS", Locale.US)

    fun stamp(): String = clock.format(Date())

    /** `[12:00:01.234] TAG message`. */
    fun line(tag: String, msg: String): String = "[${stamp()}] $tag $msg"

    /**
     * Summarize a signed URL without its signature value: host, room, and byte
     * lengths only. Never contains the `X-Gnarly` value or the full query.
     */
    fun summarizeSignedUrl(signed: String): String {
        val gnarly = signed.substringAfter("&X-Gnarly=", "")
        if (gnarly.isEmpty()) return "NOT SIGNED (${signed.length} chars)"
        val room = signed.split(Regex("[?&]room_id=")).getOrNull(1)
            ?.substringBefore("&")?.ifEmpty { null } ?: "?"
        val host = signed.substringAfter("://").substringBefore("/")
        return "$host · room=$room · ${signed.length} chars · X-Gnarly ${gnarly.length} chars"
    }
}
