package io.github.nglmercer.tiktoklive;

public record RoomUserEvent(
        String method,
        String msgId,
        boolean isHistory,
        long viewers,
        long popularity,
        long totalUser,
        long anonymous,
        java.util.List<TopViewer> topViewers)
        implements LiveEvent {
    public String type() {
        return "roomUser";
    }

    public RoomUserEvent {
        topViewers = java.util.List.copyOf(topViewers);
    }

    public java.util.List<TopViewer> rankedViewers() {
        return topViewers;
    }
}
