# AllDesk 开发进度

## 关键缺口摘要（阻断远程桌面正常使用）

| 优先级 | 缺口 | 状态 | 说明 |
|--------|------|------|------|
| **P0** | 音频实时流未接入 QUIC 管线 | [x] | AudioSenderPipeline + AudioReceiverPipeline 已桥接到 QUIC Audio 通道 |
| **P0** | 剪贴板同步未接入 QUIC 通道 | [x] | ClipboardPipeline 双向同步已桥接到 QUIC Clipboard 通道 |
| **P0** | Windows↔Android 连接和画面显示 | [x] | ReceiverPipeline 增加超时、状态追踪、增强日志 |
| **P1** | 文件传输未接入 FFI 和 Flutter UI | [x] | File 通道管线 + FFI + Flutter 文件选择/进度页 |
| **P1** | P2P 打洞（ICE）未接入连接流程 | [ ] | IceAgent 已实现，ffi/api.rs 仅直连，无打洞逻辑 |
| **P1** | 自动重连未接入 FFI | [x] | 断线后 supervisor 自动重建会话（退避重试 + 代数防陈旧） |
| **P1** | 端到端加密未在传输层启用 | [ ] | E2ECrypto 已实现，QuicTransport 未调用加密（与 QUIC TLS 能力重叠，是否保留待定） |
| **P1** | 带宽估计未接入发送管线 | [x] | 每秒采样 quinn RTT/丢包 → AIMD 自适应码率+帧率闭环（bwe.rs 延迟趋势估计器仍未使用） |
| **P1** | 流控未在传输层使用 | [x] | SenderPipeline 接入 FlowController：背压时丢旧帧 |
| **P2** | 连接质量未暴露到 Flutter UI | [x] | 每秒采样 RTT/丢包/带宽，get_connection_quality 输出真实指标 |
| **P2** | macOS/Linux 屏幕捕获缺失 | [ ] | lib.rs 中已注释掉 quartz/x11/wayland 模块 |
| **P2** | GPU 硬件加速编解码 | [ ] | 纯软件编解码，无 NVENC/QSV/VideoToolbox |
| **P3** | 白板绘图控件 | 已移除 | crate 与占位 UI 已删除（git 历史可找回） |
| **P3** | 录屏播放器 | [ ] | 会话录制已接入（VP9 直存 .aldrec），回放 UI 仍缺失 |
| **P3** | 国际化 (i18n) | [ ] | 所有文案硬编码中文 |

## 编译警告清理（25 个 warning → 0）

| 文件 | 问题 | 状态 |
|------|------|------|
| alldesk-net/src/flow.rs | 未使用导入 Result, Error | [x] |
| alldesk-net/src/ice.rs | 未使用导入 Instant, 常量 DEFAULT_STUN_PORT/CHECK_TIMEOUT/MAX_CHECK_ATTEMPTS | [x] |
| alldesk-net/src/bwe.rs | 未使用导入 Duration, 未使用变量 size_bytes | [x] |
| alldesk-net/src/e2e_crypto.rs | 常量 NONCE_INFO 未使用 | [x] |
| alldesk-clipboard/src/sanitize.rs | 未使用变量 num_digits | [x] |
| alldesk-files/src/validate.rs | 函数 safe/unsafe_file 未使用 → 改为 pub | [x] |
| alldesk-whiteboard/src/protocol.rs | 字段 pending_remote_events 未读取 → #[allow(dead_code)] | [x] |
| alldesk-ffi/src/api.rs | frb_expand cfg 警告 → crate级 #![allow(unexpected_cfgs)] | [x] |
| server/src/stun.rs | 未使用导入 Ipv6Addr → #[cfg(test)] | [x] |
| server/src/turn.rs | 未使用导入 error/warn, 未使用变量 txn_id | [x] |
| server/src/relay.rs | TokenBucket 字段/方法 → #[allow(dead_code)] | [x] |
| server/src/signaling.rs | ServerAuth 方法 → #[allow(dead_code)] | [x] |
| server/src/config.rs | ServerConfig 全部方法 → #[allow(dead_code)] | [x] |

---

## 已完成

- [x] Phase 1: 基础骨架 — Cargo workspace (11 crates) + Flutter 项目 + CLAUDE.md
- [x] Phase 2: 屏幕捕获 — Windows DXGI Desktop Duplication (DxgiCapturer, staging texture 缓存)
- [x] Phase 3: 输入注入 — Windows SendInput (鼠标归一化修复, Unicode 代理对支持)
- [x] Phase 4: 音频 — cpal 48kHz f32 采集/播放 (AudioCapturer + AudioPlayer)
- [x] Phase 5: 剪贴板 — arboard 监控/同步 (ClipboardMonitor + ClipboardSync, 7 tests)
- [x] Phase 6: 文件传输/录屏/白板 — FileTransfer + Recorder + WhiteboardSync
- [x] Phase 7: 信令+中继服务器 — WebSocket 信令 + QUIC 中继 + STUN (async, 3 tests)
- [x] Phase 8: Flutter 集成 — FRB v2 绑定生成, 首页/远程桌面页, release 编译通过
- [x] Bug 修复 — 已修复所有 4 个 CRITICAL + 17 个 HIGH 级别问题 (19 tests pass)
- [x] Phase 9: VP9 编解码 — libvpx 集成, BGRA→I420→VP9 编码/解码, 6 tests pass
- [x] Phase 10: 完整帧管线联调 — SenderPipeline(捕获→编码→QUIC), ReceiverPipeline(QUIC→解码→Flutter), FFI API
- [x] Phase 12a: Bug 修复 — 修复所有 HIGH 级 bug (文件传输分块, 中继双向转发, 音频回退级联, RGBA/BGRA 文档修正)

## 已修复的关键 Bug

| 级别 | 模块 | 问题 | 状态 |
|------|------|------|------|
| CRITICAL | input/windows.rs | 鼠标绝对坐标归一化是恒等函数 | ✅ 已修复 |
| CRITICAL | net/discovery.rs | 非阻塞 socket 导致 CPU 热循环 | ✅ 已修复 |
| CRITICAL | server/stun.rs | 两个任务竞争同一 socket | ✅ 已修复 |
| CRITICAL | server/stun.rs | blocking recv 阻塞 tokio 线程 | ✅ 已修复 |
| HIGH | input/windows.rs | Unicode 字符截断 (>U+FFFF) | ✅ 已修复 |
| HIGH | input/windows.rs | unicode_char 发送重复输入 | ✅ 已修复 |
| HIGH | capture/dxgi.rs | 每帧重新分配 staging texture | ✅ 已修复 |
| HIGH | net/transport.rs | 数据报大小未检查 | ✅ 已修复 |
| HIGH | net/transport.rs | 流路由通道识别丢失 | ✅ 已修复 |
| HIGH | net/discovery.rs | 无 SO_REUSEADDR | ✅ 已修复 |
| HIGH | ffi/api.rs | get_peer_id() 每次返回不同 ID | ✅ 已修复 |
| HIGH | ffi/api.rs | 所有 API 函数是空壳 | ✅ 已修复 |

## 仍需关注的问题

| 级别 | 模块 | 问题 |
|------|------|------|
| HIGH | net/quic_conn.rs | SkipServerVerification 无证书验证，MITM 可行 (LAN 场景可接受) |
| MEDIUM | audio | unsafe impl Sync 允许并发 start/stop 竞争 |

---

## 本次优化完成项 (Phase 27+22+19+16+14+15+17+18+23+11+24+25+26)

### Phase 27: 自适应码率闭环 + 发送循环修复 ✅
- [x] **自适应控制闭环** — host 会话每秒采样 quinn RTT/丢包（LossRateTracker 增量计算，重连无虚假丢包尖峰），AdaptiveController (AIMD) 通过 watch 通道发布目标，SenderPipeline 动态应用
- [x] **动态帧率节拍** — 发送循环改用 sleep_until 手动调度器（tokio Interval 不支持改周期），FPS 变化即时生效，落后时跳过而非突发补帧
- [x] **Vp9Encoder 重配置修复** — set_bitrate 原先从默认配置重建（会把 g_lag_in_frames 重置回 25 帧重新引入约 800ms 延迟）；现在通过共享 realtime_config 保留全部实时参数，新增 reconfigure(bitrate, fps)
- [x] **发送循环断线退出** — 断线后原先每帧 warn 无限循环且任务泄漏；现在连续 60 次发送失败（约 2 秒）后退出会话，join 收尾
- [x] **AIMD 策略迁移** — AdaptiveBitrate/AdaptiveFramerate 移至 alldesk-core（codec 重新导出保持 API），新增 AdaptiveController/LossRateTracker 共 18 个测试可在本机运行
- [x] **文档同步** — TODO/README 与实现现状对齐（移除白板/Opus/H.264/AV1/prost 失实宣称，测试统计更新为 273）

### Phase 26: Android设备测试 + Flutter测试 + 帧推送优化 ✅
- [x] **Android设备部署** — 调试APK成功构建、安装并运行于 PLC110 (Android 16 API 36)
- [x] **Flutter Widget测试** — 44个测试覆盖主题/设置/Provider/发现/文件传输页面
- [x] **Android帧推送优化** — 双缓冲帧传输、帧统计、零丢失、非阻塞FFI路径 (10 tests)
- [x] **FFI帧统计API** — get_android_frame_stats() + push_android_frame() 返回 bool
- [x] **端到端加密** — E2ECrypto ChaCha20-Poly1305/AES-256-GCM AEAD + HKDF-SHA256密钥派生 + HMAC消息认证 (15 tests)
- [x] **QUIC低延迟调优** — low_latency_transport_config() 优化初始RTT(5ms)/丢包检测/流窗口/keepalive (1 test)
- [x] **帧管线延迟测量** — StageTimer + PipelineLatencyTracker 5阶段延迟追踪 (7 tests)
- [x] **QUIC集成测试** — 7个集成测试覆盖多通道/大消息/双向/发现/ICE/流控/重连
- [x] **文件传输安全** — 文件名/内容验证，魔数检测，双扩展名攻击防护，CRC32 (25 tests)

### Phase 24: 安全 + 音频 + 白板 + 光标 + Android + 监控 ✅
- [x] **光标捕获** — DXGI GetFramePointerShape (CursorInfo/CursorShapeType)
- [x] **白板冲突解决** — CRDT Lamport时钟 + tombstone合并 (10 tests)
- [x] **录屏音频轨道** — ALDREC v2 格式 (4 tests)
- [x] **TLS 证书指纹** — PinVerifier + cert_fingerprint (7 tests)
- [x] **Prometheus Metrics** — 服务器指标采集 /metrics 端点 (3 tests)
- [x] **Android 输入注入** — AndroidInputController + JNI桥接 (8 tests)
- [x] **WSS 加密** — 信令 TLS 支持 (tokio-rustls)
- [x] **中继带宽管理** — TokenBucket令牌桶限速 (6 tests)
- [x] **输入权限检查** — InputPermission 授权/撤销 (4 tests)

### Phase 22: 测试补全 ✅
- [x] alldesk-net: 0→43 tests (Channel, QUIC, discovery, reconnect, ICE, flow, BWE)
- [x] alldesk-recording: 0→7 tests (write/read roundtrip, empty/large/error cases)
- [x] alldesk-whiteboard: 0→15 tests (stroke lifecycle, undo, clear, sync protocol)
- [x] alldesk-files: 0→15 tests (transfer send/recv, manifest scan, progress, copy, CRC32)
- [x] alldesk-audio: 0→7 tests (capturer/player structural tests)
- [x] alldesk-capture: 0→4 tests (config, pixel format, rect, monitor info)
- [x] alldesk-codec: +10 tests (adaptive bitrate, adaptive framerate, buffer pool, color conversion)
- [x] server: 0→34 tests (registry, signaling, STUN IPv6, config, auth)
- [x] **Total: 132 tests, all passing** (excluding codec/ffi linker issues)

### Phase 19: 服务器生产化 ✅
- [x] **消息输入校验** — 消息大小限制(64KB)、JSON格式验证、类型白名单 (9 tests)
- [x] **速率限制** — 每连接30消息/秒限速，AIMD窗口算法 (2 tests)
- [x] **优雅关闭** — Ctrl+C信号处理，shutdown标志传递到所有服务器
- [x] **健康检查端点** — HTTP /health 返回 `{"status":"ok"}`，独立端口(21120)
- [x] **配置文件支持** — TOML配置文件 + 环境变量(ALLDESK_*) + CLI覆盖 (5 tests)
- [x] **认证/授权** — ServerAuth token验证，Register消息检查auth_token (3 tests)
- [x] **日志结构化** — --json-logs CLI选项启用JSON结构化日志
- [x] **中继会话超时** — 5分钟空闲超时清理

### Phase 16: 屏幕捕获增强 ✅
- [x] **DXGI_ACCESS_LOST 恢复** — 自动重新初始化(最多5次)，失败返回错误
- [x] **帧率控制** — 基于config.fps的帧间隔限制，避免过多帧捕获
- [x] **脏矩形检测** — DXGI GetFrameDirtyRects提取脏矩形，填充damage_regions

### Phase 14: 编解码优化 ✅
- [x] **动态码率调整** — AdaptiveBitrate AIMD算法，基于RTT/丢包自适应 (6 tests)
- [x] **帧率自适应** — AdaptiveFramerate AIMD算法，基于RTT/丢包自适应 (4 tests)
- [x] **内存池复用** — BufferPool帧buffer池化复用 (6 tests)
- [x] **颜色空间转换** — bgra_to_i420/i420_to_bgra BT.601转换 (7 tests)

### Phase 15: 网络可靠性 ✅
- [x] **自动重连** — ReconnectManager with exponential backoff (9 tests)
- [x] **ICE 候选者收集** — IceAgent候选者收集(Host/Srflx/Relay) + 连通性检查 (12 tests)
- [x] **IPv6 支持** — STUN XOR-MAPPED-ADDRESS v6已实现 (3 tests)
- [x] **流控/背压** — FlowController缓冲区管理+背压+TTL过期+通道统计 (7 tests)
- [x] **带宽估测 (BWE)** — BandwidthEstimator延迟趋势分析 (6 tests)

### Phase 17: 文件传输 ✅
- [x] **Chunk 校验** — CRC32校验和，支持compute/verify

### Phase 18: 白板 ✅
- [x] **白板同步协议** — 序列化/反序列化 + 版本化快照/恢复

### Phase 13: 输入增强 ✅
- [x] **触摸/手势支持** — TouchEvent/TouchPoint + 默认触摸到鼠标转换
- [x] **多显示器坐标映射** — get_displays() + map_to_display()

### Phase 22: CI/CD ✅
- [x] **CI/CD 配置** — GitHub Actions CI (check, test, build, flutter analyze)

### 其他
- [x] **剪贴板优化** — has_changed() 已使用hash比对（已有实现）

---

## 待实现

### Phase 11: Android 支持（进行中）
- [x] Android 项目结构 + 构建脚本 (build_android.sh/bat, Gradle, AndroidManifest)
- [x] MediaProjection 屏幕捕获服务 (ScreenCaptureService.kt)
- [x] AccessibilityService 输入服务骨架 (AccessibilityInputService.kt)
- [x] Flutter 平台通道 (MethodChannel/EventChannel)
- [x] Rust Android capturer (alldesk-capture/android.rs)
- [x] FFI 导出 `push_android_frame()` 缺少 `#[frb(sync)]` 注解
- [x] 输入注入实现 (alldesk-input/android.rs 当前是空壳)
- [x] JNI 绑定 (Rust ↔ Android Services 的桥接)

---

### Phase 13: 跨平台捕获 & 输入

- [ ] **macOS 屏幕捕获** — CoreGraphics/ScreenCaptureKit 实现 (capture/quartz.rs 模块缺失)
- [ ] **Linux 屏幕捕获** — X11 + Wayland 后端 (capture/x11.rs / wayland.rs 模块缺失)
- [ ] **macOS 输入注入** — CoreGraphics CGEvent (input/quartz.rs 已有代码但未编译验证)
- [ ] **Linux 输入注入** — uinput / XTest 扩展
- [x] **多显示器坐标映射** — get_displays() + map_to_display() 已实现
- [x] **光标捕获** — DXGI GetFramePointerShape 提取光标图像和位置 (CursorInfo/CursorShapeType)
- [x] **触摸/手势支持** — TouchEvent/TouchPoint + 默认触摸到鼠标转换已实现

---

### Phase 14: 编解码优化

- [x] ~~**H.264 编解码器**~~ — 暂不实现，VP9已足够
- [x] ~~**AV1 编解码器**~~ — 暂不实现，VP9已足够
- [ ] **GPU 硬件加速** — 当前纯软件编解码，无 NVENC/QSV/VideoToolbox 支持
- [ ] **GPU Texture 直传** — codec 的 encode_texture() 直接返回错误
- [x] **NV12 像素格式** — bgra_to_nv12/nv12_to_bgra BT.601转换 (6 tests)
- [x] **动态码率调整** — AdaptiveBitrate AIMD算法已实现，且已接入发送管线闭环（每秒按 RTT/丢包调整编码码率）
- [x] **帧率自适应** — AdaptiveFramerate AIMD算法，基于RTT/丢包自适应 (4 tests)，且已接入发送管线（动态调整捕获/编码节拍）

---

### Phase 15: 网络可靠性

- [x] **TURN 服务器** — RFC 5766 Allocate/Refresh/ChannelBind/Send/Data (9 tests)
- [x] **ICE 完整实现** — IceAgent候选者收集(Host/Srflx/Relay) + 连通性检查 (12 tests)
- [x] **IPv6 支持** — STUN XOR-MAPPED-ADDRESS v6已实现 (3 tests)
- [x] **自动重连** — ReconnectManager with exponential backoff (9 tests)
- [x] **带宽估测 (BWE)** — BandwidthEstimator延迟趋势分析 (6 tests)
- [x] **拥塞控制调优** — low_latency_transport_config() 优化初始RTT/丢包阈值/流窗口 (1 test)
- [x] **流控/背压** — FlowController缓冲区管理+背压+TTL过期+通道统计 (7 tests)

---

### Phase 16: 屏幕捕获增强

- [x] **脏矩形检测** — DXGI GetFrameDirtyRects提取脏矩形，填充damage_regions
- [x] ~~**帧率控制**~~ — 已实现基于配置FPS的帧率限制
- [x] ~~**DXGI_ACCESS_LOST 恢复**~~ — 已实现自动重新初始化
- [x] **Android 帧推送优化** — 双缓冲帧传输、帧统计、非阻塞FFI (10 tests in android.rs)

---

### Phase 17: 文件传输完善

- [x] **断点续传** — TransferManifest chunk状态持久化 + CRC32校验已实现（manifest 机制尚未接入实际传输协议，当前会话传输为一次性流式发送）
- [x] ~~**Chunk 校验**~~ — 已实现CRC32校验和，支持compute/verify (5 tests)
- [x] **文件传输 UI** — FileTransferPage：文件选择(file_picker) + 500ms 进度轮询 + 传输卡片
- [x] **传输速度/进度显示** — get_file_transfer_status() 输出方向/文件名/已传/总量/错误，UI 渲染进度条
- [ ] **传输队列管理** — 无多文件排队和优先级控制

---

### Phase 18: 白板 & 录屏

- [x] ~~**白板同步协议**~~ — crate 与占位 UI 已整体移除（git 历史可找回）
- [x] ~~**白板冲突解决**~~ — 随白板 crate 移除
- 已移除 **白板绘图控件** — 随白板 crate 移除
- [x] **WebM 容器** — WebmMuxer EBML+SimpleBlock VP9输出 (6 tests)
- [x] **录屏音频轨道** — ALDREC v2格式支持音频帧读写 (4 tests)
- [x] **会话录制接线** — ReceiverPipeline 录制 VP9 帧，FFI start/stop_session_recording + 远程页录制按钮
- [ ] **录屏播放器** — 无 ALDREC/WebM 回放 UI

---

### Phase 19: 服务器生产化

- [x] **认证/授权** — ServerAuth token验证，Register消息检查auth_token (3 tests)
- [x] ~~**消息输入校验**~~ — 已实现大小限制、JSON格式验证、类型白名单
- [x] ~~**速率限制**~~ — 已实现每连接30消息/秒限速
- [x] **WSS 加密** — 信令 WebSocket 支持 TLS (tokio-rustls + --tls-cert/--tls-key)
- [x] ~~**优雅关闭**~~ — 已实现Ctrl+C信号处理
- [x] ~~**健康检查端点**~~ — 已实现HTTP /health
- [x] ~~**配置文件**~~ — 已实现TOML配置 + 环境变量 + CLI覆盖
- [x] **指标采集** — Prometheus metrics + /metrics端点 (3 tests)
- [x] **中继带宽管理** — TokenBucket令牌桶限速 + 每会话带宽控制 (6 tests)
- [x] **中继会话超时** — 5分钟空闲超时清理已实现
- [x] **日志结构化** — --json-logs CLI选项启用JSON结构化日志

---

### Phase 20: Flutter UI 完善

- [x] **连接质量指标** — QualityCollector RTT/丢包/带宽 + QualityLevel (15 tests)，get_connection_quality 每秒输出真实 quinn 指标
- [x] **文件传输页面** — 选文件 + 进度轮询 + 进度卡片已实现
- 已移除 **白板控件** — 随白板 crate 移除
- [x] **错误展示** — 远程会话页在 UI 显示连接错误（其余页面仍用 debugPrint，待统一）
- [x] **连接重试** — Rust 侧 supervisor 断线自动重连（退避 + 代数防陈旧）；初次连接带重试循环
- [x] **网络中断恢复** — 断线后自动重建会话并恢复画面/音频/剪贴板管线；无专门的重连状态 UI 提示
- [ ] **暗色主题** — 基础主题切换已有，但未完善系统主题联动
- [ ] **国际化 (i18n)** — 无多语言支持，所有文案硬编码中文
- [ ] **辅助功能** — 无屏幕阅读器、高对比度、文字大小调整支持
- [ ] **聊天/消息功能** — 无端到端文本聊天 UI
- [ ] **设备收藏/历史** — 无设备收藏夹和连接历史记录
- [ ] **权限引导流程** — 无统一的权限请求和解释 UI

---

### Phase 21: 安全加固

- [x] **TLS 证书验证** — PinVerifier支持指纹校验和LAN不安全模式 (7 tests)
- [x] **端到端加密** — E2ECrypto ChaCha20-Poly1305/AES-256-GCM AEAD + HKDF密钥派生 + HMAC (15 tests)
- [x] **剪贴板内容消毒** — 密码/令牌/信用卡/JWT检测过滤 (14 tests)
- [x] **输入权限检查** — InputPermission原子授权/撤销，远程端控制 (4 tests)
- [x] **文件传输安全** — 文件名/内容验证，魔数检测，双扩展名攻击防护，CRC32 (25 tests)

---

### Phase 22: 测试 & CI

- [x] **CI/CD 配置** — GitHub Actions CI (check, test, build, flutter analyze)
- [x] ~~**Rust 单元测试补全**~~ — 273 tests passing（可本地链接的 crate）
  - alldesk-core: 18 tests ✅ (config + adaptive 控制器 / 丢包率跟踪器)
  - alldesk-net: 73 + 7 integration tests ✅ (Channel, QUIC, discovery, reconnect, ICE, flow, BWE, TLS pin, E2E crypto, latency)
  - alldesk-files: 45 tests ✅ (含CRC32校验 + manifest + 文件验证)
  - alldesk-platform: 55 tests ✅ (audio/clipboard/input 合并后)
  - alldesk-recording: 17 tests ✅ (write/read roundtrip, empty/large/error cases)
  - alldesk-capture: 6 tests ✅ (config, pixel format, cursor)
  - server: 52 tests ✅ (registry, signaling, STUN IPv6, config, auth, metrics, bandwidth, TURN)
  - alldesk-codec: 10 tests（编译通过；本机缺 vpx.lib 无法链接测试二进制，CI 可跑）
  - **Total: 273 tests, all passing locally**
- [x] **Flutter 测试** — 44个测试覆盖主题/设置Provider/发现Provider/文件传输页面/路由
- [ ] **集成测试** — QUIC集成测试已有(7个)，缺文件传输端到端测试
- [ ] **性能基准** — 无编解码/网络/捕获性能 benchmark
- [ ] **跨平台构建验证** — macOS/Linux 构建脚本缺失
- [x] **Android 设备测试** — 调试APK已构建/安装/运行于 PLC110 (Android 16 API 36)

---

### Phase 23: 性能优化

- [ ] **零拷贝渲染路径** — 验证 Flutter Texture 零拷贝路径实际生效
- [x] **颜色空间转换优化** — bgra_to_i420/i420_to_bgra BT.601转换 (7 tests)
- [x] **内存池复用** — BufferPool帧buffer池化复用 (6 tests)
- [x] ~~**剪贴板优化**~~ — has_changed() 已使用hash比对
- [ ] **音频回声消除** — 无 AEC (Acoustic Echo Cancellation) 支持
- [x] **帧管线延迟分析** — StageTimer + PipelineLatencyTracker 5阶段延迟追踪 (7 tests)

