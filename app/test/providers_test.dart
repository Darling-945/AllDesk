import 'package:flutter_test/flutter_test.dart';
import 'package:alldesk/providers/discovery_provider.dart';
import 'package:alldesk/providers/settings_provider.dart';

void main() {
  group('PeerInfo', () {
    test('creates with required fields', () {
      final peer = PeerInfo(
        peerId: 'peer-123',
        peerName: 'TestDevice',
        address: '192.168.1.100:21116',
      );
      expect(peer.peerId, 'peer-123');
      expect(peer.peerName, 'TestDevice');
      expect(peer.address, '192.168.1.100:21116');
    });

    test('different peers are not equal by reference', () {
      final peer1 = PeerInfo(peerId: 'a', peerName: 'A', address: '1.1.1.1:21116');
      final peer2 = PeerInfo(peerId: 'b', peerName: 'B', address: '2.2.2.2:21116');
      expect(identical(peer1, peer2), isFalse);
    });
  });

  group('DiscoveryNotifier', () {
    test('initial state is empty', () {
      final notifier = DiscoveryNotifier();
      expect(notifier.state, isEmpty);
    });

    test('stopPolling clears state', () {
      final notifier = DiscoveryNotifier();
      // Manually set some state
      notifier.state = [
        PeerInfo(peerId: 'test', peerName: 'Test', address: '1.2.3.4:21116'),
      ];
      expect(notifier.state.length, 1);

      notifier.stopPolling();
      expect(notifier.state, isEmpty);
    });

    test('dispose does not throw', () {
      final notifier = DiscoveryNotifier();
      // Should not throw
      notifier.dispose();
    });

    test('startPolling does not throw without Rust bridge', () {
      final notifier = DiscoveryNotifier();
      // startPolling calls _poll which calls rust_api — should catch silently
      // This test verifies it doesn't throw synchronously
      notifier.startPolling();
      // Clean up immediately
      notifier.stopPolling();
    });

    test('multiple startPolling calls are safe', () {
      final notifier = DiscoveryNotifier();
      notifier.startPolling();
      notifier.startPolling(); // Should cancel previous timer
      notifier.stopPolling();
    });
  });

  group('AppSettings', () {
    test('defaults are correct', () {
      const settings = AppSettings();
      expect(settings.displayName, '');
      expect(settings.bitrateKbps, 4000);
      expect(settings.fps, 30);
      expect(settings.serverPort, 21116);
      expect(settings.showRemoteCursor, isTrue);
      expect(settings.autoConnect, isFalse);
      expect(settings.themeMode, 0);
    });

    test('copyWith creates new instance', () {
      const original = AppSettings();
      final modified = original.copyWith(displayName: 'New');
      expect(modified.displayName, 'New');
      expect(original.displayName, ''); // Original unchanged
    });

    test('copyWith partial update', () {
      const settings = AppSettings(displayName: 'Test', fps: 60);
      final updated = settings.copyWith(bitrateKbps: 10000);
      expect(updated.displayName, 'Test');
      expect(updated.fps, 60);
      expect(updated.bitrateKbps, 10000);
    });
  });

  group('SettingsNotifier', () {
    late SettingsNotifier notifier;

    setUp(() {
      notifier = SettingsNotifier();
    });

    tearDown(() {
      notifier.dispose();
    });

    test('initial state matches AppSettings defaults', () {
      expect(notifier.state.displayName, '');
      expect(notifier.state.bitrateKbps, 4000);
      expect(notifier.state.fps, 30);
    });

    test('setDisplayName updates state immediately', () async {
      try { await notifier.setDisplayName('MyPC'); } catch (_) {}
      expect(notifier.state.displayName, 'MyPC');
    });

    test('setFps updates state immediately', () async {
      try { await notifier.setFps(60); } catch (_) {}
      expect(notifier.state.fps, 60);
    });

    test('setBitrate updates state immediately', () async {
      try { await notifier.setBitrate(8000); } catch (_) {}
      expect(notifier.state.bitrateKbps, 8000);
    });

    test('setThemeMode updates state immediately', () async {
      try { await notifier.setThemeMode(2); } catch (_) {}
      expect(notifier.state.themeMode, 2);
    });

    test('setShowRemoteCursor updates state immediately', () async {
      try { await notifier.setShowRemoteCursor(false); } catch (_) {}
      expect(notifier.state.showRemoteCursor, isFalse);
    });

    test('setAutoConnect updates state immediately', () async {
      try { await notifier.setAutoConnect(true); } catch (_) {}
      expect(notifier.state.autoConnect, isTrue);
    });

    test('setServerPort updates state immediately', () async {
      try { await notifier.setServerPort(9999); } catch (_) {}
      expect(notifier.state.serverPort, 9999);
    });

    test('multiple updates chain correctly', () async {
      // Each setXxx updates state synchronously before attempting save.
      // Save may fail in test env but state should still update.
      try { await notifier.setDisplayName('Device1'); } catch (_) {}
      try { await notifier.setFps(24); } catch (_) {}
      try { await notifier.setThemeMode(1); } catch (_) {}
      expect(notifier.state.displayName, 'Device1');
      expect(notifier.state.fps, 24);
      expect(notifier.state.themeMode, 1);
      // Other fields still at defaults
      expect(notifier.state.bitrateKbps, 4000);
    });
  });
}
