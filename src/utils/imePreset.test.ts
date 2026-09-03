import { describe, expect, it } from "vitest";
import type { DeviceConfig } from "../types";
import { applyImePresetConfig } from "./imePreset";

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

describe("applyImePresetConfig", () => {
  it("migrates only after the user applies the visible WeChat hold preset", () => {
    const legacy = {
      ...configWithLegacyVoiceGestures(),
      voice_input_profile: "wechat" as const,
      trigger_mode: "Toggle" as const,
    };
    // Reading a legacy config does not mutate it; only this explicit action changes the profile.
    const next = applyImePresetConfig(legacy, "wechat-hold");

    expect(next).toMatchObject({
      button_bindings: {
        mic: { type: "ComboKey", value: [0xa2, 0x5b] },
        voice: { type: "ComboKey", value: [0xa2, 0x5b] },
      },
      voice_hotkey: ["leftctrl", "leftwin"],
      voice_input_profile: "wechat-hold",
      trigger_mode: "Hold",
    });
    expect(legacy.voice_input_profile).toBe("wechat");
  });

  it("preserves the legacy WeChat shortcut and removes legacy voice gestures", () => {
    const next = applyImePresetConfig(configWithLegacyVoiceGestures(), "wechat");

    expect(next).toMatchObject({
      button_bindings: {
        mic: { type: "ComboKey", value: [0xa2, 0x5b] },
        voice: { type: "ComboKey", value: [0xa2, 0x5b] },
      },
      voice_hotkey: ["leftctrl", "leftwin"],
      voice_input_profile: "wechat",
      voice_shortcut_enabled: true,
      trigger_mode: "Toggle",
      long_press_bindings: { menu: { type: "SingleKey", value: 0x20 } },
      multi_click_bindings: { menu: { 2: { type: "SingleKey", value: 0x41 } } },
    });
  });

  it("configures the current WeChat hold-to-talk shortcut and removes legacy voice gestures", () => {
    const next = applyImePresetConfig(configWithLegacyVoiceGestures(), "wechat-current");

    expect(next).toMatchObject({
      button_bindings: {
        mic: { type: "ComboKey", value: [0xa2, 0xa0, 0x44] },
        voice: { type: "ComboKey", value: [0xa2, 0xa0, 0x44] },
      },
      voice_hotkey: ["leftctrl", "leftshift", "d"],
      voice_input_profile: "wechat-current",
      voice_shortcut_enabled: true,
      trigger_mode: "Hold",
      long_press_bindings: { menu: { type: "SingleKey", value: 0x20 } },
      multi_click_bindings: { menu: { 2: { type: "SingleKey", value: 0x41 } } },
    });
  });

  it("configures the Doubao hold shortcut and removes legacy voice gestures", () => {
    const next = applyImePresetConfig(configWithLegacyVoiceGestures(), "doubao-hold");

    expect(next).toMatchObject({
      button_bindings: {
        mic: { type: "SingleKey", value: 0xa5 },
        voice: { type: "SingleKey", value: 0xa5 },
      },
      voice_hotkey: ["rightalt"],
      voice_input_profile: "doubao-hold",
      voice_shortcut_enabled: true,
      trigger_mode: "Hold",
      long_press_bindings: { menu: { type: "SingleKey", value: 0x20 } },
      multi_click_bindings: { menu: { 2: { type: "SingleKey", value: 0x41 } } },
    });
  });

  it("configures Qianwen with the direct right-Alt voice profile", () => {
    const next = applyImePresetConfig(configWithLegacyVoiceGestures(), "qianwen");

    expect(next).toMatchObject({
      button_bindings: {
        mic: { type: "SingleKey", value: 0xa5 },
        voice: { type: "SingleKey", value: 0xa5 },
      },
      voice_hotkey: ["rightalt"],
      voice_input_profile: "qianwen",
      voice_shortcut_enabled: true,
      trigger_mode: "Hold",
    });
  });

  it("configures the Doubao hands-free shortcut as a click-mode Alt+Space chord", () => {
    const next = applyImePresetConfig(configWithLegacyVoiceGestures(), "doubao-hands-free");

    expect(next).toMatchObject({
      button_bindings: {
        mic: { type: "ComboKey", value: [0xa5, 0x20] },
        voice: { type: "ComboKey", value: [0xa5, 0x20] },
      },
      voice_hotkey: ["rightalt", "space"],
      voice_input_profile: "doubao-hands-free",
      voice_shortcut_enabled: true,
      trigger_mode: "Toggle",
      long_press_bindings: { menu: { type: "SingleKey", value: 0x20 } },
      multi_click_bindings: { menu: { 2: { type: "SingleKey", value: 0x41 } } },
    });
  });
});
