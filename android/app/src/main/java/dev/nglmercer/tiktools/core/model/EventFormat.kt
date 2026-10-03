package dev.nglmercer.tiktools.core.model

import org.json.JSONObject

/**
 * Renders one event JSON object as a single display line. Pure and total: unknown shapes degrade to
 * a placeholder, never a crash — TikTok adds methods freely.
 */
object EventFormat {

    /** Render [json] (`{"type": …, …}`) as one line. */
    fun line(json: String): String {
        val value =
            try {
                JSONObject(json)
            } catch (e: Exception) {
                return "(unreadable event)"
            }
        val type = value.optString("type", "?")
        val user = value.optJSONObject("user")?.optString("unique_id", "").orEmpty()
        val at = user.ifEmpty { "-" }
        return when (type) {
            "chat" -> "@$at: ${value.optString("comment", "").take(140)}"
            "gift" ->
                "@$at sent ${value.optString("gift_name", "?")} " +
                    "x${value.optLong("repeat_count", 0)}"
            "like" -> "@$at liked x${value.optLong("count", 0)}"
            // action 1 is a join in every room observed; anything else stays numeric
            // rather than guessed, matching the core's own caution.
            "member" ->
                if (value.optLong("action", -1) == 1L) "@$at joined"
                else "@$at member event (action=${value.optLong("action", -1)})"
            "social" ->
                when {
                    value.optLong("follow_count", 0) > 0 -> "@$at followed"
                    value.optLong("share_count", 0) > 0 -> "@$at shared"
                    else -> "@$at social event"
                }
            "room_user" ->
                "viewers: ${value.optLong("total", 0)} " +
                    "(popularity ${value.optLong("popularity", 0)})"
            else -> {
                val method = value.optString("method", type)
                val bytes = value.optLong("bytes", -1)
                if (bytes >= 0) "unknown: $method ($bytes bytes)" else "unknown: $method"
            }
        }
    }
}
