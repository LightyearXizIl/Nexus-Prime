// 前端类型定义

export type BridgeType = "xiaomi";

export type ThemePreference = "system" | "light" | "dark";
export type AppLocale = "zh-CN" | "zh-TW" | "en";

export type BridgeStatus =
  | "Disconnected"
  | "Connecting"
  | "Connected"
  | "Error"
  | `Error|${string}`
  | `Error: ${string}`;

export interface DeviceInfo {
  bridge_type: BridgeType;
  status: BridgeStatus;
  device_name: string | null;
  device_address: string | null;
  battery_level: number | null;
  /** true only when the connected BLE device explicitly reports active charging */
  battery_charging: boolean | null;
}

export type MouseMoveValue = {
  dx: -1 | 0 | 1;
  dy: -1 | 0 | 1;
  step: number;
  accelerate: boolean;
};

export type KeyAction =
  | { type: "SingleKey"; value: number }
  | { type: "ComboKey"; value: number[] }
  | { type: "TextInput"; value: string }
  | { type: "LaunchApp"; value: string }
  | { type: "MouseClick"; value: null }
  | { type: "MouseMove"; value: MouseMoveValue }
  | { type: "None"; value: null };

export type TriggerMode = "Toggle" | "Hold";
/** Toggle=点击型快捷键；Hold=按住型快捷键（传声仍为按住遥控语音键） */

/** 输入法预设来源；缺失或 null 表示用户自定义快捷键。 */
export type VoiceInputProfile =
  | "codex"
  | "wechat-hold"
  /** Historical preset retained only so existing configurations can load unchanged. */
  | "wechat"
  /** Historical preset retained only so existing configurations can load unchanged. */
  | "wechat-current"
  | "qianwen"
  | "doubao-hold"
  | "doubao-hands-free";

export interface DeviceConfig {
  button_aliases: Record<string, string>;
  button_bindings: Record<string, KeyAction>;
  long_press_bindings?: Record<string, KeyAction>;
  multi_click_bindings?: Record<string, Partial<Record<2 | 3 | 4, KeyAction>>>;
  multi_click_interval_ms?: number;
  voice_hotkey: string[] | null;
  trigger_mode: TriggerMode;
  voice_input_profile?: VoiceInputProfile | null;
  bluetooth_address: string | null;
  /** 麦克风增益 dB（对齐 Python gain_db，默认 10） */
  gain_db?: number;
  /** 是否注入语音快捷键（传声与此项无关） */
  voice_shortcut_enabled?: boolean;
}

export interface GlobalSettings {
  autostart: boolean;
  autostart_minimized_to_tray: boolean;
  language: AppLocale;
  minimize_to_tray: boolean;
  auto_check_updates: boolean;
  theme: ThemePreference;
  log_retention_days: number;
}

export interface UpdateRelease {
  version: string;
  title: string;
  notes: string;
  publishedAt: string | null;
  assetName: string;
  assetSize: number;
  downloaded: boolean;
}

export interface UpdateCheckResult {
  currentVersion: string;
  update: UpdateRelease | null;
}

export interface UpdateDownloadProgress {
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
}

export interface AudioDevice {
  name: string;
  id: string;
  is_input: boolean;
  is_default: boolean;
}
