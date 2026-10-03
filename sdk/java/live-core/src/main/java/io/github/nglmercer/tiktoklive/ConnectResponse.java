package io.github.nglmercer.tiktoklive;

public record ConnectResponse(
        int version,
        String uniqueId,
        String roomId,
        ConnectionStatus status,
        ConnectionDescriptor connection) {}
