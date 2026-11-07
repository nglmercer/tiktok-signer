package io.github.nglmercer.tiktoklive;

import com.fasterxml.jackson.databind.ObjectMapper;

import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * Serialized, warm embedded signer. Guest failures never expose guest text or signed URLs.
 *
 * <p>The bundle runs in QuickJS — the Rust {@code ttl-sign-mobile} engine, reached through
 * {@link TikTokSigner} — which is the same engine and sandbox every other part of this project
 * signs with. Each engine call runs under the signer's deadline, so a wedged bundle is interrupted
 * inside QuickJS rather than left holding the worker.
 */
public final class QuickJsSigner implements AutoCloseable {
    public enum Product {
        WS(SignProduct.WS),
        FETCH(SignProduct.FETCH),
        FRONTIER(SignProduct.FRONTIER);

        private final SignProduct wire;

        Product(SignProduct wire) {
            this.wire = wire;
        }
    }

    public record SignedUrl(String url) {
        @Override
        public String toString() {
            return "SignedUrl[redacted]";
        }
    }

    private static final ObjectMapper JSON = new ObjectMapper();
    private final String bundle;
    private final boolean pinned;
    private String cookie = "";
    private String userAgent = DirectSocketUrlBuilder.USER_AGENT;
    private final long timeoutMillis;
    private final ThreadPoolExecutor worker;
    private final ScheduledExecutorService timer;
    private final ConcurrentHashMap<CompletableFuture<SignedUrl>, Boolean> pending =
            new ConcurrentHashMap<>();
    private final AtomicBoolean closed = new AtomicBoolean();
    private volatile TikTokSigner engine;
    private volatile CompletableFuture<SignedUrl> executing;

    boolean isClosed() {
        return closed.get();
    }

    /** Production profiles always use fresh entropy and an unfrozen clock. */
    public QuickJsSigner(String bundle) {
        this(verified(bundle), false, Duration.ofSeconds(30), 16);
    }

    private static String verified(String bundle) {
        try {
            return SigningBundleManager.verify(bundle.getBytes(StandardCharsets.UTF_8));
        } catch (java.io.IOException failure) {
            throw new IllegalArgumentException("Unverified signing bundle");
        }
    }

    QuickJsSigner(String bundle, String cookie, String userAgent) {
        this(bundle);
        this.cookie = cookie;
        this.userAgent = userAgent;
    }

    // Determinism is intentionally available only to package tests.
    QuickJsSigner(String bundle, boolean pinned, Duration timeout, int capacity) {
        if (bundle == null
                || bundle.isBlank()
                || timeout.isNegative()
                || timeout.isZero()
                || capacity < 1)
            throw new IllegalArgumentException("Invalid signing configuration");
        this.bundle = bundle;
        this.pinned = pinned;
        // The native engine accepts 1..600000 ms per call.
        this.timeoutMillis = Math.min(600_000, Math.max(1, timeout.toMillis()));
        worker =
                new ThreadPoolExecutor(
                        1,
                        1,
                        0,
                        TimeUnit.MILLISECONDS,
                        new ArrayBlockingQueue<>(capacity),
                        r -> daemon(r, "ttl-quickjs-signer"),
                        new ThreadPoolExecutor.AbortPolicy());
        var deadlines = new ScheduledThreadPoolExecutor(1, r -> daemon(r, "ttl-quickjs-timeout"));
        deadlines.setRemoveOnCancelPolicy(true);
        timer = deadlines;
    }

    private static Thread daemon(Runnable r, String name) {
        Thread t = new Thread(r, name);
        t.setDaemon(true);
        return t;
    }

    public CompletableFuture<SignedUrl> sign(String url, Product product) {
        if (url == null || product == null)
            throw new IllegalArgumentException("URL and product required");
        var result = new CompletableFuture<SignedUrl>();
        if (closed.get()) {
            result.completeExceptionally(new IllegalStateException("Signer closed"));
            return result;
        }
        pending.put(result, true);
        try {
            var deadline =
                    timer.schedule(
                            () -> {
                                if (result.completeExceptionally(
                                        new TimeoutException(
                                                "Signing deadline exceeded; signer retired")))
                                    close();
                            },
                            timeoutMillis,
                            TimeUnit.MILLISECONDS);
            result.whenComplete(
                    (v, e) -> {
                        deadline.cancel(false);
                        pending.remove(result);
                        if (result.isCancelled() && executing == result) close();
                    });
            worker.execute(
                    () -> {
                        if (result.isDone() || closed.get()) return;
                        executing = result;
                        if (result.isDone()) {
                            executing = null;
                            return;
                        }
                        try {
                            String signed = initialize().sign(url, product.wire);
                            if (signed == null || signed.isEmpty())
                                throw new IllegalStateException();
                            result.complete(new SignedUrl(signed));
                        } catch (SignerUnavailableException unavailable) {
                            // Names the missing native library, never guest text.
                            closed.set(true);
                            result.completeExceptionally(unavailable);
                            retire();
                        } catch (Throwable failure) {
                            closed.set(true);
                            result.completeExceptionally(
                                    new IllegalStateException(
                                            "Embedded JavaScript signing failed"));
                            retire();
                        } finally {
                            executing = null;
                        }
                    });
        } catch (RejectedExecutionException e) {
            pending.remove(result);
            result.completeExceptionally(new IllegalStateException("Signer closed or queue full"));
        }
        return result;
    }

    private TikTokSigner initialize() throws Exception {
        TikTokSigner active = engine;
        if (active != null) return active;
        var options = new LinkedHashMap<String, Object>();
        options.put("pinned", pinned);
        options.put("cookie", cookie);
        options.put("userAgent", userAgent);
        options.put("timeoutMs", timeoutMillis);
        TikTokSigner created = TikTokSigner.open(bundle, JSON.writeValueAsString(options));
        engine = created;
        if (closed.get()) {
            created.close();
            throw new IllegalStateException();
        }
        return created;
    }

    @Override
    public void close() {
        if (!closed.compareAndSet(false, true)) return;
        retire();
    }

    private void retire() {
        pending.keySet()
                .forEach(f -> f.completeExceptionally(new CancellationException("Signer closed")));
        worker.shutdownNow();
        timer.shutdownNow();
        TikTokSigner active = engine;
        if (active != null) {
            // Interrupt first so a running signature returns promptly; then release the engine.
            active.interrupt();
            active.close();
        }
    }
}
