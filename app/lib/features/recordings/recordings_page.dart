import 'dart:io';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:path_provider/path_provider.dart';
import '../../l10n/app_localizations.dart';
import '../../services/error_ui.dart';

/// Lists session recordings (.aldrec/.aldrc2) saved in the app documents
/// directory, newest first. Tap to play back, swipe/delete to remove.
class RecordingsPage extends StatefulWidget {
  const RecordingsPage({super.key});

  @override
  State<RecordingsPage> createState() => _RecordingsPageState();
}

class _RecordingsPageState extends State<RecordingsPage> {
  List<File> _files = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  Future<void> _refresh() async {
    setState(() => _loading = true);
    try {
      final dir = await getApplicationDocumentsDirectory();
      final recordings = dir
          .listSync()
          .whereType<File>()
          .where((f) {
            final name = f.uri.pathSegments.last;
            return name.endsWith('.aldrec') || name.endsWith('.aldrc2');
          })
          .toList()
        ..sort((a, b) => b.lastModifiedSync().compareTo(a.lastModifiedSync()));
      if (mounted) setState(() => _files = recordings);
    } catch (e) {
      if (mounted) {
        showErrorSnackBar(context, e,
            prefix: AppLocalizations.of(context)!.recListFailed);
      }
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _delete(File file) async {
    final l10n = AppLocalizations.of(context)!;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l10n.recDeleteTitle),
        content: Text(file.uri.pathSegments.last),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(l10n.commonCancel),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(l10n.commonDelete),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      try {
        await file.delete();
      } catch (e) {
        if (mounted) showErrorSnackBar(context, e, prefix: l10n.recDeleteFailed);
      }
      _refresh();
    }
  }

  String _formatSize(int bytes) {
    if (bytes >= 1024 * 1024 * 1024) {
      return '${(bytes / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
    }
    if (bytes >= 1024 * 1024) {
      return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
    }
    if (bytes >= 1024) return '${(bytes / 1024).toStringAsFixed(0)} KB';
    return '$bytes B';
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context)!;
    final theme = Theme.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l10n.recTitle)),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : _files.isEmpty
              ? Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.videocam_off, size: 48, color: theme.colorScheme.outline),
                      const SizedBox(height: 12),
                      Text(l10n.recEmpty, style: theme.textTheme.bodyLarge),
                      const SizedBox(height: 4),
                      Text(
                        l10n.recEmptyHint,
                        style: theme.textTheme.bodySmall,
                      ),
                    ],
                  ),
                )
              : ListView.builder(
                  itemCount: _files.length,
                  itemBuilder: (context, index) {
                    final file = _files[index];
                    final name = file.uri.pathSegments.last;
                    final stat = file.statSync();
                    return Card(
                      child: ListTile(
                        leading: const Icon(Icons.movie),
                        title: Text(name, overflow: TextOverflow.ellipsis),
                        subtitle: Text(
                          '${_formatSize(stat.size)} · '
                          '${stat.modified.toLocal().toString().split('.').first}',
                        ),
                        trailing: IconButton(
                          icon: const Icon(Icons.delete_outline),
                          tooltip: l10n.commonDelete,
                          onPressed: () => _delete(file),
                        ),
                        onTap: () => context.go(
                          '/recordings/play?path=${Uri.encodeComponent(file.path)}',
                        ),
                      ),
                    );
                  },
                ),
    );
  }
}
