import type { DeviceConfig } from "../types";

const VOICE_BUTTON_IDS = ["mic", "voice"] as const;

/**
 * Voice is a dedicated shortcut trigger. It must never keep a gesture mapping
 * that can take precedence over its input-method shortcut.
 */
export function normalizeVoiceShortcutConfig(config: DeviceConfig): DeviceConfig {
  const longPress = { ...(config.long_press_bindings || {}) };
  const multiClick = { ...(config.multi_click_bindings || {}) };

  for (const id of VOICE_BUTTON_IDS) {
    delete longPress[id];
    delete multiClick[id];
  }

  return {
    ...config,
    button_bindings: { ...config.button_bindings },
    long_press_bindings: longPress,
    multi_click_bindings: multiClick,
  };
}
