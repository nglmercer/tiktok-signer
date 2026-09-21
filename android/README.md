# TTL Signer — Android example app

Signs TikTok LIVE socket URLs on-device by reusing the Rust core:
`libttl_sign_mobile.so` (QuickJS engine + URL builders from
`crates/ttl-sign-mobile`) over JNI. No separate Android signing SDK — the app
is a thin UI over the reused core.

The reuse analysis lives in [docs/16-android-signing.md](../docs/16-android-signing.md).
This file is just build and run instructions.

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

JVM unit tests (discovery parsing — no device):

```sh
cd android && ./gradlew testDebugUnitTest
```

## Run

On a device or emulator:

```sh
cd android && ./gradlew installDebug
adb shell am start -n com.example.ttlsigner/.MainActivity
```

In the app: type a handle (or a numeric room id to skip the lookup), then
**Resolve** and **Sign**. The output shows latencies and the signature shape.
Signed URLs are shown on screen but never written to logcat.

On-device tests (need network for the one-time bundle download):

```sh
cd android && ./gradlew connectedDebugAndroidTest
```

## Layout

| File | What it is |
|---|---|
| `app/src/main/java/.../TtlNative.kt` | Raw JNI signatures; must match `crates/ttl-sign-mobile/src/jni.rs` |
| `app/src/main/java/.../RustSigner.kt` | Warm native signer: `open` once, `sign` per request |
| `app/src/main/java/.../Discovery.kt` | Unsigned `unique_id` → `room_id` lookup + parsing |
| `app/src/main/java/.../BundleFetch.kt` | Bundle download + SHA-256 pin |
| `app/src/main/java/.../MainActivity.kt` | Resolve / sign UI |
| `app/src/test/...` | JVM unit tests |
| `app/src/androidTest/...` | On-device tests: the reused core signs `ws` on a phone |
