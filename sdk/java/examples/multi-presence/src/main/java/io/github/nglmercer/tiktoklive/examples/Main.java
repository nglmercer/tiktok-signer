package io.github.nglmercer.tiktoklive.examples;

import io.github.nglmercer.tiktoklive.*;

public final class Main {
    public static void main(String[] args) throws Exception {
        if (args.length == 0)
            throw new IllegalArgumentException("Pass a creator (native-signer: unsigned URL)");
        try (var monitor = LivePresenceMonitor.create()) {
            for (String creator : args)
                monitor.watch(creator)
                        .onLiveStarted(e -> System.out.println(e.uniqueId() + " started LIVE"))
                        .onLiveEnded(e -> System.out.println(e.uniqueId() + " ended LIVE"));
            System.out.println("Press Enter to stop");
            System.in.read();
        }
    }
}
