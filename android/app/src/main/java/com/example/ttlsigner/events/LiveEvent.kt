package com.example.ttlsigner.events

import org.json.JSONObject

/**
 * One normalized live event, parsed from the Rust core's JSON push
 * (`{"type": …, …}`). Small and typed so the reader, points, actions, and
 * (future) TTS all share one interpretation of the stream.
 *
 * Unknown shapes degrade to [Category.UNKNOWN] with the raw payload kept —
 * TikTok adds methods freely, and nothing may crash the reader.
 */
data class LiveEvent(
    val category: Category,
    /** Stable handle without the `@`, empty when the event has no user. */
    val user: String,
    val nickname: String,
    /** Chat text, gift name, or empty. */
    val text: String,
    /** Gift/like repeat count, viewer total, member action; 0 when absent. */
    val count: Long,
    /** Gift diamond value; 0 when absent. */
    val diamonds: Long,
    /** Sender avatar URL (`user.avatar_url`); empty when absent. */
    val avatarUrl: String = "",
    /** Gift artwork URL; empty when absent or not a gift. */
    val giftImageUrl: String = "",
    /** Epoch millis when the event landed on device. */
    val at: Long = System.currentTimeMillis(),
    /** The original JSON, kept for actions templates and debugging. */
    val raw: String = "",
) {
    enum class Category {
        CHAT,
        GIFT,
        LIKE,
        FOLLOW,
        SHARE,
        JOIN,
        MEMBER,
        ROOM,
        UNKNOWN,
    }
}

/**
 * Pure parser: event JSON → [LiveEvent]. Total — garbage in gives an
 * [LiveEvent.Category.UNKNOWN] event, never an exception.
 */
object EventParser {

    fun parse(json: String, at: Long = System.currentTimeMillis()): LiveEvent {
        val value = try {
            JSONObject(json)
        } catch (e: Exception) {
            return LiveEvent(LiveEvent.Category.UNKNOWN, "", "", "", 0, 0, at = at, raw = json)
        }
        val type = value.optString("type", "?")
        val userObj = value.optJSONObject("user")
        val user = userObj?.optString("unique_id", "").orEmpty()
        val nickname = userObj?.optString("nickname", "").orEmpty()
        val base = LiveEvent(
            category = LiveEvent.Category.UNKNOWN,
            user = user,
            nickname = nickname,
            text = "",
            count = 0,
            diamonds = 0,
            // optString maps a JSON null (serde's None) to the default, so
            // older native builds without these keys parse identically.
            avatarUrl = userObj?.optString("avatar_url", "").orEmpty(),
            giftImageUrl = value.optString("gift_image_url", ""),
            at = at,
            raw = json,
        )
        return when (type) {
            "chat" -> base.copy(
                category = LiveEvent.Category.CHAT,
                text = value.optString("comment", "").take(500),
            )
            "gift" -> base.copy(
                category = LiveEvent.Category.GIFT,
                text = value.optString("gift_name", "?"),
                count = value.optLong("repeat_count", 0),
                diamonds = value.optLong("diamond_count", 0),
            )
            "like" -> base.copy(
                category = LiveEvent.Category.LIKE,
                count = value.optLong("count", 0),
            )
            // Action 1 is a join in every room observed; anything else stays a
            // generic member event, matching the core's own caution.
            "member" -> if (value.optLong("action", -1) == 1L) {
                base.copy(category = LiveEvent.Category.JOIN)
            } else {
                base.copy(
                    category = LiveEvent.Category.MEMBER,
                    count = value.optLong("action", -1),
                )
            }
            "social" -> when {
                value.optLong("follow_count", 0) > 0 ->
                    base.copy(category = LiveEvent.Category.FOLLOW)
                value.optLong("share_count", 0) > 0 ->
                    base.copy(category = LiveEvent.Category.SHARE)
                else -> base.copy(category = LiveEvent.Category.MEMBER)
            }
            "room_user" -> base.copy(
                category = LiveEvent.Category.ROOM,
                count = value.optLong("total", 0),
            )
            else -> base.copy(
                text = value.optString("method", type),
                count = value.optLong("bytes", -1),
            )
        }
    }
}
