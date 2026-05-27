import '../src/rust/api.dart' as rust_api;

class RustBridgeService {
  static RustBridgeService? _instance;
  RustBridgeService._();
  static RustBridgeService get instance => _instance ??= RustBridgeService._();

  bool _initialized = false;

  Future<void> init() async {
    if (_initialized) return;
    await rust_api.init();
    _initialized = true;
  }

  Future<String> getVersion() => rust_api.getVersion();
  Future<String> getPing(String msg) => rust_api.ping(msg: msg);
  Future<String> getPeerId() => rust_api.getPeerId();
  Future<List<rust_api.PeerInfo>> discoverPeers({int timeoutSecs = 5}) =>
      rust_api.discoverPeers(timeoutSecs: BigInt.from(timeoutSecs));
  Future<String> startServer({int port = 21116}) => rust_api.startServer(port: port);
  Future<String> connectToPeer(String addr) => rust_api.connectToPeer(addr: addr);
  Future<String> disconnect() => rust_api.disconnect();
}
