# TikTools Studio — small Android app for TikTok LIVE

A simple, Android-only studio: read live events with filters, award viewer
points from a local database, and fire fetch-only automations per event kind.
Signing and the event stream run on-device by reusing the Rust core:
`libttl_sign_mobile.so` (QuickJS engine + URL builders + live socket from
`crates/ttl-sign-mobile`) over JNI. No separate Android signing SDK — the app
is a thin UI over the reused core, plus its own SQLite studio database.

The reuse analysis lives in [docs/16-android-signing.md](../docs/16-android-signing.md).
The desktop reference (Windows/Linux) is
[TikTools-app](https://github.com/nglmercer/TikTools-app); this app is the
small Android sibling: actions, events viewer, points, and config only.

## Tabs

| Tab | What it does |
|---|---|
| Events | Live reader: per-category filter chips (chat, gift, like, follow, share, join, member, room), search, pause, per-type counts, latest award |
| Points | SQLite leaderboard, per-event rates editor (mirrors desktop `PointsConfig`), manual adjust, reset |
| Actions | Fetch-only automations: pick trigger kinds, GET/POST a URL template with `{{user}} {{name}} {{text}} {{type}} {{count}} {{diamonds}}`, cooldown, test fire, run log |
| Setup | Connect/disconnect, live feed, resolve + sign, TTS toggles, signer/session maintenance, debug console |

## Data

- `tiktools-studio.db` (SQLite, one `SQLiteOpenHelper`): `viewers` point
  balances, `actions` fetch automations, `runs` capped run log.
- `SharedPreferences`: points rates, TTS toggles. Balances stay in SQLite so
  they survive process death and feed the leaderboard query directly.

## TTS frontier

Every component that may speak takes a `tts.Speaker`. Today the app ships a
silent `NoopSpeaker` plus an `AndroidSpeaker` (platform `TextToSpeech`) behind
the Setup toggles, with `SpeechText` mapping events to spoken lines. Voices,
per-event toggles, and queue controls plug into the same seam later without
touching the event pipeline.

## Prerequisites

- JDK 17+ (`/usr/lib/jvm/java-21-openjdk` works; the wrapper was generated with Gradle 9.7.1)
- Android SDK with platform 35, NDK 27 (`sdk.dir` in `local.properties`)
- For the native library: the `*-linux-android` Rust targets, `cargo-ndk`, libclang

## Build

```sh
# 1. Native library, all four ABIs -> app/src/main/jniLibs/ (gitignored: reproducible)
./scripts/android/build-native.sh

# 2. APK
cd android && ./gradlew assembleDebug
# app/build/outputs/apk/debug/app-debug.apk
```

JVM unit tests (pure logic — no device, no network):

```sh
cd android && ./gradlew testDebugUnitTest
```

## Run

On a device or emulator:

```sh
cd android && ./gradlew installDebug
adb shell am start -n com.example.ttlsigner/.MainActivity
```

In the app: open Setup, tap a feed room (or type a handle / numeric room id),
then Connect. Watch Events land filtered, Points accrue on the leaderboard,
and Actions fire their URLs. Signed URLs are shown on screen but never written
to logcat.

On-device tests (need network for the one-time bundle download):

```sh
cd android && ./gradlew connectedDebugAndroidTest
```

## Layout

| File | What it is |
|---|---|
| `.../events/LiveEvent.kt` | Typed event model + total JSON parser |
| `.../events/EventFilter.kt` | Category set + query matching for the reader |
| `.../data/StudioDb.kt` | The only database: viewers, actions, run log |
| `.../points/PointsConfig.kt` | Rates per trigger + level threshold (prefs) |
| `.../points/PointsEngine.kt` | Pure award math (in `PointsConfig.kt`) |
| `.../points/PointsRepository.kt` | Balances over SQLite, leaderboard flow |
| `.../actions/EventAction.kt` | Fetch-action model + `{{template}}` + matcher |
| `.../actions/ActionRunner.kt` | Cooldowns, fetch execution, run log |
| `.../tts/Speaker.kt` | `Speaker` seam, noop + Android TTS, `SpeechText` |
| `.../SessionViewModel.kt` | Connection + one event pipeline fanning out to reader/points/actions/speaker |
| `.../EventsFragment.kt` | Reader: chips, search, pause, counts |
| `.../PointsFragment.kt` | Leaderboard, rates editor, adjust, reset |
| `.../ActionsFragment.kt` | Action list, editor sheet, run log |
| `.../SetupFragment.kt` | Connect, feed, sign, TTS, signer, console |
| `.../TtlNative.kt` | Raw JNI signatures; must match `crates/ttl-sign-mobile/src/jni.rs` |
| `.../RustSigner.kt` | Warm native signer: `open` once, `sign` per request |
| `.../Discovery.kt` | Unsigned `unique_id` → `room_id` lookup + parsing |
| `.../BundleFetch.kt` | Bundle download + SHA-256 pin |
| `.../Feed.kt` | Live feed: unsigned search + guest cookies, parse/sort |
| `.../FeedAdapter.kt` | Feed rows; tap selects a room for connecting |
| `.../Logger.kt` | Timestamped log lines; logcat gets signature summaries only |
| `.../LiveClient.kt` | Live event stream: worker callbacks posted to main |
| `.../TtlEvents.kt` | Stream callback interface (must match `jni_live.rs`) |
| `.../EventFormat.kt` | One event JSON object → one display line |
| `.../MainActivity.kt` | Four-tab shell over the shared view model |
| `.../ui/` | `CollapsibleCard`, `LogConsoleView`, `StatusPillView` building blocks |
| `app/src/test/...` | JVM unit tests (pure logic, no device) |
| `app/src/androidTest/...` | On-device tests: layout inflation + full connect/stream/disconnect flow |
