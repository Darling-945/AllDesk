import 'package:go_router/go_router.dart';
import '../features/chat/chat_page.dart';
import '../features/file_transfer/file_transfer_page.dart';
import '../features/home/home_page.dart';
import '../features/permissions/permission_guide_page.dart';
import '../features/recordings/recording_player_page.dart';
import '../features/recordings/recordings_page.dart';
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
        path: '/chat',
        builder: (context, state) => const ChatPage(),
      ),
      GoRoute(
        path: '/recordings',
        builder: (context, state) => const RecordingsPage(),
      ),
      GoRoute(
        path: '/recordings/play',
        builder: (context, state) {
          final path = state.uri.queryParameters['path'] ?? '';
          return RecordingPlayerPage(path: path);
        },
      ),
      GoRoute(
        path: '/settings',
        builder: (context, state) => const SettingsPage(),
      ),
      GoRoute(
        path: '/permissions',
        builder: (context, state) => const PermissionGuidePage(),
      ),
    ],
  );
}
