package io.github.nglmercer.tiktoklive;

public class DiscoveryException extends TikTokLiveException {
    public DiscoveryException(
            String code,
            String message,
            boolean retryable,
            Long retryAfterMs,
            Integer status,
            String requestId) {
        super(code, message, retryable, retryAfterMs, status, requestId);
    }

    public DiscoveryException(String message) {
        this("DISCOVERY_ERROR", message, true, null, null, null);
    }
}
