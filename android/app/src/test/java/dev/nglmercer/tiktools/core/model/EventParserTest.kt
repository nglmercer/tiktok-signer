package dev.nglmercer.tiktools.core.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Typed event parsing over synthetic JSON. No network, no native library. */
class EventParserTest {

    @Test
    fun chatKeepsUserAndComment() {
        val event =
            EventParser.parse(
                """{"type":"chat","comment":"hello","user":{"unique_id":"ay","nickname":"Ay"}}"""
            )
        assertEquals(LiveEvent.Category.CHAT, event.category)
        assertEquals("ay", event.user)
        assertEquals("Ay", event.nickname)
        assertEquals("hello", event.text)
    }

    @Test
    fun giftKeepsNameAndCounts() {
        val event =
            EventParser.parse(
                """{"type":"gift","gift_name":"Rose","repeat_count":5,"diamond_count":2,"user":{"unique_id":"ay"}}"""
            )
        assertEquals(LiveEvent.Category.GIFT, event.category)
        assertEquals("Rose", event.text)
        assertEquals(5, event.count)
        assertEquals(2, event.diamonds)
    }

    @Test
    fun memberActionOneIsJoinOthersStayMember() {
        val join = EventParser.parse("""{"type":"member","action":1,"user":{"unique_id":"ay"}}""")
        assertEquals(LiveEvent.Category.JOIN, join.category)
        val other = EventParser.parse("""{"type":"member","action":9,"user":{"unique_id":"ay"}}""")
        assertEquals(LiveEvent.Category.MEMBER, other.category)
        assertEquals(9, other.count)
    }

    @Test
    fun socialSplitsIntoFollowAndShare() {
        val follow =
            EventParser.parse("""{"type":"social","follow_count":1,"user":{"unique_id":"ay"}}""")
        assertEquals(LiveEvent.Category.FOLLOW, follow.category)
        val share =
            EventParser.parse("""{"type":"social","share_count":2,"user":{"unique_id":"ay"}}""")
        assertEquals(LiveEvent.Category.SHARE, share.category)
        val bare = EventParser.parse("""{"type":"social","user":{"unique_id":"ay"}}""")
        assertEquals(LiveEvent.Category.MEMBER, bare.category)
    }

    @Test
    fun roomUserCarriesViewerTotalWithoutAUser() {
        val event = EventParser.parse("""{"type":"room_user","total":187,"popularity":42}""")
        assertEquals(LiveEvent.Category.ROOM, event.category)
        assertEquals(187, event.count)
        assertEquals("", event.user)
    }

    @Test
    fun unknownAndGarbageDegradeToUnknown() {
        val unknown = EventParser.parse("""{"type":"mystery","method":"WebcastNew","bytes":10}""")
        assertEquals(LiveEvent.Category.UNKNOWN, unknown.category)
        assertEquals("WebcastNew", unknown.text)
        val garbage = EventParser.parse("not json")
        assertEquals(LiveEvent.Category.UNKNOWN, garbage.category)
        val empty = EventParser.parse("")
        assertEquals(LiveEvent.Category.UNKNOWN, empty.category)
    }

    @Test
    fun chatKeepsAvatarUrl() {
        val event =
            EventParser.parse(
                """{"type":"chat","comment":"hi","user":{"unique_id":"ay","nickname":"Ay","avatar_url":"https://cdn/a.webp"}}"""
            )
        assertEquals("https://cdn/a.webp", event.avatarUrl)
        assertEquals("", event.giftImageUrl)
    }

    @Test
    fun giftKeepsArtworkUrl() {
        val event =
            EventParser.parse(
                """{"type":"gift","gift_name":"Rose","gift_image_url":"https://cdn/rose.webp","user":{"unique_id":"ay"}}"""
            )
        assertEquals(LiveEvent.Category.GIFT, event.category)
        assertEquals("https://cdn/rose.webp", event.giftImageUrl)
    }

    @Test
    fun nullImageUrlsParseAsEmpty() {
        // serde serializes a missing URL as null; older builds omit the keys.
        val nulled =
            EventParser.parse(
                """{"type":"chat","comment":"hi","user":{"unique_id":"ay","avatar_url":null},"gift_image_url":null}"""
            )
        assertEquals("", nulled.avatarUrl)
        assertEquals("", nulled.giftImageUrl)
        val omitted =
            EventParser.parse("""{"type":"chat","comment":"hi","user":{"unique_id":"ay"}}""")
        assertEquals("", omitted.avatarUrl)
        assertEquals("", omitted.giftImageUrl)
    }

    @Test
    fun rawPayloadIsKeptForActionsAndDebugging() {
        val json = """{"type":"like","count":3,"user":{"unique_id":"ay"}}"""
        val event = EventParser.parse(json)
        assertEquals(LiveEvent.Category.LIKE, event.category)
        assertEquals(json, event.raw)
        assertTrue(event.at > 0)
    }
}
