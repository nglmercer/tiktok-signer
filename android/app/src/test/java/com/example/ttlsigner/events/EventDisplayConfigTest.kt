package com.example.ttlsigner.events

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Row-style defaults: the reader is minimalist until the user opts out. */
class EventDisplayConfigTest {

    @Test
    fun defaultIsMinimalist() {
        val config = EventDisplayConfig()
        assertTrue(config.compact)
        assertTrue(config.showBadge)
        assertTrue(config.showTime)
        assertTrue(config.singleLine)
    }

    @Test
    fun optionsToggleIndependently() {
        val config = EventDisplayConfig().copy(compact = false, showTime = false)
        assertFalse(config.compact)
        assertTrue(config.showBadge)
        assertFalse(config.showTime)
        assertTrue(config.singleLine)
    }
}
