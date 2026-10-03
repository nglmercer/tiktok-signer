package io.github.nglmercer.tiktoklive;

public class SocketException extends TikTokLiveException {
    public SocketException(
            String code,
            String message,
            boolean retryable,
            Long retryAfterMs,
            Integer status,
            String requestId) {
        super(code, message, retryable, retryAfterMs, status, requestId);
    }

    public SocketException(String message) {
        this("SOCKET_ERROR", message, true, null, null, null);
    }
}
