import 'dart:async';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../src/rust/api.dart' as rust_api;

class PeerInfo {
  final String peerId;
  final String peerName;
  final String address;
  PeerInfo({required this.peerId, required this.peerName, required this.address});
}

class DiscoveryNotifier extends StateNotifier<List<PeerInfo>> {
  Timer? _pollTimer;

  DiscoveryNotifier() : super([]);

  void startPolling() {
    _pollTimer?.cancel();
    // Initial active scan to find peers quickly
    _poll(active: true);
    // Subsequent polls read from background discovery cache
    _pollTimer = Timer.periodic(const Duration(seconds: 3), (_) => _poll());
  }

  void stopPolling() {
    _pollTimer?.cancel();
    _pollTimer = null;
    state = [];
  }

  Future<void> _poll({bool active = false}) async {
    try {
      final timeout = active ? BigInt.from(3) : BigInt.zero;
      final peers = await rust_api.discoverPeers(timeoutSecs: timeout);
      state = peers.map((p) => PeerInfo(
        peerId: p.peerId,
        peerName: p.peerName,
        address: p.address,
      )).toList();
    } catch (_) {}
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    super.dispose();
  }
}

final discoveryProvider =
    StateNotifierProvider<DiscoveryNotifier, List<PeerInfo>>(
  (ref) => DiscoveryNotifier(),
);

final peerIdProvider = FutureProvider<String>((ref) async {
  return await rust_api.getPeerId();
});
