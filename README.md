# AllDesk

跨平台局域网远程桌面控制软件。基于 Rust 核心 + Flutter UI 构建，支持 Windows、Linux、macOS 和 Android。

## 功能

- 屏幕捕获与实时传输（Windows DXGI；Android MediaProjection；macOS/Linux 后端待实现）
- 视频编解码（VP9 软编软解，RTT/丢包驱动的自适应码率与帧率）
- 音频采集与播放（cpal，PCM 直传）
- 键鼠控制（平台原生 API 注入）
- 剪贴板同步
- 文件传输（分块 + CRC32 校验）
- 会话录屏（ALDREC/WebM，VP9 直存）
- QUIC P2P 直连 + mDNS 发现 + 断线自动重连

## 架构

```
┌──────────────────────────────────────────────────────────┐
│                     Flutter UI (Riverpod)                 │
├──────────────────────────────────────────────────────────┤
│                alldesk-ffi (flutter_rust_bridge v2)       │
├──────────┬──────────┬───────────┬─────────┬──────────────┤
│ capture  │  codec   │ platform  │  files  │  recording   │
├──────────┴──────────┴───────────┴─────────┴──────────────┤
│             alldesk-net (QUIC Transport)                 │
├──────────────────────────────────────────────────────────┤
│        alldesk-core (共享类型 / 配置 / 自适应控制)          │
└──────────────────────────────────────────────────────────┘
```

### Monorepo 布局

| 目录 | 说明 |
|------|------|
| `crates/alldesk-core` | 共享错误类型、配置、自适应码率/帧率控制策略 |
| `crates/alldesk-capture` | 屏幕捕获（Windows DXGI / Android MediaProjection） |
| `crates/alldesk-platform` | 本机外设集成：音频采集/播放 + 剪贴板 + 键鼠注入 |
| `crates/alldesk-codec` | 视频编解码（VP9） |
| `crates/alldesk-net` | 网络层（QUIC P2P + 流控 + 重连 + mDNS；中继/STUN/ICE/TURN 在 `server/`） |
| `crates/alldesk-files` | 文件传输（分块 + CRC32 校验） |
| `crates/alldesk-recording` | 录屏（ALDREC/WebM 封装） |
| `crates/alldesk-ffi` | Flutter FFI 桥接 |
| `app/` | Flutter 应用 |
| `server/` | 信令 + 中继服务器 |

### 数据流

```
屏幕捕获 → VP9 编码（自适应码率/帧率）→ QUIC 传输 → VP9 解码 → Flutter 渲染
音频采集 → PCM → QUIC 数据报 → PCM 播放
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
| 视频编解码 | libvpx (VP9) | — |
| Flutter 桥接 | flutter_rust_bridge 2 | flutter_rust_bridge |
| 音频 I/O | cpal 0.17 | — |
| 剪贴板 | arboard 3.4 | — |
| 文件选择 | — | file_picker |
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
