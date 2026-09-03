<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { ImePreset } from "../utils/imePreset";

export type { ImePreset } from "../utils/imePreset";

export type ImeProvider = "codex" | "wechat" | "qianwen" | "doubao";

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
const providers: Array<{ id: ImeProvider; label: string }> = [
  { id: "codex", label: "Codex" },
  { id: "wechat", label: "微信" },
  { id: "qianwen", label: "千问" },
  { id: "doubao", label: "豆包" },
];

const activeProvider = ref<ImeProvider>("codex");
const lastAppliedPreset = ref<ImePreset | null>(null);
const dialogRef = ref<HTMLElement | null>(null);
let lastFocusedElement: HTMLElement | null = null;
const activeLabel = computed(
  () => providers.find((provider) => provider.id === activeProvider.value)?.label ?? "Codex"
);

function isImeProvider(value: string | null): value is ImeProvider {
  return providers.some((provider) => provider.id === value);
}

function restoreProvider() {
  const stored = window.localStorage.getItem(STORAGE_KEY);
  activeProvider.value = isImeProvider(stored) ? stored : "codex";
}

function selectProvider(provider: ImeProvider) {
  activeProvider.value = provider;
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
    const currentIndex = providers.findIndex((provider) => provider.id === activeProvider.value);
    const nextIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? providers.length - 1
          : (currentIndex + (event.key === "ArrowRight" ? 1 : -1) + providers.length) % providers.length;
    const nextProvider = providers[nextIndex];
    selectProvider(nextProvider.id);
    nextTick(() => document.getElementById(`ime-tab-${nextProvider.id}`)?.focus());
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

        <div class="ime-provider-tabs" role="tablist" aria-label="选择输入法">
          <button
            v-for="provider in providers"
            :key="provider.id"
            type="button"
            role="tab"
            :id="`ime-tab-${provider.id}`"
            :aria-selected="activeProvider === provider.id"
            :aria-controls="`ime-panel-${provider.id}`"
            :tabindex="activeProvider === provider.id ? 0 : -1"
            :class="{ active: activeProvider === provider.id }"
            @click="selectProvider(provider.id)"
          >
            {{ provider.label }}
          </button>
        </div>

        <button class="ime-button ime-button--secondary ime-close" type="button" aria-label="关闭输入法设置" @click="close">
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
            <path d="m5 5 10 10M15 5 5 15" />
          </svg>
        </button>
      </header>

      <div
        :id="`ime-panel-${activeProvider}`"
        class="ime-panel"
        role="tabpanel"
        :aria-labelledby="`ime-tab-${activeProvider}`"
      >
        <template v-if="activeProvider === 'codex'">
          <div class="ime-panel-copy">
            <span class="ime-eyebrow">{{ activeLabel }} · 默认</span>
            <h4>按住遥控器说话，松开结束听写</h4>
            <p id="ime-codex-summary">
              Codex 的语音快捷键必须与本软件完全一致。当前推荐组合为
              <code>左 Ctrl + 左 Shift + D</code>。
            </p>
            <p class="ime-detail">
              请在 Codex 设置中确认“按住进行听写或长按”为 Ctrl+Shift+D；若已改过快捷键，请在按键映射中录入相同组合。
            </p>
          </div>
          <aside class="ime-panel-action ime-callout">
            <span class="ime-status">推荐映射</span>
            <strong>左 Ctrl + 左 Shift + D</strong>
            <p>触发模式：按住</p>
            <button class="ime-button ime-button--primary" type="button" :disabled="!configReady || saving" @click="apply('codex')">
              <span>{{ saving ? "正在应用…" : "快速应用此映射" }}</span>
              <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4.5 5 5.5-5 5.5" /></svg>
            </button>
            <span v-if="applyHint && lastAppliedPreset === 'codex'" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
          </aside>
        </template>

        <template v-else-if="activeProvider === 'wechat'">
          <div class="ime-panel-copy">
            <span class="ime-eyebrow">{{ activeLabel }} · 两种语音模式</span>
            <h4>与微信输入法的语音设置保持一致</h4>
            <p id="ime-wechat-summary">
              微信输入法提供“启动语音输入”和“按住说话”两种快捷键。本软件每次只应用其中一种，
              请与微信输入法“设置 → 语音输入”中的快捷键保持一致。
            </p>
            <ol>
              <li>“启动语音输入”使用 <code>左 Ctrl + 左 Win</code>，按下即可启动。</li>
              <li>“按住说话”使用 <code>左 Ctrl + 左 Shift + D</code>，松手结束并上屏。</li>
              <li>两项可以同时在微信输入法中启用；本软件只发送下方选中的一种。</li>
            </ol>
          </div>
          <aside class="ime-panel-action ime-wechat-actions">
            <section class="ime-wechat-option">
              <div class="ime-option-head">
                <span class="ime-status">启动语音输入</span>
                <span v-if="activePreset === 'wechat'" class="ime-current">当前已应用</span>
              </div>
              <strong>左 Ctrl + 左 Win</strong>
              <p>按下即可启用语音输入，按任意键均可结束；触发模式：点击</p>
              <button class="ime-button ime-button--primary" type="button" :aria-pressed="activePreset === 'wechat'" :disabled="!configReady || saving" @click="apply('wechat')">
                <span>{{ saving ? "正在应用…" : "应用启动语音输入" }}</span>
                <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4.5 5 5.5-5 5.5" /></svg>
              </button>
              <span v-if="applyHint && lastAppliedPreset === 'wechat'" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
            </section>

            <section class="ime-wechat-option">
              <div class="ime-option-head">
                <span class="ime-status">按住说话</span>
                <span v-if="activePreset === 'wechat-current'" class="ime-current">当前已应用</span>
              </div>
              <strong>左 Ctrl + 左 Shift + D</strong>
              <p>按住可语音输入，松手结束并上屏；触发模式：按住（推荐）</p>
              <button class="ime-button ime-button--primary" type="button" :aria-pressed="activePreset === 'wechat-current'" :disabled="!configReady || saving" @click="apply('wechat-current')">
                <span>{{ saving ? "正在应用…" : "应用按住说话" }}</span>
                <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4.5 5 5.5-5 5.5" /></svg>
              </button>
              <span v-if="applyHint && lastAppliedPreset === 'wechat-current'" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
            </section>
          </aside>
        </template>

        <template v-else-if="activeProvider === 'qianwen'">
          <div class="ime-panel-copy">
            <span class="ime-eyebrow">{{ activeLabel }} · 默认</span>
            <h4>按住右 Alt 说话，松开上屏</h4>
            <p id="ime-qianwen-summary">
              千问输入法 Windows 端默认使用 <code>右 Alt</code> 唤醒语音输入；本软件需要设为同一按住快捷键。
            </p>
            <p class="ime-detail">
              若你在千问输入法里改过唤醒快捷键，请在按键映射中录入相同组合，并确认千问已获得麦克风权限。
            </p>
          </div>
          <aside class="ime-panel-action ime-callout">
            <span class="ime-status">推荐映射</span>
            <strong>右 Alt</strong>
            <p>触发模式：按住</p>
            <button class="ime-button ime-button--primary" type="button" :disabled="!configReady || saving" @click="apply('qianwen')">
              <span>{{ saving ? "正在应用…" : "快速应用此映射" }}</span>
              <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4.5 5 5.5-5 5.5" /></svg>
            </button>
            <span v-if="applyHint && lastAppliedPreset === 'qianwen'" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
          </aside>
        </template>

        <template v-else>
          <div class="ime-panel-copy">
            <span class="ime-eyebrow">豆包输入法 · 语音输入</span>
            <h4>按你的豆包语音模式选择同一套快捷键</h4>
            <p id="ime-doubao-summary">
              豆包输入法的长按模式使用 <code>右 Alt</code>；免按模式使用
              <code>右 Alt + 空格</code>。
            </p>
            <p class="ime-detail">
              免按模式更适合电脑麦克风：小米遥控器松开语音键后会停止传送音频。使用遥控器麦克风时，请选择长按模式。
            </p>
          </div>
          <aside class="ime-panel-action ime-doubao-actions">
            <section class="ime-doubao-option">
              <span class="ime-status">推荐 · 长按模式</span>
              <strong>右 Alt</strong>
              <p>触发模式：按住遥控器语音键说话，松开结束。</p>
              <button class="ime-button ime-button--primary" type="button" :disabled="!configReady || saving" @click="apply('doubao-hold')">
                <span>{{ saving ? "正在应用…" : "应用右 Alt 长按" }}</span>
                <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4.5 5 5.5-5 5.5" /></svg>
              </button>
              <span v-if="applyHint && lastAppliedPreset === 'doubao-hold'" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
            </section>
            <section class="ime-doubao-option">
              <span class="ime-status">免按模式</span>
              <strong>右 Alt + 空格</strong>
              <p>触发模式：短按启动，随后按任意键结束。</p>
              <button class="ime-button ime-button--secondary" type="button" :disabled="!configReady || saving" @click="apply('doubao-hands-free')">
                <span>{{ saving ? "正在应用…" : "应用右 Alt + 空格" }}</span>
                <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4.5 5 5.5-5 5.5" /></svg>
              </button>
              <span v-if="applyHint && lastAppliedPreset === 'doubao-hands-free'" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
            </section>
          </aside>
        </template>
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
  height: min(560px, calc(100vh - 64px));
  height: min(560px, calc(100dvh - 64px));
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
  grid-template-columns: minmax(170px, 1fr) auto minmax(80px, 1fr);
  align-items: center;
  gap: 16px;
  padding: 18px 20px 14px;
  border-bottom: 1px solid var(--border);
}

.ime-title-block h3,
.ime-title-block p,
.ime-panel-copy h4,
.ime-panel-copy p,
.ime-panel-copy ol,
.ime-panel-action p { margin: 0; }
.ime-title-block h3 { font-size: 16px; color: var(--text); }
.ime-title-block p { margin-top: 5px; font-size: 12px; color: var(--text-secondary); }

.ime-provider-tabs {
  display: inline-flex;
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
  grid-template-columns: minmax(0, 1.05fr) minmax(280px, .95fr);
  gap: 24px;
  align-items: stretch;
  min-height: 0;
  padding: 24px;
  overflow-y: auto;
  scrollbar-gutter: stable;
}
.ime-panel-copy { display: flex; flex-direction: column; align-items: flex-start; justify-content: center; gap: 12px; min-width: 0; }
.ime-eyebrow { color: var(--primary); font-size: 12px; font-weight: 700; }
.ime-panel-copy h4 { color: var(--text); font-size: 20px; line-height: 1.32; }
.ime-panel-copy p, .ime-panel-copy ol { color: var(--text-secondary); font-size: 13px; line-height: 1.65; }
.ime-panel-copy code { padding: 2px 6px; border-radius: 4px; background: var(--surface-hover); color: var(--text); font-family: ui-monospace, Consolas, monospace; font-size: .92em; }
.ime-panel-copy ol { padding-left: 1.3em; }
.ime-panel-copy li + li { margin-top: 5px; }
.ime-detail { color: var(--text-muted) !important; }

.ime-panel-action { display: flex; flex-direction: column; justify-content: center; min-width: 0; padding: 20px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface-muted); }
.ime-callout { align-items: flex-start; gap: 10px; }
.ime-status { padding: 3px 7px; border: 1px solid var(--info-border); border-radius: 999px; color: var(--info-text); background: var(--info-bg); font-size: 11px; font-weight: 600; }
.ime-callout strong, .ime-placeholder-action strong { color: var(--text); font-size: 17px; }
.ime-callout p { color: var(--text-secondary); font-size: 12px; }
.ime-callout .ime-button { margin-top: 6px; }
.ime-apply-hint { color: var(--success-text); font-size: 12px; line-height: 1.45; }

.ime-wechat-actions, .ime-doubao-actions { gap: 12px; padding: 14px; }
.ime-wechat-option, .ime-doubao-option { display: flex; flex-direction: column; align-items: flex-start; gap: 7px; padding: 12px; border: 1px solid var(--border); border-radius: 9px; background: var(--surface-raised); }
.ime-option-head { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; }
.ime-current { color: var(--success-text); font-size: 11px; font-weight: 700; }
.ime-wechat-option strong, .ime-doubao-option strong { color: var(--text); font-size: 16px; }
.ime-wechat-option p, .ime-doubao-option p { color: var(--text-secondary); font-size: 12px; line-height: 1.45; }
.ime-wechat-option .ime-button, .ime-doubao-option .ime-button { width: 100%; margin-top: 2px; }

@media (max-width: 780px) {
  .ime-backdrop { padding: 18px; }
  .ime-dialog {
    width: min(100%, calc(100vw - 36px));
    height: min(560px, calc(100vh - 36px));
    height: min(560px, calc(100dvh - 36px));
  }
  .ime-dialog-head { grid-template-columns: 1fr auto; }
  .ime-provider-tabs { grid-column: 1 / -1; grid-row: 2; justify-self: center; }
  .ime-panel { grid-template-columns: 1fr; gap: 18px; padding: 20px; }
}

@media (max-width: 520px) {
  .ime-provider-tabs button { min-width: 0; padding: 0 10px; }
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
