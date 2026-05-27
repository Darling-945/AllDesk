#!/bin/bash
# Build AllDesk for Linux (full: codegen + build)
# Requires: Rust (rustup), Flutter SDK, libx11-dev libxcb1-dev libpulse-dev
#           libvpx-dev, cmake, flutter_rust_bridge_codegen
# Usage: bash build.sh

set -e

echo "=== [1/3] Generating Flutter-Rust bindings ==="
flutter_rust_bridge_codegen generate

echo "=== [2/3] Building Rust FFI library ==="
cargo build --release -p alldesk-ffi

echo "=== [3/3] Building Flutter Linux app ==="
cd app
flutter build linux --release
cd ..

echo "=== Build complete! ==="
echo "Output: app/build/linux/x64/release/bundle/"
