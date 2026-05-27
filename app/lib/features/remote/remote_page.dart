import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../src/rust/api.dart' as rust_api;
import 'touch_gesture_handler.dart';

class RemotePage extends ConsumerStatefulWidget {
  final String peerId;
  const RemotePage({super.key, required this.peerId});

  @override
  ConsumerState<RemotePage> createState() => _RemotePageState();
}

class _RemotePageState extends ConsumerState<RemotePage> {
  bool _connecting = true;
  String? _error;
  bool _isFullscreen = false;
  Timer? _frameTimer;
  ui.Image? _currentImage;
  int _frameWidth = 0;
  int _frameHeight = 0;
  int _frameCount = 0;

  @override
  void initState() {
    super.initState();
    _connect();
  }

  @override
  void dispose() {
    _frameTimer?.cancel();
    _disconnect();
    _currentImage?.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    try {
      await rust_api.connectToPeer(addr: widget.peerId);
      if (mounted) {
        setState(() => _connecting = false);
        _startFramePolling();
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _connecting = false;
          _error = e.toString();
        });
      }
    }
  }

  void _startFramePolling() {
    _frameTimer = Timer.periodic(
      const Duration(milliseconds: 33),
      (_) => _pollFrame(),
    );
  }

  Future<void> _pollFrame() async {
    try {
      final frameData = await rust_api.pollVideoFrame();
      if (frameData != null && frameData.length > 8 && mounted) {
        // First 4 bytes = width (u32 LE), next 4 bytes = height (u32 LE)
        final widthBytes = frameData.sublist(0, 4);
        final heightBytes = frameData.sublist(4, 8);
        final width = (widthBytes[0]) | (widthBytes[1] << 8) | (widthBytes[2] << 16) | (widthBytes[3] << 24);
        final height = (heightBytes[0]) | (heightBytes[1] << 8) | (heightBytes[2] << 16) | (heightBytes[3] << 24);

        if (width > 0 && height > 0) {
          final bgraData = frameData.sublist(8);
          final image = await _bgraToImage(bgraData, width, height);
          if (image != null && mounted) {
            setState(() {
              _currentImage?.dispose();
              _currentImage = image;
              _frameWidth = width;
              _frameHeight = height;
              _frameCount++;
            });
          }
        }
      }
    } catch (_) {}
  }

  Future<ui.Image?> _bgraToImage(Uint8List bgra, int width, int height) async {
    try {
      final completer = Completer<ui.Image>();
      ui.decodeImageFromPixels(
        bgra,
        width,
        height,
        ui.PixelFormat.bgra8888,
        (image) => completer.complete(image),
      );
      return completer.future;
    } catch (_) {
      return null;
    }
  }

  Future<void> _disconnect() async {
    try {
      await rust_api.stopStream();
      await rust_api.disconnect();
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      body: Stack(
        children: [
          Center(child: _buildContent()),
          Positioned(
            top: 8,
            right: 8,
            child: _buildToolbar(),
          ),
          if (_connecting)
            Positioned(
              top: 8,
              left: 8,
              child: Card(
                color: Colors.black87,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(strokeWidth: 2, color: Colors.white),
                      ),
                      const SizedBox(width: 8),
                      Text(
                        'Connecting to ${widget.peerId}...',
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Colors.white),
                      ),
                    ],
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildContent() {
    if (_error != null) {
      return Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(Icons.error_outline, color: Colors.red, size: 48),
          const SizedBox(height: 12),
          Text(_error!, style: const TextStyle(color: Colors.white)),
          const SizedBox(height: 16),
          FilledButton(
            onPressed: () => context.go('/'),
            child: const Text('Go Back'),
          ),
        ],
      );
    }

    if (_currentImage != null) {
      final remoteSize = Size(_frameWidth.toDouble(), _frameHeight.toDouble());
      return TouchGestureHandler(
        remoteResolution: remoteSize,
        child: CustomPaint(
          size: Size.infinite,
          painter: _FramePainter(_currentImage!),
        ),
      );
    }

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Icon(Icons.desktop_windows, color: Colors.white54, size: 64),
        const SizedBox(height: 12),
        Text(
          'Connected to ${widget.peerId}',
          style: const TextStyle(color: Colors.white),
        ),
        const SizedBox(height: 4),
        const Text(
          'Video stream starting...',
          style: TextStyle(color: Colors.white54),
        ),
      ],
    );
  }

  Widget _buildToolbar() {
    return Card(
      color: Colors.black87,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: Icon(_isFullscreen ? Icons.fullscreen_exit : Icons.fullscreen, color: Colors.white),
            onPressed: _toggleFullscreen,
            tooltip: _isFullscreen ? 'Exit Fullscreen' : 'Fullscreen',
          ),
          if (_frameWidth > 0)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Text(
                '${_frameWidth}x$_frameHeight',
                style: const TextStyle(color: Colors.white54, fontSize: 11),
              ),
            ),
          IconButton(
            icon: const Icon(Icons.folder, color: Colors.white),
            onPressed: () {},
            tooltip: 'File Transfer',
          ),
          IconButton(
            icon: const Icon(Icons.call_end, color: Colors.red),
            onPressed: () => context.go('/'),
            tooltip: 'Disconnect',
          ),
        ],
      ),
    );
  }

  void _toggleFullscreen() {
    setState(() => _isFullscreen = !_isFullscreen);
  }
}

/// Paints a video frame (ui.Image) scaled to fill the available space.
class _FramePainter extends CustomPainter {
  final ui.Image _image;

  _FramePainter(this._image);

  @override
  void paint(Canvas canvas, Size size) {
    final imgW = _image.width.toDouble();
    final imgH = _image.height.toDouble();

    // Fit while preserving aspect ratio
    final scale = size.width / imgW < size.height / imgH
        ? size.width / imgW
        : size.height / imgH;
    final drawW = imgW * scale;
    final drawH = imgH * scale;
    final dx = (size.width - drawW) / 2;
    final dy = (size.height - drawH) / 2;

    canvas.drawImageRect(
      _image,
      Rect.fromLTWH(0, 0, imgW, imgH),
      Rect.fromLTWH(dx, dy, drawW, drawH),
      Paint()..filterQuality = FilterQuality.medium,
    );
  }

  @override
  bool shouldRepaint(_FramePainter old) => old._image != _image;
}
