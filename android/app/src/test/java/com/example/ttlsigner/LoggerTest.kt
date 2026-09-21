package com.example.ttlsigner

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Log lines must be timestamped, and summaries must never carry the signature. */
class LoggerTest {

    private val gnarly = "SECRET_GNARLY_VALUE_0123456789abcdef"
    private val signed =
        "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/" +
            "?room_id=7300000000000000001&aid=1988&device_id=1234567890123456789" +
            "&X-Gnarly=$gnarly"

    @Test
    fun lineIsTimestampedAndTagged() {
        val line = Logger.line("SIGN", "hello")
        assertTrue(line, Regex("""\[\d{2}:\d{2}:\d{2}\.\d{3}] SIGN hello""").matches(line))
    }

    @Test
    fun summaryKeepsHostRoomAndLengths() {
        val summary = Logger.summarizeSignedUrl(signed)
        assertTrue(summary, summary.contains("webcast-ws.tiktok.com"))
        assertTrue(summary, summary.contains("room=7300000000000000001"))
        assertTrue(summary, summary.contains("${signed.length} chars"))
        assertTrue(summary, summary.contains("X-Gnarly ${gnarly.length} chars"))
    }

    @Test
    fun summaryNeverContainsTheSignatureValue() {
        val summary = Logger.summarizeSignedUrl(signed)
        assertFalse(summary, summary.contains(gnarly))
        assertFalse(summary, summary.contains("device_id=1234567890123456789"))
    }

    @Test
    fun unsignedInputIsReportedNotParsed() {
        assertEquals("NOT SIGNED (11 chars)", Logger.summarizeSignedUrl("wss://x.inv"))
    }

    @org.junit.After
    fun resetBuffer() {
        Logger.clear()
    }

    @Test
    fun bufferKeepsAppendedLinesInOrder() {
        Logger.clear()
        Logger.append("SIGN", "first")
        Logger.append("LIVE", "second")
        val lines = Logger.snapshot().split("\n")
        assertEquals(2, lines.size)
        assertTrue(lines[0], lines[0].endsWith("SIGN first"))
        assertTrue(lines[1], lines[1].endsWith("LIVE second"))
    }

    @Test
    fun bufferCapsAtMaxLines() {
        Logger.clear()
        for (i in 0 until Logger.MAX_BUFFER_LINES + 10) {
            Logger.append("T", "line-$i")
        }
        val lines = Logger.snapshot().split("\n")
        assertEquals(Logger.MAX_BUFFER_LINES, lines.size)
        assertTrue(lines.first(), lines.first().endsWith("line-10"))
        assertTrue(lines.last(), lines.last().endsWith("line-509"))
    }

    @Test
    fun listenerSeesEveryLineAndTheClear() {
        Logger.clear()
        val seen = mutableListOf<String>()
        var clears = 0
        val listener = object : Logger.Listener {
            override fun onLine(line: String) {
                seen.add(line)
            }

            override fun onCleared() {
                clears++
            }
        }
        Logger.addListener(listener)
        try {
            Logger.append("A", "one")
            Logger.append("B", "two")
            assertEquals(2, seen.size)
            assertTrue(seen[0], seen[0].endsWith("A one"))
            Logger.clear()
            assertEquals(1, clears)
            assertEquals("", Logger.snapshot())
        } finally {
            Logger.removeListener(listener)
        }
    }

    @Test
    fun removedListenerSeesNothing() {
        Logger.clear()
        var seen = 0
        val listener = object : Logger.Listener {
            override fun onLine(line: String) {
                seen++
            }

            override fun onCleared() {
            }
        }
        Logger.addListener(listener)
        Logger.removeListener(listener)
        Logger.append("A", "one")
        assertEquals(0, seen)
    }
}
