#!/bin/bash
# Unified build script for AllDesk.
# Requires: Rust, Flutter SDK, CMake, libvpx, flutter_rust_bridge_codegen
#           Android builds additionally need: cargo-ndk, Android NDK, JDK 21+
#
# Usage:
#   bash build.sh                Windows release (default)
#   bash build.sh windows        Windows release
#   bash build.sh android        Android release APK
#   bash build.sh android-debug  Android debug APK + install [device-id]

set -e

TARGET="${1:-windows}"

export VPX_LIB_DIR="${VPX_LIB_DIR:-/tmp/vpx-install/lib}"
export VPX_VERSION="${VPX_VERSION:-1.13.0}"
export VPX_INCLUDE_DIR="${VPX_INCLUDE_DIR:-/tmp/vpx-install/include}"

case "$TARGET" in
  windows)
    export PATH="/c/Program Files/CMake/bin:$VPX_LIB_DIR:$PATH"

    echo "=== [1/4] Generating Flutter-Rust bindings ==="
    flutter_rust_bridge_codegen generate

    echo "=== [2/4] Building Rust FFI library ==="
    cargo build --release -p alldesk-ffi

    echo "=== [3/4] Building Flutter Windows app ==="
    ( cd app && flutter build windows --release )

    echo "=== [4/4] Copying native DLLs ==="
    cp target/release/alldesk_ffi.dll app/build/windows/x64/runner/Release/
    [ -f "$VPX_LIB_DIR/libvpx-1.dll" ] && \
        cp "$VPX_LIB_DIR/libvpx-1.dll" app/build/windows/x64/runner/Release/

    echo "=== Build complete ==="
    echo "Output: app/build/windows/x64/runner/Release/"
    ;;

  android|android-debug)
    export ANDROID_HOME="${ANDROID_HOME:-E:/Program Files/Android/Sdk}"
    export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/26.1.10909125}"
    export JAVA_HOME="${JAVA_HOME:-E:/Program Files/jdk-21.0.6+7}"
    export PATH="/c/Program Files/CMake/bin:$VPX_LIB_DIR:$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"

    PROFILE="release"
    [ "$TARGET" = "android-debug" ] && PROFILE="debug"

    echo "=== [1/4] Building Rust FFI for Android ($PROFILE) ==="
    cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p alldesk-ffi $([ "$PROFILE" = "release" ] && echo --release)

    echo "=== [2/4] Copying .so to jniLibs ==="
    JNI_DIR="app/android/app/src/main/jniLibs"
    mkdir -p "$JNI_DIR/arm64-v8a" "$JNI_DIR/armeabi-v7a" "$JNI_DIR/x86_64"
    cp "target/aarch64-linux-android/$PROFILE/liballdesk_ffi.so" "$JNI_DIR/arm64-v8a/"
    cp "target/armv7-linux-androideabi/$PROFILE/liballdesk_ffi.so" "$JNI_DIR/armeabi-v7a/"
    cp "target/x86_64-linux-android/$PROFILE/liballdesk_ffi.so" "$JNI_DIR/x86_64/"

    echo "=== [3/4] Generating Flutter-Rust bindings ==="
    flutter_rust_bridge_codegen generate

    echo "=== [4/4] Flutter build/install ==="
    cd app
    if [ "$TARGET" = "android-debug" ]; then
        flutter install --debug ${2:+-d "$2"}
    else
        flutter build apk --release
        echo "=== Build complete ==="
        echo "Output: app/build/app/outputs/flutter-apk/app-release.apk"
    fi
    ;;

  *)
    echo "Unknown target: $TARGET (use windows / android / android-debug)"
    exit 1
    ;;
esac
