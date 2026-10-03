package io.github.nglmercer.tiktoklive;

/** Per-creator confirmation state. UNKNOWN breaks a candidate, but preserves confirmed status. */
final class PresenceConfirmation {
    private final int required;
    PresenceStatus confirmed = PresenceStatus.UNKNOWN, candidate = PresenceStatus.UNKNOWN;
    int count, failures;

    PresenceConfirmation(int required) {
        this.required = required;
    }

    synchronized PresenceStatus accept(PresenceStatus status) {
        if (status == PresenceStatus.UNKNOWN) {
            failures++;
            candidate = PresenceStatus.UNKNOWN;
            count = 0;
            return null;
        }
        failures = 0;
        if (status == confirmed) {
            candidate = PresenceStatus.UNKNOWN;
            count = 0;
            return null;
        }
        if (status != candidate) {
            candidate = status;
            count = 1;
        } else count++;
        if (count < required) return null;
        PresenceStatus previous = confirmed;
        confirmed = status;
        candidate = PresenceStatus.UNKNOWN;
        count = 0;
        return status == PresenceStatus.LIVE || previous == PresenceStatus.LIVE ? status : null;
    }
}
