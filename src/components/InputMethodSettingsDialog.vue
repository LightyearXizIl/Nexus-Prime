<script setup lang="ts">
import AppIcon from "./AppIcon.vue";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { ImePreset } from "../utils/imePreset";
import doubaoIcon from "../assets/input-methods/doubao.jpg";
import qianwenIcon from "../assets/input-methods/qianwen.jpg";
import wechatIcon from "../assets/input-methods/wechat.jpg";

export type { ImePreset } from "../utils/imePreset";

const providerOrder = ["qianwen", "wechat", "doubao"] as const;
type ImeProvider = typeof providerOrder[number];
type ImeOption = {
  preset: Exclude<ImePreset, "codex">;
  label: string;
  shortcut: string;
  description: string;
  action: string;
};
type ImeProviderDetails = {
  label: string;
  icon: string;
  heading: string;
  summary: string;
  note: string;
  options: ImeOption[];
};

const props = defineProps<{
  open: boolean;
  configReady: boolean;
  saving: boolean;
  applyHint: string;
  activePreset: ImePreset | null;
}>();

const emit = defineEmits<{
  close: [];
  apply: [preset: ImePreset];
}>();

const STORAGE_KEY = "nexus-prime.input-method-settings.provider";
const providers: Record<ImeProvider, ImeProviderDetails> = {
  doubao: {
    label: "豆包输入法",
    icon: doubaoIcon,
    heading: "选择适合麦克风的语音模式",
    summary: "用遥控器麦克风时，推荐长按模式。用电脑麦克风时，也可以选择免按模式。",
    note: "松开遥控器语音键后，遥控器会停止传送音频。",
    options: [
      { preset: "doubao-hold", label: "长按模式 · 推荐", shortcut: "右 Alt", description: "按住遥控器语音键说话，松开结束。", action: "应用右 Alt 长按" },
      { preset: "doubao-hands-free", label: "免按模式", shortcut: "右 Alt + 空格", description: "短按开始，随后按任意键结束。", action: "应用右 Alt + 空格" },
    ],
  },
  wechat: {
    label: "微信输入法",
    icon: wechatIcon,
    heading: "选择微信输入法的语音模式",
    summary: "微信输入法支持按住说话和启动语音输入。请选用和微信输入法设置一致的快捷键。",
    note: "微信输入法里可以同时开启两种模式，遥控器会发送这里选中的一种。",
    options: [
      { preset: "wechat-current", label: "按住说话 · 推荐", shortcut: "左 Ctrl + 左 Shift + D", description: "按住说话，松手结束并上屏。", action: "应用按住说话" },
      { preset: "wechat", label: "启动语音输入", shortcut: "左 Ctrl + 左 Win", description: "短按启动，随后按任意键结束。", action: "应用启动语音输入" },
    ],
  },
  qianwen: {
    label: "千问输入法",
    icon: qianwenIcon,
    heading: "选择千问的按住说话快捷键",
    summary: "在千问输入法的语音输入设置里选择同一个组合。遥控器按下时按住该组合，松开时释放。",
    note: "右 Alt 是默认快捷键。",
    options: [
      { preset: "qianwen", label: "默认快捷键", shortcut: "右 Alt", description: "按住说话，松开结束。", action: "应用 右 Alt" },
      { preset: "qianwen-left-ctrl", label: "可选快捷键", shortcut: "左 Ctrl", description: "按住说话，松开结束。", action: "应用 左 Ctrl" },
      { preset: "qianwen-left-ctrl-win", label: "可选快捷键", shortcut: "左 Ctrl + 左 Win", description: "按住说话，松开结束。", action: "应用 左 Ctrl + 左 Win" },
      { preset: "qianwen-left-win-alt", label: "可选快捷键", shortcut: "左 Win + 左 Alt", description: "按住说话，松开结束。", action: "应用 左 Win + 左 Alt" },
    ],
  },
};

const activeProvider = ref<ImeProvider>(providerOrder[0]);
const lastAppliedPreset = ref<ImePreset | null>(null);
const dialogRef = ref<HTMLElement | null>(null);
const panelRef = ref<HTMLElement | null>(null);
let lastFocusedElement: HTMLElement | null = null;
const activeDetails = computed(() => providers[activeProvider.value]);

function isImeProvider(value: string | null): value is ImeProvider {
  return providerOrder.some(provider => value === provider);
}

function restoreProvider() {
  const stored = window.localStorage.getItem(STORAGE_KEY);
  activeProvider.value = isImeProvider(stored) ? stored : providerOrder[0];
}

function selectProvider(provider: ImeProvider) {
  activeProvider.value = provider;
  if (panelRef.value) panelRef.value.scrollTop = 0;
  window.localStorage.setItem(STORAGE_KEY, provider);
}

function close() {
  emit("close");
}

function apply(preset: ImePreset) {
  lastAppliedPreset.value = preset;
  emit("apply", preset);
}

function onKeydown(event: KeyboardEvent) {
  if (!props.open) return;

  if (event.key === "Escape") {
    event.preventDefault();
    close();
    return;
  }

  if (
    ["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key) &&
    event.target instanceof HTMLElement &&
    event.target.getAttribute("role") === "tab"
  ) {
    event.preventDefault();
    const currentIndex = providerOrder.indexOf(activeProvider.value);
    const nextIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? providerOrder.length - 1
          : (currentIndex + (event.key === "ArrowRight" ? 1 : -1) + providerOrder.length) % providerOrder.length;
    const nextProvider = providerOrder[nextIndex];
    selectProvider(nextProvider);
    nextTick(() => document.getElementById(`ime-tab-${nextProvider}`)?.focus());
    return;
  }

  if (event.key === "Tab" && dialogRef.value) {
    const focusable = Array.from(
      dialogRef.value.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
      )
    ).filter((element) => element.tabIndex >= 0);
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && (document.activeElement === first || !dialogRef.value.contains(document.activeElement))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      lastFocusedElement = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      restoreProvider();
      nextTick(() => document.getElementById(`ime-tab-${activeProvider.value}`)?.focus({ preventScroll: true }));
    } else if (lastFocusedElement) {
      lastFocusedElement.focus({ preventScroll: true });
      lastFocusedElement = null;
    }
  }
);

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <Transition name="ime-dialog-motion">
  <div v-if="open" class="ime-backdrop" @click.self="close">
    <section
      ref="dialogRef"
      class="ime-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="ime-settings-title"
      :aria-describedby="`ime-${activeProvider}-summary`"
      :aria-busy="saving"
    >
      <header class="ime-dialog-head">
        <div class="ime-title-block">
          <h3 id="ime-settings-title">输入法设置</h3>
          <p>遥控器语音键需要与所选输入法的语音快捷键一致。</p>
        </div>
        <button class="ime-button ime-button--secondary ime-close" type="button" aria-label="关闭输入法设置" @click="close">
          <AppIcon name="close" />
        </button>
        <div class="ime-provider-tabs" role="tablist" aria-label="选择输入法">
          <button
            v-for="provider in providerOrder"
            :key="provider"
            type="button"
            role="tab"
            :id="'ime-tab-' + provider"
            :aria-selected="activeProvider === provider"
            :aria-controls="'ime-panel-' + provider"
            :tabindex="activeProvider === provider ? 0 : -1"
            :class="{ active: activeProvider === provider }"
            @click="selectProvider(provider)"
          >
            {{ providers[provider].label }}
          </button>
        </div>
      </header>

      <div
        :id="'ime-panel-' + activeProvider"
        ref="panelRef"
        class="ime-panel thin-scrollbar"
        role="tabpanel"
        :aria-labelledby="'ime-tab-' + activeProvider"
      >
        <div class="ime-panel-copy">
          <img class="ime-provider-icon" :src="activeDetails.icon" alt="" width="64" height="64" />
          <h4>{{ activeDetails.heading }}</h4>
          <p :id="'ime-' + activeProvider + '-summary'">{{ activeDetails.summary }}</p>
          <p class="ime-detail">{{ activeDetails.note }}</p>
        </div>
        <div class="ime-panel-action">
          <section v-for="option in activeDetails.options" :key="option.preset" class="ime-option">
            <div class="ime-option-head">
              <span class="ime-status">{{ option.label }}</span>
              <span v-if="activePreset === option.preset" class="ime-current">当前已应用</span>
            </div>
            <strong>{{ option.shortcut }}</strong>
            <p>{{ option.description }}</p>
            <button
              class="ime-button ime-button--primary"
              type="button"
              :aria-label="option.action"
              :aria-pressed="activePreset === option.preset"
              :disabled="!configReady || saving"
              @click="apply(option.preset)"
            >
              <span>{{ saving && lastAppliedPreset === option.preset ? "正在应用…" : "应用此快捷键" }}</span>
              <AppIcon name="chevron" />
            </button>
            <span v-if="applyHint && lastAppliedPreset === option.preset" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
          </section>
        </div>
      </div>
    </section>
  </div>
  </Transition>
</template>

<style scoped>
.ime-dialog-motion-enter-active,
.ime-dialog-motion-leave-active {
  transition: opacity 200ms var(--ease-out);
}
.ime-dialog-motion-enter-active .ime-dialog,
.ime-dialog-motion-leave-active .ime-dialog {
  transform-origin: center;
  transition:
    opacity 200ms var(--ease-out),
    transform 200ms var(--ease-out);
}
.ime-dialog-motion-enter-from,
.ime-dialog-motion-leave-to,
.ime-dialog-motion-enter-from .ime-dialog,
.ime-dialog-motion-leave-to .ime-dialog {
  opacity: 0;
}
.ime-dialog-motion-enter-from .ime-dialog,
.ime-dialog-motion-leave-to .ime-dialog {
  transform: scale(.96);
}

.ime-backdrop {
  position: fixed;
  top: 0; right: 0; bottom: 0; left: 0;
  z-index: 3200;
  display: grid;
  place-items: center;
  padding: 24px 32px;
  background: var(--overlay);
}

.ime-dialog {
  width: min(920px, calc(100vw - 64px));
  height: min(500px, calc(100vh - 64px));
  height: min(500px, calc(100dvh - 64px));
  min-height: 0;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  overflow: hidden;
  box-sizing: border-box;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--card-bg);
  box-shadow: var(--dialog-shadow);
}

.ime-dialog-head {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 13px 16px;
  padding: 18px 24px 14px;
  border-bottom: 1px solid var(--border);
}

.ime-title-block h3,
.ime-title-block p,
.ime-panel-copy h4,
.ime-panel-copy p,
.ime-panel-action p { margin: 0; }
.ime-title-block h3 { font-size: 16px; color: var(--text); }
.ime-title-block p { margin-top: 5px; font-size: 12px; color: var(--text-secondary); }

.ime-provider-tabs {
  grid-column: 1 / -1;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 3px;
  padding: 3px;
  border: 1px solid var(--nav-segment-border);
  border-radius: 999px;
  background: var(--nav-segment-bg);
}
.ime-provider-tabs button {
  height: 34px;
  min-width: 62px;
  padding: 0 14px;
  border: 0;
  border-radius: 999px;
  color: var(--nav-muted);
  background: transparent;
  font: inherit;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition:
    color 140ms ease,
    background-color 140ms ease,
    box-shadow 140ms ease;
}
.ime-provider-tabs button:hover { color: var(--nav-ink); background: var(--nav-segment-hover); }
.ime-provider-tabs button.active { color: var(--nav-segment-active-ink); background: var(--nav-segment-active); box-shadow: var(--nav-segment-shadow); }
.ime-provider-tabs button:focus-visible,
.ime-button:focus-visible {
  outline: 3px solid var(--focus-ring);
  outline-offset: 2px;
}

.ime-button {
  min-height: 38px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 0 14px;
  border: 1px solid transparent;
  border-radius: 9px;
  font: inherit;
  font-size: 12px;
  font-weight: 700;
  line-height: 1;
  cursor: pointer;
  transition:
    transform 120ms var(--ease-out),
    color 140ms ease,
    background-color 140ms ease,
    border-color 140ms ease,
    box-shadow 140ms ease;
}
.ime-button svg { width: 15px; height: 15px; flex: 0 0 auto; }
.ime-button--primary {
  color: #fff;
  border-color: var(--primary);
  background: var(--primary);
  box-shadow: 0 5px 12px rgb(var(--primary-rgb) / 24%);
  box-shadow: 0 5px 12px color-mix(in srgb, var(--primary) 24%, transparent);
}
.ime-button--secondary {
  color: var(--text);
  border-color: var(--border);
  background: var(--surface-raised);
  box-shadow: 0 1px 2px rgb(var(--text-rgb) / 8%);
  box-shadow: 0 1px 2px color-mix(in srgb, var(--text) 8%, transparent);
}
.ime-button:active:not(:disabled) { transform: scale(.97); }
.ime-button:disabled {
  opacity: .5;
  cursor: not-allowed;
  box-shadow: none;
}
@media (hover: hover) and (pointer: fine) {
  .ime-button--primary:hover:not(:disabled) {
    border-color: var(--primary-dark);
    background: var(--primary-dark);
    box-shadow: 0 7px 16px rgb(var(--primary-rgb) / 28%);
    box-shadow: 0 7px 16px color-mix(in srgb, var(--primary) 28%, transparent);
  }
  .ime-button--secondary:hover:not(:disabled) {
    border-color: var(--border-strong);
    background: var(--surface-hover);
  }
}
.ime-close {
  width: 36px;
  min-height: 36px;
  justify-self: end;
  padding: 0;
  border-radius: 10px;
}
.ime-close svg { width: 17px; height: 17px; }

.ime-panel {
  display: grid;
  grid-template-columns: minmax(205px, 1fr) minmax(0, 2.35fr);
  gap: 24px;
  align-content: start;
  min-height: 0;
  padding: 24px;
  overflow-y: auto;
  scrollbar-gutter: stable;
}
.ime-panel-copy { display: grid; align-content: start; justify-items: center; gap: 10px; min-width: 0; padding: 18px 4px 0; text-align: center; }
.ime-provider-icon { display: block; width: 64px; height: 64px; margin-bottom: 4px; border-radius: 14px; object-fit: contain; }
.ime-panel-copy h4 { color: var(--text); font-size: 18px; line-height: 1.35; text-wrap: balance; }
.ime-panel-copy p { max-width: 235px; color: var(--text-secondary); font-size: 13px; line-height: 1.65; text-wrap: pretty; }
.ime-panel-copy .ime-detail { color: var(--text-muted); }
.ime-panel-action { display: grid; align-content: start; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; min-width: 0; }
.ime-option { display: flex; flex-direction: column; gap: 8px; min-width: 0; padding: 14px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-soft); }
.ime-option-head { display: flex; align-items: center; flex-wrap: wrap; gap: 5px 8px; }
.ime-status { color: var(--text-secondary); font-size: 11px; font-weight: 600; }
.ime-current { color: var(--success-text); font-size: 11px; font-weight: 700; }
.ime-option strong { color: var(--text); font-size: 15px; overflow-wrap: anywhere; }
.ime-option p { flex: 1; color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
.ime-option .ime-button { width: 100%; margin-top: 4px; }
.ime-apply-hint { color: var(--success-text); font-size: 12px; line-height: 1.45; }

@media (max-width: 780px) {
  .ime-backdrop { padding: 18px; }
  .ime-dialog {
    width: min(100%, calc(100vw - 36px));
    height: min(500px, calc(100vh - 36px));
    height: min(500px, calc(100dvh - 36px));
  }
  .ime-panel { grid-template-columns: 1fr; gap: 18px; padding: 20px; }
  .ime-panel-copy { padding-top: 0; }
}

@media (max-width: 520px) {
  .ime-provider-tabs button { min-width: 0; padding: 0 7px; font-size: 12px; }
  .ime-panel-action { grid-template-columns: 1fr; }
  .ime-title-block p { display: none; }
  .ime-panel-copy h4 { font-size: 18px; }
}

@media (prefers-reduced-motion: reduce) {
  .ime-dialog-motion-enter-active,
  .ime-dialog-motion-leave-active,
  .ime-dialog-motion-enter-active .ime-dialog,
  .ime-dialog-motion-leave-active .ime-dialog {
    transition-duration: 120ms;
  }
  .ime-dialog-motion-enter-from .ime-dialog,
  .ime-dialog-motion-leave-to .ime-dialog,
  .ime-button:active:not(:disabled) {
    transform: none;
  }
}
</style>
