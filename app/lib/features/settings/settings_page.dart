import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../providers/settings_provider.dart';
import '../../src/rust/api.dart' as rust_api;

class SettingsPage extends ConsumerStatefulWidget {
  const SettingsPage({super.key});

  @override
  ConsumerState<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends ConsumerState<SettingsPage> {
  String _version = '';
  String _peerId = '';
  final _nameController = TextEditingController();
  final _portController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _loadInfo();
  }

  @override
  void dispose() {
    _nameController.dispose();
    _portController.dispose();
    super.dispose();
  }

  Future<void> _loadInfo() async {
    try {
      final ver = await rust_api.getVersion();
      final pid = await rust_api.getPeerId();
      if (mounted) {
        setState(() {
          _version = ver;
          _peerId = pid;
        });
      }
    } catch (_) {}

    final settings = ref.read(settingsProvider);
    _nameController.text = settings.displayName;
    _portController.text = settings.serverPort.toString();
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(
        leading: IconButton(
          icon: const Icon(Icons.arrow_back),
          onPressed: () => context.go('/'),
        ),
        title: const Text('Settings'),
      ),
      body: ListView(
        children: [
          // --- Device Info ---
          _sectionHeader('Device'),
          ListTile(
            leading: const Icon(Icons.fingerprint),
            title: const Text('Peer ID'),
            subtitle: SelectableText(
              _peerId.isEmpty ? 'Loading...' : _peerId,
              style: theme.textTheme.bodySmall,
            ),
          ),
          ListTile(
            leading: const Icon(Icons.person),
            title: const Text('Display Name'),
            subtitle: Text(
              settings.displayName.isEmpty ? 'Not set (uses hostname)' : settings.displayName,
            ),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => _editDisplayName(settings),
          ),

          const Divider(),

          // --- Display ---
          _sectionHeader('Display'),
          ListTile(
            leading: const Icon(Icons.dark_mode),
            title: const Text('Theme'),
            subtitle: Text(_themeLabel(settings.themeMode)),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => _pickTheme(settings),
          ),
          SwitchListTile(
            secondary: const Icon(Icons.mouse),
            title: const Text('Show Remote Cursor'),
            subtitle: const Text('Render remote cursor on screen'),
            value: settings.showRemoteCursor,
            onChanged: (v) => ref.read(settingsProvider.notifier).setShowRemoteCursor(v),
          ),

          const Divider(),

          // --- Streaming ---
          _sectionHeader('Streaming'),
          ListTile(
            leading: const Icon(Icons.high_quality),
            title: const Text('Video Quality'),
            subtitle: Text('${settings.bitrateKbps} kbps'),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => _pickBitrate(settings),
          ),
          ListTile(
            leading: const Icon(Icons.speed),
            title: const Text('Frame Rate'),
            subtitle: Text('${settings.fps} FPS'),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => _pickFps(settings),
          ),

          const Divider(),

          // --- Network ---
          _sectionHeader('Network'),
          ListTile(
            leading: const Icon(Icons.lan),
            title: const Text('QUIC Server Port'),
            subtitle: Text('${settings.serverPort}'),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => _editPort(settings),
          ),
          SwitchListTile(
            secondary: const Icon(Icons.auto_mode),
            title: const Text('Auto-Connect to Last Device'),
            subtitle: const Text('Connect on app launch'),
            value: settings.autoConnect,
            onChanged: (v) => ref.read(settingsProvider.notifier).setAutoConnect(v),
          ),

          const Divider(),

          // --- About ---
          _sectionHeader('About'),
          ListTile(
            leading: const Icon(Icons.info_outline),
            title: const Text('Version'),
            subtitle: Text(_version.isEmpty ? 'Loading...' : _version),
          ),
          ListTile(
            leading: const Icon(Icons.code),
            title: const Text('Open Source'),
            subtitle: const Text('Built with Rust + Flutter'),
          ),
          const SizedBox(height: 24),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 24),
            child: OutlinedButton.icon(
              icon: const Icon(Icons.arrow_back),
              label: const Text('Back to Home'),
              onPressed: () => context.go('/'),
            ),
          ),
          const SizedBox(height: 32),
        ],
      ),
    );
  }

  Widget _sectionHeader(String title) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 20, 16, 4),
      child: Text(
        title,
        style: TextStyle(
          color: Theme.of(context).colorScheme.primary,
          fontWeight: FontWeight.bold,
          fontSize: 13,
        ),
      ),
    );
  }

  String _themeLabel(int mode) {
    switch (mode) {
      case 1: return 'Light';
      case 2: return 'Dark';
      default: return 'System';
    }
  }

  void _pickTheme(AppSettings settings) {
    showDialog(
      context: context,
      builder: (ctx) => SimpleDialog(
        title: const Text('Theme'),
        children: [
          _themeOption(ctx, 'System', 0, settings.themeMode),
          _themeOption(ctx, 'Light', 1, settings.themeMode),
          _themeOption(ctx, 'Dark', 2, settings.themeMode),
        ],
      ),
    );
  }

  SimpleDialogOption _themeOption(BuildContext ctx, String label, int value, int current) {
    return SimpleDialogOption(
      onPressed: () {
        ref.read(settingsProvider.notifier).setThemeMode(value);
        Navigator.pop(ctx);
      },
      child: Row(
        children: [
          Icon(current == value ? Icons.radio_button_checked : Icons.radio_button_unchecked,
              size: 20),
          const SizedBox(width: 12),
          Text(label),
        ],
      ),
    );
  }

  void _pickBitrate(AppSettings settings) {
    const options = [2000, 4000, 6000, 8000, 10000, 15000];
    showDialog(
      context: context,
      builder: (ctx) => SimpleDialog(
        title: const Text('Video Quality'),
        children: options.map((v) => SimpleDialogOption(
          onPressed: () {
            ref.read(settingsProvider.notifier).setBitrate(v);
            Navigator.pop(ctx);
          },
          child: Row(
            children: [
              Icon(settings.bitrateKbps == v ? Icons.radio_button_checked : Icons.radio_button_unchecked,
                  size: 20),
              const SizedBox(width: 12),
              Text(v <= 2000 ? '$v kbps (Low)' :
                   v <= 4000 ? '$v kbps (Medium)' :
                   v <= 8000 ? '$v kbps (High)' :
                   '$v kbps (Ultra)'),
            ],
          ),
        )).toList(),
      ),
    );
  }

  void _pickFps(AppSettings settings) {
    const options = [15, 24, 30, 60];
    showDialog(
      context: context,
      builder: (ctx) => SimpleDialog(
        title: const Text('Frame Rate'),
        children: options.map((v) => SimpleDialogOption(
          onPressed: () {
            ref.read(settingsProvider.notifier).setFps(v);
            Navigator.pop(ctx);
          },
          child: Row(
            children: [
              Icon(settings.fps == v ? Icons.radio_button_checked : Icons.radio_button_unchecked,
                  size: 20),
              const SizedBox(width: 12),
              Text('$v FPS${v == 60 ? ' (may increase CPU)' : ''}'),
            ],
          ),
        )).toList(),
      ),
    );
  }

  void _editDisplayName(AppSettings settings) {
    _nameController.text = settings.displayName;
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Display Name'),
        content: TextField(
          controller: _nameController,
          autofocus: true,
          decoration: const InputDecoration(
            hintText: 'Enter a name others will see',
            border: OutlineInputBorder(),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          FilledButton(
            onPressed: () {
              ref.read(settingsProvider.notifier).setDisplayName(_nameController.text.trim());
              Navigator.pop(ctx);
            },
            child: const Text('Save'),
          ),
        ],
      ),
    );
  }

  void _editPort(AppSettings settings) {
    _portController.text = settings.serverPort.toString();
    showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('QUIC Server Port'),
        content: TextField(
          controller: _portController,
          autofocus: true,
          keyboardType: TextInputType.number,
          decoration: const InputDecoration(
            hintText: 'Port number (1024-65535)',
            border: OutlineInputBorder(),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          FilledButton(
            onPressed: () {
              final port = int.tryParse(_portController.text.trim());
              if (port != null && port >= 1024 && port <= 65535) {
                ref.read(settingsProvider.notifier).setServerPort(port);
                Navigator.pop(ctx);
              }
            },
            child: const Text('Save'),
          ),
        ],
      ),
    );
  }
}
