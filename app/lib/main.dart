import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'src/rust/frb_generated.dart';
import 'src/rust/api.dart' as rust_api;
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
    return;
  }
  try {
    await rust_api.init().timeout(const Duration(seconds: 5));
  } catch (e) {
    debugPrint('rust_api.init() failed: $e');
  }

  // On Android, start frame listener so incoming frames from ScreenCaptureService
  // are forwarded to Rust's AndroidCapturer via FFI.
  if (Platform.isAndroid) {
    PlatformService.startFrameListener();
  }
}
