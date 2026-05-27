import 'dart:async';
import 'dart:io';
import 'dart:typed_data';
import 'package:flutter/services.dart';
import '../src/rust/api.dart' as rust_api;

/// Platform channel bridge for Android-specific functionality.
class PlatformService {
  static const _channel = MethodChannel('com.example.alldesk/platform');
  static const _frameChannel = EventChannel('com.example.alldesk/frames');

  static StreamSubscription? _frameSubscription;

  /// Request MediaProjection permission and start screen capture.
  static Future<bool> requestScreenCapture() async {
    if (!Platform.isAndroid) return false;
    try {
      final result = await _channel.invokeMethod<bool>('requestScreenCapture');
      return result ?? false;
    } on PlatformException {
      return false;
    }
  }

  /// Stop screen capture service.
  static Future<void> stopScreenCapture() async {
    if (!Platform.isAndroid) return;
    _frameSubscription?.cancel();
    _frameSubscription = null;
    try {
      await _channel.invokeMethod<void>('stopScreenCapture');
    } on PlatformException {}
  }

  /// Check if accessibility service is enabled.
  static Future<bool> isAccessibilityEnabled() async {
    if (!Platform.isAndroid) return false;
    try {
      final result = await _channel.invokeMethod<bool>('isAccessibilityEnabled');
      return result ?? false;
    } on PlatformException {
      return false;
    }
  }

  /// Open accessibility settings.
  static Future<void> openAccessibilitySettings() async {
    if (!Platform.isAndroid) return;
    try {
      await _channel.invokeMethod<void>('openAccessibilitySettings');
    } on PlatformException {}
  }

  /// Start listening for frames from Android ScreenCaptureService.
  /// Each frame is pushed into Rust via FFI.
  static void startFrameListener() {
    if (!Platform.isAndroid) return;

    _frameSubscription?.cancel();
    _frameSubscription = _frameChannel.receiveBroadcastStream().listen(
      (dynamic data) {
        if (data is Uint8List && data.length > 8) {
          // Frame format: [4 bytes width LE][4 bytes height LE][BGRA pixels]
          final widthBytes = data.sublist(0, 4);
          final heightBytes = data.sublist(4, 8);
          final width = widthBytes.buffer.asByteData().getUint32(0, Endian.little);
          final height = heightBytes.buffer.asByteData().getUint32(0, Endian.little);
          final bgra = data.sublist(8);

          // Push to Rust
          rust_api.pushAndroidFrame(
            bgraData: bgra,
            width: width,
            height: height,
          );
        }
      },
      onError: (error) {
        // Ignore frame stream errors
      },
    );
  }

  /// Stop the frame listener.
  static void stopFrameListener() {
    _frameSubscription?.cancel();
    _frameSubscription = null;
  }
}
