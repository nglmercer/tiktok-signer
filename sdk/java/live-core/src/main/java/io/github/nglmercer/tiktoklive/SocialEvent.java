package io.github.nglmercer.tiktoklive;

public record SocialEvent(
        String method,
        String msgId,
        boolean isHistory,
        EventUser user,
        long action,
        long followCount,
        long shareCount)
        implements LiveEvent {
    public String type() {
        return "social";
    }
}
