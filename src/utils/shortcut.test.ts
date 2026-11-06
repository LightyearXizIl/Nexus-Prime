import { describe, expect, it } from "vitest";
import {
  composeShortcutVks,
  isLegacyIncompleteCodexShortcut,
  normalizeShortcutVks,
  splitShortcutVks,
  parseShortcutText,
  vksToHotkeyNames,
} from "./shortcut";

describe("shortcut helpers", () => {
  it("parses manual shortcut text without changing unknown persisted values", () => {
    expect(parseShortcutText(" alt + f4 ")).toEqual({ keys: [0x12, 0x73] });
    expect(parseShortcutText("LeftCtrl + Shift + D")).toEqual({ keys: [0xa2, 0x10, 0x44] });
    expect(parseShortcutText("Alt++").error).toContain("Plus");
    expect(parseShortcutText("Ctrl+NoSuchKey").error).toContain("nosuchkey");
  });
  it("resolves symbols to physical keys and adds Shift only when needed", () => {
    expect(parseShortcutText(",")).toEqual({ keys: [0xbc] });
    expect(parseShortcutText("Ctrl + , + .")).toEqual({ keys: [0x11, 0xbc, 0xbe] });
    expect(parseShortcutText("Ctrl + ?")).toEqual({ keys: [0x11, 0x10, 0xbf] });
    expect(parseShortcutText("? + RightShift")).toEqual({ keys: [0xa1, 0xbf] });
    expect(parseShortcutText("Plus")).toEqual({ keys: [0x10, 0xbb] });
    expect(parseShortcutText("! + )")).toEqual({ keys: [0x10, 0x31, 0x30] });
    expect(parseShortcutText("? + /").error).toContain("重复的按键");
    expect(parseShortcutText("! + 1").error).toContain("重复的按键");
    expect(parseShortcutText("Ctrl + LeftCtrl").error).toContain("重复的 ctrl");
  });
  it("keeps valid modifier-only input-method shortcuts valid", () => {
    expect(isLegacyIncompleteCodexShortcut([0xa2, 0x5b])).toBe(false);
    expect(isLegacyIncompleteCodexShortcut([0xa5])).toBe(false);
    expect(isLegacyIncompleteCodexShortcut([0xa2, 0xa0])).toBe(true);
  });

  it("removes generic modifier duplicates once an explicit side is selected", () => {
    expect(normalizeShortcutVks([0x11, 0xa2, 0x44, 0x44])).toEqual([0xa2, 0x44]);
    expect(normalizeShortcutVks([0x10, 0xa1, 0xa5])).toEqual([0xa1, 0xa5]);
  });

  it("round-trips generic modifiers without turning them into hidden extra keys", () => {
    const shortcut = splitShortcutVks([0x11, 0x44, 0x70]);
    expect(shortcut).toEqual({
      modifiers: { ctrl: "generic", shift: "none", alt: "none", win: "none" },
      keys: [0x44, 0x70],
    });
    expect(composeShortcutVks(shortcut.modifiers, shortcut.keys)).toEqual({ keys: [0x11, 0x44, 0x70] });
  });

  it("serializes supported and unknown virtual keys without duplicate modifier tokens", () => {
    expect(vksToHotkeyNames([0x11, 0xa2, 0x08, 0x74, 0x25])).toEqual([
      "leftctrl",
      "backspace",
      "f5",
      "vk_25",
    ]);
  });
});
