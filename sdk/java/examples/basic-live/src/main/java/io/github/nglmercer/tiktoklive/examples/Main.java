package io.github.nglmercer.tiktoklive.examples;

import io.github.nglmercer.tiktoklive.*;

public final class Main {
    public static void main(String[] args) throws Exception {
        if (args.length == 0)
            throw new IllegalArgumentException("Pass a creator (native-signer: unsigned URL)");
        try (var live =
                TikTokLive.builder(args[0])
                        .apiUrl(
                                System.getenv()
                                        .getOrDefault(
                                                "TIKTOK_LIVE_API_URL", "http://127.0.0.1:8080"))
                        .apiKey(System.getenv("TIKTOK_LIVE_API_KEY"))
                        .fetchGifts(true)
                        .build()) {
            live.onChat(e -> System.out.println(e.user().nickname() + ": " + e.comment()));
            live.onGiftFinal(
                    e ->
                            System.out.println(
                                    e.user().nickname()
                                            + " sent "
                                            + e.repeatCount()
                                            + " "
                                            + e.giftName()));
            live.onError(Throwable::printStackTrace);
            System.out.println(live.connect().join());
            System.out.println("Press Enter to stop");
            System.in.read();
        }
    }
}
