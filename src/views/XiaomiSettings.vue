<script setup lang="ts">
import { onMounted, onUnmounted, computed, ref, nextTick, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { loggedInvoke as invoke } from "../utils/appLogger";
import { useRoute, useRouter } from "vue-router";
import { useBridgeStore } from "../stores/bridge";
import { useConfigStore } from "../stores/config";
import DeviceStatus from "../components/DeviceStatus.vue";
import KeyMappingStage from "../components/KeyMappingStage.vue";
import type { DeviceConfig } from "../types";
import { normalizeVoiceShortcutConfig } from "../utils/voiceShortcut";
import {
  connectedDeviceName,
  connectionStatusPresentation,
} from "../utils/connectionStatus";
import InputMethodSettingsDialog, {
  type ImePreset,
} from "../components/InputMethodSettingsDialog.vue";
import { applyImePresetConfig, IME_PRESETS } from "../utils/imePreset";
import remoteProductImage from "../assets/xiaomi-remote-cutout.png";
import { useI18n } from "vue-i18n";

const bridge = useBridgeStore();
const configStore = useConfigStore();
const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const type = "xiaomi" as const;

const device = computed(() => bridge.devices[type]);
const config = computed(() => configStore.configs[type]);
const isMappingPage = computed(() => route.name === "xiaomi-mapping");

interface HostStatusItem {
  id: string;
  label: string;
  state_label: string;
  tone: string;
}

interface HostStatus {
  bridge_alive: boolean;
  audio_alive: boolean;
  cable_ready: boolean;
  atvv_ok?: boolean;
  status_text: string;
  detail: string;
  tone: string;
  items: HostStatusItem[];
}

const restarting = ref(false);
const voiceRepairing = ref(false);
const virtualKeyboardRepairing = ref(false);
const atvvRepairing = ref(false);
const repairBusy = computed(
  () => restarting.value || voiceRepairing.value || virtualKeyboardRepairing.value || atvvRepairing.value,
);
const showVoiceChoice = ref(false);
const voiceChoiceMsg = ref("");
const showLogModal = ref(false);
const showSetupTips = ref(false);
const setupApplyHint = ref("");
const logText = ref("");
const logPath = ref("");
const logLoading = ref(false);
const logCopyHint = ref("");
const logFiles = ref<Array<{ name: string; size: number; current: boolean }>>([]);
const selectedLogFile = ref("");
const logWriteError = ref("");

type BleMeterState = "idle" | "session" | "receiving";
interface VoiceMeterSnapshot {
  bleState: BleMeterState;
  bleLevel: number;
  waveform: number[];
  cableActive: boolean;
  cableLevel: number;
  atvvOk: boolean;
}

const voiceMeter = ref<VoiceMeterSnapshot>({
  bleState: "idle",
  bleLevel: 0,
  waveform: Array(28).fill(0),
  cableActive: false,
  cableLevel: 0,
  atvvOk: false,
});

/** 「按键映射」标题旁：最近一次 按下/抬起 + 遥控键：映射 */
const lastMappingFlash = ref<{
  seq: number;
  phase: "down" | "up";
  remote: string;
  mapped: string | null;
} | null>(null);
let mappingFlashSeq = 0;
let mappingFlashClearTimer: ReturnType<typeof setTimeout> | null = null;

const cableActivityLabel = computed(() =>
  voiceMeter.value.cableActive ? t("dashboard.receiving") : t("dashboard.noSignal")
);

function applyVoiceMeter(p: Record<string, unknown>) {
  const bleState = (p.bleState ?? p.ble_state ?? "idle") as BleMeterState;
  const waveform = p.waveform as number[] | undefined;
  voiceMeter.value = {
    bleState,
    bleLevel: Number(p.bleLevel ?? p.ble_level ?? 0),
    waveform: Array.isArray(waveform) && waveform.length ? [...waveform] : Array(28).fill(0),
    cableActive: Boolean(p.cableActive ?? p.cable_active ?? false),
    cableLevel: Number(p.cableLevel ?? p.cable_level ?? 0),
    atvvOk: Boolean(p.atvvOk ?? p.atvv_ok ?? false),
  };
}
const showVoiceShortcutTip = ref(false);
const showGainTip = ref(false);
const showTriggerTip = ref(false);
const showRepairTip = ref(false);
const showVirtualKeyboardTip = ref(false);
const showAtvvTip = ref(false);
const showRestartTip = ref(false);
const voiceInfoBtn = ref<HTMLElement | null>(null);
const gainInfoBtn = ref<HTMLElement | null>(null);
const triggerInfoBtn = ref<HTMLElement | null>(null);
const repairInfoBtn = ref<HTMLElement | null>(null);
const virtualKeyboardInfoBtn = ref<HTMLElement | null>(null);
const atvvInfoBtn = ref<HTMLElement | null>(null);
const restartInfoBtn = ref<HTMLElement | null>(null);
const voiceTipEl = ref<HTMLElement | null>(null);
const gainTipEl = ref<HTMLElement | null>(null);
const triggerTipEl = ref<HTMLElement | null>(null);
const repairTipEl = ref<HTMLElement | null>(null);
const virtualKeyboardTipEl = ref<HTMLElement | null>(null);
const atvvTipEl = ref<HTMLElement | null>(null);
const restartTipEl = ref<HTMLElement | null>(null);
const voiceTipStyle = ref<Record<string, string>>({});
const gainTipStyle = ref<Record<string, string>>({});
const triggerTipStyle = ref<Record<string, string>>({});
const repairTipStyle = ref<Record<string, string>>({});
const virtualKeyboardTipStyle = ref<Record<string, string>>({});
const atvvTipStyle = ref<Record<string, string>>({});
const restartTipStyle = ref<Record<string, string>>({});
let voiceTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let gainTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let triggerTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let repairTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let virtualKeyboardTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let atvvTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let restartTipCloseTimer: ReturnType<typeof setTimeout> | null = null;

/** 右上 / 右下自动落位，并钳制在视口内 */
function placeInfoTip(
  anchor: HTMLElement | null,
  tip: HTMLElement | null,
  styleRef: typeof voiceTipStyle
) {
  if (!anchor || !tip) return;
  const margin = 8;
  const pad = 8;
  const ar = anchor.getBoundingClientRect();
  const tw = tip.offsetWidth || Math.min(420, window.innerWidth - pad * 2);
  const th = tip.offsetHeight || 120;
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  const spaceBelow = vh - ar.bottom - margin;
  const spaceAbove = ar.top - margin;
  // 优先右下方；下方不够且上方更宽裕则改右上方
  const placeBelow = spaceBelow >= th || spaceBelow >= spaceAbove;

  let top = placeBelow ? ar.bottom + margin : ar.top - th - margin;
  // 右对齐图标右侧（右上/右下）
  let left = ar.right - tw;

  if (left < pad) left = pad;
  if (left + tw > vw - pad) left = Math.max(pad, vw - pad - tw);
  if (top < pad) top = pad;
  if (top + th > vh - pad) top = Math.max(pad, vh - pad - th);

  styleRef.value = {
    position: "fixed",
    top: `${Math.round(top)}px`,
    left: `${Math.round(left)}px`,
    right: "auto",
    bottom: "auto",
    zIndex: "2000",
    visibility: "visible",
    maxWidth: `${Math.min(420, vw - pad * 2)}px`,
  };
}

async function openVoiceTip() {
  if (voiceTipCloseTimer) {
    clearTimeout(voiceTipCloseTimer);
    voiceTipCloseTimer = null;
  }
  voiceTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showVoiceShortcutTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(voiceInfoBtn.value, voiceTipEl.value, voiceTipStyle);
  });
}

function scheduleCloseVoiceTip() {
  if (voiceTipCloseTimer) clearTimeout(voiceTipCloseTimer);
  voiceTipCloseTimer = setTimeout(() => {
    showVoiceShortcutTip.value = false;
  }, 120);
}

function toggleVoiceTip() {
  if (showVoiceShortcutTip.value) {
    showVoiceShortcutTip.value = false;
  } else {
    void openVoiceTip();
  }
}

async function openGainTip() {
  if (gainTipCloseTimer) {
    clearTimeout(gainTipCloseTimer);
    gainTipCloseTimer = null;
  }
  gainTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showGainTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(gainInfoBtn.value, gainTipEl.value, gainTipStyle);
  });
}

function scheduleCloseGainTip() {
  if (gainTipCloseTimer) clearTimeout(gainTipCloseTimer);
  gainTipCloseTimer = setTimeout(() => {
    showGainTip.value = false;
  }, 120);
}

function toggleGainTip() {
  if (showGainTip.value) {
    showGainTip.value = false;
  } else {
    void openGainTip();
  }
}

async function openTriggerTip() {
  if (triggerTipCloseTimer) {
    clearTimeout(triggerTipCloseTimer);
    triggerTipCloseTimer = null;
  }
  triggerTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showTriggerTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(triggerInfoBtn.value, triggerTipEl.value, triggerTipStyle);
  });
}

function scheduleCloseTriggerTip() {
  if (triggerTipCloseTimer) clearTimeout(triggerTipCloseTimer);
  triggerTipCloseTimer = setTimeout(() => {
    showTriggerTip.value = false;
  }, 120);
}

function toggleTriggerTip() {
  if (showTriggerTip.value) {
    showTriggerTip.value = false;
  } else {
    void openTriggerTip();
  }
}

async function openRepairTip() {
  if (repairTipCloseTimer) {
    clearTimeout(repairTipCloseTimer);
    repairTipCloseTimer = null;
  }
  repairTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showRepairTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(repairInfoBtn.value, repairTipEl.value, repairTipStyle);
  });
}

function scheduleCloseRepairTip() {
  if (repairTipCloseTimer) clearTimeout(repairTipCloseTimer);
  repairTipCloseTimer = setTimeout(() => {
    showRepairTip.value = false;
  }, 120);
}

function toggleRepairTip() {
  if (showRepairTip.value) {
    showRepairTip.value = false;
  } else {
    void openRepairTip();
  }
}

async function openVirtualKeyboardTip() {
  if (virtualKeyboardTipCloseTimer) {
    clearTimeout(virtualKeyboardTipCloseTimer);
    virtualKeyboardTipCloseTimer = null;
  }
  virtualKeyboardTipStyle.value = { position: "fixed", top: "0px", left: "0px", visibility: "hidden", zIndex: "2000" };
  showVirtualKeyboardTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(virtualKeyboardInfoBtn.value, virtualKeyboardTipEl.value, virtualKeyboardTipStyle);
  });
}

function scheduleCloseVirtualKeyboardTip() {
  if (virtualKeyboardTipCloseTimer) clearTimeout(virtualKeyboardTipCloseTimer);
  virtualKeyboardTipCloseTimer = setTimeout(() => { showVirtualKeyboardTip.value = false; }, 120);
}

function toggleVirtualKeyboardTip() {
  if (showVirtualKeyboardTip.value) showVirtualKeyboardTip.value = false;
  else void openVirtualKeyboardTip();
}

async function openAtvvTip() {
  if (atvvTipCloseTimer) {
    clearTimeout(atvvTipCloseTimer);
    atvvTipCloseTimer = null;
  }
  atvvTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showAtvvTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(atvvInfoBtn.value, atvvTipEl.value, atvvTipStyle);
  });
}

function scheduleCloseAtvvTip() {
  if (atvvTipCloseTimer) clearTimeout(atvvTipCloseTimer);
  atvvTipCloseTimer = setTimeout(() => {
    showAtvvTip.value = false;
  }, 120);
}

function toggleAtvvTip() {
  if (showAtvvTip.value) {
    showAtvvTip.value = false;
  } else {
    void openAtvvTip();
  }
}

async function openRestartTip() {
  if (restartTipCloseTimer) {
    clearTimeout(restartTipCloseTimer);
    restartTipCloseTimer = null;
  }
  restartTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showRestartTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(restartInfoBtn.value, restartTipEl.value, restartTipStyle);
  });
}

function scheduleCloseRestartTip() {
  if (restartTipCloseTimer) clearTimeout(restartTipCloseTimer);
  restartTipCloseTimer = setTimeout(() => {
    showRestartTip.value = false;
  }, 120);
}

function toggleRestartTip() {
  if (showRestartTip.value) {
    showRestartTip.value = false;
  } else {
    void openRestartTip();
  }
}

function onViewportChange() {
  if (showVoiceShortcutTip.value) {
    placeInfoTip(voiceInfoBtn.value, voiceTipEl.value, voiceTipStyle);
  }
  if (showGainTip.value) {
    placeInfoTip(gainInfoBtn.value, gainTipEl.value, gainTipStyle);
  }
  if (showTriggerTip.value) {
    placeInfoTip(triggerInfoBtn.value, triggerTipEl.value, triggerTipStyle);
  }
  if (showRepairTip.value) {
    placeInfoTip(repairInfoBtn.value, repairTipEl.value, repairTipStyle);
  }
  if (showVirtualKeyboardTip.value) {
    placeInfoTip(virtualKeyboardInfoBtn.value, virtualKeyboardTipEl.value, virtualKeyboardTipStyle);
  }
  if (showAtvvTip.value) {
    placeInfoTip(atvvInfoBtn.value, atvvTipEl.value, atvvTipStyle);
  }
  if (showRestartTip.value) {
    placeInfoTip(restartInfoBtn.value, restartTipEl.value, restartTipStyle);
  }
}
const host = ref<HostStatus>({
  bridge_alive: false,
  audio_alive: false,
  cable_ready: false,
  atvv_ok: false,
  status_text: "正在启动",
  detail: "",
  tone: "warn",
  items: [
    { id: "cable", label: "虚拟声卡", state_label: "检测中", tone: "warn" },
    { id: "audio", label: "语音路由", state_label: "检测中", tone: "warn" },
    { id: "bridge", label: "按键桥接", state_label: "检测中", tone: "warn" },
  ],
});

/** C1：桥接在跑且 ATVV 未订阅 → 音频信号旁红字 */
const showAtvvFailLabel = computed(
  () => isDeviceConnected.value && Boolean(host.value.bridge_alive) && !(voiceMeter.value.atvvOk || host.value.atvv_ok)
);

const connectionPresentation = computed(() => connectionStatusPresentation(device.value.status));
const isDeviceConnected = computed(() => connectionPresentation.value.tone === "connected");
const connectedName = computed(() => connectedDeviceName(device.value.status, device.value.device_name));
const deviceDisplayName = computed(() => connectedName.value ?? t("status.noDeviceConnected"));
const deviceModelLabel = computed(() => {
  if (connectedName.value) return t("status.connected");
  return t(connectionPresentation.value.labelKey);
});
const batteryLabel = computed(() =>
  device.value.battery_level != null ? `${device.value.battery_level}%` : "—"
);
const audioSignalLabel = computed(() => {
  if (!isDeviceConnected.value) return "—";
  if (showAtvvFailLabel.value) return t("dashboard.atvvDisconnected");
  if (voiceMeter.value.bleState === "receiving") return t("dashboard.receiving");
  if (voiceMeter.value.bleState === "session") return t("dashboard.voiceSession");
  if (host.value.audio_alive) return t("dashboard.stable");
  return t("dashboard.noSignal");
});
const servicesSummary = computed(() => {
  if (host.value.items.some((item) => item.tone === "error")) return t("dashboard.needsAttention");
  if (host.value.items.some((item) => item.tone === "warn")) return t("dashboard.checking");
  return t("dashboard.normal");
});

function hostItemLabel(id: string) {
  return t(`dashboard.${id === "cable" ? "cable" : id === "audio" ? "route" : id === "injection" ? "injection" : "bridge"}`);
}

function hostItemState(item: HostStatusItem) {
  if (item.id === "cable") return t(item.tone === "ok" ? "dashboard.installed" : "common.unknown");
  if (item.id === "audio") return t(item.tone === "ok" ? "dashboard.running" : "dashboard.stopped");
  if (item.id === "injection") {
    if (item.state_label.includes("SendInput")) return t("dashboard.sendInputFallback");
    return t(item.tone === "ok" ? "dashboard.hardwareKeyboard" : "dashboard.inputWaiting");
  }
  return t(item.tone === "ok" ? "dashboard.listening" : "dashboard.notStarted");
}

function statusLightClass(
  tone: HostStatusItem["tone"] | "success" | "info" | "error" | "connected" | "connecting" | "disconnected",
) {
  if (tone === "ok" || tone === "success" || tone === "connected") return "is-success";
  if (tone === "warn" || tone === "connecting") return "is-warning";
  if (tone === "info") return "is-info";
  return "is-danger";
}

function bridgeStatusLabel() {
  return t(connectionPresentation.value.labelKey);
}

function activityTone(text: string): "success" | "info" | "error" {
  if (/失败|错误|异常|未连接|断开|未检测/.test(text)) return "error";
  if (/正在|检测中|等待|修复中|连接中/.test(text)) return "info";
  return "success";
}

const voiceShortcutEnabled = computed({
  get: () => config.value?.voice_shortcut_enabled !== false,
  set: (v: boolean) => {
    if (!config.value) return;
    config.value.voice_shortcut_enabled = v;
    void persistVoiceSettings();
  },
});

const GAIN_MIN = -12;
const GAIN_MAX = 30;
const GAIN_STEP = 1;

const gainDb = computed({
  get: () => config.value?.gain_db ?? 10,
  set: (v: number | string) => {
    if (!config.value) return;
    const n = typeof v === "number" ? v : Number(v);
    if (Number.isNaN(n)) return;
    config.value.gain_db = Math.min(GAIN_MAX, Math.max(GAIN_MIN, n));
    void persistVoiceSettings();
  },
});

function stepGain(delta: number) {
  gainDb.value = Math.min(GAIN_MAX, Math.max(GAIN_MIN, gainDb.value + delta));
}

function clampGainOnBlur() {
  if (!config.value) return;
  const n = Number(config.value.gain_db);
  if (Number.isNaN(n)) {
    config.value.gain_db = 10;
  } else {
    config.value.gain_db = Math.min(GAIN_MAX, Math.max(GAIN_MIN, n));
  }
  void persistVoiceSettings();
}

async function persistVoiceSettings() {
  if (!config.value) return;
  await configStore.saveConfig(type, normalizeVoiceShortcutConfig(config.value));
}

async function applyImePreset(preset: ImePreset) {
  if (!config.value) return;
  const definition = IME_PRESETS[preset];
  const next = applyImePresetConfig(config.value, preset);
  Object.assign(config.value, next);
  await configStore.saveConfig(type, next);
  setupApplyHint.value = definition.applyHint;
  prependLog(definition.logMessage);
  window.setTimeout(() => {
    if (setupApplyHint.value === definition.applyHint) setupApplyHint.value = "";
  }, 4000);
}

let hostPollTimer: ReturnType<typeof setInterval> | null = null;
let devicePollTimer: ReturnType<typeof setInterval> | null = null;

function itemToneClass(tone: string): string {
  if (tone === "ok") return "ok";
  if (tone === "warn") return "warn";
  return "error";
}

interface LogEntry {
  id: number;
  time: string;
  text: string;
}

const logs = ref<LogEntry[]>([]);
let logSeq = 0;
let unlistenKey: UnlistenFn | null = null;
let unlistenMeter: UnlistenFn | null = null;
let unlistenAtvvRepair: UnlistenFn | null = null;
let unlistenAtvvCancel: UnlistenFn | null = null;

function formatTime(d = new Date()): string {
  return d.toLocaleTimeString("zh-CN", { hour12: false });
}

function prependLog(text: string) {
  logs.value.unshift({
    id: ++logSeq,
    time: formatTime(),
    text,
  });
  if (logs.value.length > 80) {
    logs.value.length = 80;
  }
}

function resolveKeyLabel(buttonId: string): string {
  const aliases = config.value?.button_aliases;
  if (aliases && aliases[buttonId]) return aliases[buttonId];
  const fallback: Record<string, string> = {
    power: "电源",
    volume_up: "音量+",
    volume_down: "音量-",
    up: "上",
    down: "下",
    left: "左",
    right: "右",
    dpad_up: "上",
    dpad_down: "下",
    dpad_left: "左",
    dpad_right: "右",
    ok: "确认",
    back: "返回",
    home: "主页",
    menu: "菜单",
    mic: "语音",
    voice: "语音",
    volume_mute: "静音",
    mute: "静音",
    tv: "TV",
  };
  return fallback[buttonId] || buttonId;
}

function bindingAliases(buttonId: string): string[] {
  switch (buttonId) {
    case "mic":
    case "voice":
      return ["mic", "voice"];
    case "mute":
    case "volume_mute":
      return ["mute", "volume_mute"];
    case "up":
    case "dpad_up":
      return ["up", "dpad_up"];
    case "down":
    case "dpad_down":
      return ["down", "dpad_down"];
    case "left":
    case "dpad_left":
      return ["left", "dpad_left"];
    case "right":
    case "dpad_right":
      return ["right", "dpad_right"];
    default:
      return [buttonId];
  }
}

function vkDisplayName(vk: number): string {
  const map: Record<number, string> = {
    0x08: "Backspace",
    0x09: "Tab",
    0x0d: "Enter",
    0x1b: "Esc",
    0x20: "Space",
    0x25: "←",
    0x26: "↑",
    0x27: "→",
    0x28: "↓",
    0x10: "左 Shift",
    0xa0: "左 Shift",
    0xa1: "右 Shift",
    0x11: "左 Ctrl",
    0xa2: "左 Ctrl",
    0xa3: "右 Ctrl",
    0x12: "左 Alt",
    0xa4: "左 Alt",
    0xa5: "右 Alt",
    0x5b: "左 Win",
    0x5c: "右 Win",
    0xaf: "Vol+",
    0xae: "Vol-",
    0xad: "Mute",
  };
  if (map[vk]) return map[vk];
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
  if (vk >= 0x70 && vk <= 0x7b) return `F${vk - 0x6f}`;
  return `VK_0x${vk.toString(16).toUpperCase()}`;
}

function resolveMappedActionLabel(buttonId: string): string {
  const bindings = config.value?.button_bindings;
  if (!bindings) return "未绑定";
  let action = bindings[buttonId];
  if (!action) {
    for (const alt of bindingAliases(buttonId)) {
      if (bindings[alt]) {
        action = bindings[alt];
        break;
      }
    }
  }
  if (!action || action.type === "None") return "未绑定";
  if (action.type === "SingleKey") return vkDisplayName(Number(action.value));
  if (action.type === "ComboKey") {
    const arr = Array.isArray(action.value) ? action.value : [];
    return arr.map((v) => vkDisplayName(Number(v))).join(" + ");
  }
  if (action.type === "TextInput") return `文字: ${action.value}`;
  if (action.type === "LaunchApp") return `启动: ${action.value}`;
  return "—";
}

function formatKeyEventLine(
  phase: "down" | "up",
  remoteLabel: string,
  mappedLabel: string | null
): string {
  const phaseLabel = phase === "up" ? "抬起" : "按下";
  if (mappedLabel) {
    return `${phaseLabel} ${remoteLabel}：${mappedLabel}`;
  }
  return `${phaseLabel} ${remoteLabel}`;
}

function showMappingFlash(
  remoteLabel: string,
  mappedLabel: string | null,
  phase: "down" | "up" = "down"
) {
  lastMappingFlash.value = {
    seq: ++mappingFlashSeq,
    phase,
    remote: remoteLabel,
    mapped: mappedLabel,
  };
  if (mappingFlashClearTimer) clearTimeout(mappingFlashClearTimer);
  mappingFlashClearTimer = setTimeout(() => {
    lastMappingFlash.value = null;
    mappingFlashClearTimer = null;
  }, 4500);
}

async function refreshHost() {
  try {
    host.value = await invoke<HostStatus>("get_xiaomi_host_status");
  } catch (e) {
    host.value = {
      bridge_alive: false,
      audio_alive: false,
      cable_ready: false,
      atvv_ok: false,
      status_text: "桥接未运行",
      detail: String(e),
      tone: "error",
      items: [
        { id: "cable", label: "虚拟声卡", state_label: "未知", tone: "error" },
        { id: "audio", label: "语音路由", state_label: "未知", tone: "error" },
        { id: "bridge", label: "按键桥接", state_label: "未启动", tone: "error" },
      ],
    };
  }
}

async function restartBridge() {
  if (repairBusy.value) return;
  restarting.value = true;
  try {
    await invoke("restart_xiaomi_bridge");
    await refreshHost();
  } catch (e) {
    host.value = {
      ...host.value,
      status_text: "重启失败",
      detail: String(e),
      tone: "error",
    };
  } finally {
    restarting.value = false;
  }
}

interface AtvvRepairResult {
  phase: string;
  message: string;
  atvvOk: boolean;
  hadConflicts: boolean;
  resultCode?: string;
}

async function repairAtvv() {
  if (repairBusy.value) return;
  atvvRepairing.value = true;
  let awaitingClear = false;
  try {
    const result = await invoke<AtvvRepairResult>("repair_xiaomi_atvv", {
      force: false,
    });
    awaitingClear = result.phase === "awaiting_conflict_clear";
    host.value = {
      ...host.value,
      status_text: result.atvvOk
        ? "ATVV 已修复"
        : awaitingClear
          ? "等待清理占用"
          : "ATVV 修复未完成",
      detail: result.message,
      tone: result.atvvOk ? "ok" : awaitingClear ? "warn" : "error",
    };
    if (awaitingClear) {
      return;
    }
    await refreshHost();
  } catch (e) {
    host.value = {
      ...host.value,
      status_text: "ATVV 修复失败",
      detail: String(e),
      tone: "error",
    };
  } finally {
    if (!awaitingClear) {
      atvvRepairing.value = false;
    }
  }
}

interface VirtualKeyboardRepairResult {
  ready: boolean;
  restartRequired: boolean;
  message: string;
}

async function repairVirtualKeyboard() {
  if (repairBusy.value) return;
  virtualKeyboardRepairing.value = true;
  try {
    const result = await invoke<VirtualKeyboardRepairResult>("repair_xiaomi_virtual_keyboard");
    const message = result.message || "虚拟键盘修复已完成。";
    prependLog(message);
    host.value = {
      ...host.value,
      status_text: result.ready ? "虚拟键盘已修复" : result.restartRequired ? "虚拟键盘待重启" : "虚拟键盘未就绪",
      detail: message,
      tone: result.ready ? "ok" : result.restartRequired ? "warn" : "error",
    };
  } catch (error) {
    const message = `虚拟键盘修复失败: ${error}`;
    prependLog(message);
    host.value = { ...host.value, status_text: "虚拟键盘修复失败", detail: message, tone: "error" };
  } finally {
    virtualKeyboardRepairing.value = false;
  }
}

async function openLogs() {
  showLogModal.value = true;
  logCopyHint.value = "";
  selectedLogFile.value = "";
  await loadLog();
}

async function loadLog(fileName = selectedLogFile.value) {
  logLoading.value = true;
  try {
    const result = await invoke<{
      path: string;
      content: string;
      files: Array<{ name: string; size: number; current: boolean }>;
      writeError?: string | null;
    }>("get_app_log", fileName ? { fileName } : undefined);
    logPath.value = result.path || "";
    logFiles.value = result.files || [];
    selectedLogFile.value = fileName || result.files?.find((file) => file.current)?.name || "";
    logWriteError.value = result.writeError || "";
    logText.value = result.content?.trim()
      ? result.content
      : "（暂无日志）";
  } catch (e) {
    logText.value = `读取日志失败: ${e}`;
    logPath.value = "";
    logWriteError.value = "";
  } finally {
    logLoading.value = false;
  }
}

async function copyLog() {
  try {
    await navigator.clipboard.writeText(logText.value || "");
    logCopyHint.value = "已复制";
    setTimeout(() => {
      logCopyHint.value = "";
    }, 1500);
  } catch (e) {
    logCopyHint.value = `复制失败: ${e}`;
  }
}

async function openLogExternally() {
  try {
    await invoke("open_app_log", selectedLogFile.value ? { fileName: selectedLogFile.value } : undefined);
  } catch (e) {
    logCopyHint.value = `打开失败: ${e}`;
  }
}

interface VoiceEnvActionResult {
  ok: boolean;
  ready: boolean;
  needsChoice: boolean;
  needsReboot: boolean;
  message: string;
  reportPath?: string | null;
  resultCode?: string;
}

async function voiceDetectAndRepair() {
  if (repairBusy.value) return;
  voiceRepairing.value = true;
  showVoiceChoice.value = false;
  try {
    const result = await invoke<VoiceEnvActionResult>("check_xiaomi_voice_env");
    if (result.needsChoice) {
      voiceChoiceMsg.value = result.message;
      showVoiceChoice.value = true;
      return;
    }
    host.value = {
      ...host.value,
      detail: result.message,
      tone: result.ready ? "ok" : result.needsReboot ? "warn" : "error",
    };
    prependLog(result.message);
    await refreshHost();
  } catch (e) {
    const msg = `虚拟声卡检测失败: ${e}`;
    prependLog(msg);
    host.value = { ...host.value, detail: msg, tone: "error" };
  } finally {
    voiceRepairing.value = false;
  }
}

async function chooseVoiceSource(source: "embedded" | "download_page" | "download_zip") {
  if (repairBusy.value && !voiceRepairing.value) return;
  voiceRepairing.value = true;
  showVoiceChoice.value = false;
  try {
    const result = await invoke<VoiceEnvActionResult>("repair_xiaomi_voice_env", {
      source,
    });
    host.value = {
      ...host.value,
      detail: result.message,
      tone: result.ready ? "ok" : result.ok ? "warn" : "error",
    };
    prependLog(result.message);
    await refreshHost();
  } catch (e) {
    const msg = `语音修复失败: ${e}`;
    prependLog(msg);
    host.value = { ...host.value, detail: msg, tone: "error" };
  } finally {
    voiceRepairing.value = false;
  }
}

onMounted(async () => {
  prependLog("日志区准备就绪");
  await Promise.all([
    bridge.refreshStatus(type),
    configStore.loadConfig(type),
    refreshHost(),
    invoke("get_xiaomi_voice_meter")
      .then((s) => applyVoiceMeter(s as Record<string, unknown>))
      .catch(() => undefined),
  ]);
  hostPollTimer = setInterval(refreshHost, 1000);
  // 持续拉取设备信息（含电量），避免必须切页才刷新
  devicePollTimer = setInterval(() => {
    void bridge.refreshStatus(type);
  }, 1500);
  window.addEventListener("resize", onViewportChange);
  window.addEventListener("scroll", onViewportChange, true);

  try {
    unlistenKey = await listen<{
      buttonId?: string;
      label?: string;
      message?: string;
      phase?: string;
    }>("xiaomi-key", (event) => {
      const p = event.payload;
      if (p.message) {
        prependLog(p.message);
        showMappingFlash(p.message, null, "down");
        if (p.message.startsWith("电量")) {
          void bridge.refreshStatus(type);
        }
        return;
      }
      const id = p.buttonId || "unknown";
      const label = p.label || resolveKeyLabel(id);
      const phase: "down" | "up" = p.phase === "up" ? "up" : "down";
      // D1：语音映射关闭时只显示按下/抬起，不写映射段
      const lineMapped = null;
      showMappingFlash(label, lineMapped, phase);
      prependLog(formatKeyEventLine(phase, label, lineMapped));
    });
  } catch (e) {
    console.warn("listen xiaomi-key failed:", e);
  }

  try {
    unlistenMeter = await listen<Record<string, unknown>>("xiaomi-voice-meter", (event) => {
      applyVoiceMeter(event.payload);
    });
  } catch (e) {
    console.warn("listen xiaomi-voice-meter failed:", e);
  }

  try {
    unlistenAtvvRepair = await listen<{ ok?: boolean; message?: string }>(
      "xiaomi-atvv-repair-result",
      async (event) => {
        const p = event.payload || {};
        host.value = {
          ...host.value,
          status_text: p.ok ? "ATVV 已修复" : "ATVV 修复未完成",
          detail: p.message || "",
          tone: p.ok ? "ok" : "error",
        };
        atvvRepairing.value = false;
        await refreshHost();
      },
    );
  } catch (e) {
    console.warn("listen xiaomi-atvv-repair-result failed:", e);
  }

  try {
    unlistenAtvvCancel = await listen<{ message?: string }>(
      "xiaomi-atvv-repair-cancelled",
      (event) => {
        host.value = {
          ...host.value,
          status_text: "ATVV 修复已取消",
          detail: event.payload?.message || "已取消修复",
          tone: "warn",
        };
        atvvRepairing.value = false;
      },
    );
  } catch (e) {
    console.warn("listen xiaomi-atvv-repair-cancelled failed:", e);
  }
});

onUnmounted(() => {
  unlistenKey?.();
  unlistenMeter?.();
  unlistenAtvvRepair?.();
  unlistenAtvvCancel?.();
  if (hostPollTimer) clearInterval(hostPollTimer);
  if (devicePollTimer) clearInterval(devicePollTimer);
  if (voiceTipCloseTimer) clearTimeout(voiceTipCloseTimer);
  if (gainTipCloseTimer) clearTimeout(gainTipCloseTimer);
  if (triggerTipCloseTimer) clearTimeout(triggerTipCloseTimer);
  if (repairTipCloseTimer) clearTimeout(repairTipCloseTimer);
  if (virtualKeyboardTipCloseTimer) clearTimeout(virtualKeyboardTipCloseTimer);
  if (atvvTipCloseTimer) clearTimeout(atvvTipCloseTimer);
  if (restartTipCloseTimer) clearTimeout(restartTipCloseTimer);
  if (mappingFlashClearTimer) clearTimeout(mappingFlashClearTimer);
  window.removeEventListener("resize", onViewportChange);
  window.removeEventListener("scroll", onViewportChange, true);
});

watch(
  () => device.value.status,
  (status, prev) => {
    if (status === prev) return;
    if (status === "Connected") {
      const name = device.value.device_name || "MI RC";
      prependLog(`已连接 ${name}`);
    } else if (status === "Connecting") {
      prependLog("正在连接...");
    } else if (status === "Disconnected") {
      prependLog("已断开");
    } else if (status.startsWith("Error")) {
      prependLog(bridge.statusLabel(status));
    }
  }
);

function toggleConnection() {
  if (device.value.status === "Connected") {
    bridge.stopBridge(type);
  } else {
    bridge.startBridge(type);
  }
}

function closeTransientUi() {
  showVoiceChoice.value = false;
  showLogModal.value = false;
  showSetupTips.value = false;
  showVoiceShortcutTip.value = false;
  showGainTip.value = false;
  showTriggerTip.value = false;
  showRepairTip.value = false;
  showVirtualKeyboardTip.value = false;
  showAtvvTip.value = false;
  showRestartTip.value = false;
}

function navigateSection(target: "home" | "mapping") {
  const name = target === "mapping" ? "xiaomi-mapping" : "xiaomi";
  if (route.name === name) return;
  closeTransientUi();
  void router.push({ name });
}

watch(
  () => route.name,
  () => closeTransientUi()
);

</script>

<template>
  <div class="page">
    <header v-if="!isMappingPage" class="dashboard-head">
      <div>
        <h1>{{ t("dashboard.title") }}</h1>
      </div>
      <DeviceStatus
        :status="device.status"
        :loading="bridge.loading[type]"
        @toggle="toggleConnection"
      />
    </header>

    <div v-if="!isMappingPage" class="overview-row">
      <div class="overview-left">
        <section :class="['card', 'device-overview', `connection-${connectionPresentation.tone}`]">
          <div class="device-primary">
            <div class="remote-product-frame" aria-hidden="true">
              <img class="remote-product-image" :src="remoteProductImage" alt="" />
            </div>
            <div class="device-meta">
              <div :class="['model', connectionPresentation.tone]"><span class="status-dot status-light" />{{ deviceModelLabel }}</div>
              <h2>{{ deviceDisplayName }}</h2>
              <div class="tag-row">
                <span class="mini-tag">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                    <path d="m12 2 5 5-4 4 4 4-5 5V2Z" />
                    <path d="m7 7 10 10M7 17 17 7" />
                  </svg>
                  {{ t("dashboard.ble") }}
                </span>
                <span class="mini-tag">{{ t("dashboard.lowEnergy") }}</span>
              </div>
            </div>
          </div>
          <div class="device-data">
            <div class="data-item">
              <span class="label">{{ t("dashboard.battery") }}</span>
              <strong>
                <span class="battery" :class="{ 'is-charging': device.battery_charging === true }">
                  <span class="battery-fill" :style="{ width: `${device.battery_level ?? 0}%` }" />
                  <span class="battery-flow" aria-hidden="true" />
                  <svg class="battery-bolt" viewBox="0 0 10 14" aria-hidden="true">
                    <path d="M5.9.7 1.2 7h3L3.7 13.3 8.9 6.4H5.7L5.9.7Z" />
                  </svg>
                </span>
                {{ isDeviceConnected ? batteryLabel : "—" }}
              </strong>
            </div>
            <div
              class="data-item data-item-audio"
              :class="{
                'is-session': voiceMeter.bleState === 'session',
                'is-receiving': voiceMeter.bleState === 'receiving',
                'is-atvv-error': showAtvvFailLabel,
              }"
              title="遥控器 BLE 解码后的 PCM"
            >
              <span class="label">{{ t("dashboard.audio") }}</span>
              <strong>{{ audioSignalLabel }}</strong>
              <div class="ble-wave" aria-hidden="true">
                <span
                  v-for="(v, i) in voiceMeter.waveform"
                  :key="i"
                  class="ble-wave-bar"
                  :style="{ height: `${Math.max(8, Math.round(v * 100))}%` }"
                />
              </div>
            </div>
            <div class="data-item">
              <span class="label">{{ t("dashboard.address") }}</span>
              <strong>{{ isDeviceConnected ? (device.device_address || "—") : "—" }}</strong>
            </div>
            <div class="data-item">
              <span class="label">{{ t("dashboard.synced") }}</span>
              <strong>{{ isDeviceConnected ? t("dashboard.justNow") : "—" }}</strong>
            </div>
          </div>
        </section>

        <section class="card host-card">
          <div class="section-title">
            <h3>{{ t("dashboard.services") }}</h3>
            <span>{{ servicesSummary }}</span>
          </div>
          <div class="host-status-row" role="list" :aria-label="t('dashboard.services')">
            <div
              v-for="item in host.items"
              :key="item.id"
              class="host-status-item"
              :class="[`service-${item.id}`, { 'host-status-cable': item.id === 'cable' }]"
              role="listitem"
            >
              <span class="host-item-label">{{ hostItemLabel(item.id) }}</span>
              <div class="host-state-row">
                <span class="host-item-state" :class="itemToneClass(item.tone)">
                  <span class="status-light" :class="statusLightClass(item.tone)" aria-hidden="true" />
                  {{ hostItemState(item) }}
                </span>
                <div
                  v-if="item.id === 'cable'"
                  class="cable-meter"
                  :class="{ active: voiceMeter.cableActive }"
                  :title="cableActivityLabel"
                  aria-hidden="true"
                >
                  <span class="cable-meter-track">
                    <span
                      class="cable-meter-fill"
                      :style="{ width: `${Math.round(voiceMeter.cableLevel * 100)}%` }"
                    />
                  </span>
                </div>
              </div>
            </div>
          </div>
          <p v-if="host.detail" class="host-detail">{{ host.detail }}</p>
        </section>

        <section class="card quick-section">
          <div class="section-title">
            <h3>{{ t("dashboard.actions") }}</h3>
            <span>{{ t("dashboard.manageVoice") }}</span>
          </div>
          <div class="host-actions">
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="repairBusy"
                @click="voiceDetectAndRepair"
              >
                {{ voiceRepairing ? t("common.processing") : t("dashboard.repairAudioShort") }}
              </button>
              <button
                ref="repairInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showRepairTip"
                aria-label="虚拟声卡检测与修复说明"
                @mouseenter="openRepairTip"
                @mouseleave="scheduleCloseRepairTip"
                @focus="openRepairTip"
                @blur="scheduleCloseRepairTip"
                @click.stop="toggleRepairTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showRepairTip"
                  ref="repairTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="repairTipStyle"
                  @mouseenter="openRepairTip"
                  @mouseleave="scheduleCloseRepairTip"
                >
                  <p class="tip-lead">
                    用来检查并修好电脑上的语音通路（VB-CABLE 虚拟声卡），让遥控器麦克风声音能进系统、供输入法听写。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>检测 VB-CABLE 是否已安装、是否可用</li>
                      <li>已装好则尝试自动修复配置</li>
                      <li>未安装时可选用内嵌驱动，或下载官网最新版</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>首次使用语音，或重装系统 / 换电脑后</li>
                      <li>按语音键没声音、输入法听不到遥控器</li>
                      <li>提示未检测到 VB-CABLE、语音环境异常时</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    平时语音正常就不必反复点；装完驱动若提示重启电脑，按提示重启后再试。
                  </p>
                </div>
              </Teleport>
              <span class="quick-action-hint">{{ t("dashboard.repairAudioHint") }}</span>
            </div>
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="repairBusy"
                @click="repairVirtualKeyboard"
              >
                {{ virtualKeyboardRepairing ? t("common.processing") : t("dashboard.repairVirtualKeyboard") }}
              </button>
              <button
                ref="virtualKeyboardInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showVirtualKeyboardTip"
                aria-label="修复虚拟键盘说明"
                @mouseenter="openVirtualKeyboardTip"
                @mouseleave="scheduleCloseVirtualKeyboardTip"
                @focus="openVirtualKeyboardTip"
                @blur="scheduleCloseVirtualKeyboardTip"
                @click.stop="toggleVirtualKeyboardTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showVirtualKeyboardTip"
                  ref="virtualKeyboardTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="virtualKeyboardTipStyle"
                  @mouseenter="openVirtualKeyboardTip"
                  @mouseleave="scheduleCloseVirtualKeyboardTip"
                >
                  <p class="tip-lead">
                    用来修复遥控器发送给输入法的“虚拟硬件键盘”。豆包、微信等输入法会忽略普通模拟按键时，需要它来发送真正的键盘快捷键。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>重新部署 WinUHid 虚拟键盘组件</li>
                      <li>重新安装内嵌的虚拟键盘驱动</li>
                      <li>修复完成后，按遥控器语音键会按硬件键盘方式发送已配置的快捷键</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>网页键盘测试能看到按键，但豆包、微信或其它输入法没有反应</li>
                      <li>日志出现 WinUHid unavailable 或虚拟键盘未就绪</li>
                      <li>重装应用、系统更新或驱动异常后，输入法快捷键失效</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    点击后会出现 Windows 管理员确认。修复期间不要退出应用；若完成后仍提示未就绪，请重启 Windows 再测试。这和“声卡检测与修复”不同：声卡负责传送声音，虚拟键盘负责唤起输入法。
                  </p>
                </div>
              </Teleport>
              <span class="quick-action-hint">{{ t("dashboard.repairVirtualKeyboardHint") }}</span>
            </div>
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="repairBusy"
                @click="repairAtvv"
              >
                {{ atvvRepairing ? t("common.processing") : t("dashboard.repairAtvvShort") }}
              </button>
              <button
                ref="atvvInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showAtvvTip"
                aria-label="修复 ATVV 连接说明"
                @mouseenter="openAtvvTip"
                @mouseleave="scheduleCloseAtvvTip"
                @focus="openAtvvTip"
                @blur="scheduleCloseAtvvTip"
                @click.stop="toggleAtvvTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showAtvvTip"
                  ref="atvvTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="atvvTipStyle"
                  @mouseenter="openAtvvTip"
                  @mouseleave="scheduleCloseAtvvTip"
                >
                  <p class="tip-lead">
                    修好遥控器到电脑的「语音专用蓝牙通道」（ATVV）。通道正常后，按住语音键才有绿色音频波动，也不会误触发系统 F5 插入日期。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>检查是否有其它遥控桥接软件占用</li>
                      <li>暂停 HID Tap 后软重启连接，并重新订阅语音通道</li>
                      <li>有占用时会先弹窗让你结束相关进程，再继续修复</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>「音频信号」旁出现红字「ATVV 未连接」</li>
                      <li>按住语音键说话，绿色波形一直不动</li>
                      <li>按语音键后记事本等处插入了日期时间</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    平时语音和波形都正常就不必点。这和「虚拟声卡检测与修复」不同：那边管电脑声卡，这边管遥控器蓝牙语音通道。
                  </p>
                </div>
              </Teleport>
              <span class="quick-action-hint">{{ t("dashboard.repairAtvvHint") }}</span>
            </div>
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="repairBusy"
                @click="restartBridge"
              >
                {{ restarting ? t("common.processing") : t("dashboard.restartBridge") }}
              </button>
              <button
                ref="restartInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showRestartTip"
                aria-label="重启桥接说明"
                @mouseenter="openRestartTip"
                @mouseleave="scheduleCloseRestartTip"
                @focus="openRestartTip"
                @blur="scheduleCloseRestartTip"
                @click.stop="toggleRestartTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showRestartTip"
                  ref="restartTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="restartTipStyle"
                  @mouseenter="openRestartTip"
                  @mouseleave="scheduleCloseRestartTip"
                >
                  <p class="tip-lead">
                    软重启「与遥控器的蓝牙连接」，按最新配置重新连上；无需退出整个应用。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>停止并重新拉起蓝牙 / ATVV 连接</li>
                      <li>按当前映射、增益等配置重新尝试连接遥控器</li>
                      <li>语音路由异常时也会顺带尝试拉起</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>改了增益、映射等设置后不生效</li>
                      <li>状态显示异常、按键失灵、连上又掉线</li>
                      <li>长时间不用后突然不响应，想快速恢复</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    返回 / 音量专用通道会尽量保持，一般不必为此反复重启。若仍无效，可再试「虚拟声卡检测与修复」，或查看日志。
                  </p>
                </div>
              </Teleport>
              <span class="quick-action-hint">{{ t("dashboard.restartBridgeHint") }}</span>
            </div>
            <button class="btn btn-secondary quick-log-trigger" type="button" @click="openLogs">
              日志
            </button>
            <button
              class="btn btn-secondary quick-input-settings"
              type="button"
              @click="showSetupTips = true"
            >
              <span>{{ t("dashboard.inputSettings") }}</span>
              <small>{{ t("dashboard.inputSettingsHint") }}</small>
            </button>
          </div>
        </section>
      </div>

      <aside class="log-aside">
        <section class="card activity-card">
          <div class="section-title">
            <h3>{{ t("dashboard.activity") }}</h3>
            <span class="live"><span class="status-dot status-light is-success" />{{ t("dashboard.live") }}</span>
          </div>
          <div class="timeline" aria-live="polite">
            <article
              v-for="entry in logs.slice(0, 4)"
              :key="entry.id"
              :class="['activity-event', activityTone(entry.text)]"
            >
              <span class="event-dot status-light" :class="statusLightClass(activityTone(entry.text))" aria-hidden="true" />
              <div class="event-content">
                <strong>{{ entry.text }}</strong>
                <time>{{ entry.time }}</time>
              </div>
            </article>
          </div>
          <div class="activity-footer">
            <button type="button" @click="openLogs">{{ t("dashboard.logs") }}</button>
          </div>
        </section>
      </aside>
    </div>

    <div class="page-body">
      <InputMethodSettingsDialog
        :open="showSetupTips"
        :config-ready="Boolean(config)"
        :saving="configStore.saving"
        :apply-hint="setupApplyHint"
        @close="showSetupTips = false"
        @apply="applyImePreset"
      />

      <div v-if="showLogModal" class="voice-modal-backdrop" @click.self="showLogModal = false">
        <div class="voice-modal log-modal" role="dialog" aria-modal="true">
          <h3>运行日志</h3>
          <p v-if="logPath" class="log-path">{{ logPath }}</p>
          <label v-if="logFiles.length" class="log-file-picker">
            <span>日志文件</span>
            <select v-model="selectedLogFile" :disabled="logLoading" @change="loadLog(selectedLogFile)">
              <option v-for="file in logFiles" :key="file.name" :value="file.name">
                {{ file.name }}{{ file.current ? "（当前）" : "" }} · {{ Math.ceil(file.size / 1024) }} KB
              </option>
            </select>
          </label>
          <p v-if="logWriteError" class="save-error" role="alert">日志写入异常：{{ logWriteError }}</p>
          <pre class="log-viewer">{{ logLoading ? "读取中…" : logText }}</pre>
          <div class="log-modal-actions">
            <button class="btn btn-primary" type="button" :disabled="logLoading" @click="copyLog">
              {{ logCopyHint || "复制" }}
            </button>
            <button class="btn btn-secondary" type="button" @click="openLogExternally">
              用记事本打开
            </button>
            <button class="btn btn-secondary" type="button" @click="showLogModal = false">
              关闭
            </button>
          </div>
        </div>
      </div>
      <div v-if="showVoiceChoice" class="voice-modal-backdrop" @click.self="showVoiceChoice = false">
        <div class="voice-modal" role="dialog" aria-modal="true">
          <h3>未检测到 VB-CABLE</h3>
          <p>{{ voiceChoiceMsg || "请选择安装方式：" }}</p>
          <div class="voice-modal-actions">
            <button
              class="btn btn-primary"
              type="button"
              :disabled="voiceRepairing"
              @click="chooseVoiceSource('embedded')"
            >
              使用内嵌驱动安装
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="voiceRepairing"
              @click="chooseVoiceSource('download_zip')"
            >
              下载最新驱动包
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="voiceRepairing"
              @click="chooseVoiceSource('download_page')"
            >
              打开官网说明
            </button>
            <button class="btn btn-secondary" type="button" @click="showVoiceChoice = false">
              取消
            </button>
          </div>
          <p class="voice-modal-note">
            内嵌为已校验的 VB-CABLE 4.5；安装时会弹出 Windows 管理员确认。官网下载适合需要更新版本时。
          </p>
        </div>
      </div>

      <section v-if="isMappingPage && config" class="mapping-page">
        <header class="mapping-page-head">
          <div>
            <h1>{{ t("mapping.title") }}</h1>
            <p class="mapping-subtitle">{{ t("mapping.subtitle") }}</p>
          </div>
          <div class="status-capsule mapping-head-actions" :aria-label="t('mapping.title')">
            <span
              :class="['mapping-status-pill', connectionPresentation.tone]"
              :title="connectionPresentation.detail || undefined"
              :aria-label="connectionPresentation.detail ? `${bridgeStatusLabel()}：${connectionPresentation.detail}` : bridgeStatusLabel()"
            >
              <span class="status-light" :class="statusLightClass(connectionPresentation.tone)" aria-hidden="true"></span>{{ bridgeStatusLabel() }}
            </span>
            <span class="mapping-save-state" :class="{ saving: configStore.saving }">
              <span aria-hidden="true">{{ configStore.saving ? '↻' : '✓' }}</span>
              {{ configStore.saving ? t('status.savingMapping') : t('status.autosave') }}
            </span>
          </div>
        </header>

        <section class="card mapping-layout">
        <div class="mapping-heading">
          <p
            v-if="lastMappingFlash"
            :key="lastMappingFlash.seq"
            class="mapping-flash"
            role="status"
            aria-live="polite"
          >
            <span class="mapping-flash-phase">{{
              lastMappingFlash.phase === "up" ? "抬起" : "按下"
            }}</span>
            <span class="mapping-flash-remote">{{ lastMappingFlash.remote }}</span>
            <template v-if="lastMappingFlash.mapped">
              <span class="mapping-flash-sep" aria-hidden="true">：</span>
              <span class="mapping-flash-mapped">{{ lastMappingFlash.mapped }}</span>
            </template>
          </p>
        </div>
        <div class="voice-toolbar" role="group" aria-label="语音听写设置">
          <div class="voice-toolbar-item">
            <span class="voice-toolbar-label">
              <strong>语音键发送映射按键</strong>
              <small>点击语音键时同步发送快捷键</small>
            </span>
            <label class="switch" title="点击语音键是否发送映射按键">
              <input
                type="checkbox"
                v-model="voiceShortcutEnabled"
                aria-label="点击语音键是否发送映射按键"
              />
              <span class="switch-slider" aria-hidden="true"></span>
            </label>
            <button
              ref="voiceInfoBtn"
              type="button"
              class="title-info voice-info"
              :aria-expanded="showVoiceShortcutTip"
              aria-label="语音映射按键说明"
              @mouseenter="openVoiceTip"
              @mouseleave="scheduleCloseVoiceTip"
              @focus="openVoiceTip"
              @blur="scheduleCloseVoiceTip"
              @click.stop="toggleVoiceTip"
            >
              <span class="title-info-icon" aria-hidden="true">i</span>
            </button>
            <Teleport to="body">
              <div
                v-if="showVoiceShortcutTip"
                ref="voiceTipEl"
                class="floating-info-tip voice-info-tip"
                role="tooltip"
                :style="voiceTipStyle"
                @mouseenter="openVoiceTip"
                @mouseleave="scheduleCloseVoiceTip"
              >
                <p class="tip-lead">
                  只管「按语音键时要不要发映射快捷键」。传声（VB-CABLE）不受此开关影响。
                </p>
                <div class="tip-block tip-on">
                  <div class="tip-badge">开</div>
                  <ul>
                    <li>声音送到电脑</li>
                    <li>按触发模式发送你设好的映射键</li>
                  </ul>
                  <p class="tip-aside">适合靠快捷键开/关的语音输入法。</p>
                </div>
                <div class="tip-block tip-off">
                  <div class="tip-badge">关</div>
                  <ul>
                    <li>声音照样送到电脑</li>
                    <li>不发送映射键（日志只记按下/抬起语音键）</li>
                  </ul>
                  <p class="tip-aside">听写需自行打开输入法语音。</p>
                </div>
              </div>
            </Teleport>
          </div>

          <div class="voice-toolbar-item">
            <span class="voice-toolbar-label">
              <strong>默认触发模式</strong>
              <small>语音快捷键的发送方式</small>
            </span>
            <select
              v-model="config.trigger_mode"
              class="form-select voice-toolbar-select"
              @change="persistVoiceSettings"
            >
              <option value="Toggle">点击</option>
              <option value="Hold">按住</option>
            </select>
            <button
              ref="triggerInfoBtn"
              type="button"
              class="title-info voice-info"
              :aria-expanded="showTriggerTip"
              aria-label="触发模式说明"
              @mouseenter="openTriggerTip"
              @mouseleave="scheduleCloseTriggerTip"
              @focus="openTriggerTip"
              @blur="scheduleCloseTriggerTip"
              @click.stop="toggleTriggerTip"
            >
              <span class="title-info-icon" aria-hidden="true">i</span>
            </button>
            <Teleport to="body">
              <div
                v-if="showTriggerTip"
                ref="triggerTipEl"
                class="floating-info-tip voice-info-tip"
                role="tooltip"
                :style="triggerTipStyle"
                @mouseenter="openTriggerTip"
                @mouseleave="scheduleCloseTriggerTip"
              >
                <p class="tip-lead">
                  快捷键跟随遥控器实际操作：点一下就点按，按住就按住。
                </p>
                <div class="tip-block tip-on">
                  <div class="tip-badge">点击</div>
                  <ul>
                    <li>短按语音键：点按一次映射快捷键</li>
                    <li>长按语音键：按住映射快捷键，松手释放</li>
                  </ul>
                  <p class="tip-aside">适合「点一下开/关」类输入法，也会正确处理长按。</p>
                </div>
                <div class="tip-block tip-off">
                  <div class="tip-badge">按住</div>
                  <ul>
                    <li>按下语音键：立刻按住映射快捷键并传声</li>
                    <li>松开语音键：释放快捷键并结束</li>
                  </ul>
                  <p class="tip-aside">适合「按住说话」类输入法。</p>
                </div>
              </div>
            </Teleport>
          </div>

          <div class="voice-toolbar-item">
            <span class="voice-toolbar-label">
              <strong>音量增益</strong>
              <small>语音输入的增益 dB</small>
            </span>
            <div class="number-stepper" role="group" aria-label="增益分贝">
              <button
                type="button"
                class="stepper-btn"
                aria-label="减小增益"
                :disabled="gainDb <= GAIN_MIN"
                @click="stepGain(-GAIN_STEP)"
              >
                −
              </button>
              <input
                type="number"
                class="gain-input"
                v-model.number="gainDb"
                :min="GAIN_MIN"
                :max="GAIN_MAX"
                :step="GAIN_STEP"
                @blur="clampGainOnBlur"
              />
              <button
                type="button"
                class="stepper-btn"
                aria-label="增大增益"
                :disabled="gainDb >= GAIN_MAX"
                @click="stepGain(GAIN_STEP)"
              >
                +
              </button>
            </div>
            <button
              ref="gainInfoBtn"
              type="button"
              class="title-info voice-info"
              :aria-expanded="showGainTip"
              aria-label="增益说明"
              @mouseenter="openGainTip"
              @mouseleave="scheduleCloseGainTip"
              @focus="openGainTip"
              @blur="scheduleCloseGainTip"
              @click.stop="toggleGainTip"
            >
              <span class="title-info-icon" aria-hidden="true">i</span>
            </button>
            <Teleport to="body">
              <div
                v-if="showGainTip"
                ref="gainTipEl"
                class="floating-info-tip voice-info-tip"
                role="tooltip"
                :style="gainTipStyle"
                @mouseenter="openGainTip"
                @mouseleave="scheduleCloseGainTip"
              >
                <p class="tip-lead">
                  增益 = 把遥控器麦克风声音「放大或缩小」再送进电脑（VB-CABLE）。
                  只影响音量大小，不改变能不能说话。
                </p>
                <div class="tip-block tip-on">
                  <div class="tip-badge">怎么调</div>
                  <ul>
                    <li>听不清、识别漏字 → 调高（如 10 → 14）</li>
                    <li>破音、刺耳、识别乱 → 调低（如 10 → 6）</li>
                    <li>常用默认 <strong>10 dB</strong>；范围 -12 ～ 30</li>
                  </ul>
                </div>
                <div class="tip-block tip-off">
                  <div class="tip-badge">注意</div>
                  <ul>
                    <li>改完后请重新连接遥控器，或点「重启桥接」后生效</li>
                    <li>一次加减 2～4 dB 即可，别一次拉满</li>
                  </ul>
                </div>
                <p class="tip-foot">
                  简单记：声音太小就加，太吵就减。
                </p>
              </div>
            </Teleport>
          </div>
        </div>
        <KeyMappingStage
          :config="config"
          @save="(cfg) => configStore.saveConfig(type, cfg)"
        />
      </section>
      </section>
    </div>
  </div>
</template>

<style scoped>
.page {
  width: 100%;
  max-width: none;
  box-sizing: border-box;
}
.mapping-heading {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 6px 16px;
  margin-bottom: 8px;
  min-height: 1.4em;
}
.mapping-layout h3 {
  margin: 0;
  flex: 0 0 auto;
}
.mapping-flash {
  margin: 0;
  padding: 0;
  font-size: 13px;
  line-height: 1.35;
  color: var(--text-muted, #64748b);
  animation: mapping-flash-in 0.28s ease-out;
}
.mapping-flash-phase {
  margin-right: 6px;
  color: var(--text-muted, #94a3b8);
  font-weight: 500;
}
.mapping-flash-remote {
  color: var(--text, #334155);
  font-weight: 600;
}
.mapping-flash-sep {
  margin: 0 1px;
  color: var(--text-muted, #94a3b8);
}
.mapping-flash-mapped {
  color: var(--accent, #0f766e);
  font-weight: 600;
}
@keyframes mapping-flash-in {
  from {
    opacity: 0;
    transform: translateX(-4px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.voice-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: stretch;
  gap: 10px;
  margin-bottom: 12px;
  padding: 0;
  border: none;
  background: transparent;
}
.voice-toolbar-item {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  gap: 8px;
  flex: 1 1 auto;
  min-width: max-content;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--card-bg);
}
.voice-toolbar-label {
  font-size: 13px;
  line-height: 1.3;
  font-weight: 500;
  color: var(--text);
  white-space: nowrap;
}
.voice-toolbar-select {
  min-width: 72px;
  padding: 4px 8px;
}
.page-header {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
  align-items: center;
  margin-bottom: 10px;
  min-height: 40px;
}
.title-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.page-header h2 { font-size: 20px; font-weight: 600; margin: 0; }
.page-tabs {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 3px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card-bg);
  box-shadow: 0 1px 2px var(--overlay);
}
.page-tab {
  height: 30px;
  padding: 0 12px;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}
.page-tab:hover:not(.active) {
  background: var(--surface-hover);
  color: var(--text);
}
.page-tab.active {
  background: var(--primary);
  color: #fff;
}
.page-tab:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
.page-header > :deep(.device-status) {
  justify-self: end;
}
@media (max-width: 700px) {
  .page-header {
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
  }
  .page-tabs {
    grid-column: 1 / -1;
    grid-row: 2;
    justify-self: center;
  }
}
.title-info {
  position: relative;
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  padding: 0;
  border: 1.5px solid var(--text-muted);
  border-radius: 50%;
  background: transparent;
  color: var(--text-secondary);
  cursor: help;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.title-info:hover,
.title-info:focus-visible {
  border-color: var(--primary);
  color: var(--primary);
  outline: none;
}
.title-info-icon {
  font-size: 11px;
  font-weight: 700;
  font-style: italic;
  font-family: Georgia, "Times New Roman", serif;
  line-height: 1;
}

.switch {
  position: relative;
  display: inline-block;
  width: 40px;
  height: 22px;
  flex-shrink: 0;
}
.switch input {
  opacity: 0;
  width: 0;
  height: 0;
  position: absolute;
}
.switch-slider {
  position: absolute;
  top: 0; right: 0; bottom: 0; left: 0;
  border-radius: 999px;
  background: var(--border-strong);
  cursor: pointer;
  transition: background 0.15s ease;
}
.switch-slider::before {
  content: "";
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--control-thumb);
  box-shadow: 0 1px 2px var(--overlay);
  transition: transform 0.15s ease;
}
.switch input:checked + .switch-slider {
  background: var(--primary, #2563eb);
}
.switch input:checked + .switch-slider::before {
  transform: translateX(18px);
}
.switch input:focus-visible + .switch-slider {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}

.device-info-row {
  display: grid;
  grid-template-columns: 1fr 1fr 0.75fr 0.85fr minmax(140px, 1.55fr);
  gap: 10px 16px;
  margin-bottom: 0;
  padding: 12px 14px;
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  align-items: start;
}
@media (max-width: 720px) {
  .device-info-row {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .info-item-audio {
    grid-column: 1 / -1;
  }
}

.overview-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  gap: 12px;
  align-items: stretch;
  margin-bottom: 16px;
}
.overview-left {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
/* 仅由左侧撑高；日志绝对铺满同高 */
.log-aside {
  position: relative;
  min-height: 260px;
}
.log-card {
  position: absolute;
  top: 0; right: 0; bottom: 0; left: 0;
  display: flex;
  flex-direction: column;
  min-width: 0;
  width: auto;
  max-width: none;
  padding: 5px;
  overflow: hidden;
  box-sizing: border-box;
}
.log-card h3 {
  margin: 0 0 6px;
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 400;
  color: var(--text-secondary);
}
@media (max-width: 840px) {
  .overview-row {
    grid-template-columns: 1fr;
  }
  .log-aside {
    position: static;
    height: 180px;
  }
  .log-card {
    position: relative;
    top: auto; right: auto; bottom: auto; left: auto;
    height: 100%;
  }
}

.page-body { display: flex; flex-direction: column; gap: 16px; }

.card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px;
}

.card-text {
  font-size: 12px;

  margin-bottom: 8px;
  color: var(--text);
}

.host-card {
  padding: 16px 18px;
}
.host-status-row {
  display: flex;
  flex-wrap: wrap;
  align-items: stretch;
  gap: 10px;
  margin: 0 0 12px;
}
.host-status-item {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex: 1 1 0;
  min-width: 160px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface-raised);
}
.host-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  background: var(--text-muted);
}
.host-dot.ok {
  background: var(--success, #22c55e);
}
.host-dot.warn {
  background: var(--warning, #f59e0b);
}
.host-dot.error {
  background: var(--danger, #ef4444);
}
.host-item-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
  white-space: nowrap;
}
.host-item-state {
  margin-left: auto;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  white-space: nowrap;
}
.host-item-state.ok {
  color: var(--success-text);
}
.host-item-state.warn {
  color: var(--warning-text);
}
.host-item-state.error {
  color: var(--danger-text);
}
.host-status-cable {
  flex-wrap: nowrap;
}
.cable-meter {
  flex: 0 0 25%;
  max-width: 25%;
  min-width: 36px;
  display: flex;
  align-items: center;
  margin-left: 4px;
}
.cable-meter-track {
  flex: 1;
  height: 6px;
  border-radius: 3px;
  background: var(--border);
  overflow: hidden;
}
.cable-meter-fill {
  display: block;
  height: 100%;
  width: 0;
  border-radius: 3px;
  background: var(--text-muted);
  transition: width 70ms linear;
}
.cable-meter.active .cable-meter-fill {
  background: var(--success);
}
.host-detail {
  margin: 0 0 14px;
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
}
.host-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
}
.host-action-group {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.btn {
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s ease;
}
.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.btn-secondary {
  background: var(--surface-muted);
  color: var(--text);
  border: 1px solid var(--border);
}
.btn-secondary:hover:not(:disabled) {
  background: var(--surface-hover);
}
.btn-primary {
  background: var(--primary, #2563eb);
  color: #fff;
  border: 1px solid transparent;
}
.btn-primary:hover:not(:disabled) {
  filter: brightness(0.95);
}

.voice-modal-backdrop {
  position: fixed;
  top: 0; right: 0; bottom: 0; left: 0;
  z-index: 1000;
  background: var(--overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
}
.voice-modal {
  width: min(440px, 100%);
  background: var(--card-bg, #fff);
  border: 1px solid var(--border);
  border-radius: var(--radius, 8px);
  padding: 20px 22px;
  box-shadow: var(--dialog-shadow);
}
.voice-modal h3 {
  margin: 0 0 10px;
  font-size: 16px;
}
.voice-modal p {
  margin: 0 0 16px;
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
}
.voice-modal-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.voice-modal-note {
  margin-top: 14px !important;
  margin-bottom: 0 !important;
  font-size: 12px !important;
  color: var(--text-secondary) !important;
}

.log-modal {
  width: min(720px, 100%);
  max-height: min(80vh, 720px);
  display: flex;
  flex-direction: column;
}

.log-path {
  margin: 0 0 8px !important;
  font-size: 11px !important;
  color: var(--text-secondary) !important;
  word-break: break-all;
}
.log-viewer {
  flex: 1;
  min-height: 240px;
  max-height: 48vh;
  margin: 0 0 14px;
  padding: 10px 12px;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #0f172a;
  color: #e2e8f0;
  font-size: 12px;
  line-height: 1.45;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, Consolas, "Courier New", monospace;
}
.log-file-picker { display: flex; align-items: center; gap: 8px; margin: 8px 0; color: var(--text-secondary); font-size: 12px; }
.log-file-picker select { min-width: 0; flex: 1; padding: 7px 9px; border: 1px solid var(--border); border-radius: 7px; color: var(--text); background: var(--surface-raised); font: inherit; }
.log-modal-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.info-item-audio {
  gap: 3px;
}
.audio-label-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}
.audio-label-row .info-label {
  flex-shrink: 0;
}
.audio-state {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  line-height: 1.2;
  white-space: nowrap;
}
.audio-atvv-fail {
  font-size: 12px;
  font-weight: 600;
  color: var(--danger, #ef4444);
  line-height: 1.2;
  white-space: nowrap;
}
.info-item-audio.is-session .audio-state {
  color: var(--warning-text);
}
.info-item-audio.is-receiving .audio-state {
  color: var(--success-text);
}
.ble-wave {
  display: flex;
  align-items: flex-end;
  gap: 2px;
  height: 28px;
  padding: 3px 4px;
  border-radius: 4px;
  background: var(--surface-muted);
  border: 1px solid var(--border);
}
.info-item-audio.is-receiving .ble-wave {
  background: var(--success-bg);
  border-color: var(--success-border);
}
.info-item-audio.is-session .ble-wave {
  background: var(--warning-bg);
  border-color: var(--warning-border);
}
.ble-wave-bar {
  flex: 1 1 0;
  min-width: 2px;
  max-width: 6px;
  height: 8%;
  border-radius: 1px;
  background: var(--text-muted);
  transition: height 60ms linear;
}
.info-item-audio.is-receiving .ble-wave-bar {
  background: var(--success);
}
.info-item-audio.is-session .ble-wave-bar {
  background: var(--warning);
}

.info-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.info-value {
  font-size: 14px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.form-select {
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 4px;
  font-size: 13px;
  background: var(--card-bg);
  color: var(--text);
}

.number-stepper {
  display: inline-flex;
  align-items: stretch;
  border: 1px solid var(--border);
  border-radius: 4px;
  overflow: hidden;
  background: var(--card-bg);
}

.stepper-btn {
  width: 30px;
  padding: 0;
  border: none;
  background: var(--surface-muted);
  color: var(--text);
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  user-select: none;
}

.stepper-btn:hover:not(:disabled) {
  background: var(--surface-hover);
}

.stepper-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.stepper-btn + .gain-input,
.gain-input + .stepper-btn {
  border-left: 1px solid var(--border);
}

.gain-input {
  width: 56px;
  padding: 6px 4px;
  border: none;
  border-radius: 0;
  font-size: 13px;
  text-align: center;
  background: transparent;
  font-variant-numeric: tabular-nums;
  -moz-appearance: textfield;
  appearance: textfield;
}

.gain-input::-webkit-outer-spin-button,
.gain-input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.gain-input:focus {
  outline: none;
  background: var(--surface-hover);
}

.log-area {
  background: var(--surface-muted);
  border-radius: 4px;
  padding: 6px 10px;
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.45;
}

.log-entry {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  color: var(--text);
  margin: 0 0 4px;
  white-space: normal;
}

.log-time {
  color: var(--text-secondary);
  flex-shrink: 0;
}

.log-text {
  min-width: 0;
  flex: 1;
  overflow-wrap: anywhere;
  word-break: break-word;
  white-space: pre-wrap;
}

/* 首页仪表盘 */
.page {
  width: 100%;
  max-width: 1260px;
  margin: 0 auto;
}

.dashboard-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
  margin-bottom: 18px;
}

.dashboard-head h1 {
  margin: 0;
  color: var(--text);
  font-size: 24px;
  font-weight: 700;
  letter-spacing: -0.4px;
}

.overview-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 290px;
  align-items: stretch;
  gap: 18px;
  margin: 0;
}

.overview-left {
  gap: 16px;
}

.card {
  padding: 20px;
  border-color: var(--border);
  border-radius: 14px;
  background: var(--card-bg);
  box-shadow: var(--shadow-sm);
}

.device-overview {
  display: grid;
  grid-template-columns: minmax(300px, 1.25fr) minmax(280px, 1fr);
  gap: 0;
  min-height: 178px;
  padding: 24px;
}

.device-primary {
  position: relative;
  display: flex;
  align-items: center;
  gap: 20px;
  min-width: 0;
  padding-right: 24px;
}

.device-primary::after {
  position: absolute;
  top: 0;
  right: 0;
  width: 1px;
  height: 100%;
  background: var(--border);
  content: "";
}

.remote-product-frame {
  display: grid;
  width: 78px;
  height: 132px;
  flex: 0 0 auto;
  place-items: center;
}

.remote-product-image {
  display: block;
  width: auto;
  height: 132px;
  max-width: 100%;
  filter: drop-shadow(0 9px 10px rgba(15, 23, 42, 0.22));
}

.device-meta { min-width: 0; }
.model { display: flex; align-items: center; gap: 7px; color: var(--text-secondary); font-size: 13px; font-weight: 700; }
.model .status-dot { --status-light-color: var(--danger); }
.model.connected { color: var(--success-text); }
.model.connected .status-dot { --status-light-color: var(--success); }
.model.connecting { color: var(--warning-text); }
.model.connecting .status-dot { --status-light-color: var(--warning); }
.model.error,
.model.disconnected { color: var(--danger-text); }
.device-meta h2 { margin: 8px 0 13px; color: var(--text); font-size: 20px; font-weight: 700; letter-spacing: -0.25px; }
.tag-row { display: flex; flex-wrap: wrap; gap: 8px; }
.mini-tag { display: inline-flex; align-items: center; gap: 5px; min-height: 25px; padding: 0 8px; border-radius: 6px; color: var(--text-secondary); background: var(--surface-muted); font-size: 11px; font-weight: 600; }
.mini-tag svg { width: 13px; height: 13px; color: var(--primary); }

.device-data {
  display: grid;
  grid-template-columns: 1fr 1fr;
  align-content: center;
  gap: 18px 24px;
  padding-left: 30px;
}

.data-item { min-width: 0; }
.data-item .label { display: block; margin-bottom: 7px; color: var(--text-secondary); font-size: 12px; }
.data-item strong { display: flex; align-items: center; gap: 7px; min-width: 0; overflow: hidden; color: var(--text); font-size: 14px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
.battery { position: relative; width: 20px; height: 10px; flex: 0 0 auto; overflow: hidden; isolation: isolate; border: 1.5px solid var(--text-secondary); border-radius: 3px; }
.battery::after { position: absolute; top: 2px; right: -4px; width: 2px; height: 4px; border-radius: 0 2px 2px 0; background: var(--text-secondary); content: ""; }
.battery-fill { position: relative; z-index: 0; display: block; height: 100%; border-radius: 1px; background: var(--success); }
.battery-flow, .battery-bolt { display: none; }
.battery.is-charging { border-color: var(--success); box-shadow: 0 0 0 1px rgb(var(--success-rgb) / 18%); box-shadow: 0 0 0 1px color-mix(in srgb, var(--success) 18%, transparent); }
.battery.is-charging .battery-flow { position: absolute; z-index: 1; top: 1px; bottom: 1px; left: 0; display: block; width: 11px; border-radius: 999px; background: linear-gradient(90deg, transparent, rgba(255, 255, 255, .18) 28%, rgba(255, 255, 255, .88) 50%, rgba(255, 255, 255, .18) 72%, transparent); filter: blur(.25px); transform: translateX(-14px); animation: battery-charge-flow 1.15s cubic-bezier(.37, 0, .21, 1) infinite; }
.battery.is-charging .battery-bolt { position: absolute; z-index: 2; top: 50%; left: 50%; display: block; width: 7px; height: 9px; transform: translate(-50%, -50%); fill: #fff; filter: drop-shadow(0 0 1.5px rgb(var(--success-rgb) / 85%)); filter: drop-shadow(0 0 1.5px color-mix(in srgb, var(--success) 85%, #fff)); }
@keyframes battery-charge-flow { from { transform: translateX(-14px); opacity: .2; } 18% { opacity: 1; } 82% { opacity: 1; } to { transform: translateX(23px); opacity: .2; } }
@media (prefers-reduced-motion: reduce) { .battery.is-charging .battery-flow { animation: none; transform: translateX(5px); opacity: .42; } }
.data-item-audio.is-atvv-error strong { color: var(--danger); }
.data-item-audio .ble-wave { width: 62px; height: 11px; gap: 2px; margin-top: 7px; padding: 1px 0; border: 0; border-radius: 0; background: transparent; }
.data-item-audio .ble-wave-bar { min-width: 2px; max-width: 4px; border-radius: 2px; background: var(--success); }
.data-item-audio.is-session .ble-wave-bar { background: var(--warning); }

.section-title { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
.section-title h3 { margin: 0; color: var(--text); font-size: 16px; font-weight: 700; letter-spacing: -0.15px; }
.section-title > span { color: var(--text-secondary); font-size: 12px; white-space: nowrap; }

.host-card { padding: 16px; }
.host-status-row { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin: 0; }
.host-status-item { position: relative; display: grid; grid-template-columns: 28px minmax(0, 1fr); grid-template-rows: auto auto; gap: 3px 8px; min-width: 0; min-height: 68px; padding: 10px 12px; border-radius: 8px; background: var(--surface-soft); }
.host-status-item::before { grid-row: 1 / span 2; width: 28px; height: 28px; display: grid; place-items: center; align-self: center; border-radius: 8px; color: var(--primary); background: var(--surface-selected); font-size: 14px; font-weight: 700; content: "⌁"; }
.host-status-item.service-cable::before { content: "▭"; }
.host-status-item.service-audio::before { content: "◉"; }
.host-status-item.service-bridge::before { content: "⌘"; }
.host-status-item.service-injection::before { content: "⌨"; }
.host-item-label { grid-column: 2; grid-row: 1; align-self: end; margin: 0; overflow: hidden; color: var(--text); font-size: 13px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
.host-state-row { grid-column: 2; grid-row: 2; display: flex; align-items: center; gap: 8px; min-width: 0; }
.host-item-state { display: inline-flex; align-items: center; gap: 5px; min-width: 0; margin: 0; color: var(--success-text); font-size: 11px; font-weight: 700; white-space: nowrap; }
.host-item-state.warn { color: var(--warning-text); }
.host-item-state.error { color: var(--danger-text); }
.cable-meter { flex: 1 1 32px; min-width: 32px; margin: 0; }
.cable-meter-track { height: 4px; background: var(--border); }
.cable-meter.active .cable-meter-fill { background: var(--success); }
.host-detail { display: none; }

.quick-section { padding: 16px; }
.host-actions { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 10px; }
.host-action-group,
.host-actions > .btn:not(.quick-log-trigger) { position: relative; min-height: 70px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); transition: border-color 0.16s ease, transform 0.16s ease, box-shadow 0.16s ease; }
.host-action-group { display: block; }
.host-action-group:hover,
.host-actions > .btn:not(.quick-log-trigger):hover { border-color: rgba(52, 120, 246, 0.38); box-shadow: 0 8px 18px rgba(28, 39, 60, 0.08); transform: translateY(-1px); }
.host-action-group .btn { width: 100%; min-height: 68px; padding: 11px 34px 27px 12px; overflow: hidden; border: 0; border-radius: 8px; background: transparent; color: var(--text); font-weight: 700; text-align: left; text-overflow: ellipsis; white-space: nowrap; }
.host-action-group .title-info { position: absolute; top: 10px; right: 10px; z-index: 1; margin: 0; }
.quick-action-hint { position: absolute; right: 12px; bottom: 9px; left: 12px; overflow: hidden; color: var(--text-secondary); font-size: 11px; font-weight: 500; line-height: 1.2; pointer-events: none; text-overflow: ellipsis; white-space: nowrap; }
.host-actions > .btn:not(.quick-log-trigger) { display: flex; flex-direction: column; align-items: flex-start; justify-content: flex-start; gap: 7px; min-height: 70px; padding: 11px 12px; color: var(--text); background: var(--surface-raised); font-weight: 700; text-align: left; }
.quick-input-settings > span { overflow: hidden; max-width: 100%; text-overflow: ellipsis; white-space: nowrap; }
.quick-input-settings > small { overflow: hidden; max-width: 100%; margin-top: auto; color: var(--text-secondary); font-size: 11px; font-weight: 500; line-height: 1.2; text-overflow: ellipsis; white-space: nowrap; }
.quick-log-trigger { display: none; }

.log-aside { position: static; min-height: 0; }
.activity-card { display: flex; min-height: 100%; flex-direction: column; padding: 20px; overflow: hidden; }
.live { display: inline-flex; align-items: center; gap: 6px; color: var(--success) !important; font-weight: 700; }
.live .status-dot { --status-light-color: var(--success); }
.timeline { position: relative; flex: 1; min-height: 0; padding: 1px 0 0 18px; }
.timeline::before { position: absolute; top: 6px; bottom: 3px; left: 4px; width: 1px; background: var(--border); content: ""; }
.activity-event { position: relative; min-height: 58px; padding: 0 0 13px; }
.event-dot { position: absolute; top: 6px; left: -17px; }
.event-content strong { display: block; overflow: hidden; color: var(--text); font-size: 13px; font-weight: 700; line-height: 1.4; text-overflow: ellipsis; white-space: nowrap; }
.event-content time { display: block; margin-top: 5px; color: var(--text-muted); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 10px; }
.activity-footer { margin-top: auto; padding-top: 12px; border-top: 1px solid var(--border); }
.activity-footer button { width: 100%; height: 34px; border: 0; border-radius: 8px; color: var(--primary-dark); background: var(--surface-selected); font: inherit; font-size: 12px; font-weight: 700; cursor: pointer; }
.activity-footer button:hover { filter: brightness(0.97); }

@media (max-width: 1019px) {
  .overview-row { grid-template-columns: 1fr; }
  .activity-card { min-height: 270px; }
  .device-overview { grid-template-columns: 1fr; gap: 22px; }
  .device-primary { padding: 0 0 22px; }
  .device-primary::after { top: auto; right: 0; bottom: 0; left: 0; width: auto; height: 1px; }
  .device-data { grid-template-columns: repeat(4, minmax(0, 1fr)); padding-left: 0; }
  .host-actions { grid-template-columns: repeat(3, minmax(0, 1fr)); }
}

@media (max-width: 760px) {
  .dashboard-head { align-items: flex-start; flex-direction: column; }
  .device-overview { padding: 20px; }
  .device-meta h2 { font-size: 18px; }
  .device-data { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 18px; }
  .host-status-row,
  .host-actions { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .host-actions > .btn:not(.quick-log-trigger):last-child { grid-column: 1 / -1; }
}

/* 按键映射页：沿用首页的控制台层级，但让编辑区保持专注。 */
.mapping-page { width: 100%; max-width: 1260px; margin: 0 auto; }
.mapping-page-head { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: end; gap: 18px; margin: 0 0 18px; }
.mapping-page-head > div:first-child { min-width: 0; }
.mapping-page-head h1 { margin: 0; color: var(--text); font-size: 27px; font-weight: 800; letter-spacing: -0.65px; }
.mapping-subtitle { max-width: 54ch; margin: 7px 0 0; color: var(--text-secondary); font-size: 13px; line-height: 1.5; }
.mapping-head-actions { justify-self: end; justify-content: flex-end; flex-wrap: wrap; min-width: 0; }
.mapping-status-pill,
.mapping-save-state { display: inline-flex; align-items: center; gap: 7px; min-height: 34px; box-sizing: border-box; padding: 0 12px; border: 0; border-radius: 999px; color: var(--text-secondary); background: transparent; font-size: 13px; font-weight: 600; white-space: nowrap; }
.mapping-status-pill.connected { color: var(--success-text); background: var(--success-bg); }
.mapping-status-pill.connecting { color: var(--warning-text); background: var(--warning-bg); }
.mapping-status-pill.disconnected,
.mapping-status-pill.error { color: var(--danger-text); background: var(--danger-bg); }
.mapping-save-state { color: var(--success-text); background: var(--success-bg); }
.mapping-save-state.saving { color: var(--primary-dark); background: var(--info-bg); }
.mapping-save-state.saving > span { animation: mapping-saving-spin 1s linear infinite; }
@keyframes mapping-saving-spin { to { transform: rotate(360deg); } }
.mapping-layout { padding: 16px; border-radius: 14px; background: var(--card-bg); box-shadow: 0 12px 32px var(--shadow); }
.mapping-layout .mapping-heading { min-height: 0; margin: 0 0 10px; }
.mapping-layout .mapping-heading:empty { display: none; }
.mapping-layout .voice-toolbar { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(210px, 0.9fr) minmax(190px, 0.8fr); gap: 10px; margin: 0 0 16px; }
.mapping-layout .voice-toolbar-item { min-width: 0; min-height: 62px; padding: 10px 12px; border-radius: 10px; background: var(--surface-soft); }
.mapping-layout .voice-toolbar-label { display: grid; gap: 3px; min-width: 0; margin-right: auto; white-space: normal; }
.mapping-layout .voice-toolbar-label strong { color: var(--text); font-size: 12px; font-weight: 700; }
.mapping-layout .voice-toolbar-label small { color: var(--text-secondary); font-size: 10px; line-height: 1.35; }
.mapping-layout .voice-toolbar-select { min-width: 70px; height: 30px; padding-block: 3px; font-size: 12px; }
.mapping-layout .number-stepper { flex: 0 0 auto; }

@media (max-width: 1019px) {
  .mapping-layout .voice-toolbar { grid-template-columns: minmax(0, 1.2fr) minmax(190px, 0.9fr); }
  .mapping-layout .voice-toolbar-item:last-child { grid-column: 1 / -1; }
}

@media (max-width: 760px) {
  .mapping-page-head { grid-template-columns: minmax(0, 1fr); align-items: start; margin-bottom: 14px; }
  .mapping-head-actions { justify-self: start; justify-content: flex-start; }
  .mapping-page-head h1 { font-size: 24px; }
  .mapping-layout { padding: 11px; }
  .mapping-layout .voice-toolbar { grid-template-columns: 1fr; }
  .mapping-layout .voice-toolbar-item:last-child { grid-column: auto; }
}
</style>

<style>
/* Teleport 到 body：不用 scoped，避免样式丢失 */
.floating-info-tip {
  box-sizing: border-box;
  width: min(420px, calc(100vw - 16px));
  padding: 10px 12px;
  border-radius: 8px;
  background: #0f172a;
  color: #f8fafc;
  font-size: 12px;
  font-weight: 400;
  line-height: 1.55;
  text-align: left;
  box-shadow: 0 8px 24px rgba(15, 23, 42, 0.28);
  white-space: normal;
  pointer-events: auto;
}
.floating-info-tip.voice-info-tip {
  width: min(360px, calc(100vw - 16px));
  padding: 12px 14px;
}
.floating-info-tip .tip-lead {
  margin: 0 0 10px;
  color: #e2e8f0;
  line-height: 1.55;
}
.floating-info-tip .tip-block {
  margin: 0 0 8px;
  padding: 8px 10px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
}
.floating-info-tip .tip-badge {
  display: inline-block;
  margin-bottom: 6px;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
}
.floating-info-tip .tip-on .tip-badge {
  background: rgba(34, 197, 94, 0.22);
  color: #86efac;
}
.floating-info-tip .tip-off .tip-badge {
  background: rgba(148, 163, 184, 0.22);
  color: #cbd5e1;
}
.floating-info-tip ul {
  margin: 0;
  padding-left: 1.1em;
  color: #f1f5f9;
}
.floating-info-tip li {
  margin: 2px 0;
}
.floating-info-tip .tip-aside {
  margin: 6px 0 0;
  color: #94a3b8;
  font-size: 11px;
  line-height: 1.45;
}
.floating-info-tip .tip-foot {
  margin: 10px 0 0;
  padding-top: 8px;
  border-top: 1px solid rgba(148, 163, 184, 0.28);
  color: #94a3b8;
  font-size: 11px;
  line-height: 1.5;
}
</style>
