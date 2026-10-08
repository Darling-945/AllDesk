#!/bin/bash
# One-click AllDesk build for macOS. Apple Silicon (arm64) is the primary
# target; pass --universal to also embed an x86_64 slice (Intel Macs and
# Rosetta-free cross-use).
#
#   bash scripts/build-macos.sh              # arm64 release (default)
#   bash scripts/build-macos.sh --debug      # arm64 debug
#   bash scripts/build-macos.sh --universal  # arm64 + x86_64 universal dylib
#
# What it does (all idempotent — rerun any time):
#   1. Verifies Xcode + Command Line Tools (installs CLT via system prompt)
#   2. Installs Homebrew, Rust (rustup), Flutter (brew cask) if missing
#   3. Installs flutter_rust_bridge_codegen 2.12.0 if missing
#   4. Downloads + builds libvpx 1.14.1 (static, arm64 [+ x86_64]) into
#      ~/.alldesk-deps/vpx-macos (override with ALLDESK_DEPS)
#   5. Generates the FRB bindings (frb_generated.rs is gitignored)
#   6. cargo build --release -p alldesk-ffi (links libvpx statically)
#   7. flutter create --platforms=macos (once) + flutter build macos
#   8. Copies liballdesk_ffi.dylib into AllDesk.app/Contents/MacOS and
#      re-signs the bundle ad-hoc
#
# First run on a fresh Mac needs: Xcode from the App Store (macOS desktop
# builds do not work with Command Line Tools alone).

set -euo pipefail

UNIVERSAL=0
DEBUG=0
for arg in "$@"; do
  case "$arg" in
    --universal) UNIVERSAL=1 ;;
    --debug) DEBUG=1 ;;
    *) echo "unknown option: $arg (use --universal / --debug)"; exit 1 ;;
  esac
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DEPS="${ALLDESK_DEPS:-$HOME/.alldesk-deps}"
VPX_OUT="$DEPS/vpx-macos"
# Must match libvpx-native-sys pregenerated bindings (1.14.0 <-> 1.14.x).
export VPX_VERSION=1.14.0
export VPX_STATIC=1

log() { printf '\033[1;32m==> %s\033[0m\n' "$*"; }
die() { printf '\033[1;31mERROR: %s\033[0m\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1; }

# ---------------------------------------------------------------- 1. Xcode
if ! xcode-select -p >/dev/null 2>&1; then
  log "Requesting Xcode Command Line Tools installation"
  xcode-select --install >/dev/null 2>&1 || true
  die "Command Line Tools are being installed — rerun this script when the system installer finishes"
fi
if ! xcodebuild -version >/dev/null 2>&1; then
  die "Full Xcode is required for macOS desktop builds (App Store > Xcode), then: sudo xcode-select -s /Applications/Xcode.app"
fi

# ------------------------------------------------------- 2. Brew / Rust / Flutter
if ! need brew; then
  log "Installing Homebrew"
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  eval "$(/opt/homebrew/bin/brew shellenv 2>/dev/null || /usr/local/bin/brew shellenv)"
fi
if [ "$UNIVERSAL" = 1 ] && ! need nasm; then
  log "Installing nasm (x86_64 libvpx asm)"
  brew install nasm
fi
if ! need cargo; then
  log "Installing Rust via rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi
if ! need flutter; then
  log "Installing Flutter (brew cask)"
  brew install --cask flutter
fi
if ! need pod; then
  log "Installing CocoaPods (required for Flutter macOS plugin builds)"
  brew install cocoapods
fi
if ! need flutter_rust_bridge_codegen; then
  log "Installing flutter_rust_bridge_codegen 2.12.0 (compiles for a few minutes)"
  cargo install flutter_rust_bridge_codegen --version 2.12.0 --locked
fi

# ---------------------------------------------------------------- 3. libvpx
build_vpx() { # $1 = libvpx target, $2 = output prefix
  local target="$1" out="$2" src="$DEPS/libvpx-1.14.1"
  if [ -f "$out/lib/libvpx.a" ]; then
    log "libvpx ($target) already built"
    return
  fi
  if [ ! -d "$src" ]; then
    log "Downloading libvpx 1.14.1"
    mkdir -p "$DEPS"
    curl -sSL -o "$DEPS/libvpx-1.14.1.tar.gz" \
      https://github.com/webmproject/libvpx/archive/refs/tags/v1.14.1.tar.gz
    tar -xzf "$DEPS/libvpx-1.14.1.tar.gz" -C "$DEPS"
  fi
  local b="$DEPS/build-${target}"
  rm -rf "$b"; mkdir -p "$b"; cd "$b"
  log "Building libvpx ($target)"
  CC=clang CXX=clang++ "$src/configure" --target="$target" --prefix="$out" \
    --disable-examples --disable-tools --disable-docs --disable-unit-tests \
    --enable-pic --disable-install-bins --disable-install-srcs
  make -j"$(sysctl -n hw.ncpu)"
  make install
  cd "$ROOT"
}

build_vpx arm64-darwin-gcc "$VPX_OUT/arm64"

VPX_LIB_DIR_FOR_BUILD="$VPX_OUT/arm64"
if [ "$UNIVERSAL" = 1 ]; then
  build_vpx x86_64-darwin20-gcc "$VPX_OUT/x86_64"
  # Combine both arches into one static lib + one Rust dylib.
  mkdir -p "$VPX_OUT/universal/lib"
  cp -R "$VPX_OUT/arm64/include" "$VPX_OUT/universal/"
  lipo -create "$VPX_OUT/arm64/lib/libvpx.a" "$VPX_OUT/x86_64/lib/libvpx.a" \
    -output "$VPX_OUT/universal/lib/libvpx.a"
  VPX_LIB_DIR_FOR_BUILD="$VPX_OUT/universal"
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
fi
export VPX_LIB_DIR="$VPX_LIB_DIR_FOR_BUILD/lib"
export VPX_INCLUDE_DIR="$VPX_LIB_DIR_FOR_BUILD/include"

# ---------------------------------------------------------------- 4. FRB codegen
# frb_generated.rs is gitignored, so a fresh checkout cannot build without it.
log "Generating Flutter-Rust bindings"
(cd app && flutter_rust_bridge_codegen generate)

# ---------------------------------------------------------------- 5. Rust dylib
# (plain strings, not arrays: macOS ships bash 3.2 where "${arr[@]}" of an
# empty array under set -u is an error)
CARGO_PROFILE_ARGS="--release"
PROFILE_DIR="release"
if [ "$DEBUG" = 1 ]; then
  CARGO_PROFILE_ARGS=""
  PROFILE_DIR="debug"
fi

if [ "$UNIVERSAL" = 0 ]; then
  log "Building Rust library (arm64, $PROFILE_DIR)"
  # shellcheck disable=SC2086
  cargo build $CARGO_PROFILE_ARGS -p alldesk-ffi
  DYLIB="target/$PROFILE_DIR/liballdesk_ffi.dylib"
else
  log "Building Rust library (universal: aarch64 + x86_64, $PROFILE_DIR)"
  # shellcheck disable=SC2086
  cargo build $CARGO_PROFILE_ARGS --target aarch64-apple-darwin -p alldesk-ffi
  # shellcheck disable=SC2086
  cargo build $CARGO_PROFILE_ARGS --target x86_64-apple-darwin -p alldesk-ffi
  mkdir -p target/universal
  lipo -create \
    "target/aarch64-apple-darwin/$PROFILE_DIR/liballdesk_ffi.dylib" \
    "target/x86_64-apple-darwin/$PROFILE_DIR/liballdesk_ffi.dylib" \
    -output target/universal/liballdesk_ffi.dylib
  DYLIB="target/universal/liballdesk_ffi.dylib"
fi
[ -f "$DYLIB" ] || die "Rust dylib missing: $DYLIB"

# ---------------------------------------------------------------- 6. Flutter app
# The repo only ships windows/android platform folders; create macos once.
if [ ! -d app/macos ]; then
  log "Creating the macOS Flutter runner (first run only)"
  (cd app && flutter create --platforms=macos .)
fi
log "Building the Flutter macOS app"
if [ "$DEBUG" = 1 ]; then
  (cd app && flutter build macos --debug)
else
  (cd app && flutter build macos --release)
fi

# ---------------------------------------------------------------- 7. Bundle
FLUTTER_PROFILE="Release"; [ "$DEBUG" = 1 ] && FLUTTER_PROFILE="Debug"
APP=$(ls -d app/build/macos/Build/Products/"$FLUTTER_PROFILE"/*.app 2>/dev/null | head -n1 || true)
[ -n "$APP" ] || die "no .app found under app/build/macos/Build/Products/$FLUTTER_PROFILE"
log "Bundling liballdesk_ffi.dylib into $APP"
cp "$DYLIB" "$APP/Contents/MacOS/"
# Re-sign ad-hoc; copying into the bundle invalidates the release signature.
codesign --force --deep --sign - "$APP"

log "Done: $APP"
cat <<'EOF'

Before the first remote-control session, grant the app two permissions
(System Settings > Privacy & Security):
  * Screen Recording   — host side capture (restart the app after granting)
  * Accessibility      — host side mouse/keyboard injection

The viewer side needs no permissions.
EOF
