package io.github.nglmercer.tiktoklive;

public record LikeEvent(
        String method, String msgId, boolean isHistory, EventUser user, long count, long total)
        implements LiveEvent {
    public String type() {
        return "like";
    }
}
