# TikTools Studio for Android

A Jetpack Compose / Material 3 studio for TikTok LIVE. The Android app owns
its session, event history, HTTP automations, rewards, and speech controls.
Signing and the live socket still use the existing Rust JNI core. The Java
SDK and Rust implementations are unchanged by this refactor.

## Navigation

- **Home:** username or room connection, live discovery, recent creators,
  connection progress, session counters, recent activity, and quick speech controls.
- **Events:** searchable event history, category filters, display density,
  pause/clear, and event details with raw JSON copying.
- **Actions:** HTTP GET/POST automations, trigger categories, optional gift
  matching, template fields, cooldowns, test execution, and run history.
- **Rewards:** leaderboard, event rates, viewer earnings, manual adjustments,
  and confirmed resets.
- **Settings:** secondary destination for connection policies and appearance;
  separate Speech and Diagnostics screens hold engine/model controls and logs.

Phones use bottom navigation; windows at least 600 dp wide use a navigation
rail. Colors support system/light/dark appearance and Android 12+ dynamic
colors. Shared Compose components and spacing live in `ui/`.

## Architecture and compatibility

The application-scoped `AppContainer` owns `LiveSessionManager`, repositories,
and the single event pipeline. Screen ViewModels expose state to stateless
Compose screens; navigation and Activity recreation do not own the socket.
The session distinguishes resolution, preparation, connection, live, retry,
and failure. Only the native open callback marks a connection live. Cancelled
connections release their native handle and stale callbacks are ignored.

Room opens the existing `tiktools-studio.db`. Migration 1 → 2 retains viewer
balances, actions, run history, and auto-increment sequences, and adds earning
attribution for future awards. Existing balances appear as previous balance
because historical per-category earnings cannot be reconstructed. Awards and
manual adjustments use transactions. The exported Room schema is in `app/schemas/`.

DataStore migrates the existing points, speech, and event-display preferences.
It also persists filters, recent creators, theme, speech categories, and
connection policies. Event history is memory-only and limited to 300 rows;
pausing the reader does not pause rewards, actions, or speech.

The source namespace is `dev.nglmercer.tiktools`. The installed application ID
remains `com.example.ttlsigner` to preserve upgrade data. `TtlNative` and
`TtlEvents` retain their original package and signatures for JNI compatibility.

## Speech and background operation

Speech supports Off, Android device TTS, and offline Supertonic 3 (voice F1).
Models download separately into `files/supertonic3/`; Speech shows download
progress and provides cancel/delete, test, repeat, skip, and category controls.
The ONNX dependency includes native libraries and increases APK size.

When background connection is enabled, `LiveService` maintains the foreground
notification with disconnect and speech controls. Disabling this policy ends
an active connection when the Activity goes into the background. Reconnection
can be disabled independently. Notification permission is requested on Android
13+. The service does not reconstruct a session after process death.

## Build and verification

Requirements: JDK 21, Android SDK platform 35, and Gradle (the checked-in wrapper
uses 9.7.1). Native builds additionally require NDK 27, Rust Android targets,
`cargo-ndk`, and libclang. Set `sdk.dir` in ignored `android/local.properties`.

```sh
# From the repository root: build the live/signing JNI library for all ABIs.
./scripts/android/build-native.sh

cd android
./gradlew assembleDebug
./gradlew testDebugUnitTest lintDebug assembleDebugAndroidTest
# APK: app/build/outputs/apk/debug/app-debug.apk
```

JVM tests cover event/reward/template logic, session cancellation and stale
callbacks, real SQLite migration, preference migration, and Compose navigation
and touch targets using Robolectric. Screenshots can be emitted by supplying
`screenshotDir` as a test JVM system property. The APK can compile without the
JNI artifact, but signing and LIVE require `libttl_sign_mobile.so` on the device.

```sh
./gradlew installDebug
adb shell am start -n com.example.ttlsigner/dev.nglmercer.tiktools.app.MainActivity
# Explicitly opt into native + network device integration tests:
./gradlew connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.liveIntegration=true
```

Host checks do not establish real-device LIVE connectivity or ONNX audio
playback. Those require the native artifact and device/network verification.
