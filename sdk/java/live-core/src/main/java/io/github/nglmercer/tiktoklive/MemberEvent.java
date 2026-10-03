package io.github.nglmercer.tiktoklive;

public record MemberEvent(
        String method,
        String msgId,
        boolean isHistory,
        EventUser user,
        long memberCount,
        long action)
        implements LiveEvent {
    public String type() {
        return "member";
    }
}
