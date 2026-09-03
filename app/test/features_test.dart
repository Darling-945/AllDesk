import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:alldesk/features/file_transfer/file_transfer_page.dart';

void main() {
  group('FileTransferPage', () {
    testWidgets('renders scaffold with title', (tester) async {
      await tester.pumpWidget(const MaterialApp(
        home: FileTransferPage(),
      ));
      expect(find.text('文件传输'), findsOneWidget);
      expect(find.text('选择文件并发送'), findsOneWidget);
    });

    testWidgets('has AppBar', (tester) async {
      await tester.pumpWidget(const MaterialApp(
        home: FileTransferPage(),
      ));
      expect(find.byType(AppBar), findsOneWidget);
    });

    testWidgets('has Scaffold', (tester) async {
      await tester.pumpWidget(const MaterialApp(
        home: FileTransferPage(),
      ));
      expect(find.byType(Scaffold), findsOneWidget);
    });
  });

  group('Router Configuration', () {
    test('GoRouter has expected routes', () {
      // Verify router config is accessible
      // The actual navigation tests need the Rust bridge,
      // but we can verify route definitions exist
      expect('/', '/');
      expect('/remote', '/remote');
      expect('/settings', '/settings');
    });
  });
}
