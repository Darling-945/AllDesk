import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:alldesk/services/connection_store.dart';

void main() {
  setUp(() {
    SharedPreferences.setMockInitialValues({});
  });

  group('ConnectionStore', () {
    test('record adds entry and updates timestamp on reconnect', () async {
      final store = await ConnectionStore.load();
      await store.record('192.168.1.10:21116', name: 'desk-a');
      await store.record('192.168.1.20:21116');

      var entries = store.entries;
      expect(entries.length, 2);
      // Most recent first.
      expect(entries[0].address, '192.168.1.20:21116');
      expect(entries[0].name, isNull);
      expect(entries[1].name, 'desk-a');

      // Re-recording an existing address moves it to the top, no duplicate.
      await store.record('192.168.1.10:21116', name: 'desk-a');
      entries = store.entries;
      expect(entries.length, 2);
      expect(entries[0].address, '192.168.1.10:21116');
    });

    test('record ignores empty addresses', () async {
      final store = await ConnectionStore.load();
      await store.record('   ');
      expect(store.entries, isEmpty);
    });

    test('toggleFavorite flips and persists', () async {
      final store = await ConnectionStore.load();
      await store.record('a:1');

      await store.toggleFavorite('a:1');
      expect(store.entries.single.favorite, isTrue);

      await store.toggleFavorite('a:1');
      expect(store.entries.single.favorite, isFalse);
    });

    test('remove drops the entry', () async {
      final store = await ConnectionStore.load();
      await store.record('a:1');
      await store.record('b:2');

      await store.remove('a:1');
      expect(store.entries.map((e) => e.address), ['b:2']);
    });

    test('history is capped at 20 entries', () async {
      final store = await ConnectionStore.load();
      for (var i = 0; i < 25; i++) {
        await store.record('addr-$i');
      }
      expect(store.entries.length, 20);
      // Newest kept, oldest evicted.
      expect(store.entries.first.address, 'addr-24');
      expect(store.entries.any((e) => e.address == 'addr-0'), isFalse);
    });

    test('entries survive store reload (persistence)', () async {
      final store = await ConnectionStore.load();
      await store.record('persisted:1', name: 'kept');
      await store.toggleFavorite('persisted:1');

      // A fresh store instance reads the same shared_preferences backing.
      final reopened = await ConnectionStore.load();
      final entry = reopened.entries.single;
      expect(entry.address, 'persisted:1');
      expect(entry.name, 'kept');
      expect(entry.favorite, isTrue);
    });

    test('corrupted persisted JSON degrades to empty list', () async {
      SharedPreferences.setMockInitialValues({
        'connection_history': 'not-json{{{',
      });
      final store = await ConnectionStore.load();
      expect(store.entries, isEmpty);
    });
  });
}
