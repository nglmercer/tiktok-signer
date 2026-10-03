package io.github.nglmercer.tiktoklive;

import java.time.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.function.Consumer;

/** Shared asynchronous scheduler with independent debounce/backoff state for each creator. */
public final class LivePresenceMonitor implements AutoCloseable {
    private final PresenceProbe client;
    private final boolean ownClient;
    private final ScheduledExecutorService scheduler;
    private final boolean ownScheduler;
    private final Executor callback;
    private final ExecutorService events;
    private final Duration offline, live, error, maxError;
    private final int confirmations;
    private final Map<String, Watch> watches = new ConcurrentHashMap<>();
    private volatile boolean closed;
    private final java.util.concurrent.atomic.AtomicLong droppedCallbacks =
            new java.util.concurrent.atomic.AtomicLong();

    private LivePresenceMonitor(Builder b) {
        ownClient = b.client == null;
        client = ownClient ? LivePresenceClient.create() : b.client;
        ownScheduler = b.scheduler == null;
        scheduler =
                ownScheduler
                        ? Executors.newSingleThreadScheduledExecutor(
                                daemon("tiktok-presence-timer"))
                        : b.scheduler;
        callback = b.callback;
        offline = b.offline;
        live = b.live;
        error = b.error;
        maxError = b.maxError;
        confirmations = b.confirmations;
        events =
                new ThreadPoolExecutor(
                        1,
                        1,
                        0,
                        TimeUnit.MILLISECONDS,
                        new ArrayBlockingQueue<>(1024),
                        daemon("tiktok-presence-events"),
                        new ThreadPoolExecutor.AbortPolicy());
    }

    private static ThreadFactory daemon(String name) {
        return r -> {
            Thread t = new Thread(r, name);
            t.setDaemon(true);
            return t;
        };
    }

    public static LivePresenceMonitor create() {
        return builder().build();
    }

    public static Builder builder() {
        return new Builder();
    }

    public synchronized Watch watch(String uniqueId) {
        if (closed) throw new IllegalStateException("Monitor closed");
        String id = UniqueId.normalize(uniqueId);
        return watches.computeIfAbsent(
                id,
                key -> {
                    Watch w = new Watch(key);
                    w.schedule(100);
                    return w;
                });
    }

    public long droppedCallbacks() {
        return droppedCallbacks.get();
    }

    public void unwatch(String uniqueId) {
        Watch watch = watches.get(UniqueId.normalize(uniqueId));
        if (watch != null) watch.close();
    }

    public synchronized void close() {
        if (closed) return;
        closed = true;
        List.copyOf(watches.values()).forEach(Watch::close);
        watches.clear();
        if (ownScheduler) scheduler.shutdownNow();
        events.shutdownNow();
        if (ownClient && client instanceof AutoCloseable c)
            try {
                c.close();
            } catch (Exception ignored) {
            }
    }

    public record WatchState(
            PresenceStatus confirmed,
            PresenceStatus candidate,
            int confirmationCount,
            Instant lastSuccessfulRequest,
            int consecutiveFailures,
            Instant nextPoll) {}

    public final class Watch implements AutoCloseable {
        private final String id;
        private final PresenceConfirmation confirmation = new PresenceConfirmation(confirmations);
        private final List<Consumer<PresenceTransition>> started = new CopyOnWriteArrayList<>(),
                ended = new CopyOnWriteArrayList<>();
        private ScheduledFuture<?> timer;
        private CompletableFuture<LivePresence> request;
        private boolean stopped;
        private Instant lastSuccess, nextPoll;

        private Watch(String id) {
            this.id = id;
        }

        public String uniqueId() {
            return id;
        }

        public Watch onLiveStarted(Consumer<PresenceTransition> listener) {
            started.add(Objects.requireNonNull(listener));
            return this;
        }

        public Watch onLiveEnded(Consumer<PresenceTransition> listener) {
            ended.add(Objects.requireNonNull(listener));
            return this;
        }

        public synchronized WatchState state() {
            return new WatchState(
                    confirmation.confirmed,
                    confirmation.candidate,
                    confirmation.count,
                    lastSuccess,
                    confirmation.failures,
                    nextPoll);
        }

        private synchronized void schedule(long delay) {
            if (stopped || closed) return;
            nextPoll = Instant.now().plusMillis(delay);
            timer = scheduler.schedule(this::poll, delay, TimeUnit.MILLISECONDS);
        }

        private synchronized void poll() {
            if (stopped || closed) return;
            CompletableFuture<LivePresence> future;
            try {
                future = Objects.requireNonNull(client.lookup(id));
            } catch (Exception e) {
                future = CompletableFuture.completedFuture(LivePresence.unknown(id));
            }
            request = future;
            future.whenComplete(
                    (result, failure) -> {
                        synchronized (this) {
                            if (stopped || closed) return;
                            PresenceStatus status =
                                    failure == null
                                                    && result != null
                                                    && id.equals(result.uniqueId())
                                            ? result.status()
                                            : PresenceStatus.UNKNOWN;
                            PresenceStatus transition = confirmation.accept(status);
                            if (status != PresenceStatus.UNKNOWN) lastSuccess = Instant.now();
                            if (transition != null) notifyListeners(transition, result);
                            long base =
                                    status == PresenceStatus.UNKNOWN
                                            ? (long)
                                                    Math.min(
                                                            maxError.toMillis(),
                                                            error.toMillis()
                                                                    * Math.pow(
                                                                            2,
                                                                            Math.min(
                                                                                    30,
                                                                                    confirmation
                                                                                                    .failures
                                                                                            - 1)))
                                            : confirmation.confirmed == PresenceStatus.LIVE
                                                            || confirmation.candidate
                                                                    == PresenceStatus.LIVE
                                                    ? live.toMillis()
                                                    : offline.toMillis();
                            schedule(
                                    Math.max(
                                            1,
                                            (long)
                                                    (base
                                                            * (.8
                                                                    + ThreadLocalRandom.current()
                                                                                    .nextDouble()
                                                                            * .4))));
                        }
                    });
        }

        private void notifyListeners(PresenceStatus transition, LivePresence result) {
            var listeners = transition == PresenceStatus.LIVE ? started : ended;
            var event = new PresenceTransition(id, result);
            try {
                events.execute(
                        () -> {
                            for (var listener : listeners) {
                                Runnable invocation =
                                        () -> {
                                            synchronized (this) {
                                                if (stopped || closed) return;
                                            }
                                            try {
                                                listener.accept(event);
                                            } catch (RuntimeException ignored) {
                                            }
                                        };
                                try {
                                    if (callback == null) invocation.run();
                                    else callback.execute(invocation);
                                } catch (RuntimeException ignored) {
                                }
                            }
                        });
            } catch (RejectedExecutionException ignored) {
                droppedCallbacks.incrementAndGet();
            }
        }

        public void close() {
            synchronized (this) {
                if (stopped) return;
                stopped = true;
                if (timer != null) timer.cancel(false);
                if (request != null) request.cancel(true);
                started.clear();
                ended.clear();
            }
            watches.remove(id, this);
        }
    }

    public static final class Builder {
        private PresenceProbe client;
        private ScheduledExecutorService scheduler;
        private Executor callback;
        private int confirmations = 2;
        private Duration offline = Duration.ofSeconds(45),
                live = Duration.ofSeconds(20),
                error = Duration.ofSeconds(30),
                maxError = Duration.ofSeconds(300);

        public Builder client(PresenceProbe client) {
            this.client = Objects.requireNonNull(client);
            return this;
        }

        public Builder scheduler(ScheduledExecutorService scheduler) {
            this.scheduler = Objects.requireNonNull(scheduler);
            return this;
        }

        public Builder callbackExecutor(Executor executor) {
            callback = Objects.requireNonNull(executor);
            return this;
        }

        public Builder confirmations(int count) {
            if (count < 2)
                throw new IllegalArgumentException("At least two confirmations required");
            confirmations = count;
            return this;
        }

        public Builder polling(Duration offline, Duration live) {
            this.offline = positive(offline);
            this.live = positive(live);
            return this;
        }

        public Builder errorBackoff(Duration initial, Duration maximum) {
            error = positive(initial);
            maxError = positive(maximum);
            if (maximum.compareTo(initial) < 0)
                throw new IllegalArgumentException("Invalid backoff");
            return this;
        }

        private static Duration positive(Duration d) {
            Objects.requireNonNull(d);
            if (d.toMillis() < 1 || d.toMillis() > Integer.MAX_VALUE)
                throw new IllegalArgumentException("Polling interval out of range");
            return d;
        }

        public LivePresenceMonitor build() {
            return new LivePresenceMonitor(this);
        }
    }
}
