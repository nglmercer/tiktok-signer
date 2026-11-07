package io.github.nglmercer.tiktoklive;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable;

import java.nio.file.*;
import java.util.Arrays;
import java.util.concurrent.TimeUnit;

/** Opt-in real bundle measurements; never contacts TikTok. */
@EnabledIfEnvironmentVariable(named = "TTL_BENCHMARK", matches = "1")
class SigningBenchmarkTest {
    @Test
    void measure() throws Exception {
        var bundle =
                new SigningBundleManager(Path.of("unused"))
                        .loadLocal(Path.of(System.getenv("TTL_BUNDLE")));
        String raw = DirectSocketUrlBuilder.rawUrl("7300000000000000001", "7300000000000000002");
        int baseline = java.lang.management.ManagementFactory.getThreadMXBean().getThreadCount();
        long start = System.nanoTime();
        var signer = new QuickJsSigner(bundle);
        signer.sign(raw, QuickJsSigner.Product.WS).get(60, TimeUnit.SECONDS);
        double cold = (System.nanoTime() - start) / 1e6;
        Runtime runtime = Runtime.getRuntime();
        long initialHeap = runtime.totalMemory() - runtime.freeMemory();
        long[] samples = new long[200];
        for (int i = 0; i < samples.length; i++) {
            start = System.nanoTime();
            signer.sign(raw, QuickJsSigner.Product.WS).get(30, TimeUnit.SECONDS);
            samples[i] = System.nanoTime() - start;
        }
        Arrays.sort(samples);
        long heap = runtime.totalMemory() - runtime.freeMemory();
        int extra =
                java.lang.management.ManagementFactory.getThreadMXBean().getThreadCount()
                        - baseline;
        start = System.nanoTime();
        signer.close();
        double shutdown = (System.nanoTime() - start) / 1e6;
        System.out.printf(
                java.util.Locale.ROOT,
                "cold_ms=%.2f warm_p50_ms=%.2f warm_p95_ms=%.2f initial_heap_bytes=%d"
                    + " final_heap_bytes=%d additional_threads=%d shutdown_ms=%.2f%n",
                cold,
                samples[100] / 1e6,
                samples[190] / 1e6,
                initialHeap,
                heap,
                extra,
                shutdown);
    }
}
