// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appName => 'AllDesk';

  @override
  String get commonConnect => 'Connect';

  @override
  String get commonCancel => 'Cancel';

  @override
  String get commonSave => 'Save';

  @override
  String get commonDelete => 'Delete';

  @override
  String get commonPlay => 'Play';

  @override
  String get commonPause => 'Pause';

  @override
  String get commonSettings => 'Settings';

  @override
  String get commonConfirm => 'OK';

  @override
  String get commonLoading => 'Loading…';

  @override
  String get homePeerId => 'Your Peer ID';

  @override
  String get homePeerIdError => 'Error loading ID';

  @override
  String get homeConnectTitle => 'Connect to Device';

  @override
  String get homeConnectHint => 'Enter Peer ID or IP address';

  @override
  String get homeDevicesTitle => 'Devices on LAN';

  @override
  String get homeScanning => 'Scanning…';

  @override
  String get homeScanningForDevices => 'Scanning for devices…';

  @override
  String get homeDiagnostics => 'Diagnostics';

  @override
  String get homeScreenSharingActive =>
      'Screen sharing active — other devices can connect';

  @override
  String get homeStartSharing => 'Start Screen Sharing';

  @override
  String get homeStopSharing => 'Stop Screen Sharing';

  @override
  String get homeRecordings => 'Recordings';

  @override
  String get homeRecentTitle => 'Recent Connections';

  @override
  String homeFavoritesCount(int count) {
    return '$count favorites';
  }

  @override
  String get homeUnfavorite => 'Unfavorite';

  @override
  String get homeFavorite => 'Favorite';

  @override
  String remoteConnectingTo(String target) {
    return 'Connecting to $target…';
  }

  @override
  String remoteConnectedTo(String target) {
    return 'Connected to $target';
  }

  @override
  String get remoteGoBack => 'Go Back';

  @override
  String remoteWaitingForVideo(int count) {
    return 'Waiting for video… ($count frames)';
  }

  @override
  String get remoteVideoStarting => 'Video stream starting…';

  @override
  String get remoteFullscreen => 'Fullscreen';

  @override
  String get remoteExitFullscreen => 'Exit Fullscreen';

  @override
  String get remoteFileTransfer => 'File Transfer';

  @override
  String get remoteChat => 'Chat';

  @override
  String get remoteDisconnect => 'Disconnect';

  @override
  String get remoteStartRecording => 'Record Session';

  @override
  String get remoteStopRecording => 'Stop Recording';

  @override
  String remoteRecordingDone(String summary) {
    return 'Recording saved: $summary';
  }

  @override
  String remoteRecordingFailed(String error) {
    return 'Recording failed: $error';
  }

  @override
  String get filesTitle => 'File Transfer';

  @override
  String get filesSending => 'Send files to the remote device';

  @override
  String get filesReceiving => 'Receiving file';

  @override
  String get filesHint =>
      'Files received on the remote device are saved to Downloads/AllDesk';

  @override
  String get filesPick => 'Pick a file and send';

  @override
  String get filesPicking => 'Picking file…';

  @override
  String filesLastError(String error) {
    return 'Last transfer error: $error';
  }

  @override
  String get filesSendFailed => 'Send failed';

  @override
  String filesBytes(int transferred, int total) {
    return '$transferred / $total bytes';
  }

  @override
  String get recTitle => 'Recordings';

  @override
  String get recEmpty => 'No recordings yet';

  @override
  String get recEmptyHint =>
      'Tap the record button in a remote session to start recording';

  @override
  String get recDeleteTitle => 'Delete recording';

  @override
  String get recDeleteFailed => 'Delete failed';

  @override
  String get recListFailed => 'Failed to list recordings';

  @override
  String get playerEnded => 'Playback finished';

  @override
  String get playerPreparing => 'Preparing playback…';

  @override
  String get playerReplay => 'Replay';

  @override
  String get playerBackToList => 'Back to list';

  @override
  String playerStats(int count, int fps) {
    return '$count frames @ ${fps}fps';
  }

  @override
  String get settingsTitle => 'Settings';

  @override
  String get settingsDevice => 'Device';

  @override
  String get settingsDisplay => 'Display';

  @override
  String get settingsStreaming => 'Streaming';

  @override
  String get settingsNetwork => 'Network';

  @override
  String get settingsAbout => 'About';

  @override
  String get settingsPeerId => 'Peer ID';

  @override
  String get settingsDisplayName => 'Display Name';

  @override
  String get settingsNameNotSet => 'Not set (uses hostname)';

  @override
  String get settingsNameHint => 'Enter a name others will see';

  @override
  String get settingsTheme => 'Theme';

  @override
  String get settingsThemeSystem => 'System';

  @override
  String get settingsThemeLight => 'Light';

  @override
  String get settingsThemeDark => 'Dark';

  @override
  String get settingsShowCursor => 'Show Remote Cursor';

  @override
  String get settingsShowCursorSub => 'Render remote cursor on screen';

  @override
  String get settingsVideoQuality => 'Video Quality';

  @override
  String get settingsFrameRate => 'Frame Rate';

  @override
  String get settingsPort => 'QUIC Server Port';

  @override
  String get settingsPortHint => 'Port number (1024-65535)';

  @override
  String get settingsAutoConnect => 'Auto-Connect to Last Device';

  @override
  String get settingsAutoConnectSub => 'Connect on app launch';

  @override
  String get settingsVersion => 'Version';

  @override
  String get settingsOpenSource => 'Open Source';

  @override
  String get settingsOpenSourceSub => 'Built with Rust + Flutter';

  @override
  String get settingsBackHome => 'Back to Home';

  @override
  String get settingsQualityLow => 'Low';

  @override
  String get settingsQualityMedium => 'Medium';

  @override
  String get settingsQualityHigh => 'High';

  @override
  String get settingsQualityUltra => 'Ultra';

  @override
  String get settingsFpsCpuNote => '(may increase CPU)';

  @override
  String get chatTitle => 'Chat';

  @override
  String get chatHint => 'Type a message…';

  @override
  String get chatSend => 'Send';

  @override
  String get chatNotConnected => 'Not connected — cannot send messages';

  @override
  String get chatEmpty => 'Session messages will appear here';

  @override
  String get permTitle => 'Permission Guide';

  @override
  String get permIntro =>
      'The following permissions are needed to be controlled remotely';

  @override
  String get permScreenCapture => 'Screen Capture';

  @override
  String get permScreenCaptureDesc =>
      'Captures the screen to share with the controller';

  @override
  String get permAccessibility => 'Accessibility Service (input control)';

  @override
  String get permAccessibilityDesc =>
      'Receives mouse and keyboard input from the controller';

  @override
  String get permMicrophone => 'Microphone (optional)';

  @override
  String get permMicrophoneDesc => 'Shares system audio';

  @override
  String get permOpenSettings => 'Open System Settings';

  @override
  String get permRequest => 'Request Permission';

  @override
  String get permGranted => 'Enabled';

  @override
  String get permNotGranted => 'Not enabled';

  @override
  String get permSkip => 'Skip';

  @override
  String get permDone => 'Done';

  @override
  String get permDesktopHint =>
      'Desktop platforms don\'t need any of these permissions — you\'re all set';

  @override
  String get errorOperationFailed => 'Operation failed';

  @override
  String errorRustInit(String error) {
    return 'Rust runtime init failed: $error';
  }

  @override
  String errorRustNetInit(String error) {
    return 'Rust networking init failed: $error';
  }
}
