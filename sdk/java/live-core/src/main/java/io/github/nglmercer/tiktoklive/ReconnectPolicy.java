package io.github.nglmercer.tiktoklive;

import java.time.Duration;

public record ReconnectPolicy(int attempts, Duration initialDelay, Duration maxDelay) {
    public static final ReconnectPolicy DEFAULT =
            new ReconnectPolicy(5, Duration.ofSeconds(2), Duration.ofSeconds(60));

    public ReconnectPolicy {
        if (attempts < 0
                || initialDelay == null
                || maxDelay == null
                || initialDelay.isNegative()
                || initialDelay.isZero()
                || maxDelay.compareTo(initialDelay) < 0
                || maxDelay.toMillis() > Integer.MAX_VALUE)
            throw new IllegalArgumentException("Invalid reconnect policy");
    }

    public Duration delay(int attempt, double random, Long retryAfterMs) {
        double base =
                Math.min(
                        initialDelay.toMillis()
                                * Math.pow(2, Math.min(30, Math.max(0, attempt - 1))),
                        maxDelay.toMillis());
        long delay =
                Math.min(
                        maxDelay.toMillis(),
                        (long) (base * (1 + Math.max(0, Math.min(1, random)) * .5)));
        return Duration.ofMillis(
                Math.max(delay, retryAfterMs == null ? 0 : Math.max(0, retryAfterMs)));
    }
}
