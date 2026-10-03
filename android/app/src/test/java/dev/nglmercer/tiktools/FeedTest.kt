package dev.nglmercer.tiktools

import dev.nglmercer.tiktools.core.network.Feed
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Feed URL building and parsing, with a synthetic fixture. No network. */
class FeedTest {

    @Test
    fun searchUrlCarriesKeywordOffsetAndCount() {
        val url = Feed.liveSearchUrl("live music", 20)
        assertTrue(url, url.startsWith("https://www.tiktok.com/api/search/live/full/?"))
        assertTrue(url, url.contains("keyword=live%20music"))
        assertTrue(url, url.contains("offset=20"))
        assertTrue(url, url.contains("count=20"))
        assertTrue(url, url.contains("aid=1988"))
    }

    @Test
    fun percentEncodeLeavesTheUnreservedSet() {
        assertEquals("abcXYZ019-_.~", Feed.percentEncode("abcXYZ019-_.~"))
        assertEquals("a%20b%2Fc%3F", Feed.percentEncode("a b/c?"))
    }

    /** Synthetic feed: two live rooms, one replay, one broken, one roomless. */
    private fun fixture(): String {
        fun raw(
            status: Long,
            displayId: String,
            nickname: String,
            roomId: String,
            title: String,
            viewers: Long,
        ) =
            JSONObject()
                .put("status", status)
                .put("id_str", roomId)
                .put("title", title)
                .put("user_count", viewers)
                .put("owner", JSONObject().put("display_id", displayId).put("nickname", nickname))
                .toString()
        val items =
            listOf(
                // Most viewers, but listed second: parsing must sort, not trust the order.
                JSONObject()
                    .put(
                        "live_info",
                        JSONObject().put("raw_data", raw(2, "b", "Bee", "200", "Bee live", 500)),
                    ),
                JSONObject()
                    .put(
                        "live_info",
                        JSONObject().put("raw_data", raw(2, "a", "Ay", "100", "Ay live", 50)),
                    ),
                // status 4 is a replay, not a broadcast: skipped.
                JSONObject()
                    .put(
                        "live_info",
                        JSONObject().put("raw_data", raw(4, "c", "Cee", "300", "Replay", 9000)),
                    ),
                // Not JSON: skipped, not fatal.
                JSONObject().put("live_info", JSONObject().put("raw_data", "nope")),
                // No raw_data at all: skipped.
                JSONObject().put("live_info", JSONObject()),
                // Live status but room 0: skipped.
                JSONObject()
                    .put(
                        "live_info",
                        JSONObject().put("raw_data", raw(2, "d", "Dee", "0", "No room", 10)),
                    ),
            )
        return JSONObject().put("status_code", 0).put("data", org.json.JSONArray(items)).toString()
    }

    @Test
    fun parseKeepsOnlyLiveRoomsWithRoomsMostWatchedFirst() {
        val rooms = Feed.parseFeed(fixture())
        assertEquals(2, rooms.size)
        assertEquals("b", rooms[0].uniqueId)
        assertEquals("200", rooms[0].roomId)
        assertEquals("Bee", rooms[0].nickname)
        assertEquals(500L, rooms[0].viewers)
        assertEquals("a", rooms[1].uniqueId)
        assertEquals("Ay live", rooms[1].title)
    }

    @Test
    fun loginRequiredMapsToGuestRequired() {
        try {
            Feed.parseFeed(
                """{"status_code":2483,"status_msg":"Please login your account first"}"""
            )
            throw AssertionError("expected GuestRequired")
        } catch (e: Feed.GuestRequired) {
            // Expected: the caller bootstraps cookies and retries.
        }
    }

    @Test
    fun parseKeepsCoverAndAvatarUrls() {
        val raw =
            JSONObject()
                .put("status", 2)
                .put("id_str", "100")
                .put("title", "Ay live")
                .put("user_count", 50)
                .put(
                    "cover",
                    JSONObject()
                        .put(
                            "url_list",
                            org.json.JSONArray(
                                listOf("https://cdn/cover.webp", "https://cdn/cover2.webp")
                            ),
                        ),
                )
                .put(
                    "owner",
                    JSONObject()
                        .put("display_id", "a")
                        .put("nickname", "Ay")
                        .put(
                            "avatar_thumb",
                            JSONObject()
                                .put(
                                    "url_list",
                                    org.json.JSONArray(listOf("https://cdn/avatar.webp")),
                                ),
                        ),
                )
                .toString()
        val body =
            JSONObject()
                .put("status_code", 0)
                .put(
                    "data",
                    org.json.JSONArray(
                        listOf(JSONObject().put("live_info", JSONObject().put("raw_data", raw)))
                    ),
                )
                .toString()
        val rooms = Feed.parseFeed(body)
        assertEquals(1, rooms.size)
        assertEquals("https://cdn/cover.webp", rooms[0].coverUrl)
        assertEquals("https://cdn/avatar.webp", rooms[0].avatarUrl)
    }

    @Test
    fun missingImagesParseAsEmpty() {
        val rooms = Feed.parseFeed(fixture())
        assertTrue(rooms.all { it.coverUrl.isEmpty() && it.avatarUrl.isEmpty() })
        assertEquals("", Feed.firstImageUrl(null))
        assertEquals("", Feed.firstImageUrl(JSONObject()))
    }

    @Test
    fun malformedFeedIsAnError() {
        for (bad in listOf("not json", "", """{"status_code":0}""")) {
            try {
                Feed.parseFeed(bad)
                throw AssertionError("expected IllegalStateException for $bad")
            } catch (e: IllegalStateException) {
                assertTrue(e !is Feed.GuestRequired)
            }
        }
    }
}
