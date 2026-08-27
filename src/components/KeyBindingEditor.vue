<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from "vue";
import { loggedInvoke as invoke } from "../utils/appLogger";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DeviceConfig, BridgeType, KeyAction } from "../types";

const props = defineProps<{
  bridgeType: BridgeType;
  config: DeviceConfig;
  focusButtonId?: string | null;
}>();

const emit = defineEmits<{
  save: [config: DeviceConfig];
}>();

const editingKey = ref<string | null>(null);
const capturing = ref(false);
const captureError = ref<string | null>(null);
const captureStatus = ref("先点「录入」，再按目标单键或组合键");
const liveLabels = ref<string[]>([]);
const listRef = ref<HTMLElement | null>(null);

let unlistenCaptured: UnlistenFn | null = null;
let unlistenProgress: UnlistenFn | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let applied = false;

const PRIMARY_IDS = [
  "power",
  "mic",
  "up",
  "left",
  "ok",
  "right",
  "down",
  "back",
  "volume_up",
  "home",
  "volume_down",
  "menu",
  "tv",
];

const buttons = computed(() => {
  const aliases = props.config?.button_aliases || {};
  const ids = PRIMARY_IDS.filter((id) => aliases[id] || props.config.button_bindings?.[id]);
  const extra = Object.keys(aliases).filter((id) => !PRIMARY_IDS.includes(id));
  return [...ids, ...extra].map((id) => ({
    id,
    label: aliases[id] || id,
    action: props.config.button_bindings?.[id] || { type: "None", value: null },
  }));
});

watch(
  () => props.focusButtonId,
  async (id) => {
    if (!id) return;
    editingKey.value = id;
    await nextTick();
    const el = listRef.value?.querySelector(`[data-button-id="${id}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }
);

/** 录入期间只拦截 WebView 加速键；组合键结果由 Rust LL 钩子统一提交。 */
function blockBrowserKeysDuringCapture(e: KeyboardEvent) {
  if (!capturing.value) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "BrightnessUp" || e.key === "BrightnessDown") {
    captureError.value = "该亮度键未向 Windows 上报可保存的键盘事件，无法录入。";
  }
  // 浏览器事件不能作为结果来源；否则会与 OS 级事件竞争并截断三键组合。
  return;
  /* c8 ignore start -- legacy fallback retained below for source-history context */
  if (applied || e.repeat) return;
  if (e.type === "keydown") {
    const main = eventKeyToVk(e);
    if (main == null || modifierVkForKey(e.key) != null) {
      if (e.key === "BrightnessUp" || e.key === "BrightnessDown") {
        captureError.value = "该亮度键未向 Windows 上报可保存的键盘事件，无法录入。";
      }
      return;
    }
    void onCaptured([...modifierVksFromEvent(e), main!], []);
    return;
  }
  if (e.type === "keyup") {
    const modifier = modifierVkForKey(e.key);
    if (modifier != null) void onCaptured([modifier!], []);
  }
  /* c8 ignore stop */
}

function eventKeyToVk(e: KeyboardEvent): number | null {
  const key = e.key;
  const code = e.code;
  if (key.length === 1) {
    const upper = key.toUpperCase();
    const char = upper.charCodeAt(0);
    if ((char >= 0x41 && char <= 0x5a) || (char >= 0x30 && char <= 0x39)) return char;
  }
  if (/^Key[A-Z]$/.test(code)) return code.charCodeAt(3);
  if (/^Digit[0-9]$/.test(code)) return code.charCodeAt(5);
  if (/^F([1-9]|1[0-2])$/.test(key)) return 0x6f + Number(key.slice(1));
  const map: Record<string, number> = {
    Backspace: 0x08, Tab: 0x09, Enter: 0x0d, Escape: 0x1b, Esc: 0x1b,
    " ": 0x20, Spacebar: 0x20, PageUp: 0x21, PageDown: 0x22, End: 0x23,
    Home: 0x24, ArrowLeft: 0x25, ArrowUp: 0x26, ArrowRight: 0x27,
    ArrowDown: 0x28, Insert: 0x2d, Delete: 0x2e,
    AudioVolumeMute: 0xad, AudioVolumeDown: 0xae, AudioVolumeUp: 0xaf,
    MediaTrackNext: 0xb0, MediaTrackPrevious: 0xb1, MediaStop: 0xb2,
    MediaPlayPause: 0xb3, BrowserBack: 0xa6, BrowserForward: 0xa7,
    BrowserRefresh: 0xa8, BrowserStop: 0xa9, BrowserSearch: 0xaa,
    BrowserFavorites: 0xab, BrowserHome: 0xac, LaunchMail: 0xb4,
    LaunchMediaPlayer: 0xb5, LaunchApplication1: 0xb6, LaunchApplication2: 0xb7,
  };
  return map[key] ?? null;
}

function modifierVksFromEvent(e: KeyboardEvent): number[] {
  const modifiers: number[] = [];
  if (e.ctrlKey) modifiers.push(0xa2);
  if (e.shiftKey) modifiers.push(0xa0);
  if (e.altKey) modifiers.push(0xa4);
  if (e.metaKey) modifiers.push(0x5b);
  return modifiers;
}

function modifierVkForKey(key: string): number | null {
  if (key === "Control") return 0xa2;
  if (key === "Shift") return 0xa0;
  if (key === "Alt") return 0xa4;
  if (key === "Meta" || key === "OS") return 0x5b;
  return null;
}

onMounted(async () => {
  try {
    unlistenCaptured = await listen<{ keys: number[]; labels: string[] }>(
      "shortcut-captured",
      (event) => {
        const keys = event.payload?.keys;
        if (!keys?.length) return;
        onCaptured(keys, event.payload.labels || []);
      }
    );
    unlistenProgress = await listen<{ labels: string[] }>(
      "shortcut-capture-progress",
      (event) => {
        liveLabels.value = event.payload?.labels || [];
        if (capturing.value && liveLabels.value.length) {
          captureStatus.value = `正在录入：${liveLabels.value.join(" + ")} …`;
        }
      }
    );
  } catch (e) {
    console.warn("shortcut listen failed", e);
  }
  window.addEventListener("keydown", blockBrowserKeysDuringCapture, true);
  window.addEventListener("keyup", blockBrowserKeysDuringCapture, true);
});

onUnmounted(() => {
  stopPolling();
  unlistenCaptured?.();
  unlistenProgress?.();
  window.removeEventListener("keydown", blockBrowserKeysDuringCapture, true);
  window.removeEventListener("keyup", blockBrowserKeysDuringCapture, true);
  if (capturing.value) {
    invoke("capture_shortcut_stop").catch(() => {});
  }
});

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function startPolling() {
  stopPolling();
  applied = false;
  pollTimer = setInterval(async () => {
    if (!capturing.value || applied) return;
    try {
      const result = await invoke<{ keys: number[]; labels: string[] } | null>(
        "capture_shortcut_poll"
      );
      if (result && Array.isArray(result.keys) && result.keys.length > 0) {
        onCaptured(result.keys, result.labels || []);
      }
    } catch (e) {
      console.warn("capture poll failed", e);
    }
  }, 50);
}

async function onCaptured(keys: number[], labels: string[]) {
  if (applied) return;
  applied = true;
  stopPolling();
  liveLabels.value = [];

  const buttonId = editingKey.value;
  if (buttonId && keys?.length) {
    applyCapturedKeys(buttonId, keys);
    captureStatus.value = `已录入 ${labels.join(" + ") || keys.map(vkName).join(" + ")}，已保存`;
  } else {
    captureStatus.value = "录入结束";
  }
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
  capturing.value = false;
  editingKey.value = null;
}

async function startEdit(buttonId: string) {
  if (capturing.value) {
    await cancelCapture();
    return;
  }

  captureError.value = null;
  editingKey.value = buttonId;
  capturing.value = true;
  liveLabels.value = [];
  applied = false;
  captureStatus.value = "正在录入：请按目标键或组合键……";
  try {
    await invoke("capture_shortcut_start");
    startPolling();
  } catch (e) {
    capturing.value = false;
    editingKey.value = null;
    stopPolling();
    captureError.value = String(e);
    captureStatus.value = "录入失败，可以重试";
  }
}

async function cancelCapture() {
  stopPolling();
  capturing.value = false;
  editingKey.value = null;
  liveLabels.value = [];
  applied = false;
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
  captureStatus.value = "已取消录入";
}

function applyCapturedKeys(buttonId: string, vks: number[]) {
  let action: KeyAction;
  if (!vks.length) {
    action = { type: "None", value: null };
  } else if (vks.length === 1) {
    action = { type: "SingleKey", value: vks[0] };
  } else {
    action = { type: "ComboKey", value: [...vks] };
  }
  if (!props.config.button_bindings) {
    (props.config as DeviceConfig).button_bindings = {};
  }
  props.config.button_bindings[buttonId] = action;
  // 对齐 Python：mic 映射同步到 voice / voice_hotkey
  const next: DeviceConfig = {
    ...props.config,
    button_bindings: { ...props.config.button_bindings },
  };
  if (buttonId === "mic" || buttonId === "voice") {
    next.button_bindings.mic = action;
    next.button_bindings.voice = action;
    next.voice_hotkey = vksToHotkeyNames(vks);
  }
  emit("save", next);
}

function clearBinding(buttonId: string) {
  props.config.button_bindings[buttonId] = { type: "None", value: null };
  const next: DeviceConfig = {
    ...props.config,
    button_bindings: { ...props.config.button_bindings },
  };
  if (buttonId === "mic" || buttonId === "voice") {
    next.button_bindings.mic = { type: "None", value: null };
    next.button_bindings.voice = { type: "None", value: null };
    next.voice_hotkey = [];
  }
  emit("save", next);
  captureStatus.value = "已清除绑定";
}

function vksToHotkeyNames(vks: number[]): string[] {
  const map: Record<number, string> = {
    0xa2: "leftctrl",
    0xa3: "rightctrl",
    0x11: "ctrl",
    0xa0: "leftshift",
    0xa1: "rightshift",
    0x10: "shift",
    0xa4: "leftalt",
    0xa5: "rightalt",
    0x12: "alt",
    0x5b: "leftwin",
    0x5c: "rightwin",
    0x20: "space",
    0x0d: "enter",
  };
  return vks.map((vk) => {
    if (map[vk]) return map[vk];
    if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk).toLowerCase();
    if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
    if (vk >= 0x70 && vk <= 0x7b) return `f${vk - 0x6f}`;
    return `vk_${vk.toString(16)}`;
  });
}

function actionLabel(action: KeyAction): string {
  if (!action || action.type === "None") return "未绑定";
  if (action.type === "SingleKey") return vkName(Number(action.value));
  if (action.type === "ComboKey") {
    const arr = Array.isArray(action.value) ? action.value : [];
    return arr.map((v) => vkName(Number(v))).join(" + ");
  }
  if (action.type === "TextInput") return `文字: ${action.value}`;
  if (action.type === "LaunchApp") return `启动: ${action.value}`;
  return "—";
}

function vkName(vk: number): string {
  const map: Record<number, string> = {
    0x08: "Backspace",
    0x09: "Tab",
    0x0d: "Enter",
    0x1b: "Esc",
    0x20: "Space",
    0x21: "PageUp",
    0x22: "PageDown",
    0x23: "End",
    0x24: "Home",
    0x25: "←",
    0x26: "↑",
    0x27: "→",
    0x28: "↓",
    0x2d: "Insert",
    0x2e: "Delete",
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
    0xb0: "Next track",
    0xb1: "Previous track",
    0xb2: "Media stop",
    0xb3: "Play/Pause",
    0xa6: "Browser back",
    0xa7: "Browser forward",
    0xa8: "Browser refresh",
    0xa9: "Browser stop",
    0xaa: "Browser search",
    0xab: "Browser favorites",
    0xac: "Browser home",
    0xb4: "Mail",
    0xb5: "Media player",
    0xb6: "App 1",
    0xb7: "App 2",
  };
  if (map[vk]) return map[vk];
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
  if (vk >= 0x70 && vk <= 0x7b) return `F${vk - 0x6f}`;
  return `VK_0x${vk.toString(16).toUpperCase()}`;
}
</script>

<template>
  <div class="key-editor">
    <p class="capture-hint">{{ captureStatus }}</p>
    <p v-if="captureError" class="capture-error">{{ captureError }}</p>
    <p v-if="!buttons.length" class="capture-error">
      没有可映射的按键（button_aliases 为空）
    </p>

    <div class="key-list" ref="listRef">
      <div
        v-for="btn in buttons"
        :key="btn.id"
        :data-button-id="btn.id"
        :class="['key-row', { editing: editingKey === btn.id }]"
      >
        <span class="key-name">{{ btn.label }}</span>
        <div class="key-action-area">
          <span :class="['key-action', { unbound: btn.action.type === 'None' }]">
            {{ actionLabel(btn.action) }}
          </span>
          <div class="key-actions">
            <button
              class="btn-sm btn-edit"
              @click="startEdit(btn.id)"
              :disabled="capturing && editingKey !== btn.id"
            >
              {{
                editingKey === btn.id && capturing
                  ? "取消录入"
                  : "按真实键盘录入"
              }}
            </button>
            <button
              v-if="btn.action.type !== 'None'"
              class="btn-sm btn-clear"
              @click="clearBinding(btn.id)"
              :disabled="capturing"
            >
              ✕
            </button>
          </div>
        </div>
      </div>
    </div>

    <div v-if="capturing" class="capture-overlay">
      <div class="capture-box">
        <p class="capture-title">正在录入</p>
        <p class="capture-live">
          {{
            liveLabels.length
              ? liveLabels.join(" + ") + " …"
              : "请按目标键或组合键"
          }}
        </p>
        <p class="capture-note">松开后自动完成并保存</p>
        <button class="btn btn-primary" @click="cancelCapture">取消录入</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.key-editor {
  position: relative;
}

.capture-hint {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 10px;
}

.capture-error {
  color: var(--danger);
  font-size: 12px;
  margin-bottom: 8px;
}

.key-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.key-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-radius: 6px;
  border: 1px solid transparent;
  transition: all 0.15s ease;
}

.key-row:hover {
  background: var(--surface-hover);
  border-color: var(--border);
}

.key-row.editing {
  background: var(--surface-selected);
  border-color: var(--primary);
}

.key-name {
  font-size: 13px;
  font-weight: 500;
  min-width: 80px;
}

.key-action-area {
  display: flex;
  align-items: center;
  gap: 8px;
}

.key-action {
  font-size: 12px;
  font-family: monospace;
  background: var(--surface-muted);
  padding: 3px 8px;
  border-radius: 4px;
  color: var(--text);
}

.key-action.unbound {
  color: var(--text-secondary);
  background: transparent;
}

.key-actions {
  display: flex;
  gap: 4px;
}

.btn-sm {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  background: var(--card-bg);
  color: var(--text);
  transition: all 0.15s ease;
}

.btn-sm:hover {
  background: var(--surface-hover);
}

.btn-edit {
  color: var(--primary);
  border-color: var(--primary);
}
.btn-edit:hover:not(:disabled) {
  background: var(--surface-selected);
}
.btn-edit:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-clear {
  color: var(--danger);
  border-color: transparent;
}
.btn-clear:hover:not(:disabled) {
  background: var(--danger-bg);
  border-color: var(--danger);
}

.capture-overlay {
  position: fixed;
  top: 0; right: 0; bottom: 0; left: 0;
  background: var(--overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.capture-box {
  background: var(--card-bg);
  padding: 32px 40px;
  border-radius: 12px;
  text-align: center;
  box-shadow: var(--dialog-shadow);
  min-width: 320px;
}

.capture-title {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 8px;
}

.capture-live {
  font-size: 22px;
  font-family: monospace;
  font-weight: 500;
  color: var(--primary);
  min-height: 32px;
  margin-bottom: 8px;
}

.capture-note {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 20px;
}
</style>
