import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

class SettingsKeys {
  static const String displayName = 'display_name';
  static const String bitrateKbps = 'bitrate_kbps';
  static const String fps = 'fps';
  static const String serverPort = 'server_port';
  static const String showRemoteCursor = 'show_remote_cursor';
  static const String autoConnect = 'auto_connect';
  static const String themeMode = 'theme_mode'; // 0=system, 1=light, 2=dark
}

class AppSettings {
  final String displayName;
  final int bitrateKbps;
  final int fps;
  final int serverPort;
  final bool showRemoteCursor;
  final bool autoConnect;
  final int themeMode;

  const AppSettings({
    this.displayName = '',
    this.bitrateKbps = 4000,
    this.fps = 30,
    this.serverPort = 21116,
    this.showRemoteCursor = true,
    this.autoConnect = false,
    this.themeMode = 0,
  });

  AppSettings copyWith({
    String? displayName,
    int? bitrateKbps,
    int? fps,
    int? serverPort,
    bool? showRemoteCursor,
    bool? autoConnect,
    int? themeMode,
  }) =>
      AppSettings(
        displayName: displayName ?? this.displayName,
        bitrateKbps: bitrateKbps ?? this.bitrateKbps,
        fps: fps ?? this.fps,
        serverPort: serverPort ?? this.serverPort,
        showRemoteCursor: showRemoteCursor ?? this.showRemoteCursor,
        autoConnect: autoConnect ?? this.autoConnect,
        themeMode: themeMode ?? this.themeMode,
      );
}

class SettingsNotifier extends StateNotifier<AppSettings> {
  SettingsNotifier() : super(const AppSettings());

  Future<void> load() async {
    final prefs = await SharedPreferences.getInstance();
    state = AppSettings(
      displayName: prefs.getString(SettingsKeys.displayName) ?? '',
      bitrateKbps: prefs.getInt(SettingsKeys.bitrateKbps) ?? 4000,
      fps: prefs.getInt(SettingsKeys.fps) ?? 30,
      serverPort: prefs.getInt(SettingsKeys.serverPort) ?? 21116,
      showRemoteCursor: prefs.getBool(SettingsKeys.showRemoteCursor) ?? true,
      autoConnect: prefs.getBool(SettingsKeys.autoConnect) ?? false,
      themeMode: prefs.getInt(SettingsKeys.themeMode) ?? 0,
    );
  }

  Future<void> _save(String key, dynamic value) async {
    final prefs = await SharedPreferences.getInstance();
    if (value is String) await prefs.setString(key, value);
    if (value is int) await prefs.setInt(key, value);
    if (value is bool) await prefs.setBool(key, value);
  }

  Future<void> setDisplayName(String v) async {
    state = state.copyWith(displayName: v);
    await _save(SettingsKeys.displayName, v);
  }

  Future<void> setBitrate(int v) async {
    state = state.copyWith(bitrateKbps: v);
    await _save(SettingsKeys.bitrateKbps, v);
  }

  Future<void> setFps(int v) async {
    state = state.copyWith(fps: v);
    await _save(SettingsKeys.fps, v);
  }

  Future<void> setServerPort(int v) async {
    state = state.copyWith(serverPort: v);
    await _save(SettingsKeys.serverPort, v);
  }

  Future<void> setShowRemoteCursor(bool v) async {
    state = state.copyWith(showRemoteCursor: v);
    await _save(SettingsKeys.showRemoteCursor, v);
  }

  Future<void> setAutoConnect(bool v) async {
    state = state.copyWith(autoConnect: v);
    await _save(SettingsKeys.autoConnect, v);
  }

  Future<void> setThemeMode(int v) async {
    state = state.copyWith(themeMode: v);
    await _save(SettingsKeys.themeMode, v);
  }
}

final settingsProvider =
    StateNotifierProvider<SettingsNotifier, AppSettings>(
  (ref) => SettingsNotifier()..load(),
);
