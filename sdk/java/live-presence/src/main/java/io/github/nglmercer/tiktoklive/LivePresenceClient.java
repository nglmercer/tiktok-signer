package io.github.nglmercer.tiktoklive;

import okhttp3.*;

import java.time.Duration;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.*;

public final class LivePresenceClient implements PresenceProbe, AutoCloseable {
    private final PresenceProbe probe;
    private final boolean ownHttp, ownProbe;
    private final OkHttpClient http;
    private final Set<CompletableFuture<LivePresence>> pending = ConcurrentHashMap.newKeySet();
    private volatile boolean closed;

    private LivePresenceClient(Builder b) {
        ownHttp = b.probe == null && b.http == null;
        ownProbe = b.probe == null;
        http =
                b.probe != null
                        ? null
                        : b.http != null
                                ? b.http
                                : new OkHttpClient.Builder()
                                        .dispatcher(
                                                new Dispatcher(
                                                        Executors.newCachedThreadPool(
                                                                r -> {
                                                                    Thread t =
                                                                            new Thread(
                                                                                    r,
                                                                                    "tiktok-presence-http");
                                                                    t.setDaemon(true);
                                                                    return t;
                                                                })))
                                        .callTimeout(b.timeout)
                                        .followRedirects(false)
                                        .followSslRedirects(false)
                                        .build();
        probe = b.probe == null ? new RoomLookupProbe(http) : b.probe;
    }

    public static LivePresenceClient create() {
        return builder().build();
    }

    public static Builder builder() {
        return new Builder();
    }

    public CompletableFuture<LivePresence> lookup(String input) {
        String id = UniqueId.normalize(input);
        synchronized (this) {
            if (closed)
                return CompletableFuture.failedFuture(new IllegalStateException("Client closed"));
            CompletableFuture<LivePresence> source;
            try {
                source = probe.lookup(id);
            } catch (Exception e) {
                return CompletableFuture.completedFuture(LivePresence.unknown(id));
            }
            var result =
                    source.handle(
                            (value, error) ->
                                    error == null && value != null && id.equals(value.uniqueId())
                                            ? value
                                            : LivePresence.unknown(id));
            pending.add(result);
            result.whenComplete(
                    (v, e) -> {
                        pending.remove(result);
                        if (result.isCancelled()) source.cancel(true);
                    });
            return result;
        }
    }

    public synchronized void close() {
        if (closed) return;
        closed = true;
        pending.forEach(f -> f.cancel(true));
        if (ownProbe && probe instanceof AutoCloseable c)
            try {
                c.close();
            } catch (Exception ignored) {
            }
        if (ownHttp) {
            http.dispatcher().executorService().shutdownNow();
            http.connectionPool().evictAll();
        }
    }

    public static final class Builder {
        private PresenceProbe probe;
        private OkHttpClient http;
        private Duration timeout = Duration.ofSeconds(10);

        public Builder probe(PresenceProbe probe) {
            this.probe = Objects.requireNonNull(probe);
            return this;
        }

        public Builder httpClient(OkHttpClient http) {
            this.http = Objects.requireNonNull(http);
            return this;
        }

        public Builder timeout(Duration timeout) {
            if (timeout.isNegative() || timeout.isZero())
                throw new IllegalArgumentException("Positive timeout required");
            this.timeout = timeout;
            return this;
        }

        public LivePresenceClient build() {
            return new LivePresenceClient(this);
        }
    }
}
