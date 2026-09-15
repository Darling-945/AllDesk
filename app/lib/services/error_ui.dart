import 'dart:async';
import 'package:flutter/material.dart';
import '../l10n/app_localizations.dart';

/// Global messenger so errors can be surfaced even without a BuildContext
/// (e.g. the Rust startup init that runs concurrently with the first frame).
final rootScaffoldMessengerKey = GlobalKey<ScaffoldMessengerState>();

/// Show an error to the user as a snackbar. Every place that used to only
/// debugPrint (or silently swallow) Rust-layer errors goes through here.
void showErrorSnackBar(BuildContext context, Object? error, {String? prefix}) {
  final l10n = AppLocalizations.of(context);
  final label = prefix ?? l10n?.errorOperationFailed ?? '操作失败';
  ScaffoldMessenger.of(context).showSnackBar(
    SnackBar(
      content: Text(error == null || error.toString().isEmpty
          ? label
          : '$label: $error'),
      backgroundColor: Theme.of(context).colorScheme.errorContainer,
    ),
  );
}

/// Show an error via the global messenger, retrying briefly until the
/// MaterialApp exists (used during app startup, before any page context).
/// The message is built lazily so it can be localized once a context exists.
Future<void> showStartupError(
    String Function(AppLocalizations? l10n) build) async {
  for (var attempt = 0; attempt < 10; attempt++) {
    final state = rootScaffoldMessengerKey.currentState;
    if (state != null) {
      final ctx = rootScaffoldMessengerKey.currentContext;
      final l10n = (ctx != null && ctx.mounted) ? AppLocalizations.of(ctx) : null;
      state.showSnackBar(SnackBar(content: Text(build(l10n))));
      return;
    }
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }
}
