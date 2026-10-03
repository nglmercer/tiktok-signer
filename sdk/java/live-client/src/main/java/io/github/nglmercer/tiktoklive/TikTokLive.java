package io.github.nglmercer.tiktoklive;

import io.github.nglmercer.tiktoklive.internal.*;
import io.github.nglmercer.tiktoklive.internal.Protocol;

import okhttp3.*;

import okio.ByteString;

import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;
import java.util.function.Consumer;

/** Asynchronous broker-backed connection. Callbacks always cross an internal daemon executor. */
public final class TikTokLive implements AutoCloseable {
    private final String uniqueId;
    private final OkHttpClient http;
    private final boolean ownHttp, fetchGifts;
    private final ConnectApi api;
    private final ReconnectPolicy policy;
    private final ScheduledExecutorService timers;
    private final ThreadPoolExecutor events;
    private final Executor callback;
    private final Map<String, CopyOnWriteArrayList<Consumer<Object>>> listeners =
            new ConcurrentHashMap<>();
    private final GiftStreakTracker streaks = new GiftStreakTracker();
    private Map<String, GiftCatalog.Gift> gifts = Map.of();
    private ScheduledFuture<?> heartbeat, reconnect;
    private CompletableFuture<LiveState> pending;
    private WebSocket socket;
    private long generation, heartbeatInterval;
    private final java.util.concurrent.atomic.AtomicLong droppedCallbacks =
            new java.util.concurrent.atomic.AtomicLong();
    private int retries;
    private boolean active;
    private volatile boolean closed;
    private volatile LiveState state;

    private TikTokLive(Builder b) {
        uniqueId = b.id;
        ownHttp = b.http == null;
        fetchGifts = b.fetchGifts;
        policy = b.policy;
        callback = b.callback;
        http =
                ownHttp
                        ? new OkHttpClient.Builder()
                                .dispatcher(
                                        new Dispatcher(
                                                Executors.newCachedThreadPool(
                                                        daemon("tiktok-http"))))
                                .callTimeout(b.timeout)
                                .followRedirects(false)
                                .followSslRedirects(false)
                                .build()
                        : b.http;
        api = new ConnectApi(http, b.url, b.key);
        timers = Executors.newSingleThreadScheduledExecutor(daemon("tiktok-timers"));
        events =
                new ThreadPoolExecutor(
                        1,
                        1,
                        0,
                        TimeUnit.MILLISECONDS,
                        new ArrayBlockingQueue<>(1024),
                        daemon("tiktok-events"),
                        new ThreadPoolExecutor.AbortPolicy());
        state = new LiveState(uniqueId, "", null, false);
    }

    private static ThreadFactory daemon(String name) {
        return r -> {
            Thread t = new Thread(r, name);
            t.setDaemon(true);
            return t;
        };
    }

    public static Builder builder(String uniqueId) {
        return new Builder(uniqueId);
    }

    public LiveState state() {
        return state;
    }

    /** Number of callback tasks dropped because the application stalled delivery. */
    public long droppedCallbacks() {
        return droppedCallbacks.get();
    }

    public synchronized CompletableFuture<LiveState> connect() {
        if (closed)
            return CompletableFuture.failedFuture(new IllegalStateException("Client closed"));
        if (active)
            return pending != null && !pending.isDone()
                    ? pending
                    : CompletableFuture.completedFuture(state);
        active = true;
        retries = 0;
        long token = ++generation;
        pending = new CompletableFuture<>();
        var result = pending;
        result.whenComplete(
                (v, e) -> {
                    if (result.isCancelled()) {
                        synchronized (this) {
                            if (pending == result) disconnect();
                        }
                    }
                });
        request(token);
        return result;
    }

    private void request(long token) {
        api.connect(uniqueId)
                .whenComplete(
                        (response, error) -> {
                            synchronized (this) {
                                if (!valid(token)) return;
                                if (error != null) {
                                    fail(token, unwrap(error));
                                    return;
                                }
                                gifts = Map.of();
                                streaks.clear();
                                if (response.status() == ConnectionStatus.OFFLINE) {
                                    active = false;
                                    state =
                                            new LiveState(
                                                    uniqueId,
                                                    response.roomId(),
                                                    ConnectionStatus.OFFLINE,
                                                    false);
                                    complete(state, null);
                                    emit("offline", state);
                                    return;
                                }
                                var descriptor = response.connection();
                                if (fetchGifts) {
                                    GiftCatalog.load(api, response.roomId(), descriptor)
                                            .whenComplete(
                                                    (catalog, failure) -> {
                                                        synchronized (this) {
                                                            if (!valid(token)) return;
                                                            if (failure == null) gifts = catalog;
                                                            else emit("error", unwrap(failure));
                                                            open(token, response);
                                                        }
                                                    });
                                } else open(token, response);
                            }
                        });
    }

    private boolean valid(long token) {
        return !closed && active && generation == token;
    }

    private void open(long token, ConnectResponse response) {
        try {
            var request =
                    new Request.Builder()
                            .url(response.connection().url())
                            .header("Cookie", response.connection().cookies())
                            .header("User-Agent", response.connection().userAgent())
                            .build();
            socket =
                    http.newWebSocket(
                            request,
                            new WebSocketListener() {
                                public void onOpen(WebSocket ws, Response upgrade) {
                                    synchronized (TikTokLive.this) {
                                        if (!valid(token)) {
                                            ws.cancel();
                                            return;
                                        }
                                        socket = ws;
                                        if (!ws.send(
                                                ByteString.of(Protocol.enter(response.roomId())))) {
                                            fail(
                                                    token,
                                                    new SocketException(
                                                            "EnterRoom could not be sent"));
                                            return;
                                        }
                                        state =
                                                new LiveState(
                                                        uniqueId,
                                                        response.roomId(),
                                                        ConnectionStatus.LIVE,
                                                        true);
                                        retries = 0;
                                        startHeartbeat(token, ws, response.roomId(), 10000);
                                        complete(state, null);
                                        emit("connected", state);
                                    }
                                }

                                public void onMessage(WebSocket ws, ByteString bytes) {
                                    receive(token, ws, bytes.toByteArray());
                                }

                                public void onClosing(WebSocket ws, int code, String reason) {
                                    ws.close(code, reason);
                                }

                                public void onClosed(WebSocket ws, int code, String reason) {
                                    synchronized (TikTokLive.this) {
                                        if (valid(token))
                                            fail(
                                                    token,
                                                    new SocketException(
                                                            "Socket closed ("
                                                                    + code
                                                                    + "): "
                                                                    + reason));
                                    }
                                }

                                public void onFailure(
                                        WebSocket ws, Throwable error, Response response) {
                                    synchronized (TikTokLive.this) {
                                        if (valid(token)) {
                                            var e =
                                                    new SocketException(
                                                            "WebSocket failed: "
                                                                    + error.getMessage());
                                            e.initCause(error);
                                            fail(token, e);
                                        }
                                    }
                                }
                            });
        } catch (Exception e) {
            fail(token, new SocketException("Cannot open WebSocket: " + e.getMessage()));
        }
    }

    private void receive(long token, WebSocket ws, byte[] bytes) {
        synchronized (this) {
            if (!valid(token) || socket != ws) return;
        }
        try {
            if (bytes.length > 16 * 1024 * 1024) throw new java.io.IOException("Frame too large");
            var frame = Protocol.decode(bytes);
            if (!frame.getPayloadType().equals("msg")) return;
            var batch = Protocol.batch(frame);
            // ACK runs before event normalization and before any application executor.
            if (!ws.send(ByteString.of(Protocol.ack(frame, batch.getInternalExt())))) {
                synchronized (this) {
                    if (valid(token)) fail(token, new SocketException("ACK could not be sent"));
                }
                return;
            }
            synchronized (this) {
                if (!valid(token)) return;
                if (batch.getHeartbeatDuration() > 0)
                    startHeartbeat(
                            token,
                            ws,
                            state.roomId(),
                            Math.max(1000, Math.min(60000, batch.getHeartbeatDuration())));
                for (var message : batch.getMessagesList()) {
                    LiveEvent event = EventDecoder.decode(message);
                    if (event instanceof GiftEvent gift) event = GiftCatalog.enrich(gift, gifts);
                    emit("event", event);
                    emit(event.type(), event);
                    if (event instanceof GiftEvent gift)
                        streaks.accept(gift).ifPresent(finalGift -> emit("giftFinal", finalGift));
                }
            }
        } catch (Exception e) {
            emit("error", new ProtocolException("Cannot decode frame: " + e.getMessage()));
        }
    }

    private void startHeartbeat(long token, WebSocket ws, String room, long interval) {
        if (heartbeat != null && heartbeatInterval == interval) return;
        heartbeatInterval = interval;
        if (heartbeat != null) heartbeat.cancel(false);
        heartbeat =
                timers.scheduleAtFixedRate(
                        () -> {
                            synchronized (this) {
                                if (valid(token)
                                        && !ws.send(ByteString.of(Protocol.heartbeat(room))))
                                    fail(token, new SocketException("Heartbeat could not be sent"));
                            }
                        },
                        interval,
                        interval,
                        TimeUnit.MILLISECONDS);
    }

    private void fail(long token, Throwable error) {
        if (!valid(token)) return;
        ++generation;
        stopSocket();
        state = new LiveState(uniqueId, state.roomId(), state.status(), false);
        emit("error", error);
        emit("disconnected", state);
        boolean retry = !(error instanceof TikTokLiveException e) || e.retryable();
        if (!retry || retries >= policy.attempts()) {
            active = false;
            if (!pending.isDone()) complete(null, error);
            return;
        }
        Long after = error instanceof TikTokLiveException e ? e.retryAfterMs() : null;
        Duration delay = policy.delay(++retries, ThreadLocalRandom.current().nextDouble(), after);
        long next = generation;
        emit("reconnecting", new Reconnecting(retries, delay));
        reconnect =
                timers.schedule(
                        () -> {
                            synchronized (this) {
                                if (valid(next)) request(next);
                            }
                        },
                        delay.toMillis(),
                        TimeUnit.MILLISECONDS);
    }

    public record Reconnecting(int attempt, Duration delay) {}

    private static Throwable unwrap(Throwable e) {
        return e instanceof CompletionException && e.getCause() != null ? e.getCause() : e;
    }

    private void complete(LiveState value, Throwable failure) {
        CompletableFuture<LiveState> future = pending;
        CompletableFuture.runAsync(
                () -> {
                    if (failure == null) future.complete(value);
                    else future.completeExceptionally(failure);
                });
    }

    private void stopSocket() {
        if (heartbeat != null) {
            heartbeat.cancel(false);
            heartbeat = null;
        }
        heartbeatInterval = 0;
        if (reconnect != null) {
            reconnect.cancel(false);
            reconnect = null;
        }
        WebSocket old = socket;
        socket = null;
        if (old != null) {
            old.close(1000, "disconnect");
            old.cancel();
        }
    }

    public synchronized void disconnect() {
        if (closed) return;
        boolean wasActive = active;
        active = false;
        ++generation;
        stopSocket();
        api.cancelPending();
        gifts = Map.of();
        streaks.clear();
        if (pending != null && !pending.isDone()) pending.cancel(false);
        state = new LiveState(uniqueId, state.roomId(), state.status(), false);
        if (wasActive) emit("disconnected", state);
    }

    public synchronized void close() {
        if (closed) return;
        disconnect();
        closed = true;
        api.close();
        timers.shutdownNow();
        events.shutdownNow();
        listeners.clear();
        if (ownHttp) {
            http.dispatcher().executorService().shutdownNow();
            http.connectionPool().evictAll();
        }
    }

    @SuppressWarnings("unchecked")
    private <T> TikTokLive listen(String name, Consumer<T> listener) {
        Objects.requireNonNull(listener);
        listeners
                .computeIfAbsent(name, n -> new CopyOnWriteArrayList<>())
                .add(value -> listener.accept((T) value));
        return this;
    }

    private void emit(String name, Object value) {
        if (closed) return;
        try {
            events.execute(
                    () -> {
                        if (closed) return;
                        for (var listener :
                                listeners.getOrDefault(name, new CopyOnWriteArrayList<>())) {
                            Runnable invocation =
                                    () -> {
                                        if (closed) return;
                                        try {
                                            listener.accept(value);
                                        } catch (Throwable e) {
                                            if (!name.equals("error")) emit("error", e);
                                        }
                                    };
                            try {
                                if (callback == null) invocation.run();
                                else callback.execute(invocation);
                            } catch (RuntimeException e) {
                                if (!name.equals("error")) emit("error", e);
                            }
                        }
                    });
        } catch (RejectedExecutionException overflow) {
            droppedCallbacks.incrementAndGet();
            // Bound retained events when a consumer stalls. Protocol acknowledgements continue.
            // Applications needing lossless delivery must keep callbacks short and forward work.
        }
    }

    public TikTokLive onChat(Consumer<ChatEvent> l) {
        return listen("chat", l);
    }

    public TikTokLive onGift(Consumer<GiftEvent> l) {
        return listen("gift", l);
    }

    public TikTokLive onGiftFinal(Consumer<GiftEvent> l) {
        return listen("giftFinal", l);
    }

    public TikTokLive onLike(Consumer<LikeEvent> l) {
        return listen("like", l);
    }

    public TikTokLive onMember(Consumer<MemberEvent> l) {
        return listen("member", l);
    }

    public TikTokLive onSocial(Consumer<SocialEvent> l) {
        return listen("social", l);
    }

    public TikTokLive onRoomUser(Consumer<RoomUserEvent> l) {
        return listen("roomUser", l);
    }

    public TikTokLive onUnknown(Consumer<UnknownEvent> l) {
        return listen("unknown", l);
    }

    public TikTokLive onEvent(Consumer<LiveEvent> l) {
        return listen("event", l);
    }

    public TikTokLive onConnected(Consumer<LiveState> l) {
        return listen("connected", l);
    }

    public TikTokLive onOffline(Consumer<LiveState> l) {
        return listen("offline", l);
    }

    public TikTokLive onDisconnected(Consumer<LiveState> l) {
        return listen("disconnected", l);
    }

    public TikTokLive onReconnecting(Consumer<Reconnecting> l) {
        return listen("reconnecting", l);
    }

    public TikTokLive onError(Consumer<Throwable> l) {
        return listen("error", l);
    }

    public static final class Builder {
        private final String id;
        private String url = "http://127.0.0.1:8080", key;
        private OkHttpClient http;
        private Executor callback;
        private boolean fetchGifts;
        private Duration timeout = Duration.ofSeconds(10);
        private ReconnectPolicy policy = ReconnectPolicy.DEFAULT;

        private Builder(String id) {
            this.id = UniqueId.normalize(id);
        }

        public Builder apiUrl(String url) {
            HttpUrl.get(url);
            this.url = url;
            return this;
        }

        public Builder apiKey(String key) {
            this.key = key;
            return this;
        }

        public Builder callbackExecutor(Executor executor) {
            callback = Objects.requireNonNull(executor);
            return this;
        }

        public Builder httpClient(OkHttpClient client) {
            http = Objects.requireNonNull(client);
            return this;
        }

        public Builder fetchGifts(boolean enabled) {
            fetchGifts = enabled;
            return this;
        }

        public Builder reconnectPolicy(ReconnectPolicy policy) {
            this.policy = Objects.requireNonNull(policy);
            return this;
        }

        public Builder timeout(Duration timeout) {
            if (timeout.isNegative() || timeout.isZero())
                throw new IllegalArgumentException("Positive timeout required");
            this.timeout = timeout;
            return this;
        }

        public TikTokLive build() {
            return new TikTokLive(this);
        }
    }
}
