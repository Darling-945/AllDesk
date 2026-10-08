import 'package:flutter/services.dart';

/// Wire codes for "special" keys sent as key_type="special".
///
/// MUST stay in sync with the Rust decode table `decode_special_key` in
/// crates/alldesk-ffi/src/api.rs — that table is the single source of truth.
const int kKeyEnter = 0x01;
const int kKeyEscape = 0x02;
const int kKeyTab = 0x03;
const int kKeyBackspace = 0x04;
const int kKeyDelete = 0x05;
const int kKeyArrowUp = 0x06;
const int kKeyArrowDown = 0x07;
const int kKeyArrowLeft = 0x08;
const int kKeyArrowRight = 0x09;
// 0x10..0x27 = F1..F24
const int kKeySpace = 0x30;
const int kKeyHome = 0x31;
const int kKeyEnd = 0x32;
const int kKeyPageUp = 0x33;
const int kKeyPageDown = 0x34;
const int kKeyInsert = 0x35;
const int kKeyPrintScreen = 0x36;
const int kKeyPause = 0x37;
const int kKeyCapsLock = 0x38;
const int kKeyNumLock = 0x39;
const int kKeyScrollLock = 0x3A;
const int kKeyMenu = 0x3B;
const int kKeyLeftCtrl = 0x40;
const int kKeyLeftShift = 0x41;
const int kKeyLeftAlt = 0x42;
const int kKeyLeftMeta = 0x43;
const int kKeyRightCtrl = 0x44;
const int kKeyRightShift = 0x45;
const int kKeyRightAlt = 0x46;
const int kKeyRightMeta = 0x47;

final Map<int, int> _specialCodes = {
  LogicalKeyboardKey.enter.keyId: kKeyEnter,
  LogicalKeyboardKey.escape.keyId: kKeyEscape,
  LogicalKeyboardKey.tab.keyId: kKeyTab,
  LogicalKeyboardKey.backspace.keyId: kKeyBackspace,
  LogicalKeyboardKey.delete.keyId: kKeyDelete,
  LogicalKeyboardKey.arrowUp.keyId: kKeyArrowUp,
  LogicalKeyboardKey.arrowDown.keyId: kKeyArrowDown,
  LogicalKeyboardKey.arrowLeft.keyId: kKeyArrowLeft,
  LogicalKeyboardKey.arrowRight.keyId: kKeyArrowRight,
  LogicalKeyboardKey.space.keyId: kKeySpace,
  LogicalKeyboardKey.home.keyId: kKeyHome,
  LogicalKeyboardKey.end.keyId: kKeyEnd,
  LogicalKeyboardKey.pageUp.keyId: kKeyPageUp,
  LogicalKeyboardKey.pageDown.keyId: kKeyPageDown,
  LogicalKeyboardKey.insert.keyId: kKeyInsert,
  LogicalKeyboardKey.printScreen.keyId: kKeyPrintScreen,
  LogicalKeyboardKey.pause.keyId: kKeyPause,
  LogicalKeyboardKey.capsLock.keyId: kKeyCapsLock,
  LogicalKeyboardKey.numLock.keyId: kKeyNumLock,
  LogicalKeyboardKey.scrollLock.keyId: kKeyScrollLock,
  LogicalKeyboardKey.contextMenu.keyId: kKeyMenu,
  LogicalKeyboardKey.controlLeft.keyId: kKeyLeftCtrl,
  LogicalKeyboardKey.shiftLeft.keyId: kKeyLeftShift,
  LogicalKeyboardKey.altLeft.keyId: kKeyLeftAlt,
  LogicalKeyboardKey.metaLeft.keyId: kKeyLeftMeta,
  LogicalKeyboardKey.controlRight.keyId: kKeyRightCtrl,
  LogicalKeyboardKey.shiftRight.keyId: kKeyRightShift,
  LogicalKeyboardKey.altRight.keyId: kKeyRightAlt,
  LogicalKeyboardKey.metaRight.keyId: kKeyRightMeta,
  LogicalKeyboardKey.f1.keyId: 0x10,
  LogicalKeyboardKey.f2.keyId: 0x11,
  LogicalKeyboardKey.f3.keyId: 0x12,
  LogicalKeyboardKey.f4.keyId: 0x13,
  LogicalKeyboardKey.f5.keyId: 0x14,
  LogicalKeyboardKey.f6.keyId: 0x15,
  LogicalKeyboardKey.f7.keyId: 0x16,
  LogicalKeyboardKey.f8.keyId: 0x17,
  LogicalKeyboardKey.f9.keyId: 0x18,
  LogicalKeyboardKey.f10.keyId: 0x19,
  LogicalKeyboardKey.f11.keyId: 0x1A,
  LogicalKeyboardKey.f12.keyId: 0x1B,
  LogicalKeyboardKey.f13.keyId: 0x1C,
  LogicalKeyboardKey.f14.keyId: 0x1D,
  LogicalKeyboardKey.f15.keyId: 0x1E,
  LogicalKeyboardKey.f16.keyId: 0x1F,
  LogicalKeyboardKey.f17.keyId: 0x20,
  LogicalKeyboardKey.f18.keyId: 0x21,
  LogicalKeyboardKey.f19.keyId: 0x22,
  LogicalKeyboardKey.f20.keyId: 0x23,
  LogicalKeyboardKey.f21.keyId: 0x24,
  LogicalKeyboardKey.f22.keyId: 0x25,
  LogicalKeyboardKey.f23.keyId: 0x26,
  LogicalKeyboardKey.f24.keyId: 0x27,
};

/// Protocol code for keys with a dedicated "special" meaning, or null.
int? specialCodeFor(LogicalKeyboardKey key) => _specialCodes[key.keyId];

/// Windows VK code for letter/digit keys, used for modifier combos
/// (Ctrl+C etc.) where the character is a control code and the text path
/// would be wrong. ASCII VK codes for A-Z / 0-9 are universal.
int? vkForCombo(LogicalKeyboardKey key) {
  final label = key.keyLabel;
  if (label.length == 1) {
    final upper = label.toUpperCase().codeUnitAt(0);
    if ((upper >= 0x41 && upper <= 0x5A) || (upper >= 0x30 && upper <= 0x39)) {
      return upper;
    }
  }
  return null;
}

/// Whether [text] is a printable character worth sending via the char path
/// (excludes control codes like \x01-\x1F produced when Ctrl is held).
bool isPrintableText(String text) {
  if (text.isEmpty) return false;
  final rune = text.runes.first;
  return rune >= 0x20 && rune != 0x7F;
}
