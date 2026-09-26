package com.example.ttlsigner.ui

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The minimalist label rules every tab shares: counts collapse to the bare
 * label while empty, and render as a quiet "· N" suffix otherwise.
 */
class UiKitTest {

    @Test
    fun sectionTitleShowsBareLabelWhileEmpty() {
        assertEquals("Events", UiKit.sectionTitle("Events", null))
        assertEquals("Events", UiKit.sectionTitle("Events", 0))
        assertEquals("Events", UiKit.sectionTitle("Events", -1))
    }

    @Test
    fun sectionTitleAppendsCountSuffix() {
        assertEquals("Events · 1", UiKit.sectionTitle("Events", 1))
        assertEquals("Leaderboard · 12", UiKit.sectionTitle("Leaderboard", 12))
    }

    @Test
    fun countsLineFallsBackToEmptyText() {
        assertEquals("nothing yet", UiKit.countsLine(emptyList(), "nothing yet"))
    }

    @Test
    fun countsLineJoinsPairsInGivenOrder() {
        val pairs = listOf("chat" to 3, "gift" to 1, "like" to 27)
        assertEquals("chat 3 · gift 1 · like 27", UiKit.countsLine(pairs, "nothing yet"))
    }
}
