# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

AllDesk — 跨平台局域网远程桌面控制软件。Rust 核心 + Flutter UI，支持 Windows/Linux/macOS/Android。

## Commands

```bash
# 构建脚本（统一入口）
bash build.sh              # Windows release
bash build.sh android      # Android release APK
bash build.sh android-debug [device-id]
# Windows 上等价使用 build.bat

# Rust
cargo check --workspace          # Check all crates compile
cargo build --workspace           # Build all crates
cargo test --workspace            # Run all tests
cargo clippy --workspace          # Lint
cargo fmt --check                 # Check formatting
cargo build --release -p alldesk-ffi  # Build FFI library for Flutter

# Flutter
cd app && flutter pub get         # Install dependencies
cd app && flutter analyze         # Static analysis
cd app && flutter test            # Run tests
cd app && flutter run -d windows  # Run on Windows
cd app && flutter build windows --release

# Flutter-Rust Bridge
flutter_rust_bridge_codegen generate  # Regenerate FFI bindings

# Android cross-compile
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release -p alldesk-ffi
cd app && flutter build apk --release
```

## Architecture

### Monorepo Layout

- `crates/` — Rust workspace (8 crates: core, platform, capture, codec, net, files, recording, ffi)
  - `alldesk-core` — 共享错误类型、配置
  - `alldesk-capture` — 屏幕捕获（DXGI/CoreGraphics/X11/Wayland/MediaProjection）
  - `alldesk-platform` — 本机外设集成：音频采集/播放 + 剪贴板 + 键鼠注入
  - `alldesk-codec` — 视频编解码（VP9/H.264/AV1）
  - `alldesk-net` — 网络层（QUIC P2P + 中继 + STUN + mDNS 发现）
  - `alldesk-files` — 文件传输（分块、断点续传）
  - `alldesk-recording` — 录屏（WebM 封装）
  - `alldesk-ffi` — Flutter FFI 桥接（flutter_rust_bridge v2）
- `app/` — Flutter 应用（Riverpod 状态管理 + GoRouter 导航）
- `server/` — 信令 + 中继服务器

### Key Data Flow

```
Screen Capture → Video Encoder → QUIC Transport → Video Decoder → Flutter Texture
Audio Capture  → Opus Encoder  → QUIC Datagram  → Opus Decoder  → Audio Player
Flutter Input  → FFI → Input Controller (OS injection)
```

### Platform Abstraction Pattern

每个平台特定 crate 使用 `cfg_if` 编译期选择后端：

```rust
cfg_if::cfg_if! {
    if #[cfg(target_os = "windows")] { mod dxgi; }
    else if #[cfg(target_os = "macos")] { mod quartz; }
    else if #[cfg(target_os = "linux")] { mod x11; mod wayland; }
}
```

### Network Layer (alldesk-net)

统一 `Transport` trait 抽象 P2P 和中继连接。QUIC 流复用：
- 数据报（不可靠）：视频、音频
- 流（可靠）：输入、剪贴板、文件、白板、控制

连接策略：LAN 直连 → 信令交换 → P2P 打洞（3s超时）→ 中继回退

### Flutter-Rust Bridge

`alldesk-ffi` 通过 `flutter_rust_bridge` v2 暴露 API。视频帧通过 Flutter `Texture` widget 零拷贝渲染，不经过 FFI 传递像素数据。

首次设置：
```bash
cargo install flutter_rust_bridge_codegen
cd app && flutter_rust_bridge_codegen integrate --rust-crate-dir ../crates/alldesk-ffi
flutter_rust_bridge_codegen generate   # 重新生成绑定（修改 api.rs 后运行）
```

FRB 配置在 `app/pubspec.yaml` 的 `flutter_rust_bridge:` 段。

## Key Dependencies

| 用途 | Crate |
|------|-------|
| QUIC | quinn 0.11 |
| Protobuf | prost + prost-build |
| Flutter 桥接 | flutter_rust_bridge 2 |
| 音频 I/O | cpal 0.17 |
| 剪贴板 | arboard 3.4 |
| 状态管理 | flutter_riverpod |
| 导航 | go_router |


## Tips
- 当上下文变大时，将当前状态写入 tasks/mission.md。包括：已完成的、下一步的、被阻塞的、未解决的问题。
- 错误处理：最多重试 3 次。如果没有进展，记录到 pending_for_human.md 然后转到下一个任务。
- 压缩前，务必保存完整的已修改文件列表。