package io.github.nglmercer.tiktoklive.internal;

import static org.junit.jupiter.api.Assertions.*;

import io.github.nglmercer.tiktoklive.*;

import okhttp3.*;
import okhttp3.mockwebserver.*;

import okio.ByteString;

import org.junit.jupiter.api.Test;

import webcast.im.SyntheticProto.*;
import webcast.shared.message.Message.*;

import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

class WebSocketLifecycleTest {
    String descriptor(String signature) {
        return "{\"version\":1,\"uniqueId\":\"creator\",\"roomId\":\"7300\",\"status\":\"live\",\"connection\":{\"url\":\"wss://webcast-ws.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?room_id=7300&X-Gnarly="
                + signature
                + "\",\"cookies\":\"cookie=value\",\"userAgent\":\"broker-agent\"}}";
    }

    @Test
    void ackPrecedesSlowCallbacksAndReconnectIsFresh() throws Exception {
        try (var broker = new MockWebServer();
                var socketServer = new MockWebServer()) {
            var signatures = new CopyOnWriteArrayList<String>();
            var acknowledgements = new CountDownLatch(2);
            var entered = new CountDownLatch(2);
            var openAgain = new CountDownLatch(1);
            var sockets = new CopyOnWriteArrayList<WebSocket>();
            WebSocketListener peer =
                    new WebSocketListener() {
                        public void onOpen(WebSocket socket, Response response) {
                            sockets.add(socket);
                        }

                        public void onMessage(WebSocket socket, ByteString bytes) {
                            try {
                                var frame = Protocol.decode(bytes.toByteArray());
                                if (frame.getPayloadType().equals("im_enter_room")) {
                                    entered.countDown();
                                    var chat =
                                            webcast.model.message.Messages.WebcastChatMessage
                                                    .newBuilder()
                                                    .setContent("hello")
                                                    .build();
                                    var batch =
                                            ProtoMessageFetchResult.newBuilder()
                                                    .setInternalExt("ack-ext")
                                                    .addMessages(
                                                            BaseProtoMessage.newBuilder()
                                                                    .setMethod("WebcastChatMessage")
                                                                    .setPayload(
                                                                            chat.toByteString()))
                                                    .build();
                                    var incoming =
                                            WebcastPushFrame.newBuilder()
                                                    .setLogId(99)
                                                    .setPayloadType("msg")
                                                    .setPayload(batch.toByteString())
                                                    .build();
                                    socket.send(ByteString.of(incoming.toByteArray()));
                                    socket.send(ByteString.of(incoming.toByteArray()));
                                } else if (frame.getPayloadType().equals("ack")) {
                                    assertEquals(99, frame.getLogId());
                                    assertEquals("ack-ext", frame.getPayload().toStringUtf8());
                                    acknowledgements.countDown();
                                }
                            } catch (Exception e) {
                                throw new AssertionError(e);
                            }
                        }
                    };
            broker.enqueue(new MockResponse().setBody(descriptor("first")));
            broker.enqueue(new MockResponse().setBody(descriptor("second")));
            socketServer.enqueue(new MockResponse().withWebSocketUpgrade(peer));
            socketServer.enqueue(new MockResponse().withWebSocketUpgrade(peer));
            var http =
                    new OkHttpClient.Builder()
                            .addInterceptor(
                                    chain -> {
                                        if (chain.request()
                                                .url()
                                                .host()
                                                .equals("webcast-ws.tiktok.com")) {
                                            signatures.add(
                                                    chain.request()
                                                            .url()
                                                            .queryParameter("X-Gnarly"));
                                            var rerouted =
                                                    socketServer
                                                            .url(
                                                                    chain.request()
                                                                            .url()
                                                                            .encodedPath())
                                                            .newBuilder()
                                                            .encodedQuery(
                                                                    chain.request()
                                                                            .url()
                                                                            .encodedQuery())
                                                            .build();
                                            return chain.proceed(
                                                    chain.request()
                                                            .newBuilder()
                                                            .url(rerouted)
                                                            .build());
                                        }
                                        return chain.proceed(chain.request());
                                    })
                            .build();
            var callbackStarted = new CountDownLatch(1);
            var releaseCallback = new CountDownLatch(1);
            var connected = new AtomicInteger();
            try (var live =
                    TikTokLive.builder("creator")
                            .httpClient(http)
                            .apiUrl(broker.url("/").toString())
                            .callbackExecutor(Runnable::run)
                            .reconnectPolicy(
                                    new ReconnectPolicy(
                                            2, Duration.ofMillis(10), Duration.ofMillis(20)))
                            .build()) {
                live.onChat(
                        e -> {
                            callbackStarted.countDown();
                            try {
                                releaseCallback.await(3, TimeUnit.SECONDS);
                            } catch (InterruptedException interrupted) {
                                Thread.currentThread().interrupt();
                            }
                        });
                live.onConnected(
                        s -> {
                            if (connected.incrementAndGet() == 2) openAgain.countDown();
                        });
                assertTrue(live.connect().get(5, TimeUnit.SECONDS).connected());
                assertTrue(callbackStarted.await(3, TimeUnit.SECONDS));
                assertTrue(
                        acknowledgements.await(3, TimeUnit.SECONDS),
                        "ACKs must continue while direct-executor user callback blocks");
                releaseCallback.countDown();
                sockets.get(0).close(1012, "restart");
                assertTrue(openAgain.await(5, TimeUnit.SECONDS));
                assertTrue(entered.await(3, TimeUnit.SECONDS));
                assertEquals(List.of("first", "second"), signatures);
                for (int i = 0; i < 2; i++) {
                    var request = socketServer.takeRequest(3, TimeUnit.SECONDS);
                    assertEquals("cookie=value", request.getHeader("Cookie"));
                    assertEquals("broker-agent", request.getHeader("User-Agent"));
                }
                assertEquals(2, broker.getRequestCount());
                live.disconnect();
                assertFalse(live.state().connected());
            } finally {
                releaseCallback.countDown();
                http.dispatcher().executorService().shutdownNow();
                http.connectionPool().evictAll();
            }
        }
    }

    @Test
    void heartbeatIsNotPostponedByIncomingBatches() throws Exception {
        try (var broker = new MockWebServer();
                var server = new MockWebServer()) {
            var receivedHeartbeat = new CountDownLatch(1);
            var sending = Executors.newSingleThreadScheduledExecutor();
            broker.enqueue(new MockResponse().setBody(descriptor("fresh")));
            server.enqueue(
                    new MockResponse()
                            .withWebSocketUpgrade(
                                    new WebSocketListener() {
                                        public void onOpen(WebSocket ws, Response response) {
                                            var batch =
                                                    ProtoMessageFetchResult.newBuilder()
                                                            .setHeartbeatDuration(1000)
                                                            .build();
                                            var frame =
                                                    WebcastPushFrame.newBuilder()
                                                            .setPayloadType("msg")
                                                            .setPayload(batch.toByteString())
                                                            .build();
                                            sending.scheduleAtFixedRate(
                                                    () ->
                                                            ws.send(
                                                                    ByteString.of(
                                                                            frame.toByteArray())),
                                                    0,
                                                    100,
                                                    TimeUnit.MILLISECONDS);
                                        }

                                        public void onMessage(WebSocket ws, ByteString data) {
                                            try {
                                                if (Protocol.decode(data.toByteArray())
                                                        .getPayloadType()
                                                        .equals("hb"))
                                                    receivedHeartbeat.countDown();
                                            } catch (Exception e) {
                                                throw new AssertionError(e);
                                            }
                                        }
                                    }));
            var http =
                    new OkHttpClient.Builder()
                            .addInterceptor(
                                    chain ->
                                            chain.request()
                                                            .url()
                                                            .host()
                                                            .equals("webcast-ws.tiktok.com")
                                                    ? chain.proceed(
                                                            chain.request()
                                                                    .newBuilder()
                                                                    .url(server.url("/"))
                                                                    .build())
                                                    : chain.proceed(chain.request()))
                            .build();
            try (var live =
                    TikTokLive.builder("creator")
                            .apiUrl(broker.url("/").toString())
                            .httpClient(http)
                            .build()) {
                live.connect().get(3, TimeUnit.SECONDS);
                assertTrue(
                        receivedHeartbeat.await(3, TimeUnit.SECONDS),
                        "Frequent batches must not reset heartbeat timer");
            } finally {
                sending.shutdownNow();
                http.dispatcher().executorService().shutdownNow();
                http.connectionPool().evictAll();
            }
        }
    }

    @Test
    void closeCancelsConnectAndCallerExecutorSurvives() throws Exception {
        try (var broker = new MockWebServer()) {
            broker.enqueue(new MockResponse().setSocketPolicy(SocketPolicy.NO_RESPONSE));
            var callback = Executors.newSingleThreadExecutor();
            var live =
                    TikTokLive.builder("creator")
                            .apiUrl(broker.url("/").toString())
                            .callbackExecutor(callback)
                            .build();
            var future = live.connect();
            assertNotNull(broker.takeRequest(3, TimeUnit.SECONDS));
            live.close();
            assertTrue(future.isCancelled());
            assertFalse(callback.isShutdown());
            callback.shutdownNow();
        }
    }
}
