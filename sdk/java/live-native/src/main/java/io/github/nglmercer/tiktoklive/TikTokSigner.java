package io.github.nglmercer.tiktoklive;

import io.github.nglmercer.tiktoklive.internal.NativeBindings;

import java.io.*;
import java.lang.ref.Cleaner;
import java.nio.file.*;
import java.util.*;

/** Optional blocking JNI signer. Run signing on a worker thread. */
public final class TikTokSigner implements AutoCloseable {
    private static final Cleaner CLEANER = Cleaner.create();
    private static boolean loaded;

    private static final class Handle implements Runnable {
        long value;

        Handle(long value) {
            this.value = value;
        }

        public synchronized void run() {
            long old = value;
            value = 0;
            if (old != 0) NativeBindings.nClose(old);
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
            } finally {
                java.lang.ref.Reference.reachabilityFence(this);
            }
        }
    }

    public void close() {
        cleanable.clean();
    }

    private static synchronized void load() {
        if (loaded) return;
        String explicit = System.getProperty("tiktok.live.native.library");
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
