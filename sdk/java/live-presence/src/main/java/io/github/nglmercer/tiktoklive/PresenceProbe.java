package io.github.nglmercer.tiktoklive;

import java.util.concurrent.CompletableFuture;

@FunctionalInterface
public interface PresenceProbe {
    CompletableFuture<LivePresence> lookup(String uniqueId);
}
