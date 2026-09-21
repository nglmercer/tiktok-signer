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
}
