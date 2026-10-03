package io.github.nglmercer.tiktoklive;

public record UnknownEvent(String method, String msgId, boolean isHistory, byte[] payload)
        implements LiveEvent {
    public String type() {
        return "unknown";
    }

    public UnknownEvent {
        payload = payload.clone();
    }

    @Override
    public byte[] payload() {
        return payload.clone();
    }
}
