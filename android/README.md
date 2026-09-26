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
| Events | Live reader: icon filter chips that expand with live counts when selected, search, clear-filters button, pause, per-type counts, latest award |
| Points | SQLite leaderboard, per-event rates editor (mirrors desktop `PointsConfig`), manual adjust, reset |
| Actions | Fetch-only automations: pick trigger kinds, GET/POST a URL template with `{{user}} {{name}} {{text}} {{type}} {{count}} {{diamonds}}`, cooldown, test fire, run log |
| Setup | Direct login (username input + live feed, one tap connects), TTS engine console, signer/session maintenance, debug console |

## Design

The UI is deliberately minimalist: flat outlined cards (no shadows), one
12dp corner radius, a 4/8/12/16dp spacing scale (`values/dimens.xml`),
sentence-case letterspaced section labels, muted secondary lines, hairline
dividers between rows, and counts as a quiet `· N` suffix that disappears
while empty. All color comes from the Material3 theme (day/night + dynamic
color), plus the three semantic connection dots.

Shared building blocks live in `.../ui/`:

| Component | What it is |
|---|---|
| `UiKit` | One home for the label rules: `sectionTitle` (`Events · 12`), `countsLine` (`chat 3 · gift 1`), `dp`, `addMinimalDividers` |
| `EmptyStateView` | Centered muted icon + one line; every empty list (events, leaderboard, actions) |
| `SectionHeaderView` | Small label + optional count; headings of the flat cards (rates, adjust, TTS, signer) |
| `SettingRowView` | Label-over-value slot; the Setup maintenance rows keep their wired value IDs inside it |
| `MinimalDivider` | 1dp outline-variant hairline with optional insets |
| `CollapsibleCard` | Flat tappable-header card, `setTitleWithCount` for the `· N` suffix |
| `StatusPillView` | 8dp dot + one-line connection state |
| `LogConsoleView` | Shared debug console: follows, copies, clears |

## Data

- `tiktools-studio.db` (SQLite, one `SQLiteOpenHelper`): `viewers` point
  balances, `actions` fetch automations, `runs` capped run log.
- `SharedPreferences`: points rates, TTS toggles. Balances stay in SQLite so
  they survive process death and feed the leaderboard query directly.

## TTS engines

Every component that may speak takes a `tts.Speaker`. Setup offers three
engines: off, the device voice (`AndroidSpeaker` over platform
`TextToSpeech`), and on-device SuperTonic 3, with `SpeechText` mapping events
to spoken lines in all cases.

SuperTonic 3 is a port of the nabu example
([mewmix/nabu](https://github.com/mewmix/nabu),
`app/.../supertonic/`): four ONNX Runtime CPU sessions
(`duration_predictor`, `text_encoder`, `vector_estimator`, `vocoder`) plus the
unicode text processor, running only the v3 model (`Supertone/supertonic-3`,
voice F1). Model files (~7 downloads) never ship in the APK — the Setup tab
fetches them once into `files/supertonic3/` with progress, then synthesis and
`AudioTrack` playback run fully on-device. Speech drops (never queues) while
an utterance is playing, so it can't lag the stream. The ONNX dependency adds
native libraries per ABI, so the debug APK is ~100 MB.

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
| `.../events/EventIcons.kt` | Category → vector drawable for the filter chips |
| `.../tts/supertonic/` | Ported v3 engine, model manifest + downloader, `AudioTrack` speaker |
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
| `.../ui/` | `UiKit`, `EmptyStateView`, `SectionHeaderView`, `SettingRowView`, `MinimalDivider`, `CollapsibleCard`, `LogConsoleView`, `StatusPillView` |
| `values/dimens.xml` + `themes.xml` | Spacing scale, `Widget.TikTools.MinimalCard`, `SectionLabel`/`Muted` text styles, thin-indicator tabs |
| `drawable/bg_pill.xml` | Flat pill behind event badges and level chips |
| `app/src/test/...` | JVM unit tests (pure logic, no device — incl. `ui/UiKitTest`) |
| `app/src/androidTest/...` | On-device tests: layout inflation + shared-component types + full connect/stream/disconnect flow |
