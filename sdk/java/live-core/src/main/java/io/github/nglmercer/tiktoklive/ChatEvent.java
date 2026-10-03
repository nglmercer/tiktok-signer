package io.github.nglmercer.tiktoklive;

public record ChatEvent(
        String method, String msgId, boolean isHistory, EventUser user, String comment)
        implements LiveEvent {
    public String type() {
        return "chat";
    }
}
