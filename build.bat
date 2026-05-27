@echo off
REM Build AllDesk for Windows (full: codegen + build)
REM Requires: Rust (rustup), Flutter SDK, CMake, libvpx, flutter_rust_bridge_codegen
REM Usage: build.bat

setlocal

set VPX_LIB_DIR=/tmp/vpx-install/lib
set VPX_VERSION=1.13.0
set VPX_INCLUDE_DIR=/tmp/vpx-install/include
set PATH=C:\Program Files\CMake\bin;\tmp\vpx-install\lib;%PATH%

echo === [1/4] Generating Flutter-Rust bindings ===
call flutter_rust_bridge_codegen generate
if %ERRORLEVEL% neq 0 (
    echo FRB codegen failed!
    exit /b 1
)

echo === [2/4] Building Rust FFI library ===
cargo build --release -p alldesk-ffi
if %ERRORLEVEL% neq 0 (
    echo Rust build failed!
    exit /b 1
)

echo === [3/4] Building Flutter Windows app ===
cd app
call flutter build windows --release
if %ERRORLEVEL% neq 0 (
    echo Flutter build failed!
    exit /b 1
)
cd ..

echo === [4/4] Copying native DLLs ===
copy /Y target\release\alldesk_ffi.dll app\build\windows\x64\runner\Release\
copy /Y \tmp\vpx-install\lib\libvpx-1.dll app\build\windows\x64\runner\Release\

echo === Build complete! ===
echo Output: app\build\windows\x64\runner\Release\
endlocal
