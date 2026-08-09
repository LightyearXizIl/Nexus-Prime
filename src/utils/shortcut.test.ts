import { describe, expect, it } from "vitest";
import {
  composeShortcutVks,
  isLegacyIncompleteCodexShortcut,
  normalizeShortcutVks,
  splitShortcutVks,
  vksToHotkeyNames,
} from "./shortcut";

describe("shortcut helpers", () => {
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
      mainKey: 0x44,
      extraKeys: [0x70],
    });
    expect(composeShortcutVks(shortcut.modifiers, shortcut.mainKey, shortcut.extraKeys)).toEqual([0x11, 0x44, 0x70]);
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
