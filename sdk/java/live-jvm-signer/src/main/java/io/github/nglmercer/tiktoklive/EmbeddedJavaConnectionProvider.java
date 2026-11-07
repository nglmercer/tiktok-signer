package io.github.nglmercer.tiktoklive;

import com.fasterxml.jackson.databind.*;

import okhttp3.*;

import java.nio.file.Path;
import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;

/** Direct discovery and embedded signing. Owns its HTTP client, worker and signer. */
public final class EmbeddedJavaConnectionProvider implements ConnectionProvider {
    private static final ObjectMapper JSON = new ObjectMapper();
    private final OkHttpClient http;
    private final SigningBundleManager bundles;
    private final Path localBundle;
    private final String discoveryEndpoint;
    private final ThreadPoolExecutor worker;
    private final Set<CompletableFuture<ConnectResponse>> pending = ConcurrentHashMap.newKeySet();
    private final Map<CompletableFuture<ConnectResponse>, Call> calls = new ConcurrentHashMap<>();
    private volatile boolean closed;
    private GuestSessionProvider.Session session;
    private QuickJsSigner signer;

    private EmbeddedJavaConnectionProvider(Builder b) {
        bundles = new SigningBundleManager(b.cache);
        localBundle = b.local;
        discoveryEndpoint = b.discoveryEndpoint;
        session = b.cookies == null ? null : GuestSessionProvider.fromCookies(b.cookies);
        http =
                new OkHttpClient.Builder()
                        .callTimeout(Duration.ofSeconds(30))
                        .followRedirects(false)
                        .followSslRedirects(false)
                        .build();
        worker =
                new ThreadPoolExecutor(
                        1,
                        1,
                        0,
                        TimeUnit.MILLISECONDS,
                        new ArrayBlockingQueue<>(16),
                        r -> {
                            Thread t = new Thread(r, "ttl-direct-connect");
                            t.setDaemon(true);
                            return t;
                        },
                        new ThreadPoolExecutor.AbortPolicy());
    }

    @Override
    public boolean requiresRoomConfirmation() {
        return true;
    }

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private Path cache = Path.of(System.getProperty("user.home"), ".cache", "tiktok-live");
        private Path local;
        private String cookies;
        private String discoveryEndpoint = "https://www.tiktok.com/api-live/user/room/";

        Builder discoveryEndpoint(String endpoint) {
            discoveryEndpoint = endpoint;
            return this;
        }

        public Builder cacheDirectory(Path directory) {
            cache = Objects.requireNonNull(directory);
            return this;
        }

        public Builder localBundle(Path path) {
            local = Objects.requireNonNull(path);
            return this;
        }

        public Builder sessionCookies(String cookies) {
            this.cookies = Objects.requireNonNull(cookies);
            return this;
        }

        public EmbeddedJavaConnectionProvider build() {
            return new EmbeddedJavaConnectionProvider(this);
        }
    }

    public CompletableFuture<ConnectResponse> connect(String input) {
        String id = UniqueId.normalize(input);
        var result = new CompletableFuture<ConnectResponse>();
        synchronized (this) {
            if (closed)
                return CompletableFuture.failedFuture(new IllegalStateException("Provider closed"));
            pending.add(result);
        }
        result.whenComplete(
                (v, e) -> {
                    pending.remove(result);
                    Call call = calls.remove(result);
                    if (call != null && result.isCancelled()) call.cancel();
                });
        try {
            worker.execute(() -> prepare(id, result));
        } catch (RejectedExecutionException e) {
            result.completeExceptionally(
                    new IllegalStateException("Connection queue full or provider closed"));
        }
        return result;
    }

    private void track(CompletableFuture<ConnectResponse> result, Call call) {
        calls.put(result, call);
        if (closed || result.isDone()) {
            calls.remove(result, call);
            call.cancel();
        }
    }

    private void prepare(String id, CompletableFuture<ConnectResponse> result) {
        if (closed || result.isDone()) return;
        try {
            var url =
                    HttpUrl.get(discoveryEndpoint)
                            .newBuilder()
                            .addQueryParameter("aid", "1988")
                            .addQueryParameter("sourceType", "54")
                            .addQueryParameter("uniqueId", id)
                            .build();
            var request =
                    new Request.Builder()
                            .url(url)
                            .header("User-Agent", DirectSocketUrlBuilder.USER_AGENT)
                            .header("Referer", "https://www.tiktok.com/");
            if (session != null) request.header("Cookie", session.cookies());
            Call call = http.newCall(request.build());
            track(result, call);
            LivePresence presence;
            try (Response response = call.execute()) {
                int code = response.code();
                if (code == 401 || code == 403)
                    throw new AuthenticationException(
                            "TikTok discovery requires authentication or verification");
                if (code == 429)
                    throw new RateLimitException(
                            "RATE_LIMITED",
                            "TikTok discovery rate limited",
                            true,
                            60000L,
                            code,
                            null);
                if (!response.isSuccessful())
                    throw new DiscoveryException("TikTok discovery HTTP " + code);
                byte[] bytes =
                        response.body() == null
                                ? new byte[0]
                                : response.body().byteStream().readNBytes(2 * 1024 * 1024 + 1);
                if (bytes.length > 2 * 1024 * 1024)
                    throw new DiscoveryException("Discovery response exceeds limit");
                String body = new String(bytes, java.nio.charset.StandardCharsets.UTF_8);
                JsonNode root;
                try {
                    root = JSON.readTree(body);
                } catch (Exception malformed) {
                    throw new DiscoveryException("Malformed discovery JSON");
                }
                if (root == null) throw new DiscoveryException("Empty discovery response");
                if (root.has("captcha")
                        || root.has("challenge")
                        || root.has("verify_center_decision_conf"))
                    throw new AuthenticationException("TikTok verification challenge");
                if (root.path("status_code").asInt() == 10202)
                    throw new DiscoveryException(
                            "USER_NOT_FOUND", "Creator not found", false, null, code, null);
                presence = RoomLookupProbe.parse(id, code, body);
                if ((root.path("data").path("user").path("status").asInt(-1) == 2
                                || root.path("data").path("liveRoom").path("status").asInt(-1) == 2)
                        && (presence.roomId().isEmpty() || presence.roomId().equals("0")))
                    throw new DiscoveryException(
                            "LIVE discovery returned no usable room identifier");
            }
            if (presence.status() == PresenceStatus.UNKNOWN)
                throw new DiscoveryException("Unknown TikTok discovery response");
            if (presence.status() == PresenceStatus.OFFLINE) {
                result.complete(
                        new ConnectResponse(
                                1, id, presence.roomId(), ConnectionStatus.OFFLINE, null));
                return;
            }
            if (closed || result.isDone()) return;
            if (session == null)
                session = GuestSessionProvider.bootstrap(http, c -> track(result, c));
            if (closed || result.isDone()) return;
            if (signer == null || signer.isClosed()) {
                String bundle =
                        localBundle == null ? bundles.load() : bundles.loadLocal(localBundle);
                synchronized (this) {
                    if (closed || result.isDone()) return;
                    signer =
                            new QuickJsSigner(
                                    bundle, session.cookies(), DirectSocketUrlBuilder.USER_AGENT);
                }
            }
            var identity = session;
            String raw = DirectSocketUrlBuilder.rawUrl(presence.roomId(), identity.deviceId());
            var signing = signer.sign(raw, QuickJsSigner.Product.WS);
            result.whenComplete(
                    (v, e) -> {
                        if (result.isCancelled()) signing.cancel(false);
                    });
            signing.whenComplete(
                    (signed, failure) -> {
                        if (result.isDone() || closed) return;
                        if (failure != null)
                            result.completeExceptionally(
                                    new SignerUnavailableException(
                                            "Embedded signer failed; recreate provider"));
                        else {
                            try {
                                String safe = DirectSocketUrlBuilder.validated(signed.url(), raw);
                                result.complete(
                                        new ConnectResponse(
                                                1,
                                                id,
                                                presence.roomId(),
                                                ConnectionStatus.LIVE,
                                                new ConnectionDescriptor(
                                                        safe,
                                                        identity.cookies(),
                                                        DirectSocketUrlBuilder.USER_AGENT)));
                            } catch (Exception invalid) {
                                result.completeExceptionally(
                                        new IllegalStateException(
                                                "Invalid signed connection descriptor"));
                            }
                        }
                    });
        } catch (TikTokLiveException e) {
            result.completeExceptionally(e);
        } catch (Exception e) {
            result.completeExceptionally(
                    new DiscoveryException("Direct connection preparation failed"));
        }
    }

    public synchronized void close() {
        if (closed) return;
        closed = true;
        pending.forEach(f -> f.cancel(false));
        calls.values().forEach(Call::cancel);
        calls.clear();
        worker.shutdownNow();
        if (signer != null) signer.close();
        session = null;
        http.dispatcher().executorService().shutdownNow();
        http.connectionPool().evictAll();
    }
}
