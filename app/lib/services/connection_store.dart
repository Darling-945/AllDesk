import 'dart:convert';
import 'package:shared_preferences/shared_preferences.dart';

/// One remembered connection target (manual address or LAN device).
class ConnectionEntry {
  final String address;
  final String? name;
  final int lastUsedMs;
  final bool favorite;

  const ConnectionEntry({
    required this.address,
    this.name,
    required this.lastUsedMs,
    this.favorite = false,
  });

  Map<String, dynamic> toJson() => {
        'address': address,
        'name': name,
        'lastUsedMs': lastUsedMs,
        'favorite': favorite,
      };

  static ConnectionEntry fromJson(Map<String, dynamic> json) => ConnectionEntry(
        address: json['address'] as String,
        name: json['name'] as String?,
        lastUsedMs: (json['lastUsedMs'] as num?)?.toInt() ?? 0,
        favorite: json['favorite'] as bool? ?? false,
      );
}

/// Persists connection history and favorites in shared_preferences.
///
/// Favorites always sort before plain history; history is capped so the
/// list stays useful over time.
class ConnectionStore {
  static const _prefsKey = 'connection_history';
  static const _maxEntries = 20;

  final SharedPreferences _prefs;

  ConnectionStore(this._prefs);

  static Future<ConnectionStore> load() async =>
      ConnectionStore(await SharedPreferences.getInstance());

  List<ConnectionEntry> get entries {
    final raw = _prefs.getString(_prefsKey);
    if (raw == null || raw.isEmpty) return const [];
    try {
      final list = jsonDecode(raw) as List<dynamic>;
      return list
          .map((e) => ConnectionEntry.fromJson(e as Map<String, dynamic>))
          .toList();
    } catch (_) {
      return const [];
    }
  }

  /// Record a connection attempt (called on connect). Moves an existing
  /// entry for the same address to the top with a fresh timestamp.
  Future<void> record(String address, {String? name}) async {
    final addr = address.trim();
    if (addr.isEmpty) return;
    final list = [...entries]..removeWhere((e) => e.address == addr);
    list.insert(
      0,
      ConnectionEntry(
        address: addr,
        name: name,
        lastUsedMs: DateTime.now().millisecondsSinceEpoch,
      ),
    );
    await _save(list.take(_maxEntries).toList());
  }

  Future<void> toggleFavorite(String address) async {
    final list = [...entries];
    final idx = list.indexWhere((e) => e.address == address);
    if (idx == -1) return;
    final old = list[idx];
    list[idx] = ConnectionEntry(
      address: old.address,
      name: old.name,
      lastUsedMs: old.lastUsedMs,
      favorite: !old.favorite,
    );
    await _save(list);
  }

  Future<void> remove(String address) async {
    final list = [...entries]..removeWhere((e) => e.address == address);
    await _save(list);
  }

  Future<void> _save(List<ConnectionEntry> list) async {
    await _prefs.setString(
      _prefsKey,
      jsonEncode(list.map((e) => e.toJson()).toList()),
    );
  }
}
