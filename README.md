# AllDesk

跨平台局域网远程桌面控制软件。基于 Rust 核心 + Flutter UI 构建，支持 Windows、Linux、macOS 和 Android。

## 功能

- 屏幕捕获与实时传输（DXGI / CoreGraphics / X11 / Wayland / MediaProjection）
- 视频编解码（VP9 / H.264 / AV1）
- 音频采集与播放（cpal + Opus）
- 键鼠控制（平台原生 API 注入）
- 剪贴板同步
- 文件传输（分块、断点续传）
- 录屏（WebM 封装）
- 白板标注与同步
- QUIC P2P 直连 + 中继回退 + STUN + mDNS 发现

## 架构

```
┌─────────────────────────────────────────────────────┐
│                   Flutter UI (Riverpod)              │
├─────────────────────────────────────────────────────┤
│                alldesk-ffi (flutter_rust_bridge v2)  │
├──────────┬──────────┬──────────┬──────────┬─────────┤
│ capture  │  codec   │  audio   │  input   │clipboard│
├──────────┴──────────┴──────────┴──────────┴─────────┤
│            alldesk-net (QUIC Transport)              │
├─────────────────────────────────────────────────────┤
│               alldesk-core (共享类型/配置)            │
└─────────────────────────────────────────────────────┘
```

### Monorepo 布局

| 目录 | 说明 |
|------|------|
| `crates/alldesk-core` | 共享错误类型、配置 |
| `crates/alldesk-capture` | 屏幕捕获（DXGI / CoreGraphics / X11 / Wayland / MediaProjection） |
| `crates/alldesk-platform` | 本机外设集成：音频采集/播放 + 剪贴板 + 键鼠注入 |
| `crates/alldesk-codec` | 视频编解码（VP9 / H.264 / AV1） |
| `crates/alldesk-net` | 网络层（QUIC P2P + 中继 + STUN + mDNS） |
| `crates/alldesk-files` | 文件传输（分块、断点续传） |
| `crates/alldesk-recording` | 录屏（WebM 封装） |
| `crates/alldesk-ffi` | Flutter FFI 桥接 |
| `app/` | Flutter 应用 |
| `server/` | 信令 + 中继服务器 |

### 数据流

```
屏幕捕获 → 视频编码 → QUIC 传输 → 视频解码 → Flutter Texture 渲染
音频采集 → Opus 编码 → QUIC 数据报 → Opus 解码 → 音频播放
Flutter 输入 → FFI → 输入控制器（OS 注入）
```

## 快速开始

### 前置要求

- Rust 1.75+
- Flutter 3.22+
- `flutter_rust_bridge_codegen` CLI

### 构建

```bash
# Rust 编译检查
cargo check --workspace

# 构建 FFI 库
cargo build --release -p alldesk-ffi

# Flutter 依赖安装
cd app && flutter pub get

# 重新生成 FFI 绑定（修改 api.rs 后运行）
flutter_rust_bridge_codegen generate

# Windows 运行
cd app && flutter run -d windows

# Windows 发布构建
cd app && flutter build windows --release
```

### Android 交叉编译

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release -p alldesk-ffi
cd app && flutter build apk --release
```

## 主要依赖

| 用途 | Rust | Dart/Flutter |
|------|------|-------------|
| QUIC | quinn 0.11 | — |
| Protobuf | prost | — |
| Flutter 桥接 | flutter_rust_bridge 2 | flutter_rust_bridge |
| 音频 I/O | cpal 0.17 | — |
| 剪贴板 | arboard 3.4 | — |
| 状态管理 | — | flutter_riverpod |
| 导航 | — | go_router |

## 开发

```bash
# Lint
cargo clippy --workspace
cargo fmt --check

# Flutter 静态分析
cd app && flutter analyze

# 测试
cargo test --workspace
cd app && flutter test
```

## 许可证

MIT
