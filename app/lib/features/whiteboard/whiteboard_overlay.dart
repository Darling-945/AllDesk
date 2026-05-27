import 'package:flutter/material.dart';

class WhiteboardOverlay extends StatelessWidget {
  const WhiteboardOverlay({super.key});

  @override
  Widget build(BuildContext context) {
    return CustomPaint(
      painter: _WhiteboardPainter(strokes: []),
      size: Size.infinite,
    );
  }
}

class _WhiteboardPainter extends CustomPainter {
  final List<dynamic> strokes;

  _WhiteboardPainter({required this.strokes});

  @override
  void paint(Canvas canvas, Size size) {
    // TODO: Render whiteboard strokes
  }

  @override
  bool shouldRepaint(covariant _WhiteboardPainter oldDelegate) => true;
}
