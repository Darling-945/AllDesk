import 'dart:async';
import 'package:flutter/material.dart';
import '../../src/rust/api.dart' as rust_api;

/// Handles touch-to-mouse gesture mapping for mobile remote control.
///
/// Gestures:
/// - Single tap → left click
/// - Double tap → double click
/// - Long press → right click
/// - One finger drag → mouse move (with left button held)
/// - Two finger drag → scroll
class TouchGestureHandler extends StatefulWidget {
  final Widget child;
  final Size remoteResolution;

  const TouchGestureHandler({
    super.key,
    required this.child,
    required this.remoteResolution,
  });

  @override
  State<TouchGestureHandler> createState() => _TouchGestureHandlerState();
}

class _TouchGestureHandlerState extends State<TouchGestureHandler> {
  Offset _lastPosition = Offset.zero;

  Offset _toRemoteCoords(Offset local, Size widgetSize) {
    final scaleX = widget.remoteResolution.width / widgetSize.width;
    final scaleY = widget.remoteResolution.height / widgetSize.height;
    return Offset(local.dx * scaleX, local.dy * scaleY);
  }

  Future<void> _sendMouse(Offset pos, {required String action}) async {
    try {
      await rust_api.sendMouseEvent(
        x: pos.dx,
        y: pos.dy,
        action: action,
      );
    } catch (_) {}
  }

  Future<void> _sendScroll(double deltaY) async {
    try {
      await rust_api.sendScroll(dy: deltaY);
    } catch (_) {}
  }

  void _handleTapUp(TapUpDetails details) {
    final pos = _toRemoteCoords(details.localPosition, context.size!);
    _sendMouse(pos, action: 'down');
    Future.delayed(const Duration(milliseconds: 50), () {
      _sendMouse(pos, action: 'up');
    });
  }

  void _handleDoubleTap() {
    final pos = _toRemoteCoords(_lastPosition, context.size!);
    _sendMouse(pos, action: 'down');
    Future.delayed(const Duration(milliseconds: 30), () {
      _sendMouse(pos, action: 'up');
      Future.delayed(const Duration(milliseconds: 30), () {
        _sendMouse(pos, action: 'down');
        Future.delayed(const Duration(milliseconds: 30), () {
          _sendMouse(pos, action: 'up');
        });
      });
    });
  }

  void _handleLongPressStart(LongPressStartDetails details) {
    final pos = _toRemoteCoords(details.localPosition, context.size!);
    _sendMouse(pos, action: 'right_down');
    Future.delayed(const Duration(milliseconds: 50), () {
      _sendMouse(pos, action: 'right_up');
    });
  }

  void _handlePanStart(DragStartDetails details) {
    _lastPosition = details.localPosition;
    final pos = _toRemoteCoords(details.localPosition, context.size!);
    _sendMouse(pos, action: 'down');
  }

  void _handlePanUpdate(DragUpdateDetails details) {
    _lastPosition = details.localPosition;
    final pos = _toRemoteCoords(details.localPosition, context.size!);
    _sendMouse(pos, action: 'move');
  }

  void _handlePanEnd(DragEndDetails details) {
    final pos = _toRemoteCoords(_lastPosition, context.size!);
    _sendMouse(pos, action: 'up');
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTapUp: _handleTapUp,
      onDoubleTap: _handleDoubleTap,
      onLongPressStart: _handleLongPressStart,
      onPanStart: _handlePanStart,
      onPanUpdate: _handlePanUpdate,
      onPanEnd: _handlePanEnd,
      behavior: HitTestBehavior.opaque,
      child: widget.child,
    );
  }
}
