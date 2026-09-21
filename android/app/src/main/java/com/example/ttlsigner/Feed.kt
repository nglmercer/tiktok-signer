package com.example.ttlsigner

import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * Live feed: rooms broadcasting right now, for rendering and selection.
 *
 * Ports `ttl_live_discovery::live_search_url` + `interpret_live_search`: the same
 * unsigned `/api/search/live/full/` endpoint, the same item shape
 * (`live_info.raw_data` as a nested JSON string, `status == 2` only), most watched
 * first. Unsigned does not mean anonymous: without guest cookies the endpoint answers
 * `2483 "Please login your account first"`, so [GuestSession] harvests the `ttwid`
 * cookie with a plain GET of `/live` first — the same bootstrap as
 * `scripts/headless/find-live.mjs`.
 */
object Feed {

    data class LiveRoom(
        val uniqueId: String,
        val roomId: String,
        val nickname: String,
        val title: String,
        val viewers: Long,
    )

    private const val SEARCH_HOST = "https://www.tiktok.com/api/search/live/full/?"
    private const val LIVE_PAGE = "https://www.tiktok.com/live"
    private const val COUNT = 20
    private const val MAX_BODY = 2 * 1024 * 1024

    /** The search URL for [keyword] at [offset]. Unsigned; carries guest cookies instead. */
    fun liveSearchUrl(keyword: String, offset: Int = 0): String {
        val pairs = listOf(
            "aid" to "1988",
            "app_language" to "en",
            "app_name" to "tiktok_web",
            "browser_language" to "en-US",
            "browser_name" to "Mozilla",
            "browser_platform" to "Linux x86_64",
            "browser_version" to "5.0 (X11)",
            "cookie_enabled" to "true",
            "count" to COUNT.toString(),
            "device_platform" to "web_pc",
            "focus_state" to "true",
            "from_page" to "search",
            "history_len" to "4",
            "is_fullscreen" to "false",
            "is_page_visible" to "true",
            "keyword" to keyword,
            "offset" to offset.toString(),
            "os" to "linux",
            "priority_region" to "",
            "referer" to "",
            "region" to "US",
            "screen_height" to "1080",
            "screen_width" to "1920",
            "search_id" to "",
        )
        return SEARCH_HOST + pairs.joinToString("&") { (k, v) -> "$k=${percentEncode(v)}" } +
            "&tz_name=America%2FNew_York&webcast_language=en"
    }

    /** Percent-encode a query value, leaving only the unreserved set intact. */
    internal fun percentEncode(value: String): String {
        val out = StringBuilder()
        for (byte in value.toByteArray(Charsets.UTF_8)) {
            val c = byte.toInt() and 0xFF
            if (c in 0x41..0x5A || c in 0x61..0x7A || c in 0x30..0x39 ||
                c == 0x2D || c == 0x5F || c == 0x2E || c == 0x7E
            ) {
                out.append(c.toChar())
            } else {
                out.append("%").append(c.toString(16).uppercase().padStart(2, '0'))
            }
        }
        return out.toString()
    }

    /**
     * Guest cookie jar. One plain page GET issues the identity cookies; feed calls
     * replay them. Not bound to any user session — this is what a browser does.
     */
    class GuestSession {
        private val jar = LinkedHashMap<String, String>()

        fun cookieHeader(): String = jar.entries.joinToString("; ") { "${it.key}=${it.value}" }

        fun isEmpty(): Boolean = jar.isEmpty()

        /** Absorb `Set-Cookie` pairs from a page GET. Currencies, not secrets. */
        suspend fun bootstrap(userAgent: String) = withContext(Dispatchers.IO) {
            val connection = URL(LIVE_PAGE).openConnection() as HttpURLConnection
            try {
                connection.connectTimeout = 10_000
                connection.readTimeout = 15_000
                connection.setRequestProperty("User-Agent", userAgent)
                connection.setRequestProperty("Accept-Language", "en-US,en;q=0.9")
                // Drain (capped): the HTML itself is useless, the cookies are the point.
                connection.inputStream.buffered().use { stream ->
                    val buf = ByteArray(8192)
                    var left = MAX_BODY
                    while (left > 0) {
                        val n = stream.read(buf, 0, minOf(buf.size, left))
                        if (n <= 0) break
                        left -= n
                    }
                }
                absorb(connection)
            } finally {
                connection.disconnect()
            }
        }

        internal fun absorb(connection: HttpURLConnection) {
            for ((name, values) in connection.headerFields) {
                if (!name.equals("Set-Cookie", ignoreCase = true)) continue
                for (header in values ?: continue) {
                    val pair = header.substringBefore(';')
                    val at = pair.indexOf('=')
                    if (at > 0) jar[pair.substring(0, at).trim()] = pair.substring(at + 1)
                }
            }
        }
    }

    /** Parse a feed body into live rooms, most watched first. */
    fun parseFeed(body: String): List<LiveRoom> {
        val root = try {
            JSONObject(body)
        } catch (e: Exception) {
            throw IllegalStateException("feed is not JSON")
        }
        if (root.optInt("status_code", 0) == 2483) {
            throw GuestRequired()
        }
        val items = root.optJSONArray("data") ?: throw IllegalStateException("feed has no data array")
        val rooms = mutableListOf<LiveRoom>()
        for (i in 0 until items.length()) {
            val raw = items.optJSONObject(i)
                ?.optJSONObject("live_info")
                ?.optString("raw_data", "")
                .orEmpty()
            if (raw.isEmpty()) continue
            val room = try {
                JSONObject(raw)
            } catch (e: Exception) {
                continue
            }
            // status 2 is the only value that means broadcasting; anything else is
            // skipped rather than assumed live.
            if (room.optLong("status", -1) != 2L) continue
            val owner = room.optJSONObject("owner")
            val uniqueId = owner?.optString("display_id", "").orEmpty()
            val roomId = room.optString("id_str", "")
            if (uniqueId.isEmpty() || roomId.isEmpty() || roomId == "0") continue
            rooms.add(
                LiveRoom(
                    uniqueId = uniqueId,
                    roomId = roomId,
                    nickname = owner?.optString("nickname", "").orEmpty(),
                    title = room.optString("title", ""),
                    viewers = room.optLong("user_count", 0L),
                )
            )
        }
        return rooms.sortedByDescending { it.viewers }
    }

    /** The endpoint refused anonymously: bootstrap guest cookies and retry once. */
    class GuestRequired : IllegalStateException("feed needs guest cookies (status 2483)")

    /**
     * Rooms live now for [keyword], most watched first. Bootstraps the guest session
     * on first use and once more if the endpoint answers 2483 mid-run.
     */
    suspend fun fetchFeed(
        keyword: String,
        userAgent: String,
        session: GuestSession,
    ): List<LiveRoom> = withContext(Dispatchers.IO) {
        if (session.isEmpty()) session.bootstrap(userAgent)
        try {
            return@withContext readFeed(keyword, userAgent, session)
        } catch (e: GuestRequired) {
            session.bootstrap(userAgent)
            return@withContext readFeed(keyword, userAgent, session)
        }
    }

    private fun readFeed(keyword: String, userAgent: String, session: GuestSession): List<LiveRoom> {
        val connection = URL(liveSearchUrl(keyword)).openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 10_000
            connection.readTimeout = 15_000
            connection.setRequestProperty("User-Agent", userAgent)
            connection.setRequestProperty("referer", "https://www.tiktok.com/")
            connection.setRequestProperty("origin", "https://www.tiktok.com")
            val cookies = session.cookieHeader()
            if (cookies.isNotEmpty()) connection.setRequestProperty("cookie", cookies)
            if (connection.responseCode !in 200..300) {
                throw IllegalStateException("feed answered HTTP ${connection.responseCode}")
            }
            val body = connection.inputStream.bufferedReader().use { it.readText() }
            if (body.length > MAX_BODY) throw IllegalStateException("feed response too large")
            if (body.isEmpty()) throw IllegalStateException("feed answered with nothing")
            return parseFeed(body)
        } finally {
            connection.disconnect()
        }
    }
}
