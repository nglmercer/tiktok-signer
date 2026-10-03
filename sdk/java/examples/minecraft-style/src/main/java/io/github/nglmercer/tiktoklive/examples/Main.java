package io.github.nglmercer.tiktoklive.examples;

import io.github.nglmercer.tiktoklive.*;

public final class Main {
    public static void main(String[] args) throws Exception {
        if (args.length == 0)
            throw new IllegalArgumentException("Pass a creator (native-signer: unsigned URL)");
        // Represents a server scheduler. A real plugin supplies its platform scheduler here.
        var gameMainThreadExecutor =
                java.util.concurrent.Executors.newSingleThreadExecutor(
                        r -> {
                            Thread t = new Thread(r, "game-main-example");
                            t.setDaemon(true);
                            return t;
                        });
        try (var live =
                TikTokLive.builder(args[0]).apiKey(System.getenv("TIKTOK_LIVE_API_KEY")).build()) {
            live.onGift(
                    e ->
                            gameMainThreadExecutor.execute(
                                    () -> {
                                        // Minecraft/Bukkit/Paper action here, on the server's
                                        // actual main thread.
                                        System.out.println(
                                                Thread.currentThread().getName()
                                                        + ": "
                                                        + e.giftName());
                                    }));
            live.connect().join();
            System.in.read();
        } finally {
            gameMainThreadExecutor.shutdownNow();
        }
    }
}
