package com.example.ttlsigner.points

import android.content.Context
import android.content.SharedPreferences
import com.example.ttlsigner.events.LiveEvent

/**
 * Points rates per event category, mirroring TikTools desktop
 * (`PointsConfig`): one rate plus an enable flag per trigger, points per
 * level, and a currency name. Persisted in [SharedPreferences]; the viewer
 * balances live in SQLite.
 */
data class PointsConfig(
    val currencyName: String = "coins",
    val pointsPerChat: Double = 1.0,
    val pointsPerChatEnabled: Boolean = true,
    val pointsPerCoin: Double = 1.0,
    val pointsPerCoinEnabled: Boolean = true,
    val pointsPerLike: Double = 0.1,
    val pointsPerLikeEnabled: Boolean = false,
    val pointsPerFollow: Double = 10.0,
    val pointsPerFollowEnabled: Boolean = true,
    val pointsPerShare: Double = 5.0,
    val pointsPerShareEnabled: Boolean = true,
    val pointsPerJoin: Double = 1.0,
    val pointsPerJoinEnabled: Boolean = false,
    val pointsPerLevel: Double = 100.0,
) {
    /** Clamp to the same safe ranges as the desktop host. */
    fun normalize(): PointsConfig = copy(
        currencyName = currencyName.trim().ifEmpty { "coins" },
        pointsPerChat = pointsPerChat.coerceIn(0.0, MAX_RATE),
        pointsPerCoin = pointsPerCoin.coerceIn(0.0, MAX_RATE),
        pointsPerLike = pointsPerLike.coerceIn(0.0, MAX_RATE),
        pointsPerFollow = pointsPerFollow.coerceIn(0.0, MAX_RATE),
        pointsPerShare = pointsPerShare.coerceIn(0.0, MAX_RATE),
        pointsPerJoin = pointsPerJoin.coerceIn(0.0, MAX_RATE),
        pointsPerLevel = pointsPerLevel.coerceAtLeast(MIN_LEVEL),
    )

    companion object {
        const val MAX_RATE = 1_000_000_000.0
        const val MIN_LEVEL = 10.0
        private const val PREFS = "tiktools_points"
        private const val KEY = "config"

        fun load(context: Context): PointsConfig {
            val raw = prefs(context).getString(KEY, null) ?: return PointsConfig()
            return try {
                decode(raw).normalize()
            } catch (e: Exception) {
                PointsConfig()
            }
        }

        fun save(context: Context, config: PointsConfig) {
            prefs(context).edit().putString(KEY, encode(config.normalize())).apply()
        }

        private fun prefs(context: Context): SharedPreferences =
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

        /** Tiny `k=v;k=v` encoding — no JSON dependency needed for one record. */
        internal fun encode(c: PointsConfig): String = listOf(
            "currency=${c.currencyName}",
            "chat=${c.pointsPerChat},${c.pointsPerChatEnabled}",
            "coin=${c.pointsPerCoin},${c.pointsPerCoinEnabled}",
            "like=${c.pointsPerLike},${c.pointsPerLikeEnabled}",
            "follow=${c.pointsPerFollow},${c.pointsPerFollowEnabled}",
            "share=${c.pointsPerShare},${c.pointsPerShareEnabled}",
            "join=${c.pointsPerJoin},${c.pointsPerJoinEnabled}",
            "level=${c.pointsPerLevel}",
        ).joinToString(";")

        internal fun decode(raw: String): PointsConfig {
            val map = raw.split(";").mapNotNull {
                val i = it.indexOf('=')
                if (i <= 0) null else it.substring(0, i) to it.substring(i + 1)
            }.toMap()
            fun rate(key: String, fallback: Double): Double =
                map[key]?.substringBefore(',')?.toDoubleOrNull() ?: fallback
            fun flag(key: String, fallback: Boolean): Boolean =
                map[key]?.substringAfter(',', "")?.toBooleanStrictOrNull() ?: fallback
            return PointsConfig(
                currencyName = map["currency"] ?: "coins",
                pointsPerChat = rate("chat", 1.0),
                pointsPerChatEnabled = flag("chat", true),
                pointsPerCoin = rate("coin", 1.0),
                pointsPerCoinEnabled = flag("coin", true),
                pointsPerLike = rate("like", 0.1),
                pointsPerLikeEnabled = flag("like", false),
                pointsPerFollow = rate("follow", 10.0),
                pointsPerFollowEnabled = flag("follow", true),
                pointsPerShare = rate("share", 5.0),
                pointsPerShareEnabled = flag("share", true),
                pointsPerJoin = rate("join", 1.0),
                pointsPerJoinEnabled = flag("join", false),
                pointsPerLevel = map["level"]?.toDoubleOrNull() ?: 100.0,
            )
        }
    }
}

/**
 * Pure award computation: event + config → delta. Zero means "no award"
 * (disabled rate, event without a user, or a non-rewarded category).
 */
object PointsEngine {

    fun delta(event: LiveEvent, config: PointsConfig): Double {
        if (event.user.isEmpty()) return 0.0
        return when (event.category) {
            LiveEvent.Category.CHAT ->
                if (config.pointsPerChatEnabled) config.pointsPerChat else 0.0
            LiveEvent.Category.GIFT ->
                if (!config.pointsPerCoinEnabled) 0.0
                else positiveOrOne(event.diamonds.toDouble()) * config.pointsPerCoin
            LiveEvent.Category.LIKE ->
                if (!config.pointsPerLikeEnabled) 0.0
                else positiveOrOne(event.count.toDouble()) * config.pointsPerLike
            LiveEvent.Category.FOLLOW ->
                if (config.pointsPerFollowEnabled) config.pointsPerFollow else 0.0
            LiveEvent.Category.SHARE ->
                if (config.pointsPerShareEnabled) config.pointsPerShare else 0.0
            LiveEvent.Category.JOIN ->
                if (config.pointsPerJoinEnabled) config.pointsPerJoin else 0.0
            LiveEvent.Category.MEMBER,
            LiveEvent.Category.ROOM,
            LiveEvent.Category.UNKNOWN -> 0.0
        }
    }

    fun level(total: Double, config: PointsConfig): Int =
        (total / config.pointsPerLevel.coerceAtLeast(PointsConfig.MIN_LEVEL))
            .toInt().coerceAtLeast(0) + 1

    private fun positiveOrOne(value: Double): Double =
        if (value > 0) value else 1.0
}
