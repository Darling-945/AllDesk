import 'dart:async';
import 'dart:io' show Platform;
import 'dart:typed_data';
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import '../../l10n/app_localizations.dart';
import '../../src/rust/api.dart' as rust_api;

/// Sequential ALDREC playback: pulls decoded BGRA frames from Rust on a
/// timer paced by the recording's own fps, and paints them like the live
/// remote view. Supports play/pause and replay from the start.
class RecordingPlayerPage extends StatefulWidget {
  final String path;
  const RecordingPlayerPage({super.key, required this.path});

  @override
  State<RecordingPlayerPage> createState() => _RecordingPlayerPageState();
}

class _RecordingPlayerPageState extends State<RecordingPlayerPage> {
  rust_api.RecordingInfo? _info;
  String? _error;
  Timer? _timer;
  ui.Image? _currentImage;
  int _framesShown = 0;
  bool _playing = false;
  bool _ended = false;

  @override
  void initState() {
    super.initState();
    _open();
  }

  @override
  void dispose() {
    _timer?.cancel();
    _currentImage?.dispose();
    rust_api.stopRecordingPlayback();
    super.dispose();
  }

  Future<void> _open() async {
    try {
      final info = await rust_api.startRecordingPlayback(path: widget.path);
      if (!mounted) return;
      setState(() => _info = info);
      _play();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  void _play() {
    final fps = _info?.fps ?? 0;
    if (fps <= 0) return;
    _timer?.cancel();
    setState(() {
      _playing = true;
      _ended = false;
    });
    _timer = Timer.periodic(
      Duration(milliseconds: (1000 / fps).round()),
      (_) => _pullFrame(),
    );
  }

  void _pause() {
    _timer?.cancel();
    _timer = null;
    if (mounted) setState(() => _playing = false);
  }

  Future<void> _pullFrame() async {
    try {
      final frame = await rust_api.recordingNextFrame();
      if (frame == null) {
        _pause();
        if (mounted) setState(() => _ended = true);
        return;
      }
      final image = await _bgraToImage(frame.bgra, frame.width, frame.height);
      if (image != null && mounted) {
        setState(() {
          _currentImage?.dispose();
          _currentImage = image;
          _framesShown++;
        });
      }
    } catch (_) {
      _pause();
    }
  }

  Future<void> _replay() async {
    try {
      await rust_api.rewindRecordingPlayback();
      if (mounted) setState(() => _framesShown = 0);
      _play();
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

  String _formatDuration(int ms) {
    final s = ms ~/ 1000;
    return '${(s ~/ 60).toString().padLeft(2, '0')}:${(s % 60).toString().padLeft(2, '0')}';
  }

  @override
  Widget build(BuildContext context) {
    final info = _info;
    // u64 crosses the bridge as BigInt; millis fit comfortably in int.
    final durationMs = info?.durationMs.toInt() ?? 0;
    final progress =
        info != null && info.frameCount > 0 ? _framesShown / info.frameCount : 0.0;
    // Elapsed time estimated from frame index — decoding is sequential, so
    // this tracks the real playback position closely enough for a progress
    // indicator (there is no seek yet).
    final elapsedMs = (durationMs * progress).round();

    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        title: Text(widget.path.split(Platform.pathSeparator).last),
        backgroundColor: Colors.black87,
        foregroundColor: Colors.white,
      ),
      body: Column(
        children: [
          Expanded(child: Center(child: _buildContent())),
          _buildControls(durationMs, elapsedMs, progress),
        ],
      ),
    );
  }

  Widget _buildContent() {
    if (_error != null) {
      return Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.error_outline, color: Colors.red, size: 48),
            const SizedBox(height: 12),
            Text(_error!, style: const TextStyle(color: Colors.white)),
          ],
        ),
      );
    }
    final image = _currentImage;
    if (image != null) {
      return CustomPaint(
        size: Size.infinite,
        painter: _FramePainter(image),
      );
    }
    if (_info == null) {
      return const CircularProgressIndicator(color: Colors.white);
    }
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Icon(Icons.movie, color: Colors.white54, size: 48),
        const SizedBox(height: 12),
        Text(
          _ended ? AppLocalizations.of(context)!.playerEnded : AppLocalizations.of(context)!.playerPreparing,
          style: const TextStyle(color: Colors.white54),
        ),
      ],
    );
  }

  Widget _buildControls(int durationMs, int elapsedMs, double progress) {
    final l10n = AppLocalizations.of(context)!;
    return Container(
      color: Colors.black87,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              IconButton(
                icon: Icon(
                  _playing ? Icons.pause : Icons.play_arrow,
                  color: Colors.white,
                ),
                tooltip: _playing ? l10n.commonPause : l10n.commonPlay,
                onPressed: _info == null ? null : (_playing ? _pause : _play),
              ),
              IconButton(
                icon: const Icon(Icons.replay, color: Colors.white),
                tooltip: l10n.playerReplay,
                onPressed: _info == null ? null : _replay,
              ),
              Expanded(
                child: Text(
                  '${_formatDuration(elapsedMs)} / ${_formatDuration(durationMs)}'
                  '${_info != null ? '  (${l10n.playerStats(_info!.frameCount, _info!.fps)})' : ''}',
                  style: const TextStyle(color: Colors.white54, fontSize: 12),
                  textAlign: TextAlign.center,
                ),
              ),
              IconButton(
                icon: const Icon(Icons.close, color: Colors.white),
                tooltip: l10n.playerBackToList,
                onPressed: () => context.go('/recordings'),
              ),
            ],
          ),
          const SizedBox(height: 4),
          LinearProgressIndicator(value: progress, minHeight: 3),
        ],
      ),
    );
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
