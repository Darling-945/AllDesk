// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get appName => 'AllDesk';

  @override
  String get commonConnect => '连接';

  @override
  String get commonCancel => '取消';

  @override
  String get commonSave => '保存';

  @override
  String get commonDelete => '删除';

  @override
  String get commonPlay => '播放';

  @override
  String get commonPause => '暂停';

  @override
  String get commonSettings => '设置';

  @override
  String get commonConfirm => '确定';

  @override
  String get commonLoading => '加载中…';

  @override
  String get homePeerId => '本机 ID';

  @override
  String get homePeerIdError => '加载 ID 失败';

  @override
  String get homeConnectTitle => '连接设备';

  @override
  String get homeConnectHint => '输入 Peer ID 或 IP 地址';

  @override
  String get homeDevicesTitle => '局域网设备';

  @override
  String get homeScanning => '扫描中…';

  @override
  String get homeScanningForDevices => '正在扫描设备…';

  @override
  String get homeDiagnostics => '诊断';

  @override
  String get homeScreenSharingActive => '屏幕共享已开启 — 其他设备可以连接';

  @override
  String get homeStartSharing => '开始屏幕共享';

  @override
  String get homeStopSharing => '停止屏幕共享';

  @override
  String get homeRecordings => '录像回放';

  @override
  String get homeRecentTitle => '最近连接';

  @override
  String homeFavoritesCount(int count) {
    return '$count 收藏';
  }

  @override
  String get homeUnfavorite => '取消收藏';

  @override
  String get homeFavorite => '收藏';

  @override
  String remoteConnectingTo(String target) {
    return '正在连接 $target…';
  }

  @override
  String remoteConnectedTo(String target) {
    return '已连接 $target';
  }

  @override
  String get remoteGoBack => '返回';

  @override
  String remoteWaitingForVideo(int count) {
    return '等待视频流…（已收到 $count 帧）';
  }

  @override
  String get remoteVideoStarting => '视频流启动中…';

  @override
  String get remoteFullscreen => '全屏';

  @override
  String get remoteExitFullscreen => '退出全屏';

  @override
  String get remoteFileTransfer => '文件传输';

  @override
  String get remoteChat => '聊天';

  @override
  String get remoteDisconnect => '断开连接';

  @override
  String get remoteKeyboard => '键盘';

  @override
  String get remoteInputHint => '输入要发送到远程的文本…';

  @override
  String get remoteStartRecording => '录制会话';

  @override
  String get remoteStopRecording => '停止录制';

  @override
  String remoteRecordingDone(String summary) {
    return '录制完成: $summary';
  }

  @override
  String remoteRecordingFailed(String error) {
    return '录制操作失败: $error';
  }

  @override
  String get filesTitle => '文件传输';

  @override
  String get filesSending => '发送文件到远程设备';

  @override
  String get filesReceiving => '正在接收文件';

  @override
  String get filesHint => '远程设备收到的文件保存在 Downloads/AllDesk 目录';

  @override
  String get filesPick => '选择文件并发送';

  @override
  String get filesPicking => '选择文件…';

  @override
  String filesLastError(String error) {
    return '上次传输出错: $error';
  }

  @override
  String get filesSendFailed => '发送失败';

  @override
  String filesBytes(int transferred, int total) {
    return '$transferred / $total 字节';
  }

  @override
  String get recTitle => '录像回放';

  @override
  String get recEmpty => '暂无录像';

  @override
  String get recEmptyHint => '在远程会话中点击录制按钮即可开始录制';

  @override
  String get recDeleteTitle => '删除录像';

  @override
  String get recDeleteFailed => '删除失败';

  @override
  String get recListFailed => '读取录像列表失败';

  @override
  String get playerEnded => '回放结束';

  @override
  String get playerPreparing => '准备回放…';

  @override
  String get playerReplay => '重新播放';

  @override
  String get playerBackToList => '返回列表';

  @override
  String playerStats(int count, int fps) {
    return '$count 帧 @ ${fps}fps';
  }

  @override
  String get settingsTitle => '设置';

  @override
  String get settingsDevice => '设备';

  @override
  String get settingsDisplay => '显示';

  @override
  String get settingsStreaming => '串流';

  @override
  String get settingsNetwork => '网络';

  @override
  String get settingsAbout => '关于';

  @override
  String get settingsPeerId => 'Peer ID';

  @override
  String get settingsDisplayName => '设备名称';

  @override
  String get settingsNameNotSet => '未设置（使用主机名）';

  @override
  String get settingsNameHint => '输入其他人看到的名称';

  @override
  String get settingsTheme => '主题';

  @override
  String get settingsThemeSystem => '跟随系统';

  @override
  String get settingsThemeLight => '浅色';

  @override
  String get settingsThemeDark => '深色';

  @override
  String get settingsShowCursor => '显示远程光标';

  @override
  String get settingsShowCursorSub => '在画面上渲染远程光标';

  @override
  String get settingsVideoQuality => '视频画质';

  @override
  String get settingsFrameRate => '帧率';

  @override
  String get settingsPort => 'QUIC 服务端口';

  @override
  String get settingsPortHint => '端口号 (1024-65535)';

  @override
  String get settingsAutoConnect => '自动连接上次设备';

  @override
  String get settingsAutoConnectSub => '应用启动时自动连接';

  @override
  String get settingsVersion => '版本';

  @override
  String get settingsOpenSource => '开源';

  @override
  String get settingsOpenSourceSub => '基于 Rust + Flutter 构建';

  @override
  String get settingsBackHome => '返回首页';

  @override
  String get settingsQualityLow => '低';

  @override
  String get settingsQualityMedium => '中';

  @override
  String get settingsQualityHigh => '高';

  @override
  String get settingsQualityUltra => '极高';

  @override
  String get settingsFpsCpuNote => '（可能增加 CPU 占用）';

  @override
  String get chatTitle => '聊天';

  @override
  String get chatHint => '输入消息…';

  @override
  String get chatSend => '发送';

  @override
  String get chatNotConnected => '未连接，无法发送消息';

  @override
  String get chatEmpty => '会话消息将显示在这里';

  @override
  String get permTitle => '权限引导';

  @override
  String get permIntro => '被控端需要以下权限才能正常工作';

  @override
  String get permScreenCapture => '屏幕采集';

  @override
  String get permScreenCaptureDesc => '用于捕获屏幕画面共享给控制端';

  @override
  String get permAccessibility => '无障碍服务（输入控制）';

  @override
  String get permAccessibilityDesc => '用于接收控制端的鼠标键盘操作';

  @override
  String get permMicrophone => '麦克风（可选）';

  @override
  String get permMicrophoneDesc => '用于共享系统声音';

  @override
  String get permOpenSettings => '去系统设置开启';

  @override
  String get permRequest => '申请权限';

  @override
  String get permGranted => '已开启';

  @override
  String get permNotGranted => '未开启';

  @override
  String get permSkip => '跳过';

  @override
  String get permDone => '完成';

  @override
  String get permDesktopHint => '桌面平台无需这些权限，直接开始使用即可';

  @override
  String get errorOperationFailed => '操作失败';

  @override
  String errorRustInit(String error) {
    return 'Rust 运行时初始化失败: $error';
  }

  @override
  String errorRustNetInit(String error) {
    return 'Rust 网络层初始化失败: $error';
  }
}
