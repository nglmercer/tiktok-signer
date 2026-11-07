# Embedded JVM connection backend

The optional `tiktok-live-jvm-signer` module prepares a direct TikTok LIVE connection
inside a standard OpenJDK process. It signs in QuickJS — the same engine every other
signer in this repository uses — through the Rust `ttl-sign-mobile` JNI library that
`tiktok-live-native` loads. No Node, browser, Python, external broker or GraalVM is
required. The existing broker remains the default; adding this dependency alone does
not change it.

Until 2026-11-06 this module embedded GraalJS instead. It was replaced so that one
engine, with one set of semantics, produces every signature in the repository.

## Installation and compatibility

Existing core, client, presence and native artifacts retain Java 17 bytecode.
The embedded module and example require Java 21+. Use JDK 21 to build the entire
SDK (`cd sdk/java && ./gradlew build publishToMavenLocal`). For a broker-only
Java 17 build, select `:live-client:build :live-presence:build` instead.
Artifacts are snapshots published locally, not currently releases on Maven Central.

```kotlin
repositories { mavenLocal(); mavenCentral() }
dependencies {
    implementation("io.github.nglmercer:tiktok-live-client:0.1.0-SNAPSHOT")
    implementation("io.github.nglmercer:tiktok-live-jvm-signer:0.1.0-SNAPSHOT")
}
```

```xml
<dependencies>
  <dependency>
    <groupId>io.github.nglmercer</groupId>
    <artifactId>tiktok-live-client</artifactId>
    <version>0.1.0-SNAPSHOT</version>
  </dependency>
  <dependency>
    <groupId>io.github.nglmercer</groupId>
    <artifactId>tiktok-live-jvm-signer</artifactId>
    <version>0.1.0-SNAPSHOT</version>
  </dependency>
</dependencies>
```

The module needs the native library. Build it from the repository root:

```sh
cargo build --release -p ttl-sign-mobile --features jni
```

Then point the JVM at it with `-Dtiktok.live.native.library=/abs/path/libttl_sign_mobile.so`
(or the `TTL_NATIVE_LIBRARY` environment variable), put it on `java.library.path`, or
package it under `natives/<platform>-<arch>/` as described in the SDK README. On JDK 24+
loading it prints the JDK's restricted-native-access warning; pass
`--enable-native-access=ALL-UNNAMED` to acknowledge it. Module-path and shaded uber-JAR
packaging have not been validated and are not supported by this example.

## Usage

```java
import io.github.nglmercer.tiktoklive.*;
import java.nio.file.Path;

try (var provider = EmbeddedJavaConnectionProvider.builder()
        .cacheDirectory(Path.of("./cache/tiktok"))
        .build();
     var live = TikTokLive.builder("@creator")
        .connectionProvider(provider)
        .fetchGifts(true)
        .build()) {
    live.onConnected(s -> System.out.println("Connected to " + s.roomId()));
    live.onChat(e -> System.out.println(e.user().nickname() + ": " + e.comment()));
    live.onGiftFinal(e -> System.out.println(e.repeatCount() + " " + e.giftName()));
    live.onError(e -> System.err.println(e.getMessage()));
    live.connect().join(); // Only on an application worker, never a game main thread.
    System.in.read(); // Example lifetime only.
}
```

Injected providers are **caller-owned**. Close the client before closing its
provider. A default broker provider is SDK-owned. Each canceled connection future
cancels that attempt; client disconnect cancels acquisition and socket activity
without closing an injected provider. Client close is permanent. Caller-provided
HTTP clients and callback executors remain caller-owned.

The direct provider owns a bounded serial preparation worker and its HTTP client.
The signer lazily creates one context, reuses it for warm signatures, and runs
JavaScript on a dedicated serial worker with at most 16 queued requests. Requests
have a 30-second deadline including queue time. The same deadline bounds every call
inside QuickJS, which interrupts a wedged script itself; timeout also retires the
signer and interrupts any running call, rather than allowing a wedged context to
block future work.
Cancellation during execution also retires that context; a later preparation can
create a replacement. Normal reconnects reuse the warm context and session.
A signing failure is non-retryable for the active client attempt.

ReconnectPolicy retains bounded exponential backoff, jitter and retry-after
support. Every attempt rediscovers the room and obtains a new signature. Confirmed
offline and authentication errors stop retries. Guest identities remain in memory
for the provider lifetime; recreate the provider to refresh a refused or expired
identity. There is no automatic credential persistence or refresh loop.

For embedded connections, `onConnected` waits for the first valid server message
batch, after sending room entry. A successful WebSocket upgrade alone is insufficient.
No batch within 20 seconds triggers a socket failure. This confirms usable protocol
traffic, not a separate explicit application-level entry acknowledgment. Existing
broker connection notification behavior is retained for compatibility.

All typed events use the same existing protobuf decoder, gift metadata cache,
streak tracker and callback delivery queue. ACKs precede callbacks. A malformed
message is isolated so subsequent messages in its batch can still be processed.
Slow consumers can inspect `droppedCallbacks()`; overload stops the connection.
If supplying a callback executor, the application must bound its own executor queue.

## Bundle and sessions

The generated bootstrap is the tracked authoritative
`crates/ttl-sign-embedded/bootstrap.js`, compiled into the native library. Java builds
do not regenerate or copy it. The
proprietary upstream bundle is downloaded separately from the pinned HTTPS URL
in `SigningBundleManager`, version **1.0.0.388**, with SHA-256:

```text
dee22566273d398e074df6db40f39cfd827f6b8efd6fc382de03c44c501299ac
```

Every local/cached/downloaded bundle is checked before evaluation. Downloads have
a 30-second request timeout, 15-second connection timeout, 4 MiB response limit,
normal certificate validation, and no redirects. Cache writes use an atomic move;
an OS file lock prevents concurrent downloads. Corrupted cache entries are refused;
remove the corrupt cache file explicitly to retry. An unsupported atomic move fails
rather than leaving a partially written cache entry. Configure a local verified
bundle with `.localBundle(Path.of("/opt/tiktok/webmssdk.js"))`; it is never overwritten.
The default cache is `$HOME/.cache/tiktok-live`.

Guest bootstrap requests `https://www.tiktok.com/live` and requires a nonempty
`ttwid` cookie. An explicit header can be supplied using
`.sessionCookies(cookieHeader)`. Read secrets from your own secret store; do not
hardcode them or put them in CLI arguments. The example optionally reads
`TTL_SESSION_COOKIE`. Session identity and User-Agent are shared by signing and
WebSocket headers. Discovery includes supplied cookies when available. Guest
bootstrap occurs only after discovery confirms LIVE. No session file is written.

## Security and troubleshooting

Guest JavaScript runs in a bare QuickJS context: there is no Java object in it, and
no I/O, environment, process, thread or module access. Its only host callback
supplies bounded random bytes. Production explicitly sets
`pinned:false`; deterministic profiles are package-private test facilities.
Bundle evaluation remains security-sensitive code running within your JVM:
context restrictions are not an operating-system memory quota. Do not load
unreviewed modified bootstraps or bypass hash validation.

The URL builder signs the ordered raw query before escaping unsafe transport
characters; it preserves duplicate version_code, raw spaces, slashes and existing
percent escapes. Only the Linux Chrome/US-East/FHD preset is currently exposed.
Signed descriptors must preserve the complete original URL and append exactly one
nonempty signature. Only the three pinned TikTok socket hosts, `wss`, and the
expected reuse-supplement path are permitted. No broker fallback occurs.

Failures do not include guest exception text, cookies, or complete signed URLs.
Do not log connection descriptors or the explicit `.url()` accessor of signer
results. HTTP 401/403 and challenges require authentication/verification; 429 uses
a conservative retry delay. Unknown statuses, malformed JSON, missing live room
IDs and network failures are errors, not offline. User-not-found code 10202 is
non-retryable; other unexpected upstream codes remain unknown discovery errors.

Run the pinned Gradle wrapper itself on JDK 21. A build failure containing only
`27` is Gradle's bundled Kotlin DSL rejecting a Java 27 launcher, before SDK code
runs. For example, on Arch-based Linux with OpenJDK 21 installed:

```sh
JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew :examples:embedded-java:run --args='@creator'
```

The SDK test JVM and Gradle launcher are separate compatibility concerns.
If initialization fails with `SignerUnavailableException`, the native library was
not found or could not load; verify its path and that it matches the JVM's platform
and architecture.
If download fails offline, provide a local file matching the pinned hash. If TikTok
refuses anonymous bootstrap, provide a legitimate session or stop; reconnect loops
do not solve verification challenges. A valid signature does not guarantee that
TikTok permits your session, IP, region, or client fingerprint.

For Paper/Spigot, invoke `connect()` asynchronously and schedule world/player
mutations onto the server main thread yourself. Listener callbacks run off that
thread. Close the client and then provider during plugin disable. Nothing in this
module depends on Minecraft or a graphical environment.

Headless Linux example:

```sh
export JAVA_HOME=/path/to/openjdk-21
cd sdk/java
./gradlew :examples:embedded-java:run --args='@creator /opt/tiktok/webmssdk.js'
```

The example waits for SIGTERM/Ctrl-C, with a JVM shutdown hook. It starts no broker.
See [validation and measurements](EMBEDDED_JAVA_VALIDATION.md) for executed checks,
performance and remaining verification limits.

## Licensing

The engine is QuickJS (MIT) through `rquickjs` (MIT), linked into the native library
together with the Rust crates listed in `../../THIRD_PARTY_NOTICES.md`. Retain notices
from all resolved artifacts, including transitive components; this is not a license
grant for TikTok's proprietary bundle.

The downloaded bundle is not embedded in SDK artifacts or the repository.
Redistribution requires a separate licensing review. The native library does
not remove the existing modified AGPL-3.0 protobuf/schema obligations in
`crates/ttl-live-proto/LICENSE.upstream` and the client artifact. Existing legal
notices remain intact. Review them for your distribution; no legal determination
about combining this SDK with a specific application is asserted here.
