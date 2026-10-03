package io.github.nglmercer.tiktoklive;

public record LiveState(
        String uniqueId, String roomId, ConnectionStatus status, boolean connected) {}
