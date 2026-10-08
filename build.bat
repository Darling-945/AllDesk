@echo off
REM Unified build script for AllDesk (Windows host).
REM Requires: Rust, Flutter SDK, CMake, libvpx, flutter_rust_bridge_codegen
REM           Android builds additionally need: cargo-ndk, Android NDK, JDK 21+
REM
REM Usage:
REM   build.bat                 Windows release (default)
REM   build.bat windows         Windows release
REM   build.bat android         Android release APK
REM   build.bat android-debug   Android debug APK + install (optional device id:
REM                             build.bat android-debug ^<device-id^>)

setlocal

set TARGET=%1
if "%TARGET%"=="" set TARGET=windows

if "%VPX_LIB_DIR%"=="" set VPX_LIB_DIR=C:\tmp\vpx-install\lib
REM Must match the libvpx runtime in VPX_LIB_DIR (see libvpx-native-sys
REM pregenerated bindings; 1.14.0 bindings match a 1.14.x runtime).
set VPX_VERSION=1.14.0
set VPX_INCLUDE_DIR=C:\tmp\vpx-install\include
set PATH=C:\Program Files\CMake\bin;%VPX_LIB_DIR%;%PATH%

if /I "%TARGET%"=="windows" goto windows
if /I "%TARGET%"=="android" goto android_release
if /I "%TARGET%"=="android-debug" goto android_debug
echo Unknown target: %TARGET% (use windows / android / android-debug)
exit /b 1

:windows
echo === [1/4] Generating Flutter-Rust bindings ===
call flutter_rust_bridge_codegen generate
if %ERRORLEVEL% neq 0 exit /b 1

echo === [2/4] Building Rust FFI library ===
cargo build --release -p alldesk-ffi
if %ERRORLEVEL% neq 0 exit /b 1

echo === [3/4] Building Flutter Windows app ===
pushd app
call flutter build windows --release
if %ERRORLEVEL% neq 0 exit /b 1
popd

echo === [4/4] Copying native DLLs ===
copy /Y target\release\alldesk_ffi.dll app\build\windows\x64\runner\Release\
if exist "%VPX_LIB_DIR%\libvpx-1.dll" copy /Y "%VPX_LIB_DIR%\libvpx-1.dll" app\build\windows\x64\runner\Release\

echo === Build complete ===
echo Output: app\build\windows\x64\runner\Release\
endlocal
exit /b 0

:android_release
set PATH=%JAVA_HOME%\bin;%ANDROID_HOME%\platform-tools;%PATH%
if "%ANDROID_NDK_HOME%"=="" set ANDROID_NDK_HOME=E:\AndroidSdk\ndk\28.2.13676358
if "%ANDROID_HOME%"=="" set ANDROID_HOME=E:\AndroidSdk

echo === [1/4] Building Rust FFI for Android (arm64, armv7, x86_64) ===
REM Each ABI links its own statically cross-compiled libvpx (build with
REM scripts\build-vpx-android.sh once per ABI; output in C:\tmp\vpx-android).
set VPX_STATIC=1
set VPX_VERSION=1.14.0
set JNI_DIR=app\android\app\src\main\jniLibs
if not exist "%JNI_DIR%\arm64-v8a" mkdir "%JNI_DIR%\arm64-v8a"
if not exist "%JNI_DIR%\armeabi-v7a" mkdir "%JNI_DIR%\armeabi-v7a"
if not exist "%JNI_DIR%\x86_64" mkdir "%JNI_DIR%\x86_64"
for %%T in ("aarch64-linux-android:arm64-v8a" "armv7-linux-androideabi:armeabi-v7a" "x86_64-linux-android:x86_64") do (
  for /f "tokens=1,2 delims=:" %%A in (%%T) do (
    echo --- target %%A (%%B) ---
    set VPX_LIB_DIR=C:\tmp\vpx-android\%%B\lib
    set VPX_INCLUDE_DIR=C:\tmp\vpx-android\%%B\include
    cargo ndk --platform 26 -t %%A build --release -p alldesk-ffi
    if errorlevel 1 exit /b 1
    copy /Y target\%%A\release\liballdesk_ffi.so "%JNI_DIR%\%%B\"
  )
)

echo === [2/4] Generating Flutter-Rust bindings ===
call flutter_rust_bridge_codegen generate
if %ERRORLEVEL% neq 0 exit /b 1

echo === [3/4] Building Flutter APK ===
pushd app
call flutter build apk --release
if %ERRORLEVEL% neq 0 exit /b 1
popd

echo === Build complete ===
echo Output: app\build\app\outputs\flutter-apk\app-release.apk
endlocal
exit /b 0

:android_debug
set PATH=%JAVA_HOME%\bin;%ANDROID_HOME%\platform-tools;%PATH%
if "%ANDROID_NDK_HOME%"=="" set ANDROID_NDK_HOME=E:\AndroidSdk\ndk\28.2.13676358
if "%ANDROID_HOME%"=="" set ANDROID_HOME=E:\AndroidSdk

echo === [1/4] Building Rust FFI for Android (debug) ===
REM Per-ABI static libvpx, same as the release path above.
set VPX_STATIC=1
set VPX_VERSION=1.14.0
set JNI_DIR=app\android\app\src\main\jniLibs
if not exist "%JNI_DIR%\arm64-v8a" mkdir "%JNI_DIR%\arm64-v8a"
if not exist "%JNI_DIR%\armeabi-v7a" mkdir "%JNI_DIR%\armeabi-v7a"
if not exist "%JNI_DIR%\x86_64" mkdir "%JNI_DIR%\x86_64"
for %%T in ("aarch64-linux-android:arm64-v8a" "armv7-linux-androideabi:armeabi-v7a" "x86_64-linux-android:x86_64") do (
  for /f "tokens=1,2 delims=:" %%A in (%%T) do (
    echo --- target %%A ---
    set VPX_LIB_DIR=C:\tmp\vpx-android\%%B\lib
    set VPX_INCLUDE_DIR=C:\tmp\vpx-android\%%B\include
    cargo ndk --platform 26 -t %%A build -p alldesk-ffi
    if errorlevel 1 exit /b 1
    copy /Y target\%%A\debug\liballdesk_ffi.so "%JNI_DIR%\%%B\"
  )
)

echo === [3/4] Generating Flutter-Rust bindings ===
call flutter_rust_bridge_codegen generate
if %ERRORLEVEL% neq 0 exit /b 1

echo === [4/4] Installing debug APK to device ===
pushd app
if "%~2"=="" (
    call flutter install --debug
) else (
    call flutter install --debug -d %~2
)
if %ERRORLEVEL% neq 0 exit /b 1
popd

echo === Done ===
endlocal
exit /b 0
