package io.github.nglmercer.tiktoklive;

import java.util.LinkedHashMap;
import java.util.Optional;

/** Emits a streak's final total once. Unknown gifts require repeatEnd to avoid guessing. */
public final class GiftStreakTracker {
    private record Streak(long count, boolean completed, GiftEvent latest) {}

    private final LinkedHashMap<String, Streak> streaks = new LinkedHashMap<>();

    public synchronized Optional<GiftEvent> accept(GiftEvent event) {
        if (Boolean.FALSE.equals(event.streakable())) return Optional.of(event);
        String key =
                event.user().userId()
                        + ":"
                        + event.toUser().userId()
                        + ":"
                        + event.giftId()
                        + ":"
                        + event.groupId();
        Streak previous = streaks.get(key);
        if (previous != null && previous.completed()) return Optional.empty();
        long count = Math.max(event.repeatCount(), previous == null ? 0 : previous.count());
        GiftEvent merged =
                new GiftEvent(
                        event.method(),
                        event.msgId(),
                        event.isHistory(),
                        event.user(),
                        event.toUser(),
                        event.giftId(),
                        event.giftName().isEmpty() && previous != null
                                ? previous.latest().giftName()
                                : event.giftName(),
                        event.diamondCount() == 0 && previous != null
                                ? previous.latest().diamondCount()
                                : event.diamondCount(),
                        count,
                        event.comboCount(),
                        event.groupId(),
                        event.repeatEnd(),
                        event.iconUrl(),
                        event.streakable());
        streaks.put(key, new Streak(count, event.repeatEnd(), merged));
        if (streaks.size() > 4096) streaks.remove(streaks.keySet().iterator().next());
        return event.repeatEnd() ? Optional.of(merged) : Optional.empty();
    }

    public synchronized void clear() {
        streaks.clear();
    }
}
