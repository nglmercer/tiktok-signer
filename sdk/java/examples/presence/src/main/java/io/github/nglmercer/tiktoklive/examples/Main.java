package io.github.nglmercer.tiktoklive.examples;

import io.github.nglmercer.tiktoklive.*;

public final class Main {
    public static void main(String[] args) throws Exception {
        if (args.length == 0)
            throw new IllegalArgumentException("Pass a creator (native-signer: unsigned URL)");
        try (var presence = LivePresenceClient.create()) {
            System.out.println(presence.lookup(args[0]).join());
        }
    }
}
