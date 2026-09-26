package com.example.ttlsigner.actions

import com.example.ttlsigner.events.LiveEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Template rendering and trigger matching. Pure; no Android, no network. */
class EventActionTest {

    private val chat = LiveEvent(LiveEvent.Category.CHAT, "ay", "Ay", "hello!", 0, 0)
    private val gift = LiveEvent(LiveEvent.Category.GIFT, "bee", "", "Rose", 3, 5)

    @Test
    fun templateRendersEveryPlaceholder() {
        assertEquals(
            "@ay/Ay/hello!/chat/0/0",
            Template.render("{{user}}/{{name}}/{{text}}/{{type}}/{{count}}/{{diamonds}}", chat),
        )
        assertEquals("@bee/Rose/gift/3/5", Template.render("{{user}}/{{text}}/{{type}}/{{count}}/{{diamonds}}", gift))
    }

    @Test
    fun nameFallsBackToHandleThenDash() {
        assertEquals("Ay", Template.render("{{name}}", chat))
        assertEquals("bee", Template.render("{{name}}", gift))
        val anon = LiveEvent(LiveEvent.Category.ROOM, "", "", "", 7, 0)
        assertEquals("-", Template.render("{{name}}", anon))
    }

    @Test
    fun unknownPlaceholdersStayUntouched() {
        assertEquals("{{oops}}", Template.render("{{oops}}", chat))
        assertEquals("a {{user} b", Template.render("a {{user} b", chat))
    }

    @Test
    fun matcherRequiresEnabledTriggerAndUrl() {
        val action = EventAction(triggers = setOf(LiveEvent.Category.CHAT), url = "https://x.test/h")
        assertTrue(ActionMatcher.matches(action, chat, 1000, null))
        assertFalse(ActionMatcher.matches(action, gift, 1000, null))
        assertFalse(ActionMatcher.matches(action.copy(enabled = false), chat, 1000, null))
        assertFalse(ActionMatcher.matches(action.copy(url = "  "), chat, 1000, null))
    }

    @Test
    fun matcherHonorsCooldown() {
        val action = EventAction(
            triggers = setOf(LiveEvent.Category.CHAT),
            url = "https://x.test/h",
            cooldownSecs = 10,
        )
        assertFalse(ActionMatcher.matches(action, chat, nowMs = 5_000, lastFiredMs = 0))
        assertTrue(ActionMatcher.matches(action, chat, nowMs = 10_000, lastFiredMs = 0))
        assertTrue(ActionMatcher.matches(action, chat, nowMs = 5_000, lastFiredMs = null))
    }

    @Test
    fun rowRoundTripPreservesTriggersAndMethod() {
        val action = EventAction(
            id = 7,
            name = "hi",
            triggers = setOf(LiveEvent.Category.CHAT, LiveEvent.Category.GIFT),
            method = EventAction.Method.POST,
            url = "https://x.test/h",
            body = "{}",
            cooldownSecs = 5,
        )
        val back = EventAction.fromRow(action.toRow())
        assertEquals(action, back)
    }

    @Test
    fun fromRowDegradesBadTriggersToChat() {
        val row = EventAction().copy(triggers = setOf(LiveEvent.Category.CHAT)).toRow()
            .copy(triggers = "NOPE,ALSOBAD", method = "PUT")
        val back = EventAction.fromRow(row)
        assertEquals(setOf(LiveEvent.Category.CHAT), back.triggers)
        assertEquals(EventAction.Method.GET, back.method)
    }
}
