> **Historical record.** Everything below was measured on the GraalJS signer this module used
> until 2026-11-06. The module now signs in QuickJS through the Rust JNI library; its parity is
> checked against `fixtures/research/pinned-signatures-1.0.0.388.json` by `QuickJsSignerTest`
> (`TTL_NATIVE_LIBRARY=… TTL_BUNDLE=… ./gradlew :live-jvm-signer:test`). The GraalJS timings,
> footprint and JDK 25 notes do not describe the current module.

# Embedded backend validation — 2026-10-20

Platform: Linux x86_64. Standard OpenJDK was used, not a GraalVM JDK.
This is a short integration and performance evaluation, not a long-running
production deployment certification.

## Executed checks

From `sdk/java` (the commands below assume equivalent locally installed JDK paths):

| Command / runtime | Result |
| --- | --- |
| `JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew build` | Passed full SDK and example build, including offline tests |
| `JAVA_HOME=/tmp/ttl-openjdk17 ./gradlew :live-core:test :live-client:test :live-presence:test --rerun-tasks` | Passed on Temurin OpenJDK 17.0.20.1 |
| `TTL_BUNDLE=/tmp/ttl-java-webmssdk.js JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew :live-jvm-signer:test :live-client:test` | Passed real-bundle fresh-context `ws`, `fetch`, `frontier` parity against the existing V8 reference, plus regression tests |
| `TTL_BUNDLE=/tmp/ttl-java-webmssdk.js JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew :live-jvm-signer:test :live-client:test -PjavaTestHome=/tmp/ttl-openjdk25 --rerun-tasks` | Passed on Temurin OpenJDK 25.0.4.1 LTS test JVMs; Gradle itself ran on JDK 21 |
| `TTL_BUNDLE=/tmp/ttl-java-webmssdk.js TTL_BENCHMARK=1 JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew :live-jvm-signer:test :live-jvm-signer:runtimeFootprint :live-client:test :examples:embedded-java:classes` | Passed; measurements below |

Default tests require neither TikTok network access nor a proprietary bundle.
Real-bundle parity is explicitly enabled with `TTL_BUNDLE`; it verifies the pinned
SHA-256 and uses Node **only as a contributor test reference**. No Node process is
used by the Java signer, provider or application example. Each signing product is
compared from a fresh context; successive warm signatures are not required to be
identical.

The first expanded test run exposed a signer shutdown race; it was fixed and the
suite rerun successfully. An initial concurrent JDK 25 matrix invocation collided
with another Gradle build regenerating protobuf output and failed compilation.
The sequential rerun passed. Use separate workspaces or sequential commands when
running runtime matrices; these builds share generated output directories.

## Explicitly authorized real LIVE test

The existing contributor discovery tool `scripts/headless/find-live.mjs` selected
five currently LIVE channels through TikTok's live search endpoint. A random
selection chose `@gonioficiall`. The Node tool was used only to find a candidate;
all subsequent connection preparation and event streaming used Java in-process.

Executed on OpenJDK 21:

```sh
TTL_LIVE_CREATOR=gonioficiall TTL_BUNDLE=/tmp/ttl-java-webmssdk.js \
  ./gradlew :live-jvm-signer:test --tests '*RealLiveSmokeTest'
```

First run: direct connection plus four typed events in 7,459.11 ms.
Second run included explicit disconnect and reconnect:

| Attempt | Time to confirmed connection and event | Cumulative events |
| --- | ---: | ---: |
| Initial | 7,273.97 ms | 6 |
| Reconnect, same provider | 2,623.91 ms | 18 |

Both attempts passed. No broker or JNI signer was configured, and the Java backend
started no external process. Guest access worked for this IP and creator at the
time of testing. These observations do not guarantee continued anonymous access,
all TikTok regions, explicit authenticated sessions, or every event type. The
smoke test was not a gift-purchase or gift-streak transaction test. It stops after
receiving protocol events; it is not a sustained production session.

## Measurements

Real verified bundle, 200 sequential warm `ws` signatures after initialization.
Java used standard OpenJDK 21's interpreter fallback; no GraalVM JDK comparison
was performed. JUnit process heap includes the test harness and earlier tests and
is not an isolated retained-heap measurement. No explicit GC was forced.

| Metric | Measured value |
| --- | ---: |
| Lazy initialization plus first signature | 818.99 ms |
| Warm p50 | 100.44 ms |
| Warm p95 | 152.63 ms |
| JVM used heap after initialization | 67,596,408 bytes |
| JVM used heap after 200 signatures | 70,018,552 bytes |
| Additional JVM threads during benchmark | 2 |
| Signer close call | 0.79 ms |
| Resolved embedded runtime classpath | 26 artifacts, 75,399,624 bytes |
| Direct reconnect to confirmed traffic | 2,623.91 ms |

The dependency footprint includes core/presence dependencies; it excludes an
application's existing client dependencies and the separately cached upstream
bundle. Shutdown tests additionally verify that signer worker/timer threads
terminate after closing during execution.

Reference command, from repository root:

```sh
TTL_BUNDLE=/tmp/ttl-java-webmssdk.js cargo run --release -p ttl-sign-embedded \
  --no-default-features --features quickjs --example signing_bench -- 1 1 200
```

QuickJS measured p50 **29.01 ms**, p95 **32.27 ms**, 34.29 requests/second.
Warm process RSS was 7,996 KiB, versus a 3,696 KiB baseline. This is native RSS,
not directly comparable to JVM used heap. The Java signer was slower in this
short run; no equivalent-performance claim is made. Cold-start timing was not
reported by the existing QuickJS benchmark.

## Coverage and remaining limits

Offline tests cover ordered raw Rust query parity, large decimal room IDs,
post-signing escaping, existing percent escapes, rejection of appended parameters,
corrupted/missing local bundles, warm/concurrent signatures, failure redaction,
timeout retirement, shutdown, guest cookies, guest rejection, discovery errors,
rate limits, offline-without-signing, cancellation, provider ownership, room-entry
confirmation, and existing transport/decoder regressions.

The committed workflow adds offline JDK 17/21/25 jobs on Linux x86_64, Windows,
and Linux ARM64. GitHub reports Actions disabled for this repository (`actions/permissions.enabled=false`),
so no remote jobs started for PR #10. Windows and ARM64 remain unverified.
Remote jobs are not claimed passed by this local record.
No GraalVM JDK benchmark, multi-hour heap soak, module-path launch or shaded-JAR
launch has been executed. Only the Linux Chrome/US-East/FHD signing preset is
currently exposed. Anonymous search selection is a contributor tool, not part
of the provider's public API. Cache download failure and cross-process lock
behavior have not been exhaustively fault-injected on every filesystem.
