import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_zh.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
      : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations? of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations);
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
    delegate,
    GlobalMaterialLocalizations.delegate,
    GlobalCupertinoLocalizations.delegate,
    GlobalWidgetsLocalizations.delegate,
  ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('zh')
  ];

  /// No description provided for @appName.
  ///
  /// In zh, this message translates to:
  /// **'AllDesk'**
  String get appName;

  /// No description provided for @commonConnect.
  ///
  /// In zh, this message translates to:
  /// **'连接'**
  String get commonConnect;

  /// No description provided for @commonCancel.
  ///
  /// In zh, this message translates to:
  /// **'取消'**
  String get commonCancel;

  /// No description provided for @commonSave.
  ///
  /// In zh, this message translates to:
  /// **'保存'**
  String get commonSave;

  /// No description provided for @commonDelete.
  ///
  /// In zh, this message translates to:
  /// **'删除'**
  String get commonDelete;

  /// No description provided for @commonPlay.
  ///
  /// In zh, this message translates to:
  /// **'播放'**
  String get commonPlay;

  /// No description provided for @commonPause.
  ///
  /// In zh, this message translates to:
  /// **'暂停'**
  String get commonPause;

  /// No description provided for @commonSettings.
  ///
  /// In zh, this message translates to:
  /// **'设置'**
  String get commonSettings;

  /// No description provided for @commonConfirm.
  ///
  /// In zh, this message translates to:
  /// **'确定'**
  String get commonConfirm;

  /// No description provided for @commonLoading.
  ///
  /// In zh, this message translates to:
  /// **'加载中…'**
  String get commonLoading;

  /// No description provided for @homePeerId.
  ///
  /// In zh, this message translates to:
  /// **'本机 ID'**
  String get homePeerId;

  /// No description provided for @homePeerIdError.
  ///
  /// In zh, this message translates to:
  /// **'加载 ID 失败'**
  String get homePeerIdError;

  /// No description provided for @homeConnectTitle.
  ///
  /// In zh, this message translates to:
  /// **'连接设备'**
  String get homeConnectTitle;

  /// No description provided for @homeConnectHint.
  ///
  /// In zh, this message translates to:
  /// **'输入 Peer ID 或 IP 地址'**
  String get homeConnectHint;

  /// No description provided for @homeDevicesTitle.
  ///
  /// In zh, this message translates to:
  /// **'局域网设备'**
  String get homeDevicesTitle;

  /// No description provided for @homeScanning.
  ///
  /// In zh, this message translates to:
  /// **'扫描中…'**
  String get homeScanning;

  /// No description provided for @homeScanningForDevices.
  ///
  /// In zh, this message translates to:
  /// **'正在扫描设备…'**
  String get homeScanningForDevices;

  /// No description provided for @homeDiagnostics.
  ///
  /// In zh, this message translates to:
  /// **'诊断'**
  String get homeDiagnostics;

  /// No description provided for @homeScreenSharingActive.
  ///
  /// In zh, this message translates to:
  /// **'屏幕共享已开启 — 其他设备可以连接'**
  String get homeScreenSharingActive;

  /// No description provided for @homeStartSharing.
  ///
  /// In zh, this message translates to:
  /// **'开始屏幕共享'**
  String get homeStartSharing;

  /// No description provided for @homeStopSharing.
  ///
  /// In zh, this message translates to:
  /// **'停止屏幕共享'**
  String get homeStopSharing;

  /// No description provided for @homeRecordings.
  ///
  /// In zh, this message translates to:
  /// **'录像回放'**
  String get homeRecordings;

  /// No description provided for @homeRecentTitle.
  ///
  /// In zh, this message translates to:
  /// **'最近连接'**
  String get homeRecentTitle;

  /// No description provided for @homeFavoritesCount.
  ///
  /// In zh, this message translates to:
  /// **'{count} 收藏'**
  String homeFavoritesCount(int count);

  /// No description provided for @homeUnfavorite.
  ///
  /// In zh, this message translates to:
  /// **'取消收藏'**
  String get homeUnfavorite;

  /// No description provided for @homeFavorite.
  ///
  /// In zh, this message translates to:
  /// **'收藏'**
  String get homeFavorite;

  /// No description provided for @remoteConnectingTo.
  ///
  /// In zh, this message translates to:
  /// **'正在连接 {target}…'**
  String remoteConnectingTo(String target);

  /// No description provided for @remoteConnectedTo.
  ///
  /// In zh, this message translates to:
  /// **'已连接 {target}'**
  String remoteConnectedTo(String target);

  /// No description provided for @remoteGoBack.
  ///
  /// In zh, this message translates to:
  /// **'返回'**
  String get remoteGoBack;

  /// No description provided for @remoteWaitingForVideo.
  ///
  /// In zh, this message translates to:
  /// **'等待视频流…（已收到 {count} 帧）'**
  String remoteWaitingForVideo(int count);

  /// No description provided for @remoteVideoStarting.
  ///
  /// In zh, this message translates to:
  /// **'视频流启动中…'**
  String get remoteVideoStarting;

  /// No description provided for @remoteFullscreen.
  ///
  /// In zh, this message translates to:
  /// **'全屏'**
  String get remoteFullscreen;

  /// No description provided for @remoteExitFullscreen.
  ///
  /// In zh, this message translates to:
  /// **'退出全屏'**
  String get remoteExitFullscreen;

  /// No description provided for @remoteFileTransfer.
  ///
  /// In zh, this message translates to:
  /// **'文件传输'**
  String get remoteFileTransfer;

  /// No description provided for @remoteChat.
  ///
  /// In zh, this message translates to:
  /// **'聊天'**
  String get remoteChat;

  /// No description provided for @remoteDisconnect.
  ///
  /// In zh, this message translates to:
  /// **'断开连接'**
  String get remoteDisconnect;

  /// No description provided for @remoteStartRecording.
  ///
  /// In zh, this message translates to:
  /// **'录制会话'**
  String get remoteStartRecording;

  /// No description provided for @remoteStopRecording.
  ///
  /// In zh, this message translates to:
  /// **'停止录制'**
  String get remoteStopRecording;

  /// No description provided for @remoteRecordingDone.
  ///
  /// In zh, this message translates to:
  /// **'录制完成: {summary}'**
  String remoteRecordingDone(String summary);

  /// No description provided for @remoteRecordingFailed.
  ///
  /// In zh, this message translates to:
  /// **'录制操作失败: {error}'**
  String remoteRecordingFailed(String error);

  /// No description provided for @filesTitle.
  ///
  /// In zh, this message translates to:
  /// **'文件传输'**
  String get filesTitle;

  /// No description provided for @filesSending.
  ///
  /// In zh, this message translates to:
  /// **'发送文件到远程设备'**
  String get filesSending;

  /// No description provided for @filesReceiving.
  ///
  /// In zh, this message translates to:
  /// **'正在接收文件'**
  String get filesReceiving;

  /// No description provided for @filesHint.
  ///
  /// In zh, this message translates to:
  /// **'远程设备收到的文件保存在 Downloads/AllDesk 目录'**
  String get filesHint;

  /// No description provided for @filesPick.
  ///
  /// In zh, this message translates to:
  /// **'选择文件并发送'**
  String get filesPick;

  /// No description provided for @filesPicking.
  ///
  /// In zh, this message translates to:
  /// **'选择文件…'**
  String get filesPicking;

  /// No description provided for @filesLastError.
  ///
  /// In zh, this message translates to:
  /// **'上次传输出错: {error}'**
  String filesLastError(String error);

  /// No description provided for @filesSendFailed.
  ///
  /// In zh, this message translates to:
  /// **'发送失败'**
  String get filesSendFailed;

  /// No description provided for @filesBytes.
  ///
  /// In zh, this message translates to:
  /// **'{transferred} / {total} 字节'**
  String filesBytes(int transferred, int total);

  /// No description provided for @recTitle.
  ///
  /// In zh, this message translates to:
  /// **'录像回放'**
  String get recTitle;

  /// No description provided for @recEmpty.
  ///
  /// In zh, this message translates to:
  /// **'暂无录像'**
  String get recEmpty;

  /// No description provided for @recEmptyHint.
  ///
  /// In zh, this message translates to:
  /// **'在远程会话中点击录制按钮即可开始录制'**
  String get recEmptyHint;

  /// No description provided for @recDeleteTitle.
  ///
  /// In zh, this message translates to:
  /// **'删除录像'**
  String get recDeleteTitle;

  /// No description provided for @recDeleteFailed.
  ///
  /// In zh, this message translates to:
  /// **'删除失败'**
  String get recDeleteFailed;

  /// No description provided for @recListFailed.
  ///
  /// In zh, this message translates to:
  /// **'读取录像列表失败'**
  String get recListFailed;

  /// No description provided for @playerEnded.
  ///
  /// In zh, this message translates to:
  /// **'回放结束'**
  String get playerEnded;

  /// No description provided for @playerPreparing.
  ///
  /// In zh, this message translates to:
  /// **'准备回放…'**
  String get playerPreparing;

  /// No description provided for @playerReplay.
  ///
  /// In zh, this message translates to:
  /// **'重新播放'**
  String get playerReplay;

  /// No description provided for @playerBackToList.
  ///
  /// In zh, this message translates to:
  /// **'返回列表'**
  String get playerBackToList;

  /// No description provided for @playerStats.
  ///
  /// In zh, this message translates to:
  /// **'{count} 帧 @ {fps}fps'**
  String playerStats(int count, int fps);

  /// No description provided for @settingsTitle.
  ///
  /// In zh, this message translates to:
  /// **'设置'**
  String get settingsTitle;

  /// No description provided for @settingsDevice.
  ///
  /// In zh, this message translates to:
  /// **'设备'**
  String get settingsDevice;

  /// No description provided for @settingsDisplay.
  ///
  /// In zh, this message translates to:
  /// **'显示'**
  String get settingsDisplay;

  /// No description provided for @settingsStreaming.
  ///
  /// In zh, this message translates to:
  /// **'串流'**
  String get settingsStreaming;

  /// No description provided for @settingsNetwork.
  ///
  /// In zh, this message translates to:
  /// **'网络'**
  String get settingsNetwork;

  /// No description provided for @settingsAbout.
  ///
  /// In zh, this message translates to:
  /// **'关于'**
  String get settingsAbout;

  /// No description provided for @settingsPeerId.
  ///
  /// In zh, this message translates to:
  /// **'Peer ID'**
  String get settingsPeerId;

  /// No description provided for @settingsDisplayName.
  ///
  /// In zh, this message translates to:
  /// **'设备名称'**
  String get settingsDisplayName;

  /// No description provided for @settingsNameNotSet.
  ///
  /// In zh, this message translates to:
  /// **'未设置（使用主机名）'**
  String get settingsNameNotSet;

  /// No description provided for @settingsNameHint.
  ///
  /// In zh, this message translates to:
  /// **'输入其他人看到的名称'**
  String get settingsNameHint;

  /// No description provided for @settingsTheme.
  ///
  /// In zh, this message translates to:
  /// **'主题'**
  String get settingsTheme;

  /// No description provided for @settingsThemeSystem.
  ///
  /// In zh, this message translates to:
  /// **'跟随系统'**
  String get settingsThemeSystem;

  /// No description provided for @settingsThemeLight.
  ///
  /// In zh, this message translates to:
  /// **'浅色'**
  String get settingsThemeLight;

  /// No description provided for @settingsThemeDark.
  ///
  /// In zh, this message translates to:
  /// **'深色'**
  String get settingsThemeDark;

  /// No description provided for @settingsShowCursor.
  ///
  /// In zh, this message translates to:
  /// **'显示远程光标'**
  String get settingsShowCursor;

  /// No description provided for @settingsShowCursorSub.
  ///
  /// In zh, this message translates to:
  /// **'在画面上渲染远程光标'**
  String get settingsShowCursorSub;

  /// No description provided for @settingsVideoQuality.
  ///
  /// In zh, this message translates to:
  /// **'视频画质'**
  String get settingsVideoQuality;

  /// No description provided for @settingsFrameRate.
  ///
  /// In zh, this message translates to:
  /// **'帧率'**
  String get settingsFrameRate;

  /// No description provided for @settingsPort.
  ///
  /// In zh, this message translates to:
  /// **'QUIC 服务端口'**
  String get settingsPort;

  /// No description provided for @settingsPortHint.
  ///
  /// In zh, this message translates to:
  /// **'端口号 (1024-65535)'**
  String get settingsPortHint;

  /// No description provided for @settingsAutoConnect.
  ///
  /// In zh, this message translates to:
  /// **'自动连接上次设备'**
  String get settingsAutoConnect;

  /// No description provided for @settingsAutoConnectSub.
  ///
  /// In zh, this message translates to:
  /// **'应用启动时自动连接'**
  String get settingsAutoConnectSub;

  /// No description provided for @settingsVersion.
  ///
  /// In zh, this message translates to:
  /// **'版本'**
  String get settingsVersion;

  /// No description provided for @settingsOpenSource.
  ///
  /// In zh, this message translates to:
  /// **'开源'**
  String get settingsOpenSource;

  /// No description provided for @settingsOpenSourceSub.
  ///
  /// In zh, this message translates to:
  /// **'基于 Rust + Flutter 构建'**
  String get settingsOpenSourceSub;

  /// No description provided for @settingsBackHome.
  ///
  /// In zh, this message translates to:
  /// **'返回首页'**
  String get settingsBackHome;

  /// No description provided for @settingsQualityLow.
  ///
  /// In zh, this message translates to:
  /// **'低'**
  String get settingsQualityLow;

  /// No description provided for @settingsQualityMedium.
  ///
  /// In zh, this message translates to:
  /// **'中'**
  String get settingsQualityMedium;

  /// No description provided for @settingsQualityHigh.
  ///
  /// In zh, this message translates to:
  /// **'高'**
  String get settingsQualityHigh;

  /// No description provided for @settingsQualityUltra.
  ///
  /// In zh, this message translates to:
  /// **'极高'**
  String get settingsQualityUltra;

  /// No description provided for @settingsFpsCpuNote.
  ///
  /// In zh, this message translates to:
  /// **'（可能增加 CPU 占用）'**
  String get settingsFpsCpuNote;

  /// No description provided for @chatTitle.
  ///
  /// In zh, this message translates to:
  /// **'聊天'**
  String get chatTitle;

  /// No description provided for @chatHint.
  ///
  /// In zh, this message translates to:
  /// **'输入消息…'**
  String get chatHint;

  /// No description provided for @chatSend.
  ///
  /// In zh, this message translates to:
  /// **'发送'**
  String get chatSend;

  /// No description provided for @chatNotConnected.
  ///
  /// In zh, this message translates to:
  /// **'未连接，无法发送消息'**
  String get chatNotConnected;

  /// No description provided for @chatEmpty.
  ///
  /// In zh, this message translates to:
  /// **'会话消息将显示在这里'**
  String get chatEmpty;

  /// No description provided for @permTitle.
  ///
  /// In zh, this message translates to:
  /// **'权限引导'**
  String get permTitle;

  /// No description provided for @permIntro.
  ///
  /// In zh, this message translates to:
  /// **'被控端需要以下权限才能正常工作'**
  String get permIntro;

  /// No description provided for @permScreenCapture.
  ///
  /// In zh, this message translates to:
  /// **'屏幕采集'**
  String get permScreenCapture;

  /// No description provided for @permScreenCaptureDesc.
  ///
  /// In zh, this message translates to:
  /// **'用于捕获屏幕画面共享给控制端'**
  String get permScreenCaptureDesc;

  /// No description provided for @permAccessibility.
  ///
  /// In zh, this message translates to:
  /// **'无障碍服务（输入控制）'**
  String get permAccessibility;

  /// No description provided for @permAccessibilityDesc.
  ///
  /// In zh, this message translates to:
  /// **'用于接收控制端的鼠标键盘操作'**
  String get permAccessibilityDesc;

  /// No description provided for @permMicrophone.
  ///
  /// In zh, this message translates to:
  /// **'麦克风（可选）'**
  String get permMicrophone;

  /// No description provided for @permMicrophoneDesc.
  ///
  /// In zh, this message translates to:
  /// **'用于共享系统声音'**
  String get permMicrophoneDesc;

  /// No description provided for @permOpenSettings.
  ///
  /// In zh, this message translates to:
  /// **'去系统设置开启'**
  String get permOpenSettings;

  /// No description provided for @permRequest.
  ///
  /// In zh, this message translates to:
  /// **'申请权限'**
  String get permRequest;

  /// No description provided for @permGranted.
  ///
  /// In zh, this message translates to:
  /// **'已开启'**
  String get permGranted;

  /// No description provided for @permNotGranted.
  ///
  /// In zh, this message translates to:
  /// **'未开启'**
  String get permNotGranted;

  /// No description provided for @permSkip.
  ///
  /// In zh, this message translates to:
  /// **'跳过'**
  String get permSkip;

  /// No description provided for @permDone.
  ///
  /// In zh, this message translates to:
  /// **'完成'**
  String get permDone;

  /// No description provided for @permDesktopHint.
  ///
  /// In zh, this message translates to:
  /// **'桌面平台无需这些权限，直接开始使用即可'**
  String get permDesktopHint;

  /// No description provided for @errorOperationFailed.
  ///
  /// In zh, this message translates to:
  /// **'操作失败'**
  String get errorOperationFailed;

  /// No description provided for @errorRustInit.
  ///
  /// In zh, this message translates to:
  /// **'Rust 运行时初始化失败: {error}'**
  String errorRustInit(String error);

  /// No description provided for @errorRustNetInit.
  ///
  /// In zh, this message translates to:
  /// **'Rust 网络层初始化失败: {error}'**
  String errorRustNetInit(String error);
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'zh'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'zh':
      return AppLocalizationsZh();
  }

  throw FlutterError(
      'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
      'an issue with the localizations generation tool. Please file an issue '
      'on GitHub with a reproducible sample app and the gen-l10n configuration '
      'that was used.');
}
