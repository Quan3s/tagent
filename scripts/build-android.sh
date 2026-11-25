#!/usr/bin/env bash
# Cross-build ta for Android arm64 and arm32 using the Android NDK.
# Verified with NDK 25.2.9519653 and Rust 1.100.
set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
: "${ANDROID_NDK_HOME:=/usr/lib/android-sdk/ndk/25.2.9519653}"
ANDROID_API="${ANDROID_API:-24}"

BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
if [[ ! -x "$BIN/aarch64-linux-android$ANDROID_API-clang" ]]; then
  echo "Android NDK not found or API $ANDROID_API unavailable: $ANDROID_NDK_HOME" >&2
  exit 1
fi

rustup target add aarch64-linux-android armv7-linux-androideabi
mkdir -p artifacts/android

echo "==> building arm64 (aarch64-linux-android)"
CC_aarch64_linux_android="$BIN/aarch64-linux-android$ANDROID_API-clang" \
CXX_aarch64_linux_android="$BIN/aarch64-linux-android$ANDROID_API-clang++" \
AR_aarch64_linux_android="$BIN/llvm-ar" \
RANLIB_aarch64_linux_android="$BIN/llvm-ranlib" \
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$BIN/aarch64-linux-android$ANDROID_API-clang" \
  cargo build --release --target aarch64-linux-android --bin ta
cp target/aarch64-linux-android/release/ta artifacts/android/ta-arm64

echo "==> building arm32 (armv7-linux-androideabi)"
CC_armv7_linux_androideabi="$BIN/armv7a-linux-androideabi$ANDROID_API-clang" \
CXX_armv7_linux_androideabi="$BIN/armv7a-linux-androideabi$ANDROID_API-clang++" \
AR_armv7_linux_androideabi="$BIN/llvm-ar" \
RANLIB_armv7_linux_androideabi="$BIN/llvm-ranlib" \
CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$BIN/armv7a-linux-androideabi$ANDROID_API-clang" \
  cargo build --release --target armv7-linux-androideabi --bin ta
cp target/armv7-linux-androideabi/release/ta artifacts/android/ta-arm32

echo "==> artifacts"
ls -lh artifacts/android/ta-arm64 artifacts/android/ta-arm32
