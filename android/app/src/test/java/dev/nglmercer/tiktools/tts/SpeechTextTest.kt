package dev.nglmercer.tiktools.tts

import dev.nglmercer.tiktools.core.model.LiveEvent
import org.junit.Assert.assertEquals
import org.junit.Test

/** Event → spoken text mapping. Pure; no Android TTS needed. */
class SpeechTextTest {

    @Test
    fun chatReadsWhoAndComment() {
        val event = LiveEvent(LiveEvent.Category.CHAT, "ay", "Ay", "hello!", 0, 0)
        assertEquals("Ay says: hello!", SpeechText.forEvent(event))
        val noNick = event.copy(nickname = "")
        assertEquals("ay says: hello!", SpeechText.forEvent(noNick))
    }

    @Test
    fun giftFollowAndShareAnnounce() {
        assertEquals(
            "Ay sent Rose",
            SpeechText.forEvent(LiveEvent(LiveEvent.Category.GIFT, "ay", "Ay", "Rose", 1, 0)),
        )
        assertEquals(
            "ay followed",
            SpeechText.forEvent(LiveEvent(LiveEvent.Category.FOLLOW, "ay", "", "", 0, 0)),
        )
        assertEquals(
            "ay shared the live",
            SpeechText.forEvent(LiveEvent(LiveEvent.Category.SHARE, "ay", "", "", 0, 0)),
        )
    }

    @Test
    fun noisyCategoriesStaySilent() {
        for (category in
            listOf(
                LiveEvent.Category.LIKE,
                LiveEvent.Category.MEMBER,
                LiveEvent.Category.ROOM,
                LiveEvent.Category.UNKNOWN,
            )) {
            assertEquals("", SpeechText.forEvent(LiveEvent(category, "ay", "Ay", "x", 1, 0)))
        }
    }

    @Test
    fun joinsSpeakOnlyWhenAsked() {
        val join = LiveEvent(LiveEvent.Category.JOIN, "ay", "Ay", "", 0, 0)
        assertEquals("", SpeechText.forEvent(join))
        assertEquals("Ay joined", SpeechText.forEvent(join, speakJoins = true))
    }

    @Test
    fun blankChatStaysSilent() {
        assertEquals(
            "",
            SpeechText.forEvent(LiveEvent(LiveEvent.Category.CHAT, "ay", "Ay", "  ", 0, 0)),
        )
    }
}
