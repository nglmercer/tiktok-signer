package io.github.nglmercer.tiktoklive;

public class SignerUnavailableException extends TikTokLiveException {
    public SignerUnavailableException(
            String code,
            String message,
            boolean retryable,
            Long retryAfterMs,
            Integer status,
            String requestId) {
        super(code, message, retryable, retryAfterMs, status, requestId);
    }

    public SignerUnavailableException(String message) {
        this("SIGNERUNAVAILABLE_ERROR", message, false, null, null, null);
    }
}
