package io.github.nglmercer.tiktoklive;

public record LivePresence(
        String uniqueId, String roomId, String nickname, String title, PresenceStatus status) {
    public boolean isLive() {
        return status == PresenceStatus.LIVE;
    }

    public static LivePresence unknown(String id) {
        return new LivePresence(id, "", "", "", PresenceStatus.UNKNOWN);
    }
}
