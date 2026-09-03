import 'dart:async';
import 'dart:convert';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';

import '../../src/rust/api.dart' as rust_api;

/// Sends files to the connected remote peer (viewer → host).
///
/// The remote side stores received files in `Downloads/AllDesk/`.
/// Progress is polled from the Rust layer every 500 ms.
class FileTransferPage extends StatefulWidget {
  const FileTransferPage({super.key});

  @override
  State<FileTransferPage> createState() => _FileTransferPageState();
}

class _FileTransferPageState extends State<FileTransferPage> {
  Timer? _statusTimer;
  Map<String, dynamic> _status = {};
  bool _picking = false;

  @override
  void initState() {
    super.initState();
    _statusTimer = Timer.periodic(
      const Duration(milliseconds: 500),
      (_) => _pollStatus(),
    );
  }

  @override
  void dispose() {
    _statusTimer?.cancel();
    super.dispose();
  }

  Future<void> _pollStatus() async {
    try {
      final jsonStr = await rust_api.getFileTransferStatus();
      if (mounted && jsonStr.isNotEmpty) {
        setState(() => _status = jsonDecode(jsonStr) as Map<String, dynamic>);
      }
    } catch (_) {
      // Not connected yet — keep the last known status.
    }
  }

  Future<void> _pickAndSend() async {
    if (_picking) return;
    setState(() => _picking = true);
    try {
      final result = await FilePicker.platform.pickFiles();
      final path = result?.files.single.path;
      if (path == null) return;
      final message = await rust_api.sendFileToPeer(path: path);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(message)),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('发送失败: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _picking = false);
    }
  }

  double? get _progress {
    final total = (_status['total'] as num?)?.toDouble() ?? 0;
    final transferred = (_status['transferred'] as num?)?.toDouble() ?? 0;
    if (total <= 0) return null;
    return (transferred / total).clamp(0.0, 1.0);
  }

  @override
  Widget build(BuildContext context) {
    final active = _status['active'] == true;
    final direction = _status['direction'] as String? ?? 'none';
    final filename = _status['filename'] as String? ?? '';
    final error = _status['error'] as String?;
    final progress = _progress;
    final received = direction == 'receiving';

    return Scaffold(
      appBar: AppBar(title: const Text('文件传输')),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 480),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  received ? '正在接收文件' : '发送文件到远程设备',
                  style: Theme.of(context).textTheme.titleMedium,
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 8),
                Text(
                  '远程设备收到的文件保存在 Downloads/AllDesk 目录',
                  style: Theme.of(context).textTheme.bodySmall,
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 24),
                if (active || (error != null && error.isNotEmpty)) ...[
                  Card(
                    child: Padding(
                      padding: const EdgeInsets.all(16),
                      child: Column(
                        children: [
                          Text(
                            filename,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          const SizedBox(height: 8),
                          if (progress != null)
                            LinearProgressIndicator(value: progress)
                          else
                            const LinearProgressIndicator(),
                          const SizedBox(height: 8),
                          Text('${_status['transferred'] ?? 0} / ${_status['total'] ?? 0} 字节'),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(height: 16),
                ],
                FilledButton.icon(
                  onPressed: (active || _picking) ? null : _pickAndSend,
                  icon: const Icon(Icons.upload_file),
                  label: Text(_picking ? '选择文件…' : '选择文件并发送'),
                ),
                if (error != null && error.isNotEmpty) ...[
                  const SizedBox(height: 12),
                  Text(
                    '上次传输出错: $error',
                    style: TextStyle(color: Theme.of(context).colorScheme.error),
                    textAlign: TextAlign.center,
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}
