package dev.nglmercer.tiktools.core.network

import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * Unsigned `unique_id` → `room_id` lookup. Ports `ttl_sign_core::room::room_lookup_url` and
 * `RoomLookup::from_value`: same endpoint, same fields, same classification — unknown handle,
 * offline creator, and malformed response are three distinct outcomes.
 */
object Discovery {
    private const val LOOKUP_URL =
        "https://www.tiktok.com/api-live/user/room/?aid=1988&sourceType=54&uniqueId="

    fun roomLookupUrl(uniqueId: String): String {
        val handle = uniqueId.trim().trimStart('@')
        require(handle.isNotEmpty()) { "a handle is required" }
        require(handle.all { it.isLetterOrDigit() || it == '.' || it == '_' }) {
            "not a TikTok handle: $uniqueId"
        }
        return LOOKUP_URL + URLEncoder.encode(handle, "UTF-8")
    }

    data class RoomLookup(
        val uniqueId: String,
        val roomId: String,
        val nickname: String,
        val status: Long,
        val title: String,
    ) {
        /** A real room id with a live status. Room `0`/empty means no live room. */
        fun isLive(): Boolean = roomId.isNotEmpty() && roomId != "0" && status == 2L

        fun hasRoom(): Boolean = roomId.isNotEmpty() && roomId != "0"
    }

    sealed interface LookupResult {
        data class Found(val room: RoomLookup) : LookupResult

        /** Valid JSON with no `data.user`: the handle does not resolve. */
        data class UserNotFound(val handle: String) : LookupResult

        /** The endpoint answered, but the user has no live room. Normal outcome. */
        data class NoRoom(val handle: String) : LookupResult

        /** Valid JSON that is not a lookup response: schema change, not an unknown user. */
        data class DecodeError(val detail: String) : LookupResult
    }

    fun parseRoomLookup(handle: String, body: String): LookupResult {
        val clean = handle.trim().trimStart('@')
        val root =
            try {
                JSONObject(body)
            } catch (e: Exception) {
                return LookupResult.DecodeError("response is not JSON")
            }
        val user = root.optJSONObject("data")?.optJSONObject("user")
        if (user == null || user == JSONObject.NULL) {
            return LookupResult.UserNotFound(clean)
        }
        val liveRoom = root.optJSONObject("data")?.optJSONObject("liveRoom")
        // getString throws when the key is missing; optString defaults instead.
        val room =
            RoomLookup(
                uniqueId = user.optString("uniqueId", ""),
                roomId = user.optString("roomId", ""),
                nickname = user.optString("nickname", ""),
                status = user.optLong("status", liveRoom?.optLong("status", 0L) ?: 0L),
                title = liveRoom?.optString("title", "") ?: "",
            )
        if (room.uniqueId.isEmpty()) return LookupResult.DecodeError("lookup has no uniqueId")
        if (!room.hasRoom()) return LookupResult.NoRoom(clean)
        return LookupResult.Found(room)
    }

    /** Resolve [uniqueId] over plain HTTPS. Unsigned, no cookies, no signer. */
    suspend fun fetchRoomLookup(uniqueId: String, userAgent: String): LookupResult =
        withContext(Dispatchers.IO) {
            val connection = URL(roomLookupUrl(uniqueId)).openConnection() as HttpURLConnection
            try {
                connection.connectTimeout = 10_000
                connection.readTimeout = 10_000
                connection.setRequestProperty("User-Agent", userAgent)
                connection.setRequestProperty("Accept", "application/json")
                if (connection.responseCode !in 200..300) {
                    return@withContext LookupResult.DecodeError(
                        "lookup answered HTTP ${connection.responseCode}"
                    )
                }
                val body = connection.inputStream.bufferedReader().use { it.readText() }
                if (body.length > 1024 * 1024) {
                    return@withContext LookupResult.DecodeError("lookup response too large")
                }
                parseRoomLookup(uniqueId, body)
            } finally {
                connection.disconnect()
            }
        }
}
