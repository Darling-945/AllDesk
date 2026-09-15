import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'src/rust/frb_generated.dart';
import 'src/rust/api.dart' as rust_api;
import 'services/error_ui.dart';
import 'services/platform_service.dart';
import 'app.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Run Rust init in parallel — don't block the UI
  _initRust();

  runApp(const ProviderScope(child: AllDeskApp()));
}

Future<void> _initRust() async {
  try {
    await RustLib.init().timeout(const Duration(seconds: 5));
  } catch (e) {
    debugPrint('RustLib.init() failed: $e');
    // Surfaced to the user via the root messenger once the app is up.
    showStartupError((l10n) => l10n == null
        ? 'Rust 运行时初始化失败: $e'
        : l10n.errorRustInit(e.toString()));
    return;
  }
  try {
    await rust_api.init().timeout(const Duration(seconds: 5));
  } catch (e) {
    debugPrint('rust_api.init() failed: $e');
    showStartupError((l10n) => l10n == null
        ? 'Rust 网络层初始化失败: $e'
        : l10n.errorRustNetInit(e.toString()));
  }

  // On Android, start frame listener so incoming frames from ScreenCaptureService
  // are forwarded to Rust's AndroidCapturer via FFI.
  if (Platform.isAndroid) {
    PlatformService.startFrameListener();
  }
}
