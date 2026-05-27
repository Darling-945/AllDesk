#!/bin/bash
# Build AllDesk for Android (release APK)
# Requires: Rust (rustup), Flutter SDK, cargo-ndk, Android NDK, JDK 21+
# Usage: bash build_android.sh

set -e

export ANDROID_HOME="${ANDROID_HOME:-E:/Program Files/Android/Sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/26.1.10909125}"
export JAVA_HOME="${JAVA_HOME:-E:/Program Files/jdk-21.0.6+7}"
export VPX_LIB_DIR="${VPX_LIB_DIR:-/tmp/vpx-install/lib}"
export VPX_VERSION="${VPX_VERSION:-1.13.0}"
export VPX_INCLUDE_DIR="${VPX_INCLUDE_DIR:-/tmp/vpx-install/include}"
export PATH="/c/Program Files/CMake/bin:/tmp/vpx-install/lib:$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"

echo "=== [1/4] Building Rust FFI for Android (arm64, armv7, x86_64) ==="
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release -p alldesk-ffi

echo "=== [2/4] Copying .so to jniLibs ==="
JNI_DIR="app/android/app/src/main/jniLibs"
mkdir -p "$JNI_DIR/arm64-v8a" "$JNI_DIR/armeabi-v7a" "$JNI_DIR/x86_64"
cp target/aarch64-linux-android/release/liballdesk_ffi.so "$JNI_DIR/arm64-v8a/"
cp target/armv7-linux-androideabi/release/liballdesk_ffi.so "$JNI_DIR/armeabi-v7a/"
cp target/x86_64-linux-android/release/liballdesk_ffi.so "$JNI_DIR/x86_64/"

echo "=== [3/4] Generating Flutter-Rust bindings ==="
flutter_rust_bridge_codegen generate

echo "=== [4/4] Building Flutter APK ==="
cd app
flutter build apk --release
cd ..

echo "=== Done! ==="
echo "Output: app/build/app/outputs/flutter-apk/app-release.apk"
