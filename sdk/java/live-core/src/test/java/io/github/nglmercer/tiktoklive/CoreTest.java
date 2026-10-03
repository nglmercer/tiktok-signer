package io.github.nglmercer.tiktoklive;

import static org.junit.jupiter.api.Assertions.*;

import org.junit.jupiter.api.Test;

import java.time.Duration;

class CoreTest {
    @Test
    void uniqueIds() {
        assertEquals("creator", UniqueId.normalize(" @creator "));
        for (String value :
                new String[] {
                    "@@creator", "https://tiktok.com/@a", "a/b", "", "a b", "a".repeat(25)
                }) assertThrows(IllegalArgumentException.class, () -> UniqueId.normalize(value));
    }

    @Test
    void reconnectAndRetryAfter() {
        var p = ReconnectPolicy.DEFAULT;
        long[] expected = {2000, 4000, 8000, 16000, 32000, 60000};
        for (int i = 0; i < expected.length; i++)
            assertEquals(expected[i], p.delay(i + 1, 0, null).toMillis());
        assertEquals(3000, p.delay(1, 1, null).toMillis());
        assertEquals(90000, p.delay(1, 0, 90000L).toMillis());
        assertThrows(
                IllegalArgumentException.class,
                () -> new ReconnectPolicy(-1, Duration.ofSeconds(1), Duration.ofSeconds(2)));
    }

    private GiftEvent gift(long count, boolean end, Boolean streakable) {
        var user = new EventUser("1", "tester", "tester", "", "");
        return new GiftEvent(
                "WebcastGiftMessage",
                "1",
                false,
                user,
                user,
                "5655",
                "Rose",
                1,
                count,
                count,
                "group",
                end,
                "",
                streakable);
    }

    @Test
    void streaks() {
        var tracker = new GiftStreakTracker();
        assertTrue(tracker.accept(gift(1, false, true)).isEmpty());
        assertTrue(tracker.accept(gift(2, false, true)).isEmpty());
        assertTrue(tracker.accept(gift(1, false, true)).isEmpty());
        assertEquals(2, tracker.accept(gift(2, true, true)).orElseThrow().repeatCount());
        assertTrue(tracker.accept(gift(2, true, true)).isEmpty());
        tracker.clear();
        assertTrue(tracker.accept(gift(1, false, false)).isPresent());
        assertTrue(tracker.accept(gift(1, false, null)).isEmpty());
        assertTrue(tracker.accept(gift(2, true, null)).isPresent());
    }

    @Test
    void immutablePayload() {
        byte[] data = {1};
        var unknown = new UnknownEvent("future", "0", false, data);
        data[0] = 2;
        assertEquals(1, unknown.payload()[0]);
        unknown.payload()[0] = 3;
        assertEquals(1, unknown.payload()[0]);
    }
}
