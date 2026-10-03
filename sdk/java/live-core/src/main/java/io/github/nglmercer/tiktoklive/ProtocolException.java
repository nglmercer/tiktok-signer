package io.github.nglmercer.tiktoklive;

public class ProtocolException extends TikTokLiveException {
    public ProtocolException(
            String code,
            String message,
            boolean retryable,
            Long retryAfterMs,
            Integer status,
            String requestId) {
        super(code, message, retryable, retryAfterMs, status, requestId);
    }

    public ProtocolException(String message) {
        this("PROTOCOL_ERROR", message, false, null, null, null);
    }
}
