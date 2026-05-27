import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../providers/discovery_provider.dart';
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

  @override
  void initState() {
    super.initState();
    Future.microtask(() => ref.read(discoveryProvider.notifier).startPolling());
  }

  @override
  void dispose() {
    ref.read(discoveryProvider.notifier).stopPolling();
    _peerIdController.dispose();
    super.dispose();
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
    final peers = ref.watch(discoveryProvider);
    final myPeerId = ref.watch(peerIdProvider);
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        title: const Text('AllDesk'),
        actions: [
          if (Platform.isAndroid)
            IconButton(
              icon: Icon(_isSharing ? Icons.screen_share : Icons.stop_screen_share),
              tooltip: _isSharing ? 'Stop Sharing' : 'Start Screen Sharing',
              onPressed: _toggleScreenSharing,
            ),
          IconButton(
            icon: const Icon(Icons.settings),
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
                              Text('Your Peer ID', style: theme.textTheme.labelSmall),
                              myPeerId.when(
                                data: (id) => SelectableText(id, style: theme.textTheme.bodyMedium),
                                loading: () => const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                                error: (_, __) => const Text('Error loading ID'),
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
                              'Screen sharing active — other devices can connect',
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
                        Text('Connect to Device', style: theme.textTheme.titleMedium),
                        const SizedBox(height: 12),
                        Row(
                          children: [
                            Expanded(
                              child: TextField(
                                controller: _peerIdController,
                                decoration: const InputDecoration(
                                  hintText: 'Enter Peer ID or IP address',
                                  border: OutlineInputBorder(),
                                  prefixIcon: Icon(Icons.link),
                                ),
                              ),
                            ),
                            const SizedBox(width: 12),
                            FilledButton.icon(
                              onPressed: _connect,
                              icon: const Icon(Icons.arrow_forward),
                              label: const Text('Connect'),
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 24),

                // LAN device list
                Row(
                  children: [
                    Text('Devices on LAN', style: theme.textTheme.titleMedium),
                    const Spacer(),
                    const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                    const SizedBox(width: 8),
                    Text('Scanning...', style: theme.textTheme.bodySmall),
                    const SizedBox(width: 12),
                    IconButton(
                      icon: const Icon(Icons.bug_report, size: 20),
                      tooltip: 'Diagnostics',
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
                                'Scanning for devices...',
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
                                  onPressed: () => context.go('/remote?addr=${Uri.encodeComponent(peer.address)}'),
                                  child: const Text('Connect'),
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
                        const Text('Diagnostics', style: TextStyle(color: Colors.white, fontWeight: FontWeight.bold)),
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
      context.go('/remote?addr=${Uri.encodeComponent(target)}');
    }
  }
}
