import 'package:alldesk/services/key_mapping.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('key_mapping special codes', () {
    test('control keys map to protocol codes', () {
      expect(specialCodeFor(LogicalKeyboardKey.enter), kKeyEnter);
      expect(specialCodeFor(LogicalKeyboardKey.escape), kKeyEscape);
      expect(specialCodeFor(LogicalKeyboardKey.backspace), kKeyBackspace);
      expect(specialCodeFor(LogicalKeyboardKey.delete), kKeyDelete);
      expect(specialCodeFor(LogicalKeyboardKey.arrowUp), kKeyArrowUp);
      expect(specialCodeFor(LogicalKeyboardKey.space), kKeySpace);
      expect(specialCodeFor(LogicalKeyboardKey.home), kKeyHome);
      expect(specialCodeFor(LogicalKeyboardKey.end), kKeyEnd);
      expect(specialCodeFor(LogicalKeyboardKey.pageUp), kKeyPageUp);
      expect(specialCodeFor(LogicalKeyboardKey.pageDown), kKeyPageDown);
      expect(specialCodeFor(LogicalKeyboardKey.insert), kKeyInsert);
      expect(specialCodeFor(LogicalKeyboardKey.printScreen), kKeyPrintScreen);
      expect(specialCodeFor(LogicalKeyboardKey.capsLock), kKeyCapsLock);
      expect(specialCodeFor(LogicalKeyboardKey.contextMenu), kKeyMenu);
    });

    test('modifiers map to distinct left/right codes', () {
      expect(specialCodeFor(LogicalKeyboardKey.controlLeft), kKeyLeftCtrl);
      expect(specialCodeFor(LogicalKeyboardKey.controlRight), kKeyRightCtrl);
      expect(specialCodeFor(LogicalKeyboardKey.shiftLeft), kKeyLeftShift);
      expect(specialCodeFor(LogicalKeyboardKey.shiftRight), kKeyRightShift);
      expect(specialCodeFor(LogicalKeyboardKey.altLeft), kKeyLeftAlt);
      expect(specialCodeFor(LogicalKeyboardKey.altRight), kKeyRightAlt);
      expect(specialCodeFor(LogicalKeyboardKey.metaLeft), kKeyLeftMeta);
      expect(specialCodeFor(LogicalKeyboardKey.metaRight), kKeyRightMeta);
      // The eight modifier codes occupy a contiguous block.
      expect(kKeyRightMeta - kKeyLeftCtrl, 7);
    });

    test('F-keys map to the 0x10..0x27 block', () {
      expect(specialCodeFor(LogicalKeyboardKey.f1), 0x10);
      expect(specialCodeFor(LogicalKeyboardKey.f12), 0x1B);
      expect(specialCodeFor(LogicalKeyboardKey.f24), 0x27);
    });

    test('plain letters and digits have no special code', () {
      expect(specialCodeFor(LogicalKeyboardKey.keyA), isNull);
      expect(specialCodeFor(LogicalKeyboardKey.digit1), isNull);
    });
  });

  group('key_mapping combo VKs', () {
    test('letters map to uppercase ASCII VK codes', () {
      expect(vkForCombo(LogicalKeyboardKey.keyA), 0x41);
      expect(vkForCombo(LogicalKeyboardKey.keyZ), 0x5A);
    });

    test('digits map to ASCII VK codes', () {
      expect(vkForCombo(LogicalKeyboardKey.digit0), 0x30);
      expect(vkForCombo(LogicalKeyboardKey.digit9), 0x39);
    });

    test('non-letter keys have no combo VK', () {
      expect(vkForCombo(LogicalKeyboardKey.enter), isNull);
      expect(vkForCombo(LogicalKeyboardKey.space), isNull);
    });
  });

  group('isPrintableText', () {
    test('accepts regular text', () {
      expect(isPrintableText('a'), isTrue);
      expect(isPrintableText('A'), isTrue);
      expect(isPrintableText('中'), isTrue);
      expect(isPrintableText(' '), isTrue);
    });

    test('rejects control codes produced with modifiers held', () {
      expect(isPrintableText('\x01'), isFalse); // Ctrl+A
      expect(isPrintableText('\x03'), isFalse); // Ctrl+C
      expect(isPrintableText('\x7f'), isFalse);
      expect(isPrintableText(''), isFalse);
    });
  });
}
