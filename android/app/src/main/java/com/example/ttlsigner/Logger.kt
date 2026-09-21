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

    /** The console keeps the newest lines; a busy room cannot grow memory without bound. */
    const val MAX_BUFFER_LINES = 500

    interface Listener {
        fun onLine(line: String)
        fun onCleared()
    }

    private val clock = SimpleDateFormat("HH:mm:ss.SSS", Locale.US)
    private val lock = Any()
    private val buffer = ArrayDeque<String>()
    private val listeners = mutableSetOf<Listener>()

    fun stamp(): String = clock.format(Date())

    /** `[12:00:01.234] TAG message`. */
    fun line(tag: String, msg: String): String = "[${stamp()}] $tag $msg"

    /**
     * Build a line, keep it in the shared buffer, and notify listeners on the
     * caller's thread — views must post to themselves. Returns the line.
     */
    fun append(tag: String, msg: String): String = synchronized(lock) {
        val line = line(tag, msg)
        buffer.addLast(line)
        while (buffer.size > MAX_BUFFER_LINES) buffer.removeFirst()
        listeners.forEach { it.onLine(line) }
        line
    }

    /** The whole buffer, oldest first. */
    fun snapshot(): String = synchronized(lock) { buffer.joinToString("\n") }

    fun clear() = synchronized(lock) {
        buffer.clear()
        listeners.forEach { it.onCleared() }
    }

    fun addListener(listener: Listener) {
        synchronized(lock) { listeners.add(listener) }
    }

    fun removeListener(listener: Listener) {
        synchronized(lock) { listeners.remove(listener) }
    }

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
