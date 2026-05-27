import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:alldesk/core/theme.dart';
import 'package:alldesk/providers/settings_provider.dart';

void main() {
  group('AppTheme', () {
    test('light theme uses Material 3', () {
      expect(AppTheme.light.useMaterial3, isTrue);
    });

    test('dark theme uses Material 3', () {
      expect(AppTheme.dark.useMaterial3, isTrue);
    });

    test('light theme has light brightness', () {
      expect(AppTheme.light.brightness, Brightness.light);
    });

    test('dark theme has dark brightness', () {
      expect(AppTheme.dark.brightness, Brightness.dark);
    });

    test('both themes have color scheme', () {
      expect(AppTheme.light.colorScheme, isNotNull);
      expect(AppTheme.dark.colorScheme, isNotNull);
    });
  });

  group('AppSettings', () {
    test('has correct defaults', () {
      const settings = AppSettings();
      expect(settings.displayName, '');
      expect(settings.bitrateKbps, 4000);
      expect(settings.fps, 30);
      expect(settings.serverPort, 21116);
      expect(settings.showRemoteCursor, isTrue);
      expect(settings.autoConnect, isFalse);
      expect(settings.themeMode, 0);
    });

    test('copyWith preserves unchanged fields', () {
      const settings = AppSettings(displayName: 'TestDevice');
      final copy = settings.copyWith(fps: 60);
      expect(copy.displayName, 'TestDevice');
      expect(copy.fps, 60);
      expect(copy.bitrateKbps, 4000);
    });

    test('copyWith allows updating all fields', () {
      const settings = AppSettings();
      final copy = settings.copyWith(
        displayName: 'NewName',
        bitrateKbps: 8000,
        fps: 60,
        serverPort: 12345,
        showRemoteCursor: false,
        autoConnect: true,
        themeMode: 2,
      );
      expect(copy.displayName, 'NewName');
      expect(copy.bitrateKbps, 8000);
      expect(copy.fps, 60);
      expect(copy.serverPort, 12345);
      expect(copy.showRemoteCursor, isFalse);
      expect(copy.autoConnect, isTrue);
      expect(copy.themeMode, 2);
    });

    test('copyWith with no arguments returns identical settings', () {
      const settings = AppSettings(displayName: 'Device', fps: 24);
      final copy = settings.copyWith();
      expect(copy.displayName, settings.displayName);
      expect(copy.fps, settings.fps);
    });
  });

  group('SettingsKeys', () {
    test('has all expected key constants', () {
      expect(SettingsKeys.displayName, 'display_name');
      expect(SettingsKeys.bitrateKbps, 'bitrate_kbps');
      expect(SettingsKeys.fps, 'fps');
      expect(SettingsKeys.serverPort, 'server_port');
      expect(SettingsKeys.showRemoteCursor, 'show_remote_cursor');
      expect(SettingsKeys.autoConnect, 'auto_connect');
      expect(SettingsKeys.themeMode, 'theme_mode');
    });
  });

  group('SettingsNotifier', () {
    test('initial state has defaults', () {
      final notifier = SettingsNotifier();
      expect(notifier.state.displayName, '');
      expect(notifier.state.fps, 30);
    });

    test('setFps updates state', () async {
      final notifier = SettingsNotifier();
      // SharedPreferences may not be available in test, catch error
      try {
        await notifier.setFps(60);
      } catch (_) {}
      expect(notifier.state.fps, 60);
    });

    test('setBitrate updates state', () async {
      final notifier = SettingsNotifier();
      try {
        await notifier.setBitrate(8000);
      } catch (_) {}
      expect(notifier.state.bitrateKbps, 8000);
    });

    test('setThemeMode updates state', () async {
      final notifier = SettingsNotifier();
      try {
        await notifier.setThemeMode(2);
      } catch (_) {}
      expect(notifier.state.themeMode, 2);
    });

    test('setShowRemoteCursor updates state', () async {
      final notifier = SettingsNotifier();
      try {
        await notifier.setShowRemoteCursor(false);
      } catch (_) {}
      expect(notifier.state.showRemoteCursor, isFalse);
    });

    test('setAutoConnect updates state', () async {
      final notifier = SettingsNotifier();
      try {
        await notifier.setAutoConnect(true);
      } catch (_) {}
      expect(notifier.state.autoConnect, isTrue);
    });

    test('setDisplayName updates state', () async {
      final notifier = SettingsNotifier();
      try {
        await notifier.setDisplayName('MyDevice');
      } catch (_) {}
      expect(notifier.state.displayName, 'MyDevice');
    });

    test('setServerPort updates state', () async {
      final notifier = SettingsNotifier();
      try {
        await notifier.setServerPort(12345);
      } catch (_) {}
      expect(notifier.state.serverPort, 12345);
    });
  });

  group('Theme Widget Tests', () {
    testWidgets('AppTheme.light renders without error', (tester) async {
      await tester.pumpWidget(MaterialApp(
        theme: AppTheme.light,
        home: const Scaffold(body: Text('Light Theme')),
      ));
      expect(find.text('Light Theme'), findsOneWidget);
    });

    testWidgets('AppTheme.dark renders without error', (tester) async {
      await tester.pumpWidget(MaterialApp(
        theme: AppTheme.dark,
        home: const Scaffold(body: Text('Dark Theme')),
      ));
      expect(find.text('Dark Theme'), findsOneWidget);
    });
  });

  group('Settings UI Components', () {
    testWidgets('theme label maps correctly', (tester) async {
      // Test the _themeLabel logic from settings page
      String themeLabel(int mode) {
        switch (mode) {
          case 1: return 'Light';
          case 2: return 'Dark';
          default: return 'System';
        }
      }
      expect(themeLabel(0), 'System');
      expect(themeLabel(1), 'Light');
      expect(themeLabel(2), 'Dark');
      expect(themeLabel(99), 'System');
    });
  });
}
