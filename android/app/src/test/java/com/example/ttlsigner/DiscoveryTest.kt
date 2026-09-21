package com.example.ttlsigner

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Pure-JVM tests: no Android framework, no network, no native library. */
class DiscoveryTest {

    @Test
    fun lookupUrlMatchesTheCoreFormat() {
        assertEquals(
            "https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId=fixture",
            Discovery.roomLookupUrl("@fixture"),
        )
    }

    @Test
    fun lookupUrlRejectsGarbage() {
        assertFails { Discovery.roomLookupUrl("") }
        assertFails { Discovery.roomLookupUrl("@") }
        assertFails { Discovery.roomLookupUrl("a b") }
        assertFails { Discovery.roomLookupUrl("a?b=1") }
    }

    @Test
    fun aLiveRoomIsResolved() {
        val body = """{"data":{"user":{"roomId":"7300000000000000001","nickname":"Fixture",
            "uniqueId":"fixture","status":2,"avatarThumb":""}}}"""
        val outcome = Discovery.parseRoomLookup("fixture", body) as Discovery.LookupResult.Found
        assertEquals("7300000000000000001", outcome.room.roomId)
        assertTrue(outcome.room.isLive())
    }

    @Test
    fun anOfflineCreatorIsNoRoomNotAUser() {
        val body = """{"data":{"user":{"roomId":"0","nickname":"Fixture",
            "uniqueId":"fixture","status":4,"avatarThumb":""}}}"""
        val outcome = Discovery.parseRoomLookup("fixture", body)
        assertTrue(outcome is Discovery.LookupResult.NoRoom)
    }

    @Test
    fun aMissingUserIsNotFoundNotADecodeFailure() {
        assertTrue(Discovery.parseRoomLookup("fixture", """{"data":{}}""")
            is Discovery.LookupResult.UserNotFound)
        assertTrue(Discovery.parseRoomLookup("@fixture", """{"data":{"user":null}}""")
            is Discovery.LookupResult.UserNotFound)
    }

    @Test
    fun malformedJsonIsADecodeError() {
        assertTrue(Discovery.parseRoomLookup("fixture", "not json")
            is Discovery.LookupResult.DecodeError)
        assertTrue(Discovery.parseRoomLookup("fixture", "")
            is Discovery.LookupResult.DecodeError)
    }

    @Test
    fun sha256MatchesTheKnownVector() {
        assertEquals(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            BundleFetch.sha256Hex("abc"),
        )
    }

    private fun assertFails(block: () -> Unit) {
        try {
            block()
            throw AssertionError("expected IllegalArgumentException")
        } catch (e: IllegalArgumentException) {
            // Expected.
        }
    }
}
