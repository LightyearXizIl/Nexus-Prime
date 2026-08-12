import { describe, expect, it } from "vitest";
import type { DeviceConfig } from "../types";
import { normalizeVoiceShortcutConfig } from "./voiceShortcut";

function configWithLegacyVoiceGestures(): DeviceConfig {
  return {
    button_aliases: {},
    button_bindings: {
      mic: { type: "ComboKey", value: [0xa2, 0x5b] },
      voice: { type: "ComboKey", value: [0xa2, 0x5b] },
    },
    long_press_bindings: {
      mic: { type: "SingleKey", value: 0xa5 },
      menu: { type: "SingleKey", value: 0x20 },
    },
    multi_click_bindings: {
      voice: { 2: { type: "SingleKey", value: 0xa5 } },
      menu: { 2: { type: "SingleKey", value: 0x41 } },
    },
    voice_hotkey: ["leftctrl", "leftwin"],
    trigger_mode: "Hold",
    bluetooth_address: null,
  };
}

describe("normalizeVoiceShortcutConfig", () => {
  it("removes only voice gesture mappings", () => {
    const normalized = normalizeVoiceShortcutConfig(configWithLegacyVoiceGestures());

    expect(normalized.long_press_bindings).toEqual({
      menu: { type: "SingleKey", value: 0x20 },
    });
    expect(normalized.multi_click_bindings).toEqual({
      menu: { 2: { type: "SingleKey", value: 0x41 } },
    });
    expect(normalized.button_bindings.mic).toEqual({
      type: "ComboKey",
      value: [0xa2, 0x5b],
    });
  });
});
