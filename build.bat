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
set VPX_VERSION=1.13.0
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

echo === [1/4] Building Rust FFI for Android (arm64, armv7, x86_64) ===
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release -p alldesk-ffi
if %ERRORLEVEL% neq 0 exit /b 1

echo === [2/4] Copying .so to jniLibs ===
set JNI_DIR=app\android\app\src\main\jniLibs
if not exist "%JNI_DIR%\arm64-v8a" mkdir "%JNI_DIR%\arm64-v8a"
if not exist "%JNI_DIR%\armeabi-v7a" mkdir "%JNI_DIR%\armeabi-v7a"
if not exist "%JNI_DIR%\x86_64" mkdir "%JNI_DIR%\x86_64"
copy /Y target\aarch64-linux-android\release\liballdesk_ffi.so "%JNI_DIR%\arm64-v8a\"
copy /Y target\armv7-linux-androideabi\release\liballdesk_ffi.so "%JNI_DIR%\armeabi-v7a\"
copy /Y target\x86_64-linux-android\release\liballdesk_ffi.so "%JNI_DIR%\x86_64\"

echo === [3/4] Generating Flutter-Rust bindings ===
call flutter_rust_bridge_codegen generate
if %ERRORLEVEL% neq 0 exit /b 1

echo === [4/4] Building Flutter APK ===
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

echo === [1/4] Building Rust FFI for Android (debug) ===
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build -p alldesk-ffi
if %ERRORLEVEL% neq 0 exit /b 1

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
