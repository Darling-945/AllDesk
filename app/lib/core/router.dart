import 'package:go_router/go_router.dart';
import '../features/file_transfer/file_transfer_page.dart';
import '../features/home/home_page.dart';
import '../features/remote/remote_page.dart';
import '../features/settings/settings_page.dart';

class AppRouter {
  static final router = GoRouter(
    routes: [
      GoRoute(
        path: '/',
        builder: (context, state) => const HomePage(),
      ),
      GoRoute(
        path: '/remote',
        builder: (context, state) {
          final addr = state.uri.queryParameters['addr'] ?? '';
          return RemotePage(peerId: addr);
        },
      ),
      GoRoute(
        path: '/files',
        builder: (context, state) => const FileTransferPage(),
      ),
      GoRoute(
        path: '/settings',
        builder: (context, state) => const SettingsPage(),
      ),
    ],
  );
}
