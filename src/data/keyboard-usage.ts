// Browser physical key codes to USB keyboard usages.
export const CODE_TO_HID: Record<string, number> = {
  // Letters A–Z (HID 4..29)
  ...Object.fromEntries(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").map((c, i) => [`Key${c}`, 4 + i]),
  ),
  // Digits 1..9 then 0 (HID 30..39)
  ...Object.fromEntries(
    "123456789".split("").map((d, i) => [`Digit${d}`, 30 + i]),
  ),
  Digit0: 39,
  // Whitespace & editing
  Enter: 40,
  Escape: 41,
  Backspace: 42,
  Tab: 43,
  Space: 44,
  Minus: 45,
  Equal: 46,
  BracketLeft: 47,
  BracketRight: 48,
  Backslash: 49,
  Semicolon: 51,
  Quote: 52,
  Backquote: 53,
  Comma: 54,
  Period: 55,
  Slash: 56,
  CapsLock: 57,
  // F1..F12 (HID 58..69)
  ...Object.fromEntries(
    Array.from({ length: 12 }, (_, i) => [`F${i + 1}`, 58 + i]),
  ),
  ...Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`F${i + 13}`, 104 + i])),
  IntlBackslash: 100,
  PrintScreen: 70,
  ScrollLock: 71,
  Pause: 72,
  Insert: 73,
  Home: 74,
  PageUp: 75,
  Delete: 76,
  End: 77,
  PageDown: 78,
  ArrowRight: 79,
  ArrowLeft: 80,
  ArrowDown: 81,
  ArrowUp: 82,
  // Modifiers (HID 224..231)
  ControlLeft: 224,
  ShiftLeft: 225,
  AltLeft: 226,
  MetaLeft: 227,
  ControlRight: 228,
  ShiftRight: 229,
  AltRight: 230,
  MetaRight: 231,
};
