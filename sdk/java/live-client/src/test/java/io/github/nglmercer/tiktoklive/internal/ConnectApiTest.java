package io.github.nglmercer.tiktoklive.internal;

import static org.junit.jupiter.api.Assertions.*;

import com.fasterxml.jackson.databind.*;

import io.github.nglmercer.tiktoklive.*;

import okhttp3.*;
import okhttp3.mockwebserver.*;

import org.junit.jupiter.api.Test;

import java.util.concurrent.*;

class ConnectApiTest {
    final ObjectMapper json = new ObjectMapper();
    final String socket =
            "wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?room_id=7300&X-Gnarly=signature";

    String live() {
        return "{\"version\":1,\"uniqueId\":\"creator\",\"status\":\"live\",\"roomId\":\"7300\",\"connection\":{\"url\":\""
                + socket
                + "\",\"cookies\":\"cookie=value\",\"userAgent\":\"agent\"}}";
    }

    @Test
    void parsingAndValidation() throws Exception {
        var r = ConnectApi.parse(json.readTree(live()), "creator");
        assertEquals(ConnectionStatus.LIVE, r.status());
        assertEquals("cookie=value", r.connection().cookies());
        assertEquals(
                ConnectionStatus.OFFLINE,
                ConnectApi.parse(
                                json.readTree(
                                        "{\"version\":1,\"uniqueId\":\"creator\",\"status\":\"offline\"}"),
                                "creator")
                        .status());
        assertThrows(
                ProtocolException.class, () -> ConnectApi.parse(json.readTree(live()), "other"));
        assertThrows(
                ProtocolException.class,
                () -> ConnectApi.parse(json.readTree(live().replace("signature", "")), "creator"));
        for (String bad :
                new String[] {
                    socket.replace("wss:", "ws:"),
                    socket.replace("tiktok.com", "evil.com"),
                    socket.replace("7300", "7400"),
                    socket.replace("wss://", "wss://user:pass@"),
                    socket + "#fragment",
                    socket.replace(".com/", ".com:8443/")
                }) assertFalse(ConnectApi.allowedSocket(bad, "7300"), bad);
    }

    @Test
    void metadataAndTypedErrors() throws Exception {
        var root =
                json.readTree(
                        "{\"error\":{\"code\":\"RATE_LIMITED\",\"message\":\"wait\",\"retryable\":true,\"retryAfterMs\":90000}}");
        var error = ConnectApi.error(429, "request-1", root);
        assertInstanceOf(RateLimitException.class, error);
        assertEquals(90000L, error.retryAfterMs());
        assertEquals(429, error.httpStatus());
        assertEquals("request-1", error.requestId());
        assertTrue(error.retryable());
        assertInstanceOf(AuthenticationException.class, ConnectApi.error(401, null, null));
        assertInstanceOf(
                SignerUnavailableException.class,
                ConnectApi.error(
                        503, null, json.readTree("{\"error\":{\"code\":\"SIGNER_UNAVAILABLE\"}}")));
    }

    @Test
    void actualAsyncBrokerRequest() throws Exception {
        try (var server = new MockWebServer()) {
            server.enqueue(new MockResponse().setBody(live()));
            var http = new OkHttpClient();
            try (var api = new ConnectApi(http, server.url("/base").toString(), "secret")) {
                assertEquals("7300", api.connect(" @creator ").get(5, TimeUnit.SECONDS).roomId());
                var request = server.takeRequest(5, TimeUnit.SECONDS);
                assertEquals("/v1/connect", request.getPath());
                assertEquals("Bearer secret", request.getHeader("Authorization"));
                assertEquals("{\"uniqueId\":\"creator\"}", request.getBody().readUtf8());
                server.enqueue(
                        new MockResponse()
                                .setResponseCode(429)
                                .setHeader("x-request-id", "id")
                                .setBody(
                                        "{\"error\":{\"code\":\"RATE_LIMITED\",\"retryAfterMs\":3000}}"));
                var failure =
                        assertThrows(
                                ExecutionException.class,
                                () -> api.connect("creator").get(5, TimeUnit.SECONDS));
                assertInstanceOf(RateLimitException.class, failure.getCause());
            } finally {
                http.dispatcher().executorService().shutdownNow();
                http.connectionPool().evictAll();
            }
        }
    }

    @Test
    void offlineAndClose() throws Exception {
        try (var server = new MockWebServer()) {
            server.enqueue(
                    new MockResponse()
                            .setBody(
                                    "{\"version\":1,\"uniqueId\":\"creator\",\"status\":\"offline\"}"));
            var live = TikTokLive.builder("@creator").apiUrl(server.url("/").toString()).build();
            assertEquals(
                    ConnectionStatus.OFFLINE, live.connect().get(5, TimeUnit.SECONDS).status());
            live.close();
            live.close();
            assertTrue(live.connect().isCompletedExceptionally());
        }
    }

    @Test
    void retryAfterIsRespectedAndNetworkFailureIsNotOffline() throws Exception {
        try (var server = new MockWebServer()) {
            server.enqueue(
                    new MockResponse()
                            .setResponseCode(429)
                            .setBody(
                                    "{\"error\":{\"code\":\"RATE_LIMITED\",\"retryable\":true,\"retryAfterMs\":150}}"));
            server.enqueue(
                    new MockResponse()
                            .setBody(
                                    "{\"version\":1,\"uniqueId\":\"creator\",\"status\":\"offline\"}"));
            try (var live =
                    TikTokLive.builder("creator")
                            .apiUrl(server.url("/").toString())
                            .reconnectPolicy(
                                    new ReconnectPolicy(
                                            1,
                                            java.time.Duration.ofMillis(1),
                                            java.time.Duration.ofMillis(2)))
                            .build()) {
                assertNull(live.state().status());
                long start = System.nanoTime();
                assertEquals(
                        ConnectionStatus.OFFLINE, live.connect().get(3, TimeUnit.SECONDS).status());
                assertTrue(TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - start) >= 145);
                assertEquals(2, server.getRequestCount());
            }
        }
        try (var server = new MockWebServer()) {
            server.enqueue(new MockResponse().setSocketPolicy(SocketPolicy.DISCONNECT_AT_START));
            var offline = new java.util.concurrent.atomic.AtomicBoolean();
            try (var live =
                    TikTokLive.builder("creator")
                            .apiUrl(server.url("/").toString())
                            .reconnectPolicy(
                                    new ReconnectPolicy(
                                            0,
                                            java.time.Duration.ofMillis(1),
                                            java.time.Duration.ofMillis(2)))
                            .build()) {
                live.onOffline(s -> offline.set(true));
                assertThrows(
                        ExecutionException.class, () -> live.connect().get(3, TimeUnit.SECONDS));
                assertFalse(offline.get());
                assertNull(live.state().status());
            }
        }
    }
}
