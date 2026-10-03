package io.github.nglmercer.tiktoklive;

import static org.junit.jupiter.api.Assertions.*;

import com.fasterxml.jackson.databind.*;

import okhttp3.*;
import okhttp3.mockwebserver.*;

import org.junit.jupiter.api.Test;

import java.nio.file.*;
import java.time.Duration;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

class PresenceTest {
    @Test
    void sharedParityCases() throws Exception {
        var cases =
                new ObjectMapper()
                        .readTree(
                                Files.readString(
                                        Path.of(
                                                System.getProperty("repositoryRoot"),
                                                "fixtures/events/presence/cases.json")));
        for (var c : cases)
            assertEquals(
                    c.path("expected").asText(),
                    RoomLookupProbe.parse(
                                    c.path("id").asText(),
                                    c.path("httpStatus").asInt(),
                                    c.path("body").asText())
                            .status()
                            .name(),
                    c.path("name").asText());
    }

    @Test
    void unknownForChangedSchemasAndChallenges() {
        for (String body :
                new String[] {
                    "{}",
                    "{\"data\":{\"user\":{\"status\":2,\"roomId\":true}}}",
                    "{\"data\":{\"user\":{\"status\":99,\"roomId\":\"123\"}}}",
                    "{\"captcha\":{},\"data\":{\"user\":{\"status\":4}}}",
                    "{\"status_code\":2483,\"data\":{\"user\":{\"status\":4}}}",
                    "{\"data\":{\"user\":{\"roomId\":9007199254740992,\"status\":2}}}"
                })
            assertEquals(
                    PresenceStatus.UNKNOWN,
                    RoomLookupProbe.parse("creator", 200, body).status(),
                    body);
    }

    @Test
    void confirmationAndRecovery() {
        var c = new PresenceConfirmation(2);
        assertNull(c.accept(PresenceStatus.OFFLINE));
        assertNull(c.accept(PresenceStatus.OFFLINE));
        assertEquals(PresenceStatus.OFFLINE, c.confirmed);
        assertNull(c.accept(PresenceStatus.LIVE));
        assertEquals(PresenceStatus.LIVE, c.accept(PresenceStatus.LIVE));
        assertNull(c.accept(PresenceStatus.UNKNOWN));
        assertEquals(PresenceStatus.LIVE, c.confirmed);
        assertNull(c.accept(PresenceStatus.LIVE));
        assertNull(c.accept(PresenceStatus.OFFLINE));
        assertNull(c.accept(PresenceStatus.UNKNOWN));
        assertNull(c.accept(PresenceStatus.OFFLINE));
        assertEquals(PresenceStatus.OFFLINE, c.accept(PresenceStatus.OFFLINE));
    }

    @Test
    void initialLiveNeedsConfirmation() {
        var c = new PresenceConfirmation(2);
        assertNull(c.accept(PresenceStatus.LIVE));
        assertEquals(PresenceStatus.LIVE, c.accept(PresenceStatus.LIVE));
    }

    @Test
    void realHttpAndNetworkErrors() throws Exception {
        try (var server = new MockWebServer()) {
            var http = new OkHttpClient.Builder().callTimeout(Duration.ofMillis(200)).build();
            try (var probe =
                    new RoomLookupProbe(http, server.url("/api-live/user/room/").toString())) {
                server.enqueue(
                        new MockResponse()
                                .setBody(
                                        "{\"data\":{\"user\":{\"roomId\":\"123\",\"status\":2}}}"));
                assertTrue(probe.lookup("@creator").get(2, TimeUnit.SECONDS).isLive());
                assertEquals(
                        "creator", server.takeRequest().getRequestUrl().queryParameter("uniqueId"));
                server.enqueue(new MockResponse().setSocketPolicy(SocketPolicy.NO_RESPONSE));
                assertEquals(
                        PresenceStatus.UNKNOWN,
                        probe.lookup("creator").get(2, TimeUnit.SECONDS).status());
            } finally {
                http.dispatcher().executorService().shutdownNow();
                http.connectionPool().evictAll();
            }
        }
    }

    @Test
    void multiCreatorMonitoringAndCleanup() throws Exception {
        var counts = new ConcurrentHashMap<String, AtomicInteger>();
        var active = new ConcurrentHashMap<String, AtomicInteger>();
        var started = new CountDownLatch(2);
        var ended = new CountDownLatch(2);
        PresenceProbe probe =
                id -> {
                    assertEquals(
                            1,
                            active.computeIfAbsent(id, k -> new AtomicInteger()).incrementAndGet());
                    int n = counts.computeIfAbsent(id, k -> new AtomicInteger()).incrementAndGet();
                    active.get(id).decrementAndGet();
                    var status =
                            n <= 2
                                    ? PresenceStatus.LIVE
                                    : n == 3 ? PresenceStatus.UNKNOWN : PresenceStatus.OFFLINE;
                    return CompletableFuture.completedFuture(
                            new LivePresence(id, "123", "", "", status));
                };
        var scheduler = Executors.newSingleThreadScheduledExecutor();
        try (var monitor =
                LivePresenceMonitor.builder()
                        .client(probe)
                        .scheduler(scheduler)
                        .polling(Duration.ofMillis(10), Duration.ofMillis(10))
                        .errorBackoff(Duration.ofMillis(10), Duration.ofMillis(40))
                        .build()) {
            for (String id : new String[] {"a", "b"})
                monitor.watch(id)
                        .onLiveStarted(e -> started.countDown())
                        .onLiveEnded(e -> ended.countDown());
            assertTrue(started.await(3, TimeUnit.SECONDS));
            assertTrue(ended.await(3, TimeUnit.SECONDS));
            assertEquals(PresenceStatus.OFFLINE, monitor.watch("a").state().confirmed());
            monitor.close();
            monitor.close();
            assertFalse(scheduler.isShutdown());
            assertThrows(IllegalStateException.class, () -> monitor.watch("c"));
        } finally {
            scheduler.shutdownNow();
        }
    }

    @Test
    void canceledLookupPropagates() {
        var source = new CompletableFuture<LivePresence>();
        try (var client = LivePresenceClient.builder().probe(id -> source).build()) {
            var result = client.lookup("creator");
            result.cancel(true);
            assertTrue(source.isCancelled());
            assertThrows(IllegalArgumentException.class, () -> client.lookup("@@bad"));
        }
    }

    @Test
    void shutdownCancelsPendingWatch() throws Exception {
        var future = new CompletableFuture<LivePresence>();
        var called = new CountDownLatch(1);
        var monitor =
                LivePresenceMonitor.builder()
                        .client(
                                id -> {
                                    called.countDown();
                                    return future;
                                })
                        .build();
        monitor.watch("creator");
        assertTrue(called.await(2, TimeUnit.SECONDS));
        monitor.close();
        assertTrue(future.isCancelled());
    }
}
