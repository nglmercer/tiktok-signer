package io.github.nglmercer.tiktoklive;

public class RateLimitException extends TikTokLiveException {
    public RateLimitException(
            String code,
            String message,
            boolean retryable,
            Long retryAfterMs,
            Integer status,
            String requestId) {
        super(code, message, retryable, retryAfterMs, status, requestId);
    }

    public RateLimitException(String message) {
        this("RATELIMIT_ERROR", message, false, null, null, null);
    }
}
