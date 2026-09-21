#!/bin/sh
# Build `libttl_sign_mobile.so` for Android and drop it into the demo app.
#
#   scripts/android/build-native.sh [arm64-v8a ...]
#
# With no arguments, builds all four ABIs. Each ABI is its own `cargo ndk`
# invocation so bindgen gets that ABI's `--target`/`--sysroot`: rquickjs-sys
# generates its C bindings at build time on Android (see the `bindgen` note in
# `crates/ttl-sign-mobile/Cargo.toml`), and generating them under the host's
# data layout would be wrong for the 32-bit ABIs.
#
# Needs: the Android NDK (`ANDROID_NDK_HOME`), the `*-linux-android` Rust
# targets (`rustup target add ...`), `cargo-ndk`, and libclang (for bindgen).
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUT="$ROOT/android/app/src/main/jniLibs"
API=26 # must match the demo app's minSdk

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  for candidate in "${ANDROID_HOME:-}/ndk" "${ANDROID_SDK_ROOT:-}/ndk" /opt/android-sdk/ndk; do
    if [ -d "$candidate" ]; then
      ANDROID_NDK_HOME=$(ls -d "$candidate"/*/ 2>/dev/null | sort -V | tail -n 1)
      ANDROID_NDK_HOME=${ANDROID_NDK_HOME%/}
      break
    fi
  done
fi
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK directory}"
SYSROOT="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot"
if [ ! -d "$SYSROOT" ]; then
  echo "no NDK sysroot at $SYSROOT" >&2
  exit 1
fi

# bindgen needs libclang; point it at the system one when it is not already set.
if [ -z "${LIBCLANG_PATH:-}" ]; then
  for lib in /usr/lib/llvm*/lib/libclang.so* /usr/lib/x86_64-linux-gnu/libclang.so*; do
    if [ -f "$lib" ]; then
      LIBCLANG_PATH=$(dirname "$lib")
      export LIBCLANG_PATH
      break
    fi
  done
fi

command -v cargo-ndk >/dev/null 2>&1 || {
  echo "install cargo-ndk first: cargo install cargo-ndk" >&2
  exit 1
}

abis=${*:-"arm64-v8a armeabi-v7a x86_64 x86"}

for abi in $abis; do
  case $abi in
    arm64-v8a) triple=aarch64-linux-android ;;
    armeabi-v7a) triple=armv7-linux-androideabi ;;
    x86_64) triple=x86_64-linux-android ;;
    x86) triple=i686-linux-android ;;
    *) echo "unknown ABI: $abi" >&2; exit 1 ;;
  esac
  echo "=== $abi ($triple) ==="
  export BINDGEN_EXTRA_CLANG_ARGS="--target=$triple$API --sysroot=$SYSROOT"
  (cd "$ROOT" && cargo ndk -t "$abi" -o "$OUT" build --release \
    -p ttl-sign-mobile --features jni)
done

echo "=== installed ==="
ls -la "$OUT"/*/*.so
