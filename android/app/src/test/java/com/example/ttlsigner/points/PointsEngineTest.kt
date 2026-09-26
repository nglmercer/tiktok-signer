package com.example.ttlsigner.points

import com.example.ttlsigner.events.LiveEvent
import org.junit.Assert.assertEquals
import org.junit.Test

/** Award math and config persistence encoding. Pure; no Android needed. */
class PointsEngineTest {

    private val config = PointsConfig()

    @Test
    fun chatAwardFollowsItsRateAndFlag() {
        val chat = LiveEvent(LiveEvent.Category.CHAT, "ay", "", "hi", 0, 0)
        assertEquals(1.0, PointsEngine.delta(chat, config), 0.0)
        assertEquals(
            0.0,
            PointsEngine.delta(chat, config.copy(pointsPerChatEnabled = false)),
            0.0,
        )
        assertEquals(
            2.5,
            PointsEngine.delta(chat, config.copy(pointsPerChat = 2.5)),
            0.0,
        )
    }

    @Test
    fun giftMultipliesDiamondsWithFallbackToOne() {
        val gift = LiveEvent(LiveEvent.Category.GIFT, "ay", "", "Rose", 3, 5)
        assertEquals(5.0, PointsEngine.delta(gift, config), 0.0)
        val noDiamonds = gift.copy(diamonds = 0)
        assertEquals(1.0, PointsEngine.delta(noDiamonds, config), 0.0)
        assertEquals(
            0.0,
            PointsEngine.delta(gift, config.copy(pointsPerCoinEnabled = false)),
            0.0,
        )
    }

    @Test
    fun likeIsDisabledByDefault() {
        val like = LiveEvent(LiveEvent.Category.LIKE, "ay", "", "", 10, 0)
        assertEquals(0.0, PointsEngine.delta(like, config), 0.0)
        assertEquals(
            1.0,
            PointsEngine.delta(like, config.copy(pointsPerLikeEnabled = true)),
            0.0001,
        )
    }

    @Test
    fun eventsWithoutAUserEarnNothing() {
        val room = LiveEvent(LiveEvent.Category.ROOM, "", "", "", 187, 0)
        assertEquals(0.0, PointsEngine.delta(room, config), 0.0)
        val anonChat = LiveEvent(LiveEvent.Category.CHAT, "", "", "hi", 0, 0)
        assertEquals(0.0, PointsEngine.delta(anonChat, config), 0.0)
    }

    @Test
    fun levelsStartAtOneAndStepPerThreshold() {
        assertEquals(1, PointsEngine.level(0.0, config))
        assertEquals(1, PointsEngine.level(99.0, config))
        assertEquals(2, PointsEngine.level(100.0, config))
        assertEquals(3, PointsEngine.level(250.0, config.copy(pointsPerLevel = 100.0)))
    }

    @Test
    fun normalizeClampsRatesAndLevel() {
        val clean = PointsConfig(
            currencyName = "  ",
            pointsPerChat = -5.0,
            pointsPerLevel = 1.0,
        ).normalize()
        assertEquals("coins", clean.currencyName)
        assertEquals(0.0, clean.pointsPerChat, 0.0)
        assertEquals(PointsConfig.MIN_LEVEL, clean.pointsPerLevel, 0.0)
    }

    @Test
    fun encodeDecodeRoundTrips() {
        val original = PointsConfig(
            currencyName = "stars",
            pointsPerChat = 2.0,
            pointsPerChatEnabled = false,
            pointsPerLike = 0.5,
            pointsPerLikeEnabled = true,
            pointsPerLevel = 250.0,
        )
        assertEquals(original, PointsConfig.decode(PointsConfig.encode(original)))
    }

    @Test
    fun decodeToleratesGarbage() {
        val decoded = PointsConfig.decode("not;a=config")
        assertEquals(PointsConfig(), decoded.normalize())
    }
}
