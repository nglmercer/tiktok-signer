package io.github.nglmercer.tiktoklive;

import static org.junit.jupiter.api.Assertions.*;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import com.fasterxml.jackson.databind.ObjectMapper;

import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable;

import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.MessageDigest;
import java.time.Duration;
import java.util.HexFormat;
import java.util.concurrent.*;

/** Needs the QuickJS JNI library: set TTL_NATIVE_LIBRARY to a built libttl_sign_mobile. */
class QuickJsSignerTest {
    private static final String URL =
            "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?room_id=7300000000000000001&aid=1988&version_code=180800";
    private static final String FETCH_URL =
            "https://webcast.tiktok.com/webcast/im/fetch/?aid=1988&room_id=7300000000000000001";
    private static final String FAKE =
            "window.byted_acrawler={init:function(){},registerWsSigner:function(){var i=0;return"
                + " function(){return {'X-Gnarly':String(++i)}}}};";

    @BeforeAll
    static void requireNativeEngine() {
        String library =
                System.getProperty(
                        "tiktok.live.native.library", System.getenv("TTL_NATIVE_LIBRARY"));
        assumeTrue(library != null && !library.isBlank(), "QuickJS JNI library required");
    }

    @Test
    void warmContextAndConcurrentRequests() throws Exception {
        try (var signer = new QuickJsSigner(FAKE, true, Duration.ofSeconds(30), 16)) {
            var first = signer.sign(URL, QuickJsSigner.Product.WS);
            var second = signer.sign(URL, QuickJsSigner.Product.WS);
            assertEquals(URL + "&X-Gnarly=1", first.get(40, TimeUnit.SECONDS).url());
            assertEquals(URL + "&X-Gnarly=2", second.get(40, TimeUnit.SECONDS).url());
            assertFalse(second.join().toString().contains("730000"));
        }
    }

    @Test
    void failureIsRedactedAndRetiresSigner() {
        try (var signer =
                new QuickJsSigner(
                        "throw new Error('secret-cookie')", false, Duration.ofSeconds(30), 16)) {
            var failure =
                    assertThrows(
                            CompletionException.class,
                            () -> signer.sign(URL, QuickJsSigner.Product.WS).join());
            assertFalse(failure.toString().contains("secret-cookie"));
            assertThrows(
                    CompletionException.class,
                    () -> signer.sign(URL, QuickJsSigner.Product.WS).join());
        }
    }

    @Test
    void timeoutRetiresWedgedContext() {
        try (var signer = new QuickJsSigner("while(true){}", true, Duration.ofMillis(500), 1)) {
            long started = System.nanoTime();
            assertThrows(
                    CompletionException.class,
                    () -> signer.sign(URL, QuickJsSigner.Product.WS).join());
            assertThrows(
                    CompletionException.class,
                    () -> signer.sign(URL, QuickJsSigner.Product.WS).join());
            assertTrue(System.nanoTime() - started < TimeUnit.SECONDS.toNanos(10));
        }
    }

    @Test
    void closeIsIdempotent() {
        var signer = new QuickJsSigner(FAKE, false, Duration.ofSeconds(30), 16);
        signer.close();
        signer.close();
        assertThrows(
                CompletionException.class, () -> signer.sign(URL, QuickJsSigner.Product.WS).join());
    }

    @Test
    void closingBusySignerCancelsTaskAndReleasesWorkers() throws Exception {
        // Loads, then wedges on the first signature: close must interrupt QuickJS mid-call.
        String wedged =
                "window.byted_acrawler={init:function(){},registerWsSigner:function(){"
                        + "return function(){while(true){}}}};";
        var signer = new QuickJsSigner(wedged, true, Duration.ofSeconds(60), 1);
        var pending = signer.sign(URL, QuickJsSigner.Product.WS);
        Thread.sleep(300);
        signer.close();
        assertTrue(pending.isDone());
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(3);
        while (System.nanoTime() < deadline
                && Thread.getAllStackTraces().keySet().stream()
                        .anyMatch(t -> t.isAlive() && t.getName().startsWith("ttl-quickjs-")))
            Thread.sleep(10);
        assertFalse(
                Thread.getAllStackTraces().keySet().stream()
                        .anyMatch(t -> t.isAlive() && t.getName().startsWith("ttl-quickjs-")));
    }

    /** Byte parity with the signatures recorded from V8, by SHA-256 so none is committed. */
    @Test
    @EnabledIfEnvironmentVariable(named = "TTL_BUNDLE", matches = ".+")
    void freshContextParityAgainstRecordedSignaturesForEveryProduct() throws Exception {
        Path bundle = Path.of(System.getenv("TTL_BUNDLE"));
        byte[] bytes = Files.readAllBytes(bundle);
        var recorded =
                new ObjectMapper()
                        .readTree(
                                Path.of(
                                                System.getProperty("repositoryRoot"),
                                                "fixtures/research/pinned-signatures-1.0.0.388.json")
                                        .toFile());
        assertEquals(recorded.get("bundle_sha256").asText(), sha256(bytes));
        for (var product : QuickJsSigner.Product.values()) {
            String url = product == QuickJsSigner.Product.WS ? URL : FETCH_URL;
            String name = product.name().toLowerCase(java.util.Locale.ROOT);
            var expected =
                    java.util.stream.StreamSupport.stream(recorded.get("cases").spliterator(), false)
                            .filter(c -> c.get("product").asText().equals(name))
                            .findFirst()
                            .orElseThrow();
            assertEquals(url, expected.get("url").asText());
            try (var signer =
                    new QuickJsSigner(
                            new String(bytes, StandardCharsets.UTF_8),
                            true,
                            Duration.ofSeconds(60),
                            16)) {
                String signed = signer.sign(url, product).get(70, TimeUnit.SECONDS).url();
                // Avoid exposing full signed URLs in assertion diagnostics.
                assertTrue(
                        expected.get("signed_sha256")
                                .asText()
                                .equals(sha256(signed.getBytes(StandardCharsets.UTF_8))),
                        product + " parity mismatch");
            }
        }
    }

    private static String sha256(byte[] bytes) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }
}
