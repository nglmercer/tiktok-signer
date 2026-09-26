package com.example.ttlsigner.events

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Reader filter matching. Pure; no Android needed. */
class EventFilterTest {

    private fun chat(user: String = "ay", text: String = "hello") =
        LiveEvent(LiveEvent.Category.CHAT, user, "", text, 0, 0)

    @Test
    fun defaultFilterShowsEverything() {
        val filter = EventFilter()
        for (category in LiveEvent.Category.values()) {
            assertTrue(filter.matches(LiveEvent(category, "u", "", "t", 0, 0)))
        }
    }

    @Test
    fun disabledCategoryIsHidden() {
        val filter = EventFilter().toggle(LiveEvent.Category.CHAT)
        assertFalse(filter.matches(chat()))
        assertTrue(filter.matches(LiveEvent(LiveEvent.Category.GIFT, "u", "", "Rose", 1, 0)))
    }

    @Test
    fun lastVisibleCategoryCannotBeDisabled() {
        var filter = EventFilter()
        for (category in LiveEvent.Category.values()) {
            filter = filter.toggle(category)
        }
        assertEquals(1, filter.enabled.size)
    }

    @Test
    fun queryMatchesUserNicknameAndTextCaseInsensitively() {
        val byUser = EventFilter(query = "AY")
        assertTrue(byUser.matches(chat(user = "ay")))
        assertFalse(byUser.matches(chat(user = "bee")))

        val byText = EventFilter(query = "hell")
        assertTrue(byText.matches(chat(text = "hello there")))
        assertFalse(byText.matches(chat(text = "bye")))

        val byNick = EventFilter(query = "aya")
        assertTrue(byNick.matches(LiveEvent(LiveEvent.Category.CHAT, "x", "Aya", "hi", 0, 0)))
    }

    @Test
    fun queryAndCategoryCombine() {
        val filter = EventFilter(
            enabled = setOf(LiveEvent.Category.CHAT),
            query = "ay",
        )
        assertTrue(filter.matches(chat()))
        assertFalse(filter.matches(LiveEvent(LiveEvent.Category.GIFT, "ay", "", "Rose", 1, 0)))
        assertFalse(filter.matches(chat(user = "bee")))
    }

    @Test
    fun showAllResets() {
        val filter = EventFilter(setOf(LiveEvent.Category.CHAT), "ay").showAll()
        assertEquals(LiveEvent.Category.values().toSet(), filter.enabled)
        assertEquals("", filter.query)
    }
}
