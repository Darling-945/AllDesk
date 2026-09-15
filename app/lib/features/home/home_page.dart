import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../../l10n/app_localizations.dart';
import '../../providers/discovery_provider.dart';
import '../../services/connection_store.dart';
import '../../services/platform_service.dart';
import '../../src/rust/api.dart' as rust_api;

class HomePage extends ConsumerStatefulWidget {
  const HomePage({super.key});

  @override
  ConsumerState<HomePage> createState() => _HomePageState();
}

class _HomePageState extends ConsumerState<HomePage> {
  final _peerIdController = TextEditingController();
  String _diagnostics = '';
  bool _showDiag = false;
  bool _isSharing = false;
  ConnectionStore? _store;
  List<ConnectionEntry> _history = const [];

  @override
  void initState() {
    super.initState();
    Future.microtask(() => ref.read(discoveryProvider.notifier).startPolling());
    _loadHistory();
    if (Platform.isAndroid) _maybeShowPermissionGuide();
  }

  /// One-time first-launch permission checklist (Android only).
  Future<void> _maybeShowPermissionGuide() async {
    final prefs = await SharedPreferences.getInstance();
    if (prefs.getBool('perm_guide_shown') != true) {
      await prefs.setBool('perm_guide_shown', true);
      if (mounted) context.push('/permissions');
    }
  }

  @override
  void dispose() {
    ref.read(discoveryProvider.notifier).stopPolling();
    _peerIdController.dispose();
    super.dispose();
  }

  Future<void> _loadHistory() async {
    final store = await ConnectionStore.load();
    if (mounted) {
      setState(() {
        _store = store;
        _history = store.entries;
      });
    }
  }

  Future<void> _recordConnection(String address, {String? name}) async {
    final store = _store;
    if (store == null) return;
    await store.record(address, name: name);
    _loadHistory();
  }

  Future<void> _runDiag() async {
    try {
      final result = await rust_api.runDiagnostics();
      if (mounted) setState(() { _diagnostics = result; _showDiag = true; });
    } catch (e) {
      if (mounted) setState(() { _diagnostics = 'Error: $e'; _showDiag = true; });
    }
  }

  Future<void> _toggleScreenSharing() async {
    if (_isSharing) {
      await PlatformService.stopScreenCapture();
      if (mounted) setState(() => _isSharing = false);
    } else {
      final granted = await PlatformService.requestScreenCapture();
      if (granted && mounted) {
        setState(() => _isSharing = true);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context)!;
    final peers = ref.watch(discoveryProvider);
    final myPeerId = ref.watch(peerIdProvider);
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('AllDesk'),
        actions: [
          IconButton(
            icon: const Icon(Icons.video_library),
            tooltip: l10n.homeRecordings,
            onPressed: () => context.go('/recordings'),
          ),
          if (Platform.isAndroid) ...[
            IconButton(
              icon: const Icon(Icons.privacy_tip_outlined),
              tooltip: l10n.permTitle,
              onPressed: () => context.push('/permissions'),
            ),
            IconButton(
              icon: Icon(_isSharing ? Icons.screen_share : Icons.stop_screen_share),
              tooltip: _isSharing ? l10n.homeStopSharing : l10n.homeStartSharing,
              onPressed: _toggleScreenSharing,
            ),
          ],
          IconButton(
            icon: const Icon(Icons.settings),
            tooltip: l10n.commonSettings,
            onPressed: () => context.go('/settings'),
          ),
        ],
      ),
      body: Stack(
        children: [
          Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                // My Peer ID
                Card(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Row(
                      children: [
                        Icon(Icons.badge, color: theme.colorScheme.primary),
                        const SizedBox(width: 12),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(l10n.homePeerId, style: theme.textTheme.labelSmall),
                              myPeerId.when(
                                data: (id) => SelectableText(id, style: theme.textTheme.bodyMedium),
                                loading: () => const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                                error: (_, __) => Text(l10n.homePeerIdError),
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                if (Platform.isAndroid && _isSharing)
                  Card(
                    color: theme.colorScheme.primaryContainer,
                    child: Padding(
                      padding: const EdgeInsets.all(12),
                      child: Row(
                        children: [
                          Icon(Icons.screen_share, size: 18, color: theme.colorScheme.primary),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              l10n.homeScreenSharingActive,
                              style: theme.textTheme.bodySmall?.copyWith(
                                color: theme.colorScheme.onPrimaryContainer,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                const SizedBox(height: 16),

                // Manual connect
                Card(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(l10n.homeConnectTitle, style: theme.textTheme.titleMedium),
                        const SizedBox(height: 12),
                        Row(
                          children: [
                            Expanded(
                              child: TextField(
                                controller: _peerIdController,
                                decoration: InputDecoration(
                                  hintText: l10n.homeConnectHint,
                                  border: const OutlineInputBorder(),
                                  prefixIcon: const Icon(Icons.link),
                                ),
                              ),
                            ),
                            const SizedBox(width: 12),
                            FilledButton.icon(
                              onPressed: _connect,
                              icon: const Icon(Icons.arrow_forward),
                              label: Text(l10n.commonConnect),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 24),

                // Recent connections & favorites
                if (_history.isNotEmpty) ...[
                  Row(
                    children: [
                      Text(l10n.homeRecentTitle, style: theme.textTheme.titleMedium),
                      const Spacer(),
                      Text(
                        l10n.homeFavoritesCount(
                            _history.where((e) => e.favorite).length),
                        style: theme.textTheme.bodySmall,
                      ),
                    ],
                  ),
                  const SizedBox(height: 4),
                  ..._history.take(5).map((entry) => Card(
                        child: ListTile(
                          dense: true,
                          leading: Icon(
                            entry.favorite ? Icons.star : Icons.history,
                            color: entry.favorite
                                ? Colors.amber
                                : theme.colorScheme.outline,
                          ),
                          title: Text(entry.name ?? entry.address),
                          subtitle: Text(entry.address),
                          trailing: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              IconButton(
                                icon: Icon(
                                  entry.favorite
                                      ? Icons.star
                                      : Icons.star_border,
                                  color: entry.favorite
                                      ? Colors.amber
                                      : theme.colorScheme.outline,
                                ),
                                tooltip: entry.favorite
                                    ? l10n.homeUnfavorite
                                    : l10n.homeFavorite,
                                onPressed: () async {
                                  await _store?.toggleFavorite(entry.address);
                                  _loadHistory();
                                },
                              ),
                              FilledButton.tonal(
                                onPressed: () => _connectTo(entry.address,
                                    name: entry.name),
                                child: Text(l10n.commonConnect),
                              ),
                            ],
                          ),
                          onTap: () => _connectTo(entry.address, name: entry.name),
                        ),
                      )),
                  const SizedBox(height: 24),
                ],

                // LAN device list
                Row(
                  children: [
                    Text(l10n.homeDevicesTitle, style: theme.textTheme.titleMedium),
                    const Spacer(),
                    const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                    const SizedBox(width: 8),
                    Text(l10n.homeScanning, style: theme.textTheme.bodySmall),
                    const SizedBox(width: 12),
                    IconButton(
                      icon: const Icon(Icons.bug_report, size: 20),
                      tooltip: l10n.homeDiagnostics,
                      onPressed: _runDiag,
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                Expanded(
                  child: peers.isEmpty
                      ? Center(
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Icon(Icons.wifi_find, size: 48, color: theme.colorScheme.outline),
                              const SizedBox(height: 12),
                              Text(
                                l10n.homeScanningForDevices,
                                style: theme.textTheme.bodyLarge,
                              ),
                            ],
                          ),
                        )
                      : ListView.builder(
                          itemCount: peers.length,
                          itemBuilder: (context, index) {
                            final peer = peers[index];
                            return Card(
                              child: ListTile(
                                leading: CircleAvatar(
                                  child: Text(peer.peerName[0].toUpperCase()),
                                ),
                                title: Text(peer.peerName),
                                subtitle: Text('${peer.peerId}\n${peer.address}'),
                                isThreeLine: true,
                                trailing: FilledButton(
                                  onPressed: () => _connectTo(peer.address,
                                      name: peer.peerName),
                                  child: Text(l10n.commonConnect),
                                ),
                              ),
                            );
                          },
                        ),
                ),
              ],
            ),
          ),

          // Diagnostics panel
          if (_showDiag)
            Positioned(
              bottom: 0,
              left: 0,
              right: 0,
              child: Container(
                color: Colors.black87,
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Text(l10n.homeDiagnostics,
                            style: const TextStyle(
                                color: Colors.white,
                                fontWeight: FontWeight.bold)),
                        const Spacer(),
                        IconButton(
                          icon: const Icon(Icons.close, color: Colors.white, size: 20),
                          onPressed: () => setState(() => _showDiag = false),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    SelectableText(
                      _diagnostics,
                      style: const TextStyle(color: Colors.white70, fontFamily: 'monospace', fontSize: 12),
                    ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }

  void _connect() {
    final target = _peerIdController.text.trim();
    if (target.isNotEmpty) {
      _connectTo(target);
    }
  }

  void _connectTo(String address, {String? name}) {
    _recordConnection(address, name: name);
    context.go('/remote?addr=${Uri.encodeComponent(address)}');
  }
}
