package io.github.nglmercer.tiktoklive;

public class TikTokLiveException extends RuntimeException {
    private final String code, requestId;
    private final boolean retryable;
    private final Long retryAfterMs;
    private final Integer httpStatus;

    public TikTokLiveException(
            String code,
            String message,
            boolean retryable,
            Long retryAfterMs,
            Integer httpStatus,
            String requestId) {
        super(message);
        this.code = code;
        this.retryable = retryable;
        this.retryAfterMs = retryAfterMs;
        this.httpStatus = httpStatus;
        this.requestId = requestId;
    }

    public String code() {
        return code;
    }

    public boolean retryable() {
        return retryable;
    }

    public Long retryAfterMs() {
        return retryAfterMs;
    }

    public Integer httpStatus() {
        return httpStatus;
    }

    public String requestId() {
        return requestId;
    }
}
