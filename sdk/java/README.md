# TikTok LIVE Java SDK

Java 17+ SDK for the existing `tiktok-signer` broker and direct TikTok LIVE protocol.
The default client requires no Node, browser, Python, or JNI. Presence performs only HTTP lookup;
it never connects to TikTok's LIVE WebSocket. Generated protobuf types are implementation details.

## Build and installation

From `sdk/java`, with a JDK 17 or newer:

```sh
./gradlew build
./gradlew publishToMavenLocal
```

The artifacts are currently `0.1.0-SNAPSHOT`, built locally; they are not yet published to Maven Central.
Use `mavenLocal()` with Gradle until a release is published:

```kotlin
repositories { mavenLocal(); mavenCentral() }
dependencies {
    implementation("io.github.nglmercer:tiktok-live-client:0.1.0-SNAPSHOT")
    implementation("io.github.nglmercer:tiktok-live-presence:0.1.0-SNAPSHOT")
}
```

Maven resolves the locally installed artifacts:

```xml
<dependencies>
  <dependency>
    <groupId>io.github.nglmercer</groupId>
    <artifactId>tiktok-live-client</artifactId>
    <version>0.1.0-SNAPSHOT</version>
  </dependency>
  <dependency>
    <groupId>io.github.nglmercer</groupId>
    <artifactId>tiktok-live-presence</artifactId>
    <version>0.1.0-SNAPSHOT</version>
  </dependency>
</dependencies>
```

`live-core` has no dependencies. Client/presence use OkHttp and Jackson; client additionally uses
protobuf-java. `live-proto` is an internal Gradle module, generated locally using pinned protoc
from `../../crates/ttl-live-proto/proto/v3`. The client JAR bundles those generated classes and
upstream notices, so consumers do not need an unpublished proto artifact. Builds never download
TikTok schemas. Dependency/tool downloads are normal on the first build; tests use no TikTok network.

## Connection and listeners

```java
import io.github.nglmercer.tiktoklive.*;

try (TikTokLive live = TikTokLive.builder("@creator")
    .apiUrl("http://127.0.0.1:8080")
    .apiKey(System.getenv("TIKTOK_LIVE_API_KEY"))
    .fetchGifts(true)
    .build()) {
    live.onChat(e -> System.out.println(e.user().nickname() + ": " + e.comment()));
    live.onGift(e -> System.out.println(e.giftName() + " count=" + e.repeatCount()));
    live.onGiftFinal(e -> System.out.println("Final streak: " + e.repeatCount()));
    live.onError(Throwable::printStackTrace);
    live.connect().thenAccept(s -> System.out.println("Room: " + s.roomId())).join();
    System.in.read(); // Keep this command-line example alive until Enter.
}
```

`connect()` returns `CompletableFuture<LiveState>` and resolves after WebSocket open and EnterRoom,
or with `OFFLINE` when the broker reports offline. Before the first successful broker response, `state().status()` is null (unconfirmed),
so a network failure does not report OFFLINE. Calling it while connecting shares the pending
operation; calling it while connected returns current state. `disconnect()` stops the connection
but permits a later `connect()`. Canceling an in-flight connect stops its timers and requests.

Listeners: `onChat`, `onGift`, `onGiftFinal`, `onLike`, `onMember`, `onSocial`, `onRoomUser`,
`onUnknown`, `onEvent`, `onConnected`, `onOffline`, `onDisconnected`, `onReconnecting`, `onError`.
Event identifiers remain decimal strings, including values beyond JavaScript's safe integer range.
Normalized counts use the Node API's nonnegative safe-integer clamp. Unknown/malformed event bytes
are retained, and one bad event does not terminate the batch.

Only a normalized creator handle is sent to `POST /v1/connect`; the SDK validates its version,
identity, LIVE room and signed WSS descriptor against the repository's three TikTok socket hosts.
The socket uses the broker's URL, Cookie, and User-Agent. There is no public room/signature override.

## Reconnect and errors

Default reconnect: five attempts, 2s initial delay, 60s maximum, exponential backoff and positive
0–50% jitter. Every attempt calls the broker again for a fresh signed URL. Backend `retryAfterMs`
is a minimum delay, even beyond the configured backoff maximum. A successful socket resets the
retry budget. Offline is a resolved state and stops retries; networking failures are exceptions.

```java
live.onError(error -> {
    if (error instanceof TikTokLiveException e) {
        System.err.println(e.code() + " HTTP=" + e.httpStatus() + " request=" + e.requestId());
    }
});
```

Exceptions distinguish API, discovery, protocol, socket, authentication, rate limit, and signer
failures, retaining backend code/message/retryability/retry-after/HTTP status/request ID.
Malformed frames emit protocol errors; malformed events become `UnknownEvent`.

## Presence lookup

```java
try (LivePresenceClient client = LivePresenceClient.create()) {
    LivePresence presence = client.lookup("@creator").join();
    System.out.println(presence.status());
}
```

The unsigned `/api-live/user/room/` lookup requires status 2 plus a nonempty room other than `0`
for LIVE. Status 0 or 4 gives OFFLINE. A persisted room ID alone never implies LIVE. Unknown status
values, missing status, schema changes, challenges, non-success HTTP (including 403/429), invalid
JSON, timeouts, and network failures return UNKNOWN. This is deliberately more conservative than
older discovery parsers that defaulted a missing status to zero. IDs stay exact; unsafe numeric
IDs are rejected just as in Node. Invalid creator arguments throw `IllegalArgumentException`.
`PresenceProbe` allows a future reliable fallback; no HTML/browser fallback is guessed today.

## Presence notifications

```java
try (LivePresenceMonitor monitor = LivePresenceMonitor.create()) {
    monitor.watch("@creator")
        .onLiveStarted(e -> System.out.println(e.uniqueId() + " started LIVE"))
        .onLiveEnded(e -> System.out.println(e.uniqueId() + " ended LIVE"));
    monitor.watch("@another");
    System.in.read();
}
```

The initial confirmed state is UNKNOWN. Two consecutive successful observations confirm a change.
Initial LIVE emits started; initial OFFLINE establishes a baseline without ended. OFFLINE→LIVE
emits started and LIVE→OFFLINE emits ended. UNKNOWN resets an unconfirmed candidate and preserves
the last confirmed state, so a timeout cannot end a broadcast. Register listeners immediately
when creating a watch. A small initial scheduling delay allows the fluent registration pattern.
Repeated `watch` calls for the same normalized ID return the same watch; `Watch.close()` or
`unwatch(id)` removes it. `Watch.state()` exposes confirmation, last success, failures and next poll.

Defaults: OFFLINE 45s, LIVE (including a LIVE candidate) 20s; UNKNOWN failures back off from 30s
to 300s exponentially. All poll intervals have ±20% jitter. Polls never overlap for a creator.
One shared scheduler handles all creators; async HTTP handles network wait. Builders accept a
client/probe, scheduler, callback executor, confirmation count (minimum two), and polling/backoff
intervals. Use sensible intervals to avoid aggressive polling.

## Threading, Minecraft, Android and shutdown

OkHttp receives/decodes a PushFrame and batch, sends the ACK immediately, then normalizes events
and queues callbacks on an internal daemon executor. Even a supplied direct executor cannot run
listeners on a protocol thread. Future completion for a new connection also leaves network threads.
Callbacks are ordered by default; a supplied parallel executor determines its own delivery order.
Application callback failures are isolated. Keep listeners short and forward expensive work.
The internal callback queue is bounded to 1024 tasks; overloaded consumers may drop notifications (observable through `droppedCallbacks()`).

```java
live.onGift(e -> gameMainThreadExecutor.execute(() -> {
    // Invoke the Minecraft server API here using its real scheduler.
}));
```

There are no Bukkit/Paper/Fabric/Forge dependencies. Never call `.join()` on a game's main thread:
use `thenAccept` or listeners. Supply the platform scheduler as an `Executor` when appropriate.
Java 17 records/sealed types require an Android toolchain supporting them (recent AGP/desugaring);
this is JVM shared code, not a claim of compatibility with every Android API level.

All owned threads are daemon threads. Always close clients/monitors/signers: repeated close is
safe, timers and pending requests are canceled, sockets closed, watches stopped, native handles
released, and owned executors released. Caller-supplied HTTP clients, executors, probes and
schedulers remain caller-owned. Closing a monitor with a supplied client closes watches but not
that client. Already running application callbacks cannot be forcibly stopped; queued callbacks
check shutdown before invocation. `close()` cancels network operations without waiting for a
network handshake or for application work to finish.

## Gift metadata and streaks

`fetchGifts(true)` downloads and caches a catalog once per LIVE descriptor/session before opening
the socket; failures report an error and continue with wire metadata. It enriches name, diamonds,
icon and streakability. `onGift` reports raw normalized updates (not independent totals).
`onGiftFinal` uses `GiftStreakTracker`: rising counts are merged, only repeat-end produces a streak
total, duplicate finals are suppressed, and known non-streakable gifts are immediately final.
Unknown gifts conservatively await repeat-end. Tracking is bounded to 4096 groups and cleared for
each connection. Applications should not sum every repeatCount from `onGift`.

## Optional native signer

Install `tiktok-live-native` separately; client and presence have no dependency on it.
Build the existing Rust signer with its JNI feature, from the repository root:

```sh
cargo build --release -p ttl-sign-mobile --features jni
```

```java
try (TikTokSigner signer = TikTokSigner.open()) {
    String signed = signer.sign(unsignedUrl, SignProduct.WS);
}
```

Set `TTL_BUNDLE` or `-Dtiktok.live.bundle=/path/webmssdk.js` to a local signing bundle.
The bundle is deliberately not vendored or downloaded automatically. `open(bundleSource, optionsJson)`
also supports explicit trusted source. Signing is blocking: run it on a worker, never a main/UI
thread. Native calls reuse `MobileSigner`, the existing Rust QuickJS implementation sharing
`ttl-sign-embedded/bootstrap.js` and `ttl-sign-core`. No signing logic exists in Java. The stable
JNI namespace is `io.github.nglmercer.tiktoklive.internal.NativeBindings`; Android demo entry points
remain compatible. Package-linked exports follow the existing JNI boundary rather than adding a
second registration system. Access to each native handle is serialized, and a Cleaner handles
abandoned signers without racing an in-flight call.

Set `-Dtiktok.live.native.library=/absolute/path/libttl_sign_mobile.so`, use `java.library.path`,
or package the appropriate build under `natives/<platform>-<arch>/<mapped-library-name>`:
`windows-x86_64`, `linux-x86_64`, `linux-arm64`, `macos-x86_64`, `macos-arm64`.
The loader supports these resource layouts; prebuilt cross-platform binaries are not bundled.

## Runnable examples and tests

```sh
./gradlew :examples:basic-live:run --args='@creator'
./gradlew :examples:presence:run --args='@creator'
./gradlew :examples:multi-presence:run --args='@creator @another'
./gradlew :examples:minecraft-style:run --args='@creator'
./gradlew :examples:native-signer:run --args='UNSIGNED_SOCKET_URL'
```

Default tests use MockWebServer and shared `fixtures/events`, including Node-generated golden
normalization, transport bytes, and presence cases. The WebSocket test verifies ACK-before-callback,
broker headers, fresh reconnect descriptors, and shutdown offline. Live network/native signing
examples are explicit manual integration checks, not part of default `test`.
Regenerate transport fixtures after building the existing Node package:
`node packages/tiktok-live/scripts/java-parity-fixtures.mjs` from the repository root.
Rust and Node tests also consume the shared transport/presence fixtures.

## Architecture and licensing

`live-core` → stable models/errors; `live-client` → broker, OkHttp socket, protocol adapter;
`live-proto` → local generation from shared v3 schemas; `live-presence` → unsigned HTTP probes and
confirmed monitoring; optional `live-native` → existing Rust JNI signer.

The surrounding repository is MIT, but the vendored schemas and their generated bindings use
modified AGPL-3.0 with additional permissions. Read `../../THIRD_PARTY_NOTICES.md` and
`../../crates/ttl-live-proto/LICENSE.upstream` before redistribution or hosting. Both notices are
included in the client JAR. The schema exception has conditions for services offered to third
parties; generated Java classes do not remove those conditions.
