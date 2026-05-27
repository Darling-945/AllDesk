@echo off
REM Build AllDesk for Windows (release)
REM Requires: Rust (rustup), Flutter SDK, CMake, libvpx
REM Usage: build_windows.bat

setlocal

set VPX_LIB_DIR=/tmp/vpx-install/lib
set VPX_VERSION=1.13.0
set VPX_INCLUDE_DIR=/tmp/vpx-install/include
set PATH=C:\Program Files\CMake\bin;\tmp\vpx-install\lib;%PATH%

echo === [1/3] Building Rust FFI library ===
cargo build --release -p alldesk-ffi
if %ERRORLEVEL% neq 0 (
    echo Rust build failed!
    exit /b 1
)

echo === [2/3] Building Flutter Windows app ===
cd app
flutter build windows --release
if %ERRORLEVEL% neq 0 (
    echo Flutter build failed!
    exit /b 1
)
cd ..

echo === [3/3] Copying native DLLs ===
copy /Y target\release\alldesk_ffi.dll app\build\windows\x64\runner\Release\
copy /Y \tmp\vpx-install\lib\libvpx-1.dll app\build\windows\x64\runner\Release\

echo === Done! ===
echo Output: app\build\windows\x64\runner\Release\alldesk.exe
endlocal
pause
