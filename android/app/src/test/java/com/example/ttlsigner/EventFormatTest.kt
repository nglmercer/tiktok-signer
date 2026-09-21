package com.example.ttlsigner

import org.junit.Assert.assertEquals
import org.junit.Test

/** Event rendering over synthetic JSON. No network, no native library. */
class EventFormatTest {

    @Test
    fun chatShowsUserAndComment() {
        assertEquals(
            "@ay: hello there",
            EventFormat.line("""{"type":"chat","comment":"hello there","user":{"unique_id":"ay"}}"""),
        )
    }

    @Test
    fun giftShowsNameAndCount() {
        assertEquals(
            "@ay sent Rose x5",
            EventFormat.line("""{"type":"gift","gift_name":"Rose","repeat_count":5,"user":{"unique_id":"ay"}}"""),
        )
    }

    @Test
    fun likeShowsCount() {
        assertEquals(
            "@ay liked x15",
            EventFormat.line("""{"type":"like","count":15,"user":{"unique_id":"ay"}}"""),
        )
    }

    @Test
    fun memberJoinAndUnknownAction() {
        assertEquals(
            "@ay joined",
            EventFormat.line("""{"type":"member","action":1,"user":{"unique_id":"ay"}}"""),
        )
        assertEquals(
            "@ay member event (action=9)",
            EventFormat.line("""{"type":"member","action":9,"user":{"unique_id":"ay"}}"""),
        )
    }

    @Test
    fun socialFollowAndShare() {
        assertEquals(
            "@ay followed",
            EventFormat.line("""{"type":"social","follow_count":1,"user":{"unique_id":"ay"}}"""),
        )
        assertEquals(
            "@ay shared",
            EventFormat.line("""{"type":"social","share_count":2,"user":{"unique_id":"ay"}}"""),
        )
    }

    @Test
    fun roomUserShowsTotalsWithoutAUser() {
        assertEquals(
            "viewers: 187 (popularity 42)",
            EventFormat.line("""{"type":"room_user","total":187,"popularity":42}"""),
        )
    }

    @Test
    fun unknownAndGarbageDegradeGracefully() {
        assertEquals(
            "unknown: WebcastNewThing (10 bytes)",
            EventFormat.line("""{"type":"unknown","method":"WebcastNewThing","bytes":10}"""),
        )
        assertEquals("(unreadable event)", EventFormat.line("not json"))
        assertEquals("(unreadable event)", EventFormat.line(""))
    }
}
