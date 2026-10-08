import 'dart:async';
import 'dart:io' show Platform;
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:path_provider/path_provider.dart';
import '../../l10n/app_localizations.dart';
import '../../services/key_mapping.dart';
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
  StreamSubscription<rust_api.VideoFrameMsg>? _frameSub;
  Timer? _statusTimer;
  ui.Image? _currentImage;
  int _frameWidth = 0;
  int _frameHeight = 0;
  int _frameCount = 0;
  String _connectionStatus = '';
  bool _recording = false;

  /// Receives hardware keyboard events for forwarding to the remote.
  final _keyboardFocus = FocusNode(debugLabel: 'remote-keyboard');
  /// Mobile soft-keyboard input bar (desktop keyboards go through
  /// [_keyboardFocus] directly).
  final _softInputController = TextEditingController();
  String _lastSoftInput = '';
  bool _showInputBar = false;

  @override
  void initState() {
    super.initState();
    _connect();
  }

  @override
  void dispose() {
    _keyboardFocus.dispose();
    _softInputController.dispose();
    _frameSub?.cancel();
    _statusTimer?.cancel();
    _disconnect();
    _currentImage?.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    try {
      await rust_api.connectToPeer(addr: widget.peerId);
      if (mounted) {
        setState(() => _connecting = false);
        _startFrameStream();
        _startStatusPolling();
        _requestKeyboardFocus();
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

  void _requestKeyboardFocus() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && !_showInputBar) _keyboardFocus.requestFocus();
    });
  }

  // --------------------------------------------------------------------------
  // Keyboard forwarding
  // --------------------------------------------------------------------------

  KeyEventResult _handleKeyEvent(FocusNode node, KeyEvent event) {
    final pressed = event is KeyDownEvent || event is KeyRepeatEvent;
    if (!pressed && event is! KeyUpEvent) return KeyEventResult.ignored;

    // Control keys and modifiers go by protocol code; printable text goes
    // as Unicode chars. With Ctrl/Meta held the character is a control
    // code, so letters fall through to the raw-VK path for shortcuts like
    // Ctrl+C (the host injects the real virtual key).
    final code = specialCodeFor(event.logicalKey);
    if (code != null) {
      _sendKey('special', code, pressed);
      return KeyEventResult.handled;
    }

    final combo = HardwareKeyboard.instance.isControlPressed ||
        HardwareKeyboard.instance.isMetaPressed;

    if (combo) {
      final vk = vkForCombo(event.logicalKey);
      if (vk != null) {
        _sendKey('vk', vk, pressed);
        return KeyEventResult.handled;
      }
      return KeyEventResult.ignored;
    }

    if (pressed) {
      final ch = event.character;
      if (ch != null && isPrintableText(ch)) {
        _sendKey('char', ch.runes.first, true);
        return KeyEventResult.handled;
      }
    }
    return KeyEventResult.ignored;
  }

  void _sendKey(String keyType, int key, bool pressed) {
    rust_api.sendKeyEvent(keyType: keyType, key: key, pressed: pressed)
        .catchError((_) {});
  }

  /// Mobile soft keyboard: forward appended characters as char events and
  /// deletions as Backspace presses.
  void _onSoftInputChanged(String text) {
    if (text.length > _lastSoftInput.length) {
      final appended = text.substring(_lastSoftInput.length);
      for (final rune in appended.runes) {
        _sendKey('char', rune, true);
      }
    } else if (text.length < _lastSoftInput.length) {
      for (var i = text.length; i < _lastSoftInput.length; i++) {
        _sendKey('special', kKeyBackspace, true);
      }
    }
    _lastSoftInput = text;
  }

  void _toggleInputBar() {
    setState(() {
      _showInputBar = !_showInputBar;
      if (_showInputBar) {
        _keyboardFocus.unfocus();
        _lastSoftInput = '';
        _softInputController.clear();
      } else {
        _requestKeyboardFocus();
      }
    });
  }

  void _startStatusPolling() {
    _statusTimer = Timer.periodic(
      const Duration(seconds: 3),
      (_) => _pollStatus(),
    );
  }

  Future<void> _pollStatus() async {
    try {
      final jsonStr = await rust_api.getConnectionQuality();
      if (mounted && jsonStr.isNotEmpty) {
        setState(() => _connectionStatus = jsonStr);
      }
    } catch (_) {}
  }

  /// Frames are pushed from Rust the moment they are decoded — no poll
  /// interval latency. The Rust-side stream survives reconnects on its own.
  void _startFrameStream() {
    _frameSub?.cancel();
    _frameSub = rust_api.watchVideoFrames().listen(
          (msg) => _onFrame(msg.width, msg.height, msg.bgra),
          onError: (_) {},
        );
  }

  Future<void> _onFrame(int width, int height, Uint8List bgra) async {
    final image = await _bgraToImage(bgra, width, height);
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
    final l10n = AppLocalizations.of(context)!;
    return Scaffold(
      backgroundColor: Colors.black,
      // Hardware keyboard: focus is acquired after connecting and re-acquired
      // whenever the user taps the remote view (toolbar buttons steal it).
      body: Focus(
        focusNode: _keyboardFocus,
        onKeyEvent: _handleKeyEvent,
        child: Stack(
          children: [
            Listener(
              onPointerDown: (_) {
                if (!_showInputBar) _keyboardFocus.requestFocus();
              },
              child: Center(child: _buildContent()),
            ),
            Positioned(
              top: 8,
              right: 8,
              child: _buildToolbar(),
            ),
            if (_showInputBar) _buildInputBar(l10n),
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
                          l10n.remoteConnectingTo(widget.peerId),
                          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Colors.white),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  /// Mobile input bar: pops the soft keyboard and forwards edits to the
  /// remote as char/backspace events.
  Widget _buildInputBar(AppLocalizations l10n) {
    return Positioned(
      bottom: 0,
      left: 0,
      right: 0,
      child: Container(
        color: Colors.black87,
        child: SafeArea(
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _softInputController,
                  autofocus: true,
                  onChanged: _onSoftInputChanged,
                  onSubmitted: (_) {
                    _sendKey('special', kKeyEnter, true);
                    _softInputController.clear();
                    _lastSoftInput = '';
                  },
                  style: const TextStyle(color: Colors.white),
                  decoration: InputDecoration(
                    hintText: l10n.remoteInputHint,
                    hintStyle: const TextStyle(color: Colors.white38),
                    border: const OutlineInputBorder(),
                    isDense: true,
                  ),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.keyboard_hide, color: Colors.white),
                tooltip: l10n.remoteKeyboard,
                onPressed: _toggleInputBar,
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildContent() {
    final l10n = AppLocalizations.of(context)!;
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
            child: Text(l10n.remoteGoBack),
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
          l10n.remoteConnectedTo(widget.peerId),
          style: const TextStyle(color: Colors.white),
        ),
        const SizedBox(height: 4),
        Text(
          _frameCount > 0
              ? l10n.remoteWaitingForVideo(_frameCount)
              : l10n.remoteVideoStarting,
          style: const TextStyle(color: Colors.white54),
        ),
        if (_connectionStatus.isNotEmpty) ...[
          const SizedBox(height: 12),
          Container(
            constraints: const BoxConstraints(maxWidth: 300),
            child: Text(
              _connectionStatus,
              style: const TextStyle(color: Colors.white38, fontSize: 10, fontFamily: 'monospace'),
              textAlign: TextAlign.center,
            ),
          ),
        ],
      ],
    );
  }

  Widget _buildToolbar() {
    final l10n = AppLocalizations.of(context)!;
    return Card(
      color: Colors.black87,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: Icon(_isFullscreen ? Icons.fullscreen_exit : Icons.fullscreen, color: Colors.white),
            onPressed: _toggleFullscreen,
            tooltip: _isFullscreen ? l10n.remoteExitFullscreen : l10n.remoteFullscreen,
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
            icon: Icon(
              _showInputBar ? Icons.keyboard_hide : Icons.keyboard,
              color: Colors.white,
            ),
            onPressed: _toggleInputBar,
            tooltip: l10n.remoteKeyboard,
          ),
          IconButton(
            icon: const Icon(Icons.chat, color: Colors.white),
            onPressed: () => context.go('/chat'),
            tooltip: l10n.remoteChat,
          ),
          IconButton(
            icon: const Icon(Icons.folder, color: Colors.white),
            onPressed: () => context.go('/files'),
            tooltip: l10n.remoteFileTransfer,
          ),
          IconButton(
            icon: Icon(
              _recording ? Icons.stop_circle : Icons.fiber_manual_record,
              color: _recording ? Colors.red : Colors.white,
            ),
            onPressed: _toggleRecording,
            tooltip: _recording ? l10n.remoteStopRecording : l10n.remoteStartRecording,
          ),
          IconButton(
            icon: const Icon(Icons.call_end, color: Colors.red),
            onPressed: () => context.go('/'),
            tooltip: l10n.remoteDisconnect,
          ),
        ],
      ),
    );
  }

  Future<void> _toggleRecording() async {
    final l10n = AppLocalizations.of(context)!;
    try {
      if (_recording) {
        final summary = await rust_api.stopSessionRecording();
        if (mounted) setState(() => _recording = false);
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text(l10n.remoteRecordingDone(summary))),
          );
        }
      } else {
        final dir = await getApplicationDocumentsDirectory();
        final ts = DateTime.now().toIso8601String().replaceAll(':', '-');
        final path = '${dir.path}${Platform.pathSeparator}alldesk_$ts.aldrec';
        await rust_api.startSessionRecording(path: path);
        if (mounted) setState(() => _recording = true);
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(l10n.remoteRecordingFailed(e.toString()))),
        );
      }
    }
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
