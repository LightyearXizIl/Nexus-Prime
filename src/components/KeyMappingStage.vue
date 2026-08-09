<script setup lang="ts">
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DeviceConfig, KeyAction } from "../types";
import RemoteHotspot from "./RemoteHotspot.vue";
import VoiceShortcutComposer from "./VoiceShortcutComposer.vue";
import {
  isLegacyIncompleteCodexShortcut,
  keyLabel,
  vksToHotkeyNames,
} from "../utils/shortcut";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

const props = defineProps<{
  config: DeviceConfig;
}>();

const emit = defineEmits<{
  save: [config: DeviceConfig];
}>();

const LEFT_IDS = [
  "power",
  "up",
  "left",
  "ok",
  "down",
  "back",
  "home",
  "menu",
] as const;
const RIGHT_IDS = [
  "mic",
  "right",
  "volume_up",
  "volume_down",
  "tv",
] as const;
const MAPPING_ORDER = [
  "power",
  "mic",
  "up",
  "right",
  "left",
  "down",
  "ok",
  "back",
  "home",
  "volume_up",
  "menu",
  "volume_down",
  "tv",
] as const;


const selectedId = ref<string | null>(null);
const hoverId = ref<string | null>(null);
const searchQuery = ref("");
const capturing = ref(false);
const captureError = ref<string | null>(null);
const liveLabels = ref<string[]>([]);
type ClickCount = 1 | 2 | 3 | 4;
type MappingSlot = ClickCount | "long";
const MAPPING_SLOTS: MappingSlot[] = [1, 2, 3, 4, "long"];
const SLOT_LABELS = computed<Record<MappingSlot, string>>(() => ({ 1: t("mapping.single"), 2: t("mapping.double"), 3: t("mapping.triple"), 4: t("mapping.quadruple"), long: t("mapping.long") }));
const VOLUME_DEFAULT_BINDINGS: Record<string, KeyAction> = {
  volume_up: { type: "SingleKey", value: 0xaf },
  volume_down: { type: "SingleKey", value: 0xae },
};
const selectedClickById = ref<Record<string, MappingSlot>>({});
const openClickMenuId = ref<string | null>(null);
const manualEditor = ref<{
  buttonId: string;
  slot: MappingSlot;
  initialKeys: number[];
} | null>(null);

const stageRef = ref<HTMLElement | null>(null);
const remoteRef = ref<InstanceType<typeof RemoteHotspot> | null>(null);
const cardRefs = ref<Record<string, HTMLElement | null>>({});

const linePath = ref("");
const lineOpacity = ref(0);
const lineStrong = ref(true);
const dotA = ref({ x: 0, y: 0 });
const dotB = ref({ x: 0, y: 0 });
const svgSize = ref({ w: 0, h: 0 });

let unlistenCaptured: UnlistenFn | null = null;
let unlistenProgress: UnlistenFn | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let applied = false;
let resizeObs: ResizeObserver | null = null;

function setCardRef(id: string, el: unknown) {
  cardRefs.value[id] = (el as HTMLElement) || null;
}

function labelOf(id: string): string {
  const known = ["power", "mic", "up", "left", "ok", "right", "down", "back", "volume_up", "home", "volume_down", "menu", "tv"];
  return known.includes(id) ? t(`keys.${id}`) : props.config.button_aliases?.[id] || id;
}

function mappingGlyph(id: string): string {
  const glyphs: Record<string, string> = {
    power: "⏻",
    mic: "●",
    up: "↑",
    right: "→",
    left: "←",
    down: "↓",
    ok: "OK",
    back: "↩",
    home: "⌂",
    volume_up: "+",
    menu: "≡",
    volume_down: "−",
    tv: "TV",
  };
  return glyphs[id] || "•";
}

function isVoiceButton(id: string): boolean {
  return id === "mic" || id === "voice";
}

function isVolumeButton(id: string): id is "volume_up" | "volume_down" {
  return id === "volume_up" || id === "volume_down";
}

function selectedClick(id: string): MappingSlot {
  return selectedClickById.value[id] || 1;
}

function actionOf(id: string, count: MappingSlot = 1): KeyAction {
  if (count === "long") {
    return props.config.long_press_bindings?.[id] || { type: "None", value: null };
  }
  if (count === 1) {
    return props.config.button_bindings?.[id] || { type: "None", value: null };
  }
  return (
    props.config.multi_click_bindings?.[id]?.[count] || { type: "None", value: null }
  );
}

function visibleActionOf(id: string): KeyAction {
  return actionOf(id, selectedClick(id));
}

function hasMultiBadge(id: string, count: ClickCount): boolean {
  if (count === 1) return false;
  const action = actionOf(id, count);
  return Boolean(action && action.type !== "None");
}

function configuredMultiCounts(id: string): ClickCount[] {
  return ([2, 3, 4] as ClickCount[]).filter((count) => hasMultiBadge(id, count));
}

function configuredGestureSlots(id: string): MappingSlot[] {
  const slots: MappingSlot[] = [...configuredMultiCounts(id)];
  if (actionOf(id, "long").type !== "None") {
    slots.push("long");
  }
  return slots;
}

function ensureMultiClickBindings() {
  if (!props.config.multi_click_bindings) {
    props.config.multi_click_bindings = {};
  }
}

function ensureLongPressBindings() {
  if (!props.config.long_press_bindings) {
    props.config.long_press_bindings = {};
  }
}

function clickMenuId(id: string) {
  return `click-menu-${id}`;
}

function toggleClickMenu(id: string) {
  openClickMenuId.value = openClickMenuId.value === id ? null : id;
}

function setClickCount(id: string, count: MappingSlot) {
  selectedClickById.value = {
    ...selectedClickById.value,
    [id]: count,
  };
}

function selectClickCount(id: string, count: MappingSlot) {
  closeManualShortcutEditor();
  setClickCount(id, count);
  openClickMenuId.value = null;
}

function closeClickMenu() {
  openClickMenuId.value = null;
}

const multiClickInterval = computed({
  get: () => props.config.multi_click_interval_ms ?? 300,
  set: (value: number | string) => {
    const n = Number(value);
    if (Number.isNaN(n)) return;
    const rounded = Math.round(n / 50) * 50;
    const clamped = Math.min(800, Math.max(150, rounded));
    const next: DeviceConfig = {
      ...props.config,
      multi_click_interval_ms: clamped,
      button_bindings: { ...props.config.button_bindings },
      long_press_bindings: { ...(props.config.long_press_bindings || {}) },
      multi_click_bindings: { ...(props.config.multi_click_bindings || {}) },
    };
    emit("save", next);
  },
});

function stepMultiClickInterval(delta: number) {
  multiClickInterval.value = multiClickInterval.value + delta;
}

function actionLabel(action: KeyAction): string {
  if (!action || action.type === "None") return t("mapping.unbound");
  if (action.type === "SingleKey") return vkName(Number(action.value));
  if (action.type === "ComboKey") {
    const arr = Array.isArray(action.value) ? action.value : [];
    return arr.map((v) => vkName(Number(v))).join(" + ");
  }
  if (action.type === "TextInput") return t("mapping.text", { value: action.value });
  if (action.type === "LaunchApp") return t("mapping.launch", { value: action.value });
  return "—";
}

function vkName(vk: number): string {
  return keyLabel(vk);
}

const leftButtons = computed(() =>
  LEFT_IDS.map((id) => ({
    id,
    label: labelOf(id),
    action: actionOf(id),
    selectedAction: visibleActionOf(id),
    selectedClick: selectedClick(id),
    multiCounts: configuredGestureSlots(id),
    side: "left" as const,
  }))
);

const rightButtons = computed(() =>
  RIGHT_IDS.map((id) => ({
    id,
    label: labelOf(id),
    action: actionOf(id),
    selectedAction: visibleActionOf(id),
    selectedClick: selectedClick(id),
    multiCounts: configuredGestureSlots(id),
    side: "right" as const,
  }))
);

const mappingButtons = computed(() =>
  MAPPING_ORDER.map((id) => ({
    id,
    label: labelOf(id),
    action: actionOf(id),
    selectedAction: visibleActionOf(id),
    selectedClick: selectedClick(id),
    multiCounts: configuredGestureSlots(id),
  }))
);

const filteredMappingButtons = computed(() => {
  const query = searchQuery.value.trim().toLocaleLowerCase();
  if (!query) return mappingButtons.value;
  return mappingButtons.value.filter((button) => {
    const gestures = button.multiCounts.map((slot) => SLOT_LABELS.value[slot]).join(" ");
    const text = `${button.label} ${button.id} ${actionLabel(button.action)} ${actionLabel(button.selectedAction)} ${gestures}`;
    return text.toLocaleLowerCase().includes(query);
  });
});

const selectedMappingButton = computed(() =>
  mappingButtons.value.find((button) => button.id === selectedId.value) || null
);

const activeLineId = computed(
  () => selectedId.value || hoverId.value || null
);

function edgeToward(
  el: HTMLElement,
  stageBox: DOMRect,
  side: "left" | "right"
) {
  const r = el.getBoundingClientRect();
  const y = r.top + r.height / 2 - stageBox.top;
  // left 侧卡片：取右边缘；right 侧卡片：取左边缘
  if (side === "left") {
    return { x: r.right - stageBox.left, y };
  }
  return { x: r.left - stageBox.left, y };
}

/** 按键锚点：朝映射块一侧的边缘中点，避免线穿过键帽文字 */
function keyEdgeToward(
  el: HTMLElement,
  stageBox: DOMRect,
  side: "left" | "right"
) {
  const r = el.getBoundingClientRect();
  const y = r.top + r.height / 2 - stageBox.top;
  // 左栏连线接到按键左缘；右栏接到按键右缘
  if (side === "left") {
    return { x: r.left - stageBox.left, y };
  }
  return { x: r.right - stageBox.left, y };
}

function updateLine() {
  const id = activeLineId.value;
  const stage = stageRef.value;
  if (!id || !stage) {
    lineOpacity.value = 0;
    linePath.value = "";
    return;
  }

  const stageBox = stage.getBoundingClientRect();
  svgSize.value = { w: stageBox.width, h: stageBox.height };

  const card = cardRefs.value[id];
  const key = remoteRef.value?.keyEl?.(id) as HTMLElement | null;
  if (!card || !key) {
    lineOpacity.value = 0;
    linePath.value = "";
    return;
  }

  const side = (LEFT_IDS as readonly string[]).includes(id) ? "left" : "right";
  const keyPt = keyEdgeToward(key, stageBox, side);
  const cardPt = edgeToward(card, stageBox, side);

  const dx = Math.max(40, Math.abs(keyPt.x - cardPt.x) * 0.45);
  const c1 =
    side === "left"
      ? { x: cardPt.x + dx, y: cardPt.y }
      : { x: cardPt.x - dx, y: cardPt.y };
  const c2 =
    side === "left"
      ? { x: keyPt.x - dx * 0.25, y: keyPt.y }
      : { x: keyPt.x + dx * 0.25, y: keyPt.y };

  linePath.value = `M ${cardPt.x} ${cardPt.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${keyPt.x} ${keyPt.y}`;
  dotA.value = cardPt;
  dotB.value = keyPt;
  lineStrong.value = selectedId.value === id;
  lineOpacity.value = lineStrong.value ? 1 : 0.45;
}

async function selectButton(id: string) {
  closeManualShortcutEditor();
  selectedId.value = id;
  await nextTick();
  updateLine();
}

function actionKeys(action: KeyAction): number[] {
  if (action.type === "SingleKey") return [Number(action.value)];
  if (action.type === "ComboKey" && Array.isArray(action.value)) return action.value.map(Number);
  return [];
}

const voiceBindingNeedsManualCompletion = computed(() => {
  const action = actionOf("mic", 1);
  const keys = action.type === "ComboKey" && Array.isArray(action.value) ? action.value.map(Number) : [];
  return isLegacyIncompleteCodexShortcut(keys);
});

const voiceUsesExtendedGestures = computed(() => configuredGestureSlots("mic").length > 0);

function openManualShortcutEditor() {
  const button = selectedMappingButton.value;
  if (!button || !isVoiceButton(button.id)) return;
  manualEditor.value = {
    buttonId: button.id,
    slot: button.selectedClick,
    initialKeys: actionKeys(button.selectedAction),
  };
  openClickMenuId.value = null;
}

function closeManualShortcutEditor() {
  manualEditor.value = null;
}

function applyManualShortcut(keys: number[]) {
  const editor = manualEditor.value;
  if (!editor) return;
  manualEditor.value = null;
  applyShortcutKeys(editor.buttonId, keys, editor.slot);
}

function onRemoteHover(id: string | null) {
  hoverId.value = id;
  updateLine();
}

function onCardHover(id: string | null) {
  hoverId.value = id;
  updateLine();
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

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
    const mod = modifierVkForKey(e.key);
    if (mod != null) {
      void onCaptured([mod!], []);
    }
  }
  /* c8 ignore stop */
}

function eventKeyToVk(e: KeyboardEvent): number | null {
  const key = e.key;
  const code = e.code;
  if (key.length === 1) {
    const upper = key.toUpperCase();
    const c = upper.charCodeAt(0);
    if (c >= 0x41 && c <= 0x5a) return c;
    if (c >= 0x30 && c <= 0x39) return c;
  }
  if (/^Key[A-Z]$/.test(code)) return code.charCodeAt(3);
  if (/^Digit[0-9]$/.test(code)) return code.charCodeAt(5);
  if (/^F([1-9]|1[0-2])$/.test(key)) return 0x6f + Number(key.slice(1));
  const map: Record<string, number> = {
    Backspace: 0x08,
    Tab: 0x09,
    Enter: 0x0d,
    Escape: 0x1b,
    Esc: 0x1b,
    " ": 0x20,
    Spacebar: 0x20,
    PageUp: 0x21,
    PageDown: 0x22,
    End: 0x23,
    Home: 0x24,
    ArrowLeft: 0x25,
    ArrowUp: 0x26,
    ArrowRight: 0x27,
    ArrowDown: 0x28,
    Insert: 0x2d,
    Delete: 0x2e,
    AudioVolumeMute: 0xad,
    AudioVolumeDown: 0xae,
    AudioVolumeUp: 0xaf,
    MediaTrackNext: 0xb0,
    MediaTrackPrevious: 0xb1,
    MediaStop: 0xb2,
    MediaPlayPause: 0xb3,
    BrowserBack: 0xa6,
    BrowserForward: 0xa7,
    BrowserRefresh: 0xa8,
    BrowserStop: 0xa9,
    BrowserSearch: 0xaa,
    BrowserFavorites: 0xab,
    BrowserHome: 0xac,
    LaunchMail: 0xb4,
    LaunchMediaPlayer: 0xb5,
    LaunchApplication1: 0xb6,
    LaunchApplication2: 0xb7,
  };
  return map[key] ?? null;
}

function modifierVksFromEvent(e: KeyboardEvent): number[] {
  const mods: number[] = [];
  if (e.ctrlKey) mods.push(0xa2);
  if (e.shiftKey) mods.push(0xa0);
  if (e.altKey) mods.push(0xa4);
  if (e.metaKey) mods.push(0x5b);
  return mods;
}

function modifierVkForKey(key: string): number | null {
  switch (key) {
    case "Control":
      return 0xa2;
    case "Shift":
      return 0xa0;
    case "Alt":
      return 0xa4;
    case "Meta":
    case "OS":
      return 0x5b;
    default:
      return null;
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

  const buttonId = selectedId.value;
  if (buttonId && keys?.length) {
    applyCapturedKeys(buttonId, keys);
  }
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
  capturing.value = false;
  void nextTick().then(updateLine);
}

async function startCapture(buttonId = selectedId.value) {
  if (!buttonId) return;
  closeManualShortcutEditor();
  selectedId.value = buttonId;
  if (capturing.value) {
    await cancelCapture();
    return;
  }
  captureError.value = null;
  capturing.value = true;
  liveLabels.value = [];
  applied = false;
  try {
    await invoke("capture_shortcut_start");
    startPolling();
  } catch (e) {
    capturing.value = false;
    stopPolling();
    captureError.value = String(e);
  }
}

async function cancelCapture() {
  stopPolling();
  capturing.value = false;
  liveLabels.value = [];
  applied = false;
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
}

function applyCapturedKeys(buttonId: string, vks: number[]) {
  applyShortcutKeys(buttonId, vks, selectedClick(buttonId));
}

function applyShortcutKeys(buttonId: string, vks: number[], count: MappingSlot) {
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
  if (count === "long") {
    ensureLongPressBindings();
    props.config.long_press_bindings![buttonId] = action;
  } else if (count === 1) {
    props.config.button_bindings[buttonId] = action;
  } else {
    ensureMultiClickBindings();
    props.config.multi_click_bindings![buttonId] = {
      ...(props.config.multi_click_bindings![buttonId] || {}),
      [count]: action,
    };
  }
  const next: DeviceConfig = {
    ...props.config,
    button_bindings: { ...props.config.button_bindings },
    long_press_bindings: { ...(props.config.long_press_bindings || {}) },
    multi_click_bindings: { ...(props.config.multi_click_bindings || {}) },
  };
  if ((buttonId === "mic" || buttonId === "voice") && count === 1) {
    next.button_bindings.mic = action;
    next.button_bindings.voice = action;
    next.voice_hotkey = vksToHotkeyNames(vks);
  }
  emit("save", next);
}

function clearBinding(buttonId: string) {
  const count = selectedClick(buttonId);
  if (count === "long") {
    if (props.config.long_press_bindings) {
      const nextLong = { ...props.config.long_press_bindings };
      delete nextLong[buttonId];
      props.config.long_press_bindings = nextLong;
    }
  } else if (count === 1) {
    props.config.button_bindings[buttonId] = { type: "None", value: null };
  } else if (props.config.multi_click_bindings?.[buttonId]) {
    const nextSlots = { ...props.config.multi_click_bindings[buttonId] };
    delete nextSlots[count];
    props.config.multi_click_bindings[buttonId] = nextSlots;
  }
  const next: DeviceConfig = {
    ...props.config,
    button_bindings: { ...props.config.button_bindings },
    long_press_bindings: { ...(props.config.long_press_bindings || {}) },
    multi_click_bindings: { ...(props.config.multi_click_bindings || {}) },
  };
  if ((buttonId === "mic" || buttonId === "voice") && count === 1) {
    next.button_bindings.mic = { type: "None", value: null };
    next.button_bindings.voice = { type: "None", value: null };
    next.voice_hotkey = [];
  }
  emit("save", next);
}

function resetVolumeBinding(buttonId: "volume_up" | "volume_down") {
  const defaultAction = VOLUME_DEFAULT_BINDINGS[buttonId];
  props.config.button_bindings[buttonId] = { ...defaultAction };
  setClickCount(buttonId, 1);
  const next: DeviceConfig = {
    ...props.config,
    button_bindings: { ...props.config.button_bindings },
    long_press_bindings: { ...(props.config.long_press_bindings || {}) },
    multi_click_bindings: { ...(props.config.multi_click_bindings || {}) },
  };
  emit("save", next);
}

watch([selectedId, hoverId], () => {
  void nextTick().then(updateLine);
});

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
      }
    );
  } catch (e) {
    console.warn("shortcut listen failed", e);
  }

  if (stageRef.value) {
    resizeObs = new ResizeObserver(() => updateLine());
    resizeObs.observe(stageRef.value);
  }
  stageRef.value?.addEventListener("scroll", updateLine, { passive: true });
  window.addEventListener("resize", updateLine);
  window.addEventListener("keydown", blockBrowserKeysDuringCapture, true);
  window.addEventListener("keyup", blockBrowserKeysDuringCapture, true);
  window.addEventListener("click", closeClickMenu);
});

onUnmounted(() => {
  stopPolling();
  unlistenCaptured?.();
  unlistenProgress?.();
  resizeObs?.disconnect();
  stageRef.value?.removeEventListener("scroll", updateLine);
  window.removeEventListener("resize", updateLine);
  window.removeEventListener("keydown", blockBrowserKeysDuringCapture, true);
  window.removeEventListener("keyup", blockBrowserKeysDuringCapture, true);
  window.removeEventListener("click", closeClickMenu);
  if (capturing.value) {
    invoke("capture_shortcut_stop").catch(() => {});
  }
});
</script>

<template>
  <div class="mapping-stage-v2">
    <section class="mapping-remote-panel">
      <div class="mapping-panel-heading">
        <h3>{{ t("mapping.remotePreview") }}</h3>
        <span>{{ t("mapping.locate") }}</span>
      </div>
      <div class="mapping-remote-wrap">
        <RemoteHotspot
          ref="remoteRef"
          :selected-id="selectedId"
          :hover-id="hoverId"
          @select="selectButton"
          @hover="onRemoteHover"
        />
      </div>

      <div v-if="selectedMappingButton" class="mapping-selection-card">
        <div class="selection-heading">
          <span class="selection-key" aria-hidden="true">{{ mappingGlyph(selectedMappingButton.id) }}</span>
          <div class="selection-summary">
            <span>{{ t("mapping.selected") }}</span>
            <strong>{{ selectedMappingButton.label }}键 · {{ SLOT_LABELS[selectedMappingButton.selectedClick] }}</strong>
          </div>
        </div>
        <span
          :class="[
            'mapping-keycap',
            { unbound: selectedMappingButton.selectedAction.type === 'None' },
          ]"
        >
          {{ actionLabel(selectedMappingButton.selectedAction) }}
        </span>

        <div class="selection-actions" @click.stop>
          <button
            type="button"
            class="selection-action primary"
            :disabled="capturing && selectedId !== selectedMappingButton.id"
            @click.stop="startCapture(selectedMappingButton.id)"
          >
            {{ capturing && selectedId === selectedMappingButton.id ? t("mapping.cancelCapture") : t("mapping.capture") }}
          </button>
          <button
            v-if="isVoiceButton(selectedMappingButton.id)"
            type="button"
            class="selection-action"
            :disabled="capturing"
            @click.stop="openManualShortcutEditor"
          >手动组合</button>
          <div class="click-select-wrap">
            <button
              type="button"
              class="selection-action"
              :aria-expanded="openClickMenuId === selectedMappingButton.id"
              :aria-controls="clickMenuId(selectedMappingButton.id)"
              :disabled="capturing"
              @click.stop="toggleClickMenu(selectedMappingButton.id)"
              @keydown.esc.stop.prevent="closeClickMenu"
            >
              {{ SLOT_LABELS[selectedMappingButton.selectedClick] }} ▾
            </button>
            <div
              v-if="openClickMenuId === selectedMappingButton.id"
              :id="clickMenuId(selectedMappingButton.id)"
              class="click-menu"
              role="menu"
              @click.stop
              @keydown.esc.stop.prevent="closeClickMenu"
            >
              <button
                v-for="slot in MAPPING_SLOTS"
                :key="slot"
                type="button"
                class="click-menu-item"
                :class="{ active: selectedMappingButton.selectedClick === slot }"
                role="menuitemradio"
                :aria-checked="selectedMappingButton.selectedClick === slot"
                @click="selectClickCount(selectedMappingButton.id, slot)"
              >
                <span>{{ SLOT_LABELS[slot] }}</span>
                <span class="click-menu-action">{{ actionLabel(actionOf(selectedMappingButton.id, slot)) }}</span>
              </button>
              <div class="click-menu-divider" />
              <div class="interval-editor" role="group" aria-label="连击间隔">
                <div class="interval-head"><span>连击间隔</span><strong>{{ multiClickInterval }}ms</strong></div>
                <div class="interval-controls">
                  <button type="button" class="stepper-mini" :disabled="multiClickInterval <= 150" @click="stepMultiClickInterval(-50)">−</button>
                  <input v-model.number="multiClickInterval" type="range" min="150" max="800" step="50" aria-label="连击间隔毫秒" />
                  <button type="button" class="stepper-mini" :disabled="multiClickInterval >= 800" @click="stepMultiClickInterval(50)">+</button>
                </div>
                <p class="interval-note">间隔越短响应越快，间隔越长则更容易识别连续点击。</p>
              </div>
            </div>
          </div>
          <button
            v-if="isVolumeButton(selectedMappingButton.id)"
            type="button"
            class="selection-action"
            :disabled="capturing"
            @click.stop="resetVolumeBinding(selectedMappingButton.id)"
          >{{ t("mapping.reset") }}</button>
          <button
            type="button"
            class="selection-action danger"
            :disabled="capturing"
            @click.stop="clearBinding(selectedMappingButton.id)"
          >{{ t("mapping.clear") }}</button>
        </div>
        <p v-if="isVoiceButton(selectedMappingButton.id) && voiceBindingNeedsManualCompletion" class="capture-err">
          检测到旧版 Codex 映射可能缺少主键 D。
          <button type="button" class="selection-action primary" @click.stop="openManualShortcutEditor">手动补全</button>
        </p>
        <p v-if="isVoiceButton(selectedMappingButton.id) && voiceUsesExtendedGestures" class="capture-hint">
          已启用语音五档手势；单击、双击、三击、四连击和长按优先于旧的点击/按住触发模式。
        </p>
        <p v-if="capturing && selectedId === selectedMappingButton.id" class="capture-live">
          {{ liveLabels.length ? liveLabels.join(" + ") + " …" : t("mapping.chooseKey") }}
        </p>
        <p v-if="captureError && selectedId === selectedMappingButton.id" class="capture-err">{{ captureError }}</p>
      </div>
      <div v-else class="mapping-selection-empty">{{ t("mapping.noSelection") }}</div>
    </section>

    <section class="mapping-list-panel">
      <div class="mapping-panel-heading mapping-list-heading">
        <div>
          <h3>{{ t("mapping.mappingList") }}</h3>
          <span>{{ t("mapping.keysAndAutosave", { count: filteredMappingButtons.length }) }}</span>
        </div>
        <label class="mapping-search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="11" cy="11" r="6" /><path d="m16 16 4 4" /></svg>
          <input v-model="searchQuery" type="search" :placeholder="t('mapping.search')" :aria-label="t('mapping.search')" />
        </label>
      </div>

      <VoiceShortcutComposer
        v-if="manualEditor"
        :initial-keys="manualEditor.initialKeys"
        :button-label="labelOf(manualEditor.buttonId)"
        :slot-label="SLOT_LABELS[manualEditor.slot]"
        @apply="applyManualShortcut"
        @cancel="closeManualShortcutEditor"
      />

      <div v-if="filteredMappingButtons.length" class="mapping-row-grid">
        <button
          v-for="button in filteredMappingButtons"
          :key="button.id"
          type="button"
          :class="['mapping-row', { active: selectedId === button.id, hover: hoverId === button.id && selectedId !== button.id }]"
          :aria-pressed="selectedId === button.id"
          :aria-label="`${button.label}键，${SLOT_LABELS[button.selectedClick]}，当前映射 ${actionLabel(button.selectedAction)}`"
          @mouseenter="onCardHover(button.id)"
          @mouseleave="onCardHover(null)"
          @click="selectButton(button.id)"
        >
          <span class="mapping-row-icon">{{ mappingGlyph(button.id) }}</span>
          <span class="mapping-row-copy">
            <strong>{{ button.label }}键</strong>
            <small>{{ SLOT_LABELS[button.selectedClick] }} {{ t("mapping.trigger") }}<span v-if="button.multiCounts.length"> · {{ button.multiCounts.length }}</span></small>
          </span>
          <span class="mapping-row-tail">
            <span :class="['mapping-row-keycap', { unbound: button.selectedAction.type === 'None' }]">{{ actionLabel(button.selectedAction) }}</span>
            <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4.5 5 5.5-5 5.5" /></svg>
          </span>
        </button>
      </div>
      <div v-else class="mapping-search-empty">{{ t("mapping.noMatch") }}</div>
      <p class="mapping-list-note"><span>i</span>{{ t("mapping.listHint") }}</p>
    </section>
  </div>

  <div v-if="false" class="legacy-mapping-stage" aria-hidden="true">
    <div class="stage-scroll">
    <div ref="stageRef" class="mapping-stage">
      <svg
        class="line-layer"
        :width="svgSize.w"
        :height="svgSize.h"
        :viewBox="`0 0 ${svgSize.w} ${svgSize.h}`"
        aria-hidden="true"
      >
        <path
          v-if="linePath"
          :d="linePath"
          fill="none"
          :stroke="lineStrong ? 'var(--primary)' : 'var(--text-muted)'"
          :stroke-width="lineStrong ? 2.2 : 1.5"
          stroke-linecap="round"
          :opacity="lineOpacity"
        />
        <circle
          v-if="linePath"
          :cx="dotA.x"
          :cy="dotA.y"
          r="3.5"
          :fill="lineStrong ? 'var(--primary)' : 'var(--text-muted)'"
          :opacity="lineOpacity"
        />
        <circle
          v-if="linePath"
          :cx="dotB.x"
          :cy="dotB.y"
          r="3.5"
          :fill="lineStrong ? 'var(--primary)' : 'var(--text-muted)'"
          :opacity="lineOpacity"
        />
      </svg>

      <aside class="side-col left-col">
        <div
          v-for="btn in leftButtons"
          :key="btn.id"
          :ref="(el) => setCardRef(btn.id, el)"
          class="map-card"
          :class="{
            active: selectedId === btn.id,
            hover: hoverId === btn.id && selectedId !== btn.id,
          }"
          @mouseenter="onCardHover(btn.id)"
          @mouseleave="onCardHover(null)"
          @click="selectButton(btn.id)"
        >
          <div class="map-card-main">
            <span class="map-name">{{ btn.label }}</span>
            <span
              :class="[
                'map-bind',
                { unbound: (selectedId === btn.id ? btn.selectedAction : btn.action).type === 'None' },
              ]"
            >
              {{ actionLabel(selectedId === btn.id ? btn.selectedAction : btn.action) }}
            </span>
          </div>
          <div v-if="btn.multiCounts.length" class="multi-badges" aria-label="已配置附加手势">
            <span v-for="slot in btn.multiCounts" :key="slot" class="multi-badge">
              {{ SLOT_LABELS[slot] }}
            </span>
          </div>
          <div v-if="selectedId === btn.id" class="map-card-actions" @click.stop>
            <button
              type="button"
              class="btn-sm btn-edit"
              :disabled="capturing && selectedId !== btn.id"
              @click.stop="startCapture(btn.id)"
            >
              {{ capturing && selectedId === btn.id ? "取消录入" : "录入" }}
            </button>
            <div class="click-select-wrap">
              <button
                type="button"
                class="btn-sm btn-click-select"
                :aria-expanded="openClickMenuId === btn.id"
                :aria-controls="clickMenuId(btn.id)"
                aria-label="选择点击或长按槽位"
                :disabled="capturing"
                @click.stop="toggleClickMenu(btn.id)"
                @keydown.esc.stop.prevent="closeClickMenu"
              >
                {{ SLOT_LABELS[btn.selectedClick] }} ▾
              </button>
              <div
                v-if="openClickMenuId === btn.id"
                :id="clickMenuId(btn.id)"
                class="click-menu"
                role="menu"
                @click.stop
                @keydown.esc.stop.prevent="closeClickMenu"
              >
                <button
                  v-for="slot in MAPPING_SLOTS"
                  :key="slot"
                  type="button"
                  class="click-menu-item"
                  :class="{ active: btn.selectedClick === slot }"
                  role="menuitemradio"
                  :aria-checked="btn.selectedClick === slot"
                  @click="selectClickCount(btn.id, slot)"
                >
                  <span>{{ SLOT_LABELS[slot] }}</span>
                  <span class="click-menu-action">{{ actionLabel(actionOf(btn.id, slot)) }}</span>
                </button>
                <div class="click-menu-divider" />
                <div class="interval-editor" role="group" aria-label="连击间隔">
                  <div class="interval-head">
                    <span>连击间隔</span>
                    <strong>{{ multiClickInterval }}ms</strong>
                  </div>
                  <div class="interval-controls">
                    <button type="button" class="stepper-mini" :disabled="multiClickInterval <= 150" @click="stepMultiClickInterval(-50)">−</button>
                    <input v-model.number="multiClickInterval" type="range" min="150" max="800" step="50" aria-label="连击间隔毫秒" />
                    <button type="button" class="stepper-mini" :disabled="multiClickInterval >= 800" @click="stepMultiClickInterval(50)">+</button>
                  </div>
                  <p class="interval-note">
                    间隔越短响应越快，但连续点击更难识别；间隔越长识别更宽松，但单击等待更久。
                  </p>
                </div>
              </div>
            </div>
            <button
              v-if="isVolumeButton(btn.id)"
              type="button"
              class="btn-sm btn-reset"
              :disabled="capturing"
              @click.stop="resetVolumeBinding(btn.id)"
            >
              重置
            </button>
            <button
              type="button"
              class="btn-sm btn-clear"
              :disabled="capturing"
              @click.stop="clearBinding(btn.id)"
            >
              清除
            </button>
            <p v-if="capturing && selectedId === btn.id" class="capture-live">
              {{
                liveLabels.length
                  ? liveLabels.join(" + ") + " …"
                  : "请按目标键或组合键"
              }}
            </p>
            <p v-if="captureError && selectedId === btn.id" class="capture-err">
              {{ captureError }}
            </p>
          </div>
        </div>
      </aside>

      <div class="center-stage">
        <RemoteHotspot
          ref="remoteRef"
          :selected-id="selectedId"
          :hover-id="hoverId"
          @select="selectButton"
          @hover="onRemoteHover"
        />
      </div>

      <aside class="side-col right-col">
        <div
          v-for="btn in rightButtons"
          :key="btn.id"
          :ref="(el) => setCardRef(btn.id, el)"
          class="map-card"
          :class="{
            active: selectedId === btn.id,
            hover: hoverId === btn.id && selectedId !== btn.id,
          }"
          @mouseenter="onCardHover(btn.id)"
          @mouseleave="onCardHover(null)"
          @click="selectButton(btn.id)"
        >
          <div class="map-card-main">
            <span class="map-name">{{ btn.label }}</span>
            <span
              :class="[
                'map-bind',
                { unbound: (selectedId === btn.id ? btn.selectedAction : btn.action).type === 'None' },
              ]"
            >
              {{ actionLabel(selectedId === btn.id ? btn.selectedAction : btn.action) }}
            </span>
          </div>
          <div v-if="btn.multiCounts.length" class="multi-badges" aria-label="已配置附加手势">
            <span v-for="slot in btn.multiCounts" :key="slot" class="multi-badge">
              {{ SLOT_LABELS[slot] }}
            </span>
          </div>
          <div v-if="selectedId === btn.id" class="map-card-actions" @click.stop>
            <button
              type="button"
              class="btn-sm btn-edit"
              :disabled="capturing && selectedId !== btn.id"
              @click.stop="startCapture(btn.id)"
            >
              {{ capturing && selectedId === btn.id ? "取消录入" : "录入" }}
            </button>
            <div v-if="!isVoiceButton(btn.id)" class="click-select-wrap">
              <button
                type="button"
                class="btn-sm btn-click-select"
                :aria-expanded="openClickMenuId === btn.id"
                :aria-controls="clickMenuId(btn.id)"
                aria-label="选择点击或长按槽位"
                :disabled="capturing"
                @click.stop="toggleClickMenu(btn.id)"
                @keydown.esc.stop.prevent="closeClickMenu"
              >
                {{ SLOT_LABELS[btn.selectedClick] }} ▾
              </button>
              <div
                v-if="openClickMenuId === btn.id"
                :id="clickMenuId(btn.id)"
                class="click-menu"
                role="menu"
                @click.stop
                @keydown.esc.stop.prevent="closeClickMenu"
              >
                <button
                  v-for="slot in MAPPING_SLOTS"
                  :key="slot"
                  type="button"
                  class="click-menu-item"
                  :class="{ active: btn.selectedClick === slot }"
                  role="menuitemradio"
                  :aria-checked="btn.selectedClick === slot"
                  @click="selectClickCount(btn.id, slot)"
                >
                  <span>{{ SLOT_LABELS[slot] }}</span>
                  <span class="click-menu-action">{{ actionLabel(actionOf(btn.id, slot)) }}</span>
                </button>
                <div class="click-menu-divider" />
                <div class="interval-editor" role="group" aria-label="连击间隔">
                  <div class="interval-head">
                    <span>连击间隔</span>
                    <strong>{{ multiClickInterval }}ms</strong>
                  </div>
                  <div class="interval-controls">
                    <button type="button" class="stepper-mini" :disabled="multiClickInterval <= 150" @click="stepMultiClickInterval(-50)">−</button>
                    <input v-model.number="multiClickInterval" type="range" min="150" max="800" step="50" aria-label="连击间隔毫秒" />
                    <button type="button" class="stepper-mini" :disabled="multiClickInterval >= 800" @click="stepMultiClickInterval(50)">+</button>
                  </div>
                  <p class="interval-note">
                    间隔越短响应越快，但连续点击更难识别；间隔越长识别更宽松，但单击等待更久。
                  </p>
                </div>
              </div>
            </div>
            <button
              v-if="isVolumeButton(btn.id)"
              type="button"
              class="btn-sm btn-reset"
              :disabled="capturing"
              @click.stop="resetVolumeBinding(btn.id)"
            >
              重置
            </button>
            <button
              type="button"
              class="btn-sm btn-clear"
              :disabled="capturing"
              @click.stop="clearBinding(btn.id)"
            >
              清除
            </button>
            <p v-if="capturing && selectedId === btn.id" class="capture-live">
              {{
                liveLabels.length
                  ? liveLabels.join(" + ") + " …"
                  : "请按目标键或组合键"
              }}
            </p>
            <p v-if="captureError && selectedId === btn.id" class="capture-err">
              {{ captureError }}
            </p>
          </div>
        </div>
      </aside>
    </div>
  </div>
  </div>
</template>

<style scoped>
.stage-scroll {
  overflow-x: auto;
  margin: 0 -4px;
  padding-bottom: 4px;
}

.mapping-stage {
  position: relative;
  display: grid;
  /* 左右平分剩余宽度；单侧最小宽度 100px（原 200 的一半） */
  grid-template-columns: minmax(100px, 1fr) auto minmax(100px, 1fr);
  gap: 10px 12px;
  align-items: start;
  min-width: 560px;
  width: 100%;
  padding: 4px 0 8px;
  box-sizing: border-box;
}

.line-layer {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 5;
  overflow: visible;
}

.side-col {
  display: flex;
  flex-direction: column;
  gap: 6px;
  z-index: 2;
  min-width: 0;
  width: 100%;
  padding-top: 0;
}

.left-col {
  align-items: stretch;
}

.right-col {
  align-items: stretch;
}

.center-stage {
  z-index: 2;
  justify-self: center;
  align-self: start;
  padding: 0;
  margin: 0;
  background: transparent;
  border: none;
  box-shadow: none;
}

.map-card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 8px 10px;
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
  min-width: 0;
}

.map-card:hover,
.map-card.hover {
  border-color: var(--primary);
  background: var(--surface-hover);
}

.map-card.active {
  border-color: var(--primary);
  box-shadow: 0 0 0 2px var(--focus-ring);
  background: var(--surface-selected);
}

.map-card-main {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.map-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
  flex-shrink: 0;
}

.map-bind {
  font-size: 12px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  background: var(--surface-muted);
  color: var(--text);
  padding: 2px 8px;
  border-radius: 4px;
  min-width: 0;
  max-width: none;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: right;
}

.map-bind.unbound {
  background: transparent;
  color: var(--text-muted);
}

.multi-badges {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}

.multi-badge {
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--info-bg);
  color: var(--info-text);
  font-size: 10px;
  font-weight: 700;
}

.map-card-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-top: 10px;
  padding-top: 8px;
  border-top: 1px solid var(--border);
}

.btn-sm {
  padding: 4px 10px;
  border: 1px solid var(--border-strong);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  background: var(--card-bg);
  color: var(--text);
  min-width: 46px;
  white-space: nowrap;
}

.btn-edit {
  color: var(--primary);
  border-color: var(--primary);
}
.btn-edit:hover:not(:disabled) {
  background: var(--surface-selected);
}
.btn-clear {
  color: var(--danger);
  border-color: var(--danger-border);
}
.btn-reset {
  color: var(--warning-text);
  border-color: var(--warning-border);
  background: var(--warning-bg);
}
.btn-reset:hover:not(:disabled) {
  background: var(--warning-bg);
}
.btn-clear:hover:not(:disabled) {
  background: var(--danger-bg);
}
.click-select-wrap {
  position: relative;
}
.btn-click-select {
  color: var(--success-text);
  border-color: var(--success-border);
  background: var(--success-bg);
}
.btn-click-select:hover:not(:disabled) {
  background: var(--surface-hover);
}
.click-menu {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 20;
  width: 260px;
  padding: 6px;
  border: 1px solid var(--border-strong);
  border-radius: 8px;
  background: var(--card-bg);
  box-shadow: var(--dialog-shadow);
}
.click-menu-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  padding: 7px 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  font-size: 12px;
  text-align: left;
}
.click-menu-item:hover,
.click-menu-item.active {
  background: var(--surface-selected);
  color: var(--info-text);
}
.click-menu-action {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
}
.click-menu-divider {
  height: 1px;
  margin: 6px 0;
  background: var(--border);
}
.interval-editor {
  padding: 4px 6px 2px;
}
.interval-head,
.interval-controls {
  display: flex;
  align-items: center;
  gap: 8px;
}
.interval-head {
  justify-content: space-between;
  margin-bottom: 5px;
  font-size: 12px;
  color: var(--text);
}
.interval-controls input[type="range"] {
  flex: 1;
}
.stepper-mini {
  width: 24px;
  height: 24px;
  border: 1px solid var(--border-strong);
  border-radius: 4px;
  background: var(--card-bg);
  color: var(--text);
  cursor: pointer;
}
.stepper-mini:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.interval-note {
  margin: 6px 0 0;
  color: var(--text-secondary);
  font-size: 11px;
  line-height: 1.35;
}
.btn-sm:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.capture-live {
  width: 100%;
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--primary);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}

.capture-err {
  width: 100%;
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--danger);
}

/* 原型式工作区：遥控器预览 + 映射列表 */
.mapping-stage-v2 {
  display: grid;
  grid-template-columns: 290px minmax(0, 1fr);
  align-items: stretch;
  gap: 16px;
  min-height: 520px;
}

.mapping-remote-panel,
.mapping-list-panel {
  min-width: 0;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--card-bg);
  box-shadow: var(--shadow-sm);
}

.mapping-remote-panel {
  display: flex;
  flex-direction: column;
  padding: 20px;
  background: var(--card-bg);
}

.mapping-list-panel {
  display: flex;
  flex-direction: column;
  padding: 20px;
}

.mapping-panel-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.mapping-panel-heading h3 {
  margin: 0;
  color: var(--text);
  font-size: 16px;
  font-weight: 760;
}

.mapping-panel-heading > span,
.mapping-list-heading span {
  color: var(--text-secondary);
  font-size: 11px;
  white-space: nowrap;
}

.mapping-remote-wrap {
  min-height: 392px;
  display: grid;
  place-items: center;
}

.mapping-remote-wrap :deep(.remote-schematic) {
  width: 82px;
}

.mapping-selection-card,
.mapping-selection-empty {
  margin-top: auto;
  border: 1px solid var(--info-border);
  border-radius: 10px;
  background: var(--surface-selected);
}

.mapping-selection-card {
  display: grid;
  gap: 10px;
  padding: 12px;
}

.selection-heading { display: flex; align-items: center; gap: 9px; min-width: 0; }
.selection-key,
.mapping-row-icon {
  display: grid;
  place-items: center;
  min-width: 36px;
  height: 36px;
  padding: 0 6px;
  border-radius: 9px;
  color: var(--primary-dark);
  background: var(--info-bg);
  font-size: 11px;
  font-weight: 800;
}

.selection-summary { min-width: 0; flex: 1; }
.selection-summary span { display: block; margin-bottom: 2px; color: var(--text-secondary); font-size: 10px; }
.selection-summary strong { display: block; overflow-wrap: anywhere; color: var(--text); font-size: 12px; line-height: 1.4; }

.mapping-keycap,
.mapping-row-keycap {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 46px;
  max-width: 120px;
  min-height: 27px;
  padding: 0 8px;
  overflow: hidden;
  border: 1px solid var(--border-strong);
  border-radius: 6px;
  color: var(--text);
  background: var(--surface-raised);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11px;
  font-weight: 650;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mapping-keycap.unbound,
.mapping-row-keycap.unbound { color: var(--text-muted); }

.mapping-selection-card .mapping-keycap {
  width: 100%;
  max-width: none;
  min-height: 34px;
  box-sizing: border-box;
  justify-content: flex-start;
  padding: 6px 10px;
  overflow: visible;
  text-align: left;
  text-overflow: clip;
  white-space: normal;
  overflow-wrap: anywhere;
  line-height: 1.45;
}

.selection-actions { display: flex; width: 100%; flex-wrap: wrap; gap: 6px; padding-top: 8px; border-top: 1px solid var(--info-border); }
.selection-action { min-height: 29px; padding: 0 9px; border: 1px solid var(--border-strong); border-radius: 6px; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 11px; font-weight: 700; cursor: pointer; }
.selection-action.primary { color: #fff; border-color: var(--primary); background: var(--primary); }
.selection-action.danger { color: var(--danger); border-color: var(--danger-border); }
.selection-action:hover:not(:disabled) { background: var(--surface-hover); }
.selection-action.primary:hover:not(:disabled) { background: var(--primary-dark); }
.selection-action.danger:hover:not(:disabled) { background: var(--danger-bg); }
.selection-action:disabled { opacity: 0.55; cursor: not-allowed; }
.mapping-selection-card .click-menu {
  top: auto;
  right: auto;
  bottom: calc(100% + 6px);
  left: 0;
  z-index: 60;
  width: min(250px, calc(100vw - 46px));
  max-height: min(350px, calc(100vh - 38px));
  overflow-y: auto;
  overscroll-behavior: contain;
}
.mapping-selection-empty { padding: 14px; color: var(--text-secondary); font-size: 12px; line-height: 1.55; }

.mapping-list-heading { align-items: center; margin-bottom: 16px; }
.mapping-search { width: min(210px, 44%); min-width: 156px; height: 36px; display: flex; align-items: center; gap: 8px; padding: 0 11px; border: 1px solid var(--border); border-radius: 9px; color: var(--text-muted); background: var(--surface-muted); transition: border-color 140ms ease, box-shadow 140ms ease, background-color 140ms ease; }
.mapping-search:focus-within { border-color: var(--primary); background: var(--surface-raised); box-shadow: 0 0 0 3px var(--focus-ring); }
.mapping-search svg { width: 15px; height: 15px; flex: 0 0 auto; }
.mapping-search input { min-width: 0; width: 100%; border: 0; outline: 0; color: var(--text); background: transparent; font: inherit; font-size: 12px; }
.mapping-search input::placeholder { color: var(--text-muted); }

.mapping-row-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1px;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--border);
  box-shadow: 0 1px 2px color-mix(in srgb, var(--text) 4%, transparent);
}
.mapping-row {
  position: relative;
  min-width: 0;
  min-height: 60px;
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 11px;
  padding: 10px 12px;
  border: 0;
  border-radius: 0;
  color: var(--text);
  background: var(--card-bg);
  text-align: left;
  cursor: pointer;
  transition: color 120ms ease, background-color 120ms ease;
}
.mapping-row::before {
  position: absolute;
  inset: 10px auto 10px 0;
  width: 3px;
  border-radius: 0 3px 3px 0;
  background: transparent;
  content: "";
}
.mapping-row.hover { background: var(--surface-hover); }
.mapping-row.active { background: var(--surface-selected); }
.mapping-row.active::before { background: var(--primary); }
.mapping-row:last-child:nth-child(odd) { grid-column: 1 / -1; }
.mapping-row:focus-visible { z-index: 1; outline: 3px solid var(--focus-ring); outline-offset: -3px; }
.mapping-row-icon { min-width: 34px; height: 34px; padding: 0 4px; border-radius: 9px; color: var(--text-secondary); background: var(--surface-muted); font-size: 10px; transition: color 120ms ease, background-color 120ms ease; }
.mapping-row.active .mapping-row-icon { color: var(--primary-dark); background: var(--info-bg); }
.mapping-row-copy { min-width: 0; }
.mapping-row-copy strong,
.mapping-row-copy small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.mapping-row-copy strong { margin-bottom: 4px; font-size: 12px; font-weight: 760; }
.mapping-row-copy small { color: var(--text-secondary); font-size: 10px; line-height: 1.35; }
.mapping-row-tail { min-width: 0; display: inline-flex; align-items: center; justify-content: flex-end; gap: 7px; color: var(--text-muted); }
.mapping-row-tail > svg { width: 14px; height: 14px; flex: 0 0 auto; }
.mapping-row-keycap { min-width: 44px; max-width: 104px; min-height: 27px; border-radius: 7px; font-size: 10px; box-shadow: inset 0 -1px 0 var(--border-strong), 0 1px 2px color-mix(in srgb, var(--text) 7%, transparent); }
.mapping-row.active .mapping-row-keycap { border-color: color-mix(in srgb, var(--primary) 44%, var(--border)); }
.mapping-search-empty { display: grid; min-height: 220px; place-items: center; color: var(--text-secondary); font-size: 13px; }
.mapping-list-note { display: flex; align-items: flex-start; gap: 7px; margin: 14px 0 0; color: var(--text-secondary); font-size: 11px; line-height: 1.45; }
.mapping-list-note > span { display: grid; width: 14px; height: 14px; flex: 0 0 auto; place-items: center; border-radius: 50%; color: var(--primary-dark); background: var(--info-bg); font-size: 10px; font-weight: 800; }

@media (hover: hover) and (pointer: fine) {
  .mapping-row:hover { background: var(--surface-hover); }
  .mapping-row.active:hover { background: var(--surface-selected); }
}

@media (max-width: 1019px) {
  .mapping-stage-v2 { grid-template-columns: 250px minmax(0, 1fr); }
  .mapping-remote-panel,
  .mapping-list-panel { padding: 16px; }
  .mapping-row-grid { grid-template-columns: 1fr; }
  .mapping-row:last-child:nth-child(odd) { grid-column: auto; }
  .mapping-remote-wrap { min-height: 360px; }
  .mapping-remote-wrap :deep(.remote-schematic) { width: 75px; }
}

@media (max-width: 760px) {
  .mapping-stage-v2 { grid-template-columns: 1fr; }
  .mapping-remote-panel { min-height: 0; }
  .mapping-remote-wrap { min-height: 370px; }
  .mapping-remote-wrap :deep(.remote-schematic) { width: 78px; }
  .mapping-search { width: 180px; }
}
</style>
