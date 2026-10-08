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
# Must match the libvpx runtime in VPX_LIB_DIR (libvpx-native-sys pregenerated
# bindings; 1.14.0 bindings match a 1.14.x runtime).
export VPX_VERSION="${VPX_VERSION:-1.14.0}"
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
    export ANDROID_HOME="${ANDROID_HOME:-E:/AndroidSdk}"
    export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-E:/AndroidSdk/ndk/28.2.13676358}"
    export JAVA_HOME="${JAVA_HOME:-C:/Program Files/Microsoft/jdk-21.0.10.7-hotspot}"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"

    PROFILE="release"
    [ "$TARGET" = "android-debug" ] && PROFILE="debug"

    echo "=== [1/4] Building Rust FFI for Android ($PROFILE) ==="
    # Each ABI links its own statically cross-compiled libvpx (built with
    # scripts/build-vpx-android.sh once per ABI; output in /c/tmp/vpx-android).
    export VPX_STATIC=1 VPX_VERSION=1.14.0
    JNI_DIR="app/android/app/src/main/jniLibs"
    mkdir -p "$JNI_DIR/arm64-v8a" "$JNI_DIR/armeabi-v7a" "$JNI_DIR/x86_64"
    for pair in aarch64-linux-android:arm64-v8a \
                armv7-linux-androideabi:armeabi-v7a \
                x86_64-linux-android:x86_64; do
      rust_target=${pair%%:*}; abi=${pair##*:}
      echo "--- target $rust_target ($abi) ---"
      export VPX_LIB_DIR="/c/tmp/vpx-android/$abi/lib"
      export VPX_INCLUDE_DIR="/c/tmp/vpx-android/$abi/include"
      # Platform 26: cpal's aaudio backend needs API 26+.
      cargo ndk --platform 26 -t "$rust_target" build -p alldesk-ffi $([ "$PROFILE" = "release" ] && echo --release)
      cp "target/$rust_target/$PROFILE/liballdesk_ffi.so" "$JNI_DIR/$abi/"
    done

    echo "=== [2/4] Generating Flutter-Rust bindings ==="
    flutter_rust_bridge_codegen generate

    echo "=== [3/4] Flutter build/install ==="
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
