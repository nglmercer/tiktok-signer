package com.example.ttlsigner.tts

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** [TtsController] repeat / skip / visibility rules. Pure; no Android TTS needed. */
class TtsControllerTest {

    private class FakeSpeaker : Speaker {
        override val enabled: Boolean = true
        val spoken = mutableListOf<String>()
        var stops = 0
        override fun speak(text: String) {
            spoken.add(text)
        }
        override fun stop() {
            stops++
        }
        override fun shutdown() = Unit
    }

    @After
    fun tearDown() {
        TtsController.resetForTest()
    }

    @Test
    fun speakRemembersLastLine() {
        val fake = FakeSpeaker()
        TtsController.attach(fake, TtsEngine.DEVICE)
        TtsController.speak("hello")
        assertEquals("hello", TtsController.lastSpoken.value)
        assertEquals(listOf("hello"), fake.spoken)
    }

    @Test
    fun blankLinesAreNeitherSpokenNorRemembered() {
        val fake = FakeSpeaker()
        TtsController.attach(fake, TtsEngine.DEVICE)
        TtsController.speak("  ")
        assertNull(TtsController.lastSpoken.value)
        assertTrue(fake.spoken.isEmpty())
    }

    @Test
    fun offEngineStaysSilent() {
        val fake = FakeSpeaker()
        TtsController.attach(fake, TtsEngine.OFF)
        TtsController.speak("hello")
        assertFalse(TtsController.enabled)
        assertNull(TtsController.lastSpoken.value)
        assertTrue(fake.spoken.isEmpty())
    }

    @Test
    fun repeatReplaysLastLineAfterStopping() {
        val fake = FakeSpeaker()
        TtsController.attach(fake, TtsEngine.DEVICE)
        TtsController.speak("first")
        TtsController.speak("second")
        assertTrue(TtsController.repeatLast())
        assertEquals(listOf("first", "second", "second"), fake.spoken)
        assertEquals(1, fake.stops)
    }

    @Test
    fun repeatWithNothingSpokenReturnsFalse() {
        TtsController.attach(FakeSpeaker(), TtsEngine.DEVICE)
        assertFalse(TtsController.repeatLast())
    }

    @Test
    fun repeatWhileOffReturnsFalse() {
        val fake = FakeSpeaker()
        TtsController.attach(fake, TtsEngine.DEVICE)
        TtsController.speak("hello")
        TtsController.attach(fake, TtsEngine.OFF)
        assertFalse(TtsController.repeatLast())
    }

    @Test
    fun engineSwitchForgetsLastLine() {
        val fake = FakeSpeaker()
        TtsController.attach(fake, TtsEngine.DEVICE)
        TtsController.speak("hello")
        TtsController.attach(fake, TtsEngine.SUPERTONIC)
        assertNull(TtsController.lastSpoken.value)
        assertFalse(TtsController.repeatLast())
    }

    @Test
    fun skipStopsSpeaker() {
        val fake = FakeSpeaker()
        TtsController.attach(fake, TtsEngine.DEVICE)
        TtsController.skip()
        assertEquals(1, fake.stops)
    }

    @Test
    fun controlsShowOnlyWhenEngineEnabled() {
        assertFalse(TtsController.controlsVisible(TtsEngine.OFF))
        assertTrue(TtsController.controlsVisible(TtsEngine.DEVICE))
        assertTrue(TtsController.controlsVisible(TtsEngine.SUPERTONIC))
    }
}
