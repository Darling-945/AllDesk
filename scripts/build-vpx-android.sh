#!/bin/bash
# Cross-compile libvpx for the three Android ABIs AllDesk ships, using the
# NDK clang toolchain. Run once per ABI from any POSIX-ish shell on the
# Windows host (Git Bash works; requires GNU make + perl, both bundled with
# Git for Windows / available in PATH).
#
#   bash scripts/build-vpx-android.sh arm64-v8a      # or armeabi-v7a | x86_64
#
# Prerequisites:
#   - libvpx 1.14.1 source extracted to $VPX_SRC (default /c/tmp/libvpx-1.14.1)
#   - Android NDK with clang wrappers at $NDK_TOOLCHAIN_BIN
#   - x86_64 additionally needs nasm on PATH (default /c/tmp/nasm)
# Output: $VPX_OUT/<abi>/{include,lib/libvpx.a}   (default /c/tmp/vpx-android)
#
# Notes for native-Windows make (why the script looks the way it does):
# - configure is invoked via a RELATIVE path so SRC_PATH_BARE stays relative;
#   native make cannot follow /c/...-style includes in the generated files.
# - make runs with SHELL pointed at Git's sh.exe: recipes use POSIX syntax and
#   the extensionless NDK clang wrappers are shell scripts.
# - armv7: the %.S.o rule has no -c, so plain clang would try to LINK the
#   converted asm; we pass AS="$cc -c". x86_64 uses nasm, which needs no -c.
set -e

NDK_TOOLCHAIN_BIN="${NDK_TOOLCHAIN_BIN:-/e/AndroidSdk/ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin}"
VPX_SRC="${VPX_SRC:-/c/tmp/libvpx-1.14.1}"
VPX_OUT="${VPX_OUT:-/c/tmp/vpx-android}"

export PATH="$NDK_TOOLCHAIN_BIN:/usr/bin:$PATH"

abi=$1
case $abi in
  arm64-v8a)    target=arm64-android-gcc;  cc=aarch64-linux-android21-clang ;;
  armeabi-v7a)  target=armv7-android-gcc;  cc=armv7a-linux-androideabi21-clang ;;
  x86_64)       target=x86_64-android-gcc; cc=x86_64-linux-android21-clang ;;
  *) echo "usage: $0 <arm64-v8a|armeabi-v7a|x86_64>"; exit 1 ;;
esac

if [ ! -x "$VPX_SRC/configure" ]; then
  echo "libvpx source not found at $VPX_SRC"
  echo "download v1.14.1 and extract, or set VPX_SRC"
  exit 1
fi

b=$VPX_OUT/build-$abi
rm -rf "$b"; mkdir -p "$b"; cd "$b"

# Relative path from the build dir back to the source tree (native make
# cannot follow absolute MSYS paths, so configure must be invoked relatively).
rel_path() { # $1: from dir (build, exists), $2: to dir (source, exists)
  local from=$1 to=$2 common=$from ups=0 out="" _ tail
  while [ "$to" != "$common" ] && [[ "$to" != "$common"/* ]]; do
    common=$(dirname "$common"); ups=$((ups + 1))
  done
  for _ in $(seq 1 $ups); do out="$out../"; done
  if [ "$to" = "$common" ]; then echo "${out%/}"; else echo "$out${to#"$common"/}"; fi
}
VPX_SRC_ABS=$(cd "$VPX_SRC" && pwd -P)
VPX_OUT_ABS=$(mkdir -p "$VPX_OUT" && cd "$VPX_OUT" && pwd -P)
rel=$(rel_path "$VPX_OUT_ABS/build-$abi" "$VPX_SRC_ABS")

if [ "$abi" = "x86_64" ]; then
  command -v nasm >/dev/null 2>&1 || export PATH=/c/tmp/nasm:$PATH
  command -v nasm >/dev/null 2>&1 || { echo "x86_64 needs nasm (put nasm.exe in /c/tmp/nasm or PATH)"; exit 1; }
  as=nasm
elif [ "$abi" = "armeabi-v7a" ]; then
  as="$cc -c"
else
  as=$cc
fi

SHELL8=$(cygpath -d /usr/bin/sh.exe 2>/dev/null || echo /usr/bin/sh)
echo "using SHELL=$SHELL8"

CC=$cc CXX=${cc}++ LD=$cc AS=$as AR=llvm-ar NM=llvm-nm RANLIB=llvm-ranlib STRIP=llvm-strip \
"$rel/configure" --target=$target --prefix=$VPX_OUT/$abi \
  --disable-examples --disable-tools --disable-docs --disable-unit-tests \
  --enable-pic --disable-install-bins --disable-install-srcs

make -j8 SHELL="$SHELL8"
make SHELL="$SHELL8" install
echo "DONE $abi -> $VPX_OUT/$abi"
