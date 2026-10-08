# AllDesk 开发进度

## 关键缺口摘要（阻断远程桌面正常使用）

| 优先级 | 缺口 | 状态 | 说明 |
|--------|------|------|------|
| **P0** | 音频实时流未接入 QUIC 管线 | [x] | AudioSenderPipeline + AudioReceiverPipeline 已桥接到 QUIC Audio 通道 |
| **P0** | 剪贴板同步未接入 QUIC 通道 | [x] | ClipboardPipeline 双向同步已桥接到 QUIC Clipboard 通道 |
| **P0** | Windows↔Android 连接和画面显示 | [x] | ReceiverPipeline 超时/状态追踪/增强日志；**Android 端视频管线实际自 Phase 34 才存在**（此前 jniLibs 为无 codec 的 4 月旧 .so，libvpx 从未为 Android 交叉编译过） |
| **P1** | 文件传输未接入 FFI 和 Flutter UI | [x] | File 通道管线 + FFI + Flutter 文件选择/进度页 |
| **P1** | P2P 打洞（ICE）未接入连接流程 | [ ] | IceAgent 已实现，ffi/api.rs 仅直连，无打洞逻辑 |
| **P1** | 自动重连未接入 FFI | [x] | 断线后 supervisor 自动重建会话（退避重试 + 代数防陈旧） |
| **P1** | 端到端加密未在传输层启用 | [ ] | E2ECrypto 已实现，QuicTransport 未调用加密（与 QUIC TLS 能力重叠，是否保留待定） |
| **P1** | 带宽估计未接入发送管线 | [x] | 每秒采样 quinn RTT/丢包 → AIMD 自适应码率+帧率闭环（bwe.rs 延迟趋势估计器仍未使用） |
| **P1** | 流控未在传输层使用 | [x] | SenderPipeline 接入 FlowController：背压时丢旧帧 |
| **P2** | 连接质量未暴露到 Flutter UI | [x] | 每秒采样 RTT/丢包/带宽，get_connection_quality 输出真实指标 |
| **P2** | macOS 屏幕捕获 | [x] | QuartzCapturer: CGDisplayCreateImage 轮询 + TCC 权限预检 + Retina 像素坐标适配（Phase 35） |
| **P2** | Linux 屏幕捕获 | [ ] | x11/wayland 模块缺失（x11rb 依赖已声明） |
| **P2** | GPU 硬件加速编解码 | [ ] | 纯软件编解码，无 NVENC/QSV/VideoToolbox |
| **P3** | 白板绘图控件 | 已移除 | crate 与占位 UI 已删除（git 历史可找回） |
| **P3** | 录屏播放器 | [x] | RecordingPlayer + FFI 逐帧解码 + 录像列表/播放器页（Phase 30） |
| **P3** | 国际化 (i18n) | [x] | gen-l10n + zh/en arb 双语言，全部页面接入（Phase 31） |

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

## 本次优化完成项 (Phase 35+34+33+32+31+30+29+28+27+22+19+16+14+15+17+18+23+11+24+25+26)

### Phase 35: macOS 支持落地（捕获 + 输入 + 一键构建）✅
目标: 项目搬到 M 系列 Mac 后一条命令得到可用产物。**所有新增 macOS Rust 代码已通过 `cargo check --target aarch64-apple-darwin` 在本机交叉验证**(ring/dart-sys/oslog 三个 C 依赖因需 Apple 编译器只能到真机构建, Rust 源全部过检)。
- [x] **QuartzCapturer 屏幕捕获** — `CGDisplayCreateImage` 每次调用返回当前桌面(管线节拍为唯一限频器, 与 DXGI 契约一致; 天然无"静屏无首帧"问题); 复用型 BGRA 位图上下文(分辨率变化时才重建); 奇数宽高裁到偶数(VP9 要求); 显示器枚举主屏置首; TCC 权限预检(`CGPreflightScreenCaptureAccess`/`CGRequestScreenCapture`)未授权时给出明确指引; 已知限制: CGDisplayCreateImage 不含系统光标, `cursor` 恒 None(ScreenCaptureKit 可后续补)
- [x] **Retina 输入坐标适配** — 帧是背板像素(2x), CGEvent 鼠标坐标是全局点坐标; api.rs 新增 PointScaledController 适配器(原点偏移 + 缩放换算后委托 MacInputController)
- [x] **macos.rs 输入编译修复** — core-graphics 0.24 的 KeyCode 并无 ANSI_* 打印键常量(此前从未编译过, 47 处引用全错); 补齐标准 HID 虚拟键码表(0x00-0x32), 特殊键仍用 crate 常量
- [x] **scripts/build-macos.sh 一键脚本** — 检查/安装 Xcode CLT、Homebrew、Rust、Flutter、flutter_rust_bridge_codegen 2.12.0; 下载并静态编译 libvpx 1.14.1(默认 arm64, `--universal` 加 x86_64 lipo 合一); 跑 FRB codegen(frb_generated.rs 被 gitignore, 新机必需); cargo 构建 + `flutter create --platforms=macos`(仓库原本没有 macos 平台目录) + `flutter build macos`; 把 liballdesk_ffi.dylib 拷进 .app/Contents/MacOS 并 ad-hoc 重签名; 末尾打印屏幕录制/辅助功能授权指引。bash 3.2 兼容(无空数组展开)
- [x] **测试平台门控** — first_frame.rs 的 dxgi import 移入 cfg(windows) 测试内; loopback 视频测试 cfg(windows)(避免 mac 上 cargo test 弹 TCC 授权窗), 文件传输测试保持跨平台

### Phase 34: Android 视频管线落地 + 文件传输数据损坏修复 ✅
审计发现 **Android 端从未包含视频功能**: jniLibs 里的 liballdesk_ffi.so 是 4/21 的旧构建(内无 alldesk-codec/alldesk-capture/watch_video 任何符号, 甚至晚至 5/9 的 target 产物同样没有) — Android 侧 VP9 依赖的 libvpx 从未交叉编译过, vpx 相关 Android 构建全部静默失败。
- [x] **libvpx 1.14.1 Android 三架构交叉编译** — 新增 `scripts/build-vpx-android.sh`(Git Bash + NDK r28 clang + 原生 GNU make; 相对路径调 configure 规避原生 make 无法解析 MSYS 路径、强制 SHELL=sh.exe 执行 POSIX recipe、armv7 的 %.S.o 规则补 `-c` 防误链接、x86_64 用便携 nasm)。产出 `C:\tmp\vpx-android\{arm64-v8a,armeabi-v7a,x86_64}\lib\libvpx.a`(NEON 全开, readelf 校验架构正确)
- [x] **Android 构建链修复** — Rust Android targets 重装; build.bat/build.sh 的 android 分支改为逐 ABI 设置 `VPX_LIB_DIR/VPX_STATIC=1`(三个 ABI 各自静态链接自己的 libvpx.a); NDK 路径默认值更新为实际安装位置(r28)
- [x] **文件传输真实数据损坏 bug 修复** — 新增文件传输端到端集成测试(真实 QUIC + 生产收发代码路径)立即抓到: 接收端把每条 chunk 消息的 8 字节块索引前缀也写进文件(`&msg[1..]` 应为 `&msg[9..]`), **每个 64KB 分块污染 8 字节, 收到的文件全部损坏**。已修复并被测试守护(700KB 逐字节比对通过)
- [x] **接收目录可配置** — `ALLDESK_RECEIVED_FILES_DIR` 环境变量覆盖默认的 `~/Downloads/AllDesk`(测试隔离 + 用户自定义)
- [x] **Kotlin isScreenCaptureGranted** — MainActivity 方法通道补齐(ScreenCaptureService.isRunning 静态标志), Android 权限引导页状态不再恒为"未授权"

### Phase 33: "连接后无画面"根因修复 ✅
实测发现"设备能发现、连接后一直无画面"。新增**端到端回环集成测试**(本机真实 QUIC+DXGI+VP9 全链路,`ffi/tests/loopback.rs`)复现并锁定两个根因:
- [x] **根因1: 静止桌面无首帧** — Win11 上 DXGI `AcquireNextFrame` 不返回当前画面(只等"变化"),重建 duplication 也无效(实测 10 秒 0 帧/92 次超时)。被控端桌面安静时观察端永远看不到画面。修复:首次产出帧前,超时即用 **GDI BitBlt 一次性兜底**抓当前桌面(BitBlt 永远返回当前内容),之后 DXGI 接管增量。新增 `capture/tests/first_frame.rs` 静屏首帧测试(实测 0 超时出帧)
- [x] **根因2: libvpx 版本不匹配** — 本机 libvpx-1.dll 是 v1.14.1,而 libvpx-native-sys 5.0.13 绑定按 1.13.0 结构体编译 → `enc_init_ver` 返回 INVALID_PARAM(3),VP9 编码器从未创建成功(此前被 raw 回退掩盖)。修复:升级 libvpx-native-sys → 5.0.17(带 1.14.0 预生成绑定)+ `VPX_VERSION=1.14.0`(build.bat/sh 同步);新增运行时版本探针测试(打印 DLL 版本并逐步定位失败调用)
- [x] **恢复本机构建链** — `C:\tmp\vpx-install\lib` 曾被清空(vpx.lib 丢失,导致 4 月后所有构建失败、只能跑旧二进制)。从存活的 `libvpx-1.dll`(dumpbin 导出表 → .def → lib.exe)重建导入库;alldesk-ffi 增加 rlib crate-type 使集成测试可链接。**至此本机可完整构建 Windows 版,且 323 个测试首次全部本地可跑**(此前 codec/ffi 测试无法链接)

### Phase 32: 键盘接入 + 键表补全 + 视频稳定性 ✅
- [x] **键盘接入 UI** — 远程页 Focus(onKeyEvent) 转发硬件键盘（连接后自动获焦、点击画面重新获焦）；移动端工具栏键盘按钮弹出软键盘输入条（增量字符 + 退格 + 回车转发）
- [x] **键表补全** — KeyCode/协议表/VK 映射新增 Ctrl/Alt/Shift/Win 左右修饰键、Space/Home/End/PgUp/PgDn/Insert/PrintScreen/Pause/CapsLock/NumLock/ScrollLock/Menu；新增 key_type="vk" 原始虚拟键透传通道（修饰键组合如 Ctrl+C 走真实 VK 而非文本字符）；macOS 映射同步（含左右修饰键 CGKeyCode，常量名已对照 core-graphics 0.24 源码核实）
- [x] **Dart 键映射服务** — key_mapping.dart 与 Rust decode_special_key 表一一对应（F1-F24 显式映射）；组合键路径用 HardwareKeyboard 修饰符状态判定
- [x] **分辨率变化处理** — SenderPipeline 检测帧尺寸与编码器脱节即重建编码器（新编码器自动出关键帧，接收端立即重同步）并同步重建 flow 上限（raw 回退帧不再被静默丢弃）
- [x] **raw 回退治理** — 编码连续失败 30 次（~1 秒）后重建编码器，不再无限期发 8MB/帧的原始 BGRA
- [x] **剪贴板内存** — ClipboardMonitor 移除只写不读的 last_content 缓存（大图不再常驻内存；Android 侧缓存有真实用途保留）

### Phase 31: i18n + 聊天 + 权限引导 ✅
- [x] **国际化 (i18n)** — flutter gen-l10n + arb 双语言(中文模板 + English,~90 键);MaterialApp 接入 localizationsDelegates/supportedLocales,语言跟随系统;全部页面(首页/远程/文件传输/录像/播放器/设置/聊天/权限)字符串改经 AppLocalizations;启动错误提示按当前语言构建;测试改为本地化 pump + zh/en 覆盖断言
- [x] **聊天功能** — Channel::Whiteboard 复用为 Channel::Chat(流 ID 稳定);ChatPipeline 双向 UTF-8 消息(观察端开流、被控端 accept,一条消息一次 transport 发送);FFI send_chat_message + watch_chat_messages 推送流(重连存活,与帧流同模式);Flutter 聊天页(气泡列表/输入/自动滚动,会话内不持久化)+ 远程页工具栏入口
- [x] **权限引导流程** — Android 首次启动自动弹出权限检查清单页(屏幕采集/无障碍输入/可选麦克风,逐项说明+状态+对应系统动作);不再显示记录在 shared_preferences;首页 AppBar 常驻入口;PlatformService 新增 isScreenCaptureGranted(宿主未实现时优雅降级)与 requestMicrophonePermission(permission_handler)

### Phase 30: 功能补全 ✅
- [x] **录屏播放器** — recording crate 新增 RecordingPlayer(游标式 next_frame/rewind,音频区隔离,2 tests);FFI start_recording_playback/recording_next_frame/rewind/stop(VP9 逐帧解码,坏帧跳过);录像列表页 + 播放器页(播放/暂停/重放/进度,按录像自身 fps 节拍);首页 AppBar 入口
- [x] **设备收藏/连接历史** — ConnectionStore 基于 shared_preferences:连接(手动+LAN)自动入史,同地址去重置顶,上限 20;收藏星标;首页"最近连接"区一键重连(7 tests)
- [x] **错误展示统一** — error_ui.dart(showErrorSnackBar + 全局 rootScaffoldMessengerKey);Rust 启动初始化失败改为 SnackBar 可见提示;录像/删除失败不再静默

### Phase 29: 次要项清理 ✅
- [x] **帧推送事件驱动** — `watch_video_frames` FRB Stream 替代 33ms 轮询:帧解码完成即推送(消除平均 ~16ms 显示延迟与空轮询),Rust 侧合并旧帧只发最新;流在重连后自动切换到新会话通道(subscribe 后丢弃本地 sender 克隆,旧通道关闭即重新等待)
- [x] **桌面悬停鼠标** — 无按键的鼠标移动直接转发为远程 move(此前 GestureDetector 只在按住拖动时发 move,桌面观看时远程光标不跟手)
- [x] **双指滚动节流** — 合并为每帧(16ms)最多一条消息;锚点仅在实际发送时推进,被节流的增量累积而非丢失
- [x] **剪贴板 Windows 快路径** — `GetClipboardSequenceNumber` 序列号检测变化,内容(尤其是大图像)仅在真正变化时读取,不再 250ms 全量拉取+哈希;`set_content`/`get_content` 同步吸收自身写入的序列号变化;非 Windows 保持原哈希轮询
- [x] **编解码缓冲复用** — Vp9Encoder/Vp9Decoder 持有可复用的 I420 scratch 缓冲(`bgra_to_i420_into`),1080p 下每帧 ~3MB 的堆分配降为一次性,分配器churn 显著降低

### Phase 28: 性能/卡顿修复 ✅
- [x] **双重限频帧率减半修复** — 删除 DXGI 捕获器内部限频（与发送管线节拍独立漂移，实际输出约一半帧率）；管线 sleep_until 节拍成为唯一限频器
- [x] **帧交付零拷贝优化** — Dart 侧 sublist(8) 整帧拷贝改 Uint8List.sublistView 视图；poll_video_frame 排空队列只返回最新帧（UI 卡顿后跳过旧帧而非回放追赶）
- [x] **音频抖动缓冲预填充** — AudioPlayer 缓冲积累 ~60ms 后才开始消费，消除网络抖动造成的可闻断续
- [x] **色彩转换优化** — vp9.rs 两侧 BT.601 转换改行切片 + chunks_exact（实测 1080p 编码侧 3.7→2.0ms、解码侧 11.7→6.0ms，输出逐字节一致）；另注明 color.rs 中同名函数是全范围 f64 版本、与热路径不可合并

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

### Phase 11: Android 支持 ✅
- [x] Android 项目结构 + 构建脚本 (build.bat/build.sh android 分支, Gradle, AndroidManifest)
- [x] MediaProjection 屏幕捕获服务 (ScreenCaptureService.kt)
- [x] AccessibilityService 输入服务骨架 (AccessibilityInputService.kt)
- [x] Flutter 平台通道 (MethodChannel/EventChannel, 含 isScreenCaptureGranted)
- [x] Rust Android capturer (alldesk-capture/android.rs)
- [x] FFI 导出 `push_android_frame()` 缺少 `#[frb(sync)]` 注解
- [x] 输入注入实现 (alldesk-input/android.rs 当前是空壳)
- [x] JNI 绑定 (Rust ↔ Android Services 的桥接)
- [x] **视频管线落地 (Phase 34)** — 此前 jniLibs 的 .so 是 4 月旧构建(无 codec/capture/FFI 符号); libvpx 三架构交叉编译 + 逐 ABI 静态链接后重建成真; minSdk 24→26 (cpal aaudio)

---

### Phase 13: 跨平台捕获 & 输入

- [x] **macOS 屏幕捕获** — QuartzCapturer (CGDisplayCreateImage 轮询, 见 Phase 35)
- [ ] **Linux 屏幕捕获** — X11 + Wayland 后端 (capture/x11.rs / wayland.rs 模块缺失)
- [x] **macOS 输入注入** — MacInputController (CoreGraphics CGEvent)；Phase 35 修复 47 处不存在的 ANSI_* 常量引用并补齐标准 HID 键码表，已过 aarch64-apple-darwin 编译检查；Retina 像素→点坐标适配 (PointScaledController)
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
- [x] **录屏播放器** — RecordingPlayer 游标式顺序读取 + FFI(start/next_frame/rewind/stop，VP9 逐帧解码) + 录像列表页(删除/大小/日期) + 播放器页(按录像 fps 节拍的播放/暂停/重放/进度)，无 seek(VP9 帧间依赖，需关键帧索引才能支持)

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
- [x] **错误展示** — 统一 error_ui 工具(全局 messenger + showErrorSnackBar)；启动期 Rust 初始化失败会以 SnackBar 告知(此前仅 debugPrint)；录像读取/删除失败、传输失败均有可见提示；远程会话页在 UI 显示连接错误
- [x] **连接重试** — Rust 侧 supervisor 断线自动重连（退避 + 代数防陈旧）；初次连接带重试循环
- [x] **网络中断恢复** — 断线后自动重建会话并恢复画面/音频/剪贴板管线；无专门的重连状态 UI 提示
- [ ] **暗色主题** — 基础主题切换已有，但未完善系统主题联动
- [x] **国际化 (i18n)** — gen-l10n + arb 双语言（zh 模板 + en），语言跟随系统，全部页面已接入
- [ ] **辅助功能** — 无屏幕阅读器、高对比度、文字大小调整支持
- [x] **聊天/消息功能** — QUIC Chat 通道双向文本聊天（推送流 + 气泡 UI，会话内不持久化）
- [x] **设备收藏/历史** — ConnectionStore(shared_preferences 持久化)：连接自动入史(置顶去重、上限 20)、收藏星标、一键重连；首页"最近连接"区
- [x] **权限引导流程** — Android 首启权限清单页（屏幕采集/无障碍/麦克风）+ 常驻入口

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
  - alldesk-platform: 59 tests ✅ (audio/clipboard/input 合并后，含序列号稳定性 + 导航键/修饰键 VK 映射测试)
  - alldesk-recording: 19 tests ✅ (write/read roundtrip + RecordingPlayer 顺序读取/回绕/音频区隔离)
  - alldesk-capture: 6 tests ✅ (config, pixel format, cursor)（另有静屏首帧测试在 tests/first_frame.rs）
  - server: 52 tests ✅ (registry, signaling, STUN IPv6, config, auth, metrics, bandwidth, TURN)
  - alldesk-codec: 27 tests ✅（编码/解码往返 + vpx 运行时版本探针；需 PATH 中有 libvpx-1.dll）
  - alldesk-ffi: 15 单元测试 + **端到端回环集成测试×2**（真实 QUIC+DXGI+VP9 全链路"连接后无画面"级回归守卫；文件传输全链路逐字节校验——上线即抓到分块索引污染 bug）✅
  - alldesk-capture: 7 tests ✅（含静屏首帧 GDI 兜底测试）
  - **Total: 324 tests, all passing locally**（libvpx 导入库已从运行时 DLL 重建，codec/ffi 测试首次可本地运行；Android 三架构自 Phase 34 起同样可构建）
- [x] **Flutter 测试** — 63个测试覆盖主题/设置Provider/发现Provider/文件传输页面/录像页面/路由/ConnectionStore(7个)/本地化(zh+en+参数化消息)/键映射(9个:控制键/修饰键左右独立/F键块/组合VK/可打印判定)
- [x] **集成测试** — QUIC 集成测试(7个) + 端到端回环视频测试 + 文件传输端到端测试(真实 QUIC 全链路, 逐字节校验)
- [ ] **性能基准** — 无编解码/网络/捕获性能 benchmark
- [x] **跨平台构建验证** — macOS: `scripts/build-macos.sh` 一键(依赖/libvpx/FRB/打包/签名) + `cargo check --target aarch64-apple-darwin` 过检；Linux 构建脚本仍缺
- [x] **Android 设备测试** — 调试APK已构建/安装/运行于 PLC110 (Android 16 API 36)

---

### Phase 23: 性能优化

- [ ] **零拷贝渲染路径** — 验证 Flutter Texture 零拷贝路径实际生效
- [x] **颜色空间转换优化** — bgra_to_i420/i420_to_bgra BT.601转换 (7 tests)
- [x] **内存池复用** — BufferPool帧buffer池化复用 (6 tests)
- [x] ~~**剪贴板优化**~~ — has_changed() 已使用hash比对
- [ ] **音频回声消除** — 无 AEC (Acoustic Echo Cancellation) 支持
- [x] **帧管线延迟分析** — StageTimer + PipelineLatencyTracker 5阶段延迟追踪 (7 tests)

