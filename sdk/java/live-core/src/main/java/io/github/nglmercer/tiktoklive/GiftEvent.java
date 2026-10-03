package io.github.nglmercer.tiktoklive;

public record GiftEvent(
        String method,
        String msgId,
        boolean isHistory,
        EventUser user,
        EventUser toUser,
        String giftId,
        String giftName,
        long diamondCount,
        long repeatCount,
        long comboCount,
        String groupId,
        boolean repeatEnd,
        String iconUrl,
        Boolean streakable)
        implements LiveEvent {
    public String type() {
        return "gift";
    }
}
