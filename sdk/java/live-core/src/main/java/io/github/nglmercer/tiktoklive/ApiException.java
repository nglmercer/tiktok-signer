package io.github.nglmercer.tiktoklive;

public class ApiException extends TikTokLiveException {
    public ApiException(
            String code,
            String message,
            boolean retryable,
            Long retryAfterMs,
            Integer status,
            String requestId) {
        super(code, message, retryable, retryAfterMs, status, requestId);
    }

    public ApiException(String message) {
        this("API_ERROR", message, false, null, null, null);
    }
}
