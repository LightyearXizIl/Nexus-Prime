import type { DeviceConfig, KeyAction, TriggerMode } from "../types";
import { normalizeVoiceShortcutConfig } from "./voiceShortcut";

export type ImePreset =
  | "codex"
  | "wechat"
  | "qianwen"
  | "doubao-hold"
  | "doubao-hands-free";

export interface ImePresetDefinition {
  shortcutVks: number[];
  voiceHotkey: string[];
  triggerMode: TriggerMode;
  applyHint: string;
  logMessage: string;
}

export const IME_PRESETS: Record<ImePreset, ImePresetDefinition> = {
  codex: {
    shortcutVks: [0xa2, 0xa0, 0x44],
    voiceHotkey: ["leftctrl", "leftshift", "d"],
    triggerMode: "Hold",
    applyHint: "已应用：语音键 = 左 Ctrl + 左 Shift + D，触发模式 = 按住",
    logMessage: "设置建议：已快速应用 Codex 按住听写映射（左 Ctrl + 左 Shift + D）",
  },
  wechat: {
    shortcutVks: [0xa2, 0x5b],
    voiceHotkey: ["leftctrl", "leftwin"],
    triggerMode: "Hold",
    applyHint: "已应用：语音键 = 左 Ctrl + 左 Win，触发模式 = 按住",
    logMessage: "设置建议：已快速应用微信按住说话映射（左 Ctrl + 左 Win）",
  },
  qianwen: {
    shortcutVks: [0xa5],
    voiceHotkey: ["rightalt"],
    triggerMode: "Hold",
    applyHint: "已应用：语音键 = 右 Alt，触发模式 = 按住",
    logMessage: "设置建议：已快速应用千问按住说话映射（右 Alt）",
  },
  "doubao-hold": {
    shortcutVks: [0xa5],
    voiceHotkey: ["rightalt"],
    triggerMode: "Hold",
    applyHint: "已应用：豆包长按模式，语音键 = 右 Alt，触发模式 = 按住",
    logMessage: "设置建议：已快速应用豆包长按语音映射（右 Alt）",
  },
  "doubao-hands-free": {
    shortcutVks: [0xa5, 0x20],
    voiceHotkey: ["rightalt", "space"],
    triggerMode: "Toggle",
    applyHint: "已应用：豆包免按模式，语音键 = 右 Alt + 空格，触发模式 = 点击",
    logMessage: "设置建议：已快速应用豆包免按语音映射（右 Alt + 空格）",
  },
};

function shortcutAction(shortcutVks: readonly number[]): KeyAction {
  if (shortcutVks.length === 1) {
    return { type: "SingleKey", value: shortcutVks[0] };
  }
  return { type: "ComboKey", value: [...shortcutVks] };
}

/** Build a complete, dedicated voice-key configuration for an input-method preset. */
export function applyImePresetConfig(config: DeviceConfig, preset: ImePreset): DeviceConfig {
  const definition = IME_PRESETS[preset];
  const action = shortcutAction(definition.shortcutVks);

  return normalizeVoiceShortcutConfig({
    ...config,
    button_bindings: {
      ...config.button_bindings,
      mic: action,
      voice: action,
    },
    voice_hotkey: [...definition.voiceHotkey],
    voice_shortcut_enabled: true,
    trigger_mode: definition.triggerMode,
  });
}
