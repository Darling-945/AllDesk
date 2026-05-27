@echo off
REM Build and install AllDesk debug APK to connected device
REM Requires: Rust (rustup), Flutter SDK, cargo-ndk, Android NDK, JDK 21+
REM Usage: build_android_debug.bat [device_id]
REM Example: build_android_debug.bat 3B6F5PE8GCL3PFEP

setlocal

set ANDROID_HOME=E:\Program Files\Android\Sdk
set ANDROID_NDK_HOME=%ANDROID_HOME%\ndk\26.1.10909125
set JAVA_HOME=E:\Program Files\jdk-21.0.6+7
set VPX_LIB_DIR=/tmp/vpx-install/lib
set VPX_VERSION=1.13.0
set VPX_INCLUDE_DIR=/tmp/vpx-install/include
set PATH=C:\Program Files\CMake\bin;\tmp\vpx-install\lib;%JAVA_HOME%\bin;%ANDROID_HOME%\platform-tools;%PATH%

echo === [1/4] Building Rust FFI for Android (debug) ===
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p alldesk-ffi
if %ERRORLEVEL% neq 0 (
    echo Rust cross-compile failed!
    exit /b 1
)

echo === [2/4] Copying .so to jniLibs ===
set JNI_DIR=app\android\app\src\main\jniLibs
if not exist "%JNI_DIR%\arm64-v8a" mkdir "%JNI_DIR%\arm64-v8a"
if not exist "%JNI_DIR%\armeabi-v7a" mkdir "%JNI_DIR%\armeabi-v7a"
if not exist "%JNI_DIR%\x86_64" mkdir "%JNI_DIR%\x86_64"
copy /Y target\aarch64-linux-android\debug\liballdesk_ffi.so "%JNI_DIR%\arm64-v8a\"
copy /Y target\armv7-linux-androideabi\debug\liballdesk_ffi.so "%JNI_DIR%\armeabi-v7a\"
copy /Y target\x86_64-linux-android\debug\liballdesk_ffi.so "%JNI_DIR%\x86_64\"

echo === [3/4] Generating Flutter-Rust bindings ===
call flutter_rust_bridge_codegen generate
if %ERRORLEVEL% neq 0 (
    echo FRB codegen failed!
    exit /b 1
)

echo === [4/4] Installing debug APK to device ===
cd app
if "%~1"=="" (
    call flutter install --debug
) else (
    call flutter install --debug -d %1
)
if %ERRORLEVEL% neq 0 (
    echo Install failed!
    exit /b 1
)
cd ..

echo === Done! ===
endlocal
