package io.github.nglmercer.tiktoklive;

import io.github.nglmercer.tiktoklive.internal.NativeBindings;

import java.io.*;
import java.lang.ref.Cleaner;
import java.nio.file.*;
import java.util.*;

/**
 * Optional blocking JNI signer backed by the Rust QuickJS engine. Run signing on a worker thread.
 *
 * <p>Options JSON accepts {@code pinned}, {@code userAgent}, {@code cookie}, {@code xmst} and
 * {@code timeoutMs}, the longest one engine call (loading the bundle, or one signature) may run
 * before QuickJS interrupts it.
 */
public final class TikTokSigner implements AutoCloseable {
    private static final Cleaner CLEANER = Cleaner.create();
    private static boolean loaded;

    private static final class Handle implements Runnable {
        long value;
        // Guards freeing the native signer, separately from the handle's own monitor (which a
        // running sign holds), so interrupt() can reach a busy signer without racing its free.
        private final Object freeing = new Object();
        private long live;
        volatile boolean interrupted;

        Handle(long value) {
            this.value = value;
            this.live = value;
        }

        void interrupt() {
            interrupted = true;
            synchronized (freeing) {
                if (live != 0) NativeBindings.nInterrupt(live);
            }
        }

        public void run() {
            interrupt();
            synchronized (this) {
                long old = value;
                value = 0;
                synchronized (freeing) {
                    live = 0;
                }
                if (old != 0) NativeBindings.nClose(old);
            }
        }
    }

    private final Handle handle;
    private final Cleaner.Cleanable cleanable;

    private TikTokSigner(String bundle, String options) {
        load();
        long value = NativeBindings.nOpen(bundle, options);
        if (value == 0) throw new SignerUnavailableException("Native signer could not open");
        handle = new Handle(value);
        cleanable = CLEANER.register(this, handle);
    }

    public static TikTokSigner open() {
        String path = System.getProperty("tiktok.live.bundle", System.getenv("TTL_BUNDLE"));
        if (path == null || path.isBlank())
            throw new SignerUnavailableException(
                    "Set TTL_BUNDLE or -Dtiktok.live.bundle to the local webmssdk.js path");
        try {
            return open(Files.readString(Path.of(path)), "{}");
        } catch (IOException e) {
            throw new SignerUnavailableException("Cannot read signing bundle: " + e.getMessage());
        }
    }

    public static TikTokSigner open(String bundle, String optionsJson) {
        return new TikTokSigner(
                Objects.requireNonNull(bundle), Objects.requireNonNull(optionsJson));
    }

    public String sign(String url, SignProduct product) {
        synchronized (handle) {
            if (handle.value == 0) throw new IllegalStateException("Signer closed");
            try {
                return NativeBindings.nSign(
                        handle.value,
                        Objects.requireNonNull(url),
                        Objects.requireNonNull(product).wire());
            } catch (RuntimeException failure) {
                // An interrupted engine fails its call; report that as the close it was.
                if (handle.interrupted) throw new IllegalStateException("Signer closed");
                throw failure;
            } finally {
                java.lang.ref.Reference.reachabilityFence(this);
            }
        }
    }

    /**
     * Stops the engine without releasing it: a sign running on another thread fails promptly, and
     * later signs fail. Safe to call from any thread, any number of times.
     */
    public void interrupt() {
        handle.interrupt();
    }

    /** Interrupts any running sign, then releases the native signer. Idempotent. */
    public void close() {
        cleanable.clean();
    }

    private static synchronized void load() {
        if (loaded) return;
        String explicit =
                System.getProperty(
                        "tiktok.live.native.library", System.getenv("TTL_NATIVE_LIBRARY"));
        try {
            if (explicit != null) System.load(Path.of(explicit).toAbsolutePath().toString());
            else {
                String os = System.getProperty("os.name").toLowerCase(Locale.ROOT);
                String platform =
                        os.contains("win")
                                ? "windows"
                                : os.contains("mac")
                                        ? "macos"
                                        : os.contains("linux") ? "linux" : "unsupported";
                String arch = System.getProperty("os.arch").toLowerCase(Locale.ROOT);
                arch =
                        arch.equals("amd64") || arch.equals("x86_64")
                                ? "x86_64"
                                : arch.equals("aarch64") || arch.equals("arm64") ? "arm64" : arch;
                String name = System.mapLibraryName("ttl_sign_mobile");
                try (InputStream in =
                        TikTokSigner.class.getResourceAsStream(
                                "/natives/" + platform + "-" + arch + "/" + name)) {
                    if (in == null) System.loadLibrary("ttl_sign_mobile");
                    else {
                        Path extracted =
                                Files.createTempFile(
                                        "tiktok-signer-", name.substring(name.lastIndexOf('.')));
                        Files.copy(in, extracted, StandardCopyOption.REPLACE_EXISTING);
                        extracted.toFile().deleteOnExit();
                        System.load(extracted.toAbsolutePath().toString());
                    }
                }
            }
            loaded = true;
        } catch (IOException | UnsatisfiedLinkError e) {
            throw new SignerUnavailableException("Native library unavailable: " + e.getMessage());
        }
    }
}
