import 'dart:io';
import 'package:flutter/material.dart';
import '../../l10n/app_localizations.dart';
import '../../services/platform_service.dart';
import '../../services/error_ui.dart';

/// Android first-run permission checklist: screen capture (MediaProjection),
/// accessibility input service, and optional microphone. Each item explains
/// what it is for and links to the matching system action. Shown once;
/// re-openable later from the entry point.
class PermissionGuidePage extends StatefulWidget {
  const PermissionGuidePage({super.key});

  @override
  State<PermissionGuidePage> createState() => _PermissionGuidePageState();
}

class _PermissionGuidePageState extends State<PermissionGuidePage> {
  bool _screenCaptureGranted = false;
  bool _accessibilityEnabled = false;

  @override
  void initState() {
    super.initState();
    _refreshStatus();
  }

  Future<void> _refreshStatus() async {
    try {
      final results = await Future.wait([
        PlatformService.isScreenCaptureGranted(),
        PlatformService.isAccessibilityEnabled(),
      ]);
      if (mounted) {
        setState(() {
          _screenCaptureGranted = results[0];
          _accessibilityEnabled = results[1];
        });
      }
    } catch (_) {
      // Status checks are best-effort; items just show as not enabled.
    }
  }

  Future<void> _requestScreenCapture() async {
    try {
      final granted = await PlatformService.requestScreenCapture();
      if (mounted) setState(() => _screenCaptureGranted = granted);
    } catch (e) {
      if (mounted) showErrorSnackBar(context, e);
    }
  }

  Future<void> _openAccessibilitySettings() async {
    try {
      await PlatformService.openAccessibilitySettings();
    } catch (e) {
      if (mounted) showErrorSnackBar(context, e);
    }
  }

  Future<void> _requestMicrophone() async {
    try {
      await PlatformService.requestMicrophonePermission();
    } catch (e) {
      if (mounted) showErrorSnackBar(context, e);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context)!;
    final theme = Theme.of(context);

    return Scaffold(
      appBar: AppBar(title: Text(l10n.permTitle)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(l10n.permIntro, style: theme.textTheme.bodyMedium),
          const SizedBox(height: 16),

          // Screen capture
          _PermissionCard(
            icon: Icons.screen_share,
            title: l10n.permScreenCapture,
            description: l10n.permScreenCaptureDesc,
            granted: _screenCaptureGranted,
            grantedLabel: l10n.permGranted,
            notGrantedLabel: l10n.permNotGranted,
            actionLabel: l10n.permRequest,
            onAction: _requestScreenCapture,
          ),

          // Accessibility input service
          _PermissionCard(
            icon: Icons.accessibility_new,
            title: l10n.permAccessibility,
            description: l10n.permAccessibilityDesc,
            granted: _accessibilityEnabled,
            grantedLabel: l10n.permGranted,
            notGrantedLabel: l10n.permNotGranted,
            actionLabel: l10n.permOpenSettings,
            onAction: _openAccessibilitySettings,
          ),

          // Microphone (optional)
          _PermissionCard(
            icon: Icons.mic,
            title: l10n.permMicrophone,
            description: l10n.permMicrophoneDesc,
            granted: false,
            grantedLabel: l10n.permGranted,
            notGrantedLabel: l10n.permNotGranted,
            actionLabel: l10n.permRequest,
            onAction: _requestMicrophone,
          ),

          const SizedBox(height: 24),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(l10n.permDone),
          ),
          const SizedBox(height: 8),
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(l10n.permSkip),
          ),
          if (!Platform.isAndroid)
            Padding(
              padding: const EdgeInsets.only(top: 16),
              child: Text(
                l10n.permDesktopHint,
                style: theme.textTheme.bodySmall
                    ?.copyWith(color: theme.colorScheme.outline),
                textAlign: TextAlign.center,
              ),
            ),
        ],
      ),
    );
  }
}

class _PermissionCard extends StatelessWidget {
  final IconData icon;
  final String title;
  final String description;
  final bool granted;
  final String grantedLabel;
  final String notGrantedLabel;
  final String actionLabel;
  final VoidCallback onAction;

  const _PermissionCard({
    required this.icon,
    required this.title,
    required this.description,
    required this.granted,
    required this.grantedLabel,
    required this.notGrantedLabel,
    required this.actionLabel,
    required this.onAction,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(icon, color: theme.colorScheme.primary),
                const SizedBox(width: 12),
                Expanded(child: Text(title, style: theme.textTheme.titleSmall)),
                Icon(
                  granted ? Icons.check_circle : Icons.cancel,
                  color: granted ? Colors.green : theme.colorScheme.outline,
                  size: 20,
                ),
                const SizedBox(width: 4),
                Text(
                  granted ? grantedLabel : notGrantedLabel,
                  style: theme.textTheme.labelSmall?.copyWith(
                    color: granted ? Colors.green : theme.colorScheme.outline,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(description, style: theme.textTheme.bodySmall),
            const SizedBox(height: 12),
            Align(
              alignment: Alignment.centerRight,
              child: FilledButton.tonal(
                onPressed: onAction,
                child: Text(actionLabel),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
