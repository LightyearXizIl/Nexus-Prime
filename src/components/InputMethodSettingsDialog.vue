<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import wechatImeHotkeysImg from "../assets/guides/wechat-ime-hotkeys.png";

export type ImeProvider = "codex" | "wechat" | "qianwen" | "doubao";

const props = defineProps<{
  open: boolean;
  configReady: boolean;
  saving: boolean;
  applyHint: string;
}>();

const emit = defineEmits<{
  close: [];
  apply: [provider: Exclude<ImeProvider, "doubao">];
}>();

const STORAGE_KEY = "nexus-prime.input-method-settings.provider";
const providers: Array<{ id: ImeProvider; label: string }> = [
  { id: "codex", label: "Codex" },
  { id: "wechat", label: "微信" },
  { id: "qianwen", label: "千问" },
  { id: "doubao", label: "豆包" },
];

const activeProvider = ref<ImeProvider>("codex");
const lastAppliedProvider = ref<Exclude<ImeProvider, "doubao"> | null>(null);
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

function apply(provider: Exclude<ImeProvider, "doubao">) {
  lastAppliedProvider.value = provider;
  emit("apply", provider);
}

function onKeydown(event: KeyboardEvent) {
  if (props.open && event.key === "Escape") {
    event.preventDefault();
    close();
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) restoreProvider();
  }
);

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div v-if="open" class="ime-backdrop" @click.self="close">
    <section
      class="ime-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="ime-settings-title"
      :aria-describedby="`ime-${activeProvider}-summary`"
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

        <button class="btn btn-secondary ime-close" type="button" @click="close">关闭</button>
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
            <button class="btn btn-primary" type="button" :disabled="!configReady || saving" @click="apply('codex')">
              快速应用此映射
            </button>
            <span v-if="applyHint && activeProvider === lastAppliedProvider" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
          </aside>
        </template>

        <template v-else-if="activeProvider === 'wechat'">
          <div class="ime-panel-copy">
            <span class="ime-eyebrow">{{ activeLabel }} · 推荐</span>
            <h4>按住说话，松开输入文字</h4>
            <p id="ime-wechat-summary">
              请先在本软件应用映射，再在微信输入法中将“启动语音输入”设为
              <code>左 Ctrl + 左 Win</code>。
            </p>
            <ol>
              <li>录入前先临时关闭、修改微信语音快捷键，或切换到其他输入法。</li>
              <li>点击右侧快速应用，将遥控器语音键设为左 Ctrl + 左 Win。</li>
              <li>回到微信输入法，确认语音快捷键与本软件一致。</li>
            </ol>
          </div>
          <aside class="ime-panel-action ime-wechat-guide">
            <img :src="wechatImeHotkeysImg" alt="微信输入法语音输入快捷键设置示意图" />
            <div class="ime-action-row">
              <button class="btn btn-primary" type="button" :disabled="!configReady || saving" @click="apply('wechat')">
                应用左 Ctrl + 左 Win
              </button>
              <span v-if="applyHint && activeProvider === lastAppliedProvider" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
            </div>
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
            <button class="btn btn-primary" type="button" :disabled="!configReady || saving" @click="apply('qianwen')">
              快速应用此映射
            </button>
            <span v-if="applyHint && activeProvider === lastAppliedProvider" class="ime-apply-hint" aria-live="polite">{{ applyHint }}</span>
          </aside>
        </template>

        <template v-else>
          <div class="ime-panel-copy ime-placeholder-copy">
            <span class="ime-eyebrow">豆包输入法 · 预告</span>
            <h4>Windows 版尚未完全发布</h4>
            <p id="ime-doubao-summary">当前仅提供产品预告，暂时没有可核实的 Windows 语音快捷键。</p>
            <p class="ime-detail">正式发布后，再根据实际快捷键补充一键映射；现在不会修改你的遥控器语音键配置。</p>
          </div>
          <aside class="ime-panel-action ime-placeholder-action">
            <span class="ime-placeholder-mark" aria-hidden="true">豆</span>
            <strong>等待 Windows 版发布</strong>
            <button class="btn btn-secondary" type="button" disabled>暂不可设置</button>
          </aside>
        </template>
      </div>
    </section>
  </div>
</template>

<style scoped>
.ime-backdrop {
  position: fixed;
  inset: 0;
  z-index: 3200;
  display: grid;
  place-items: center;
  padding: 24px 32px;
  background: var(--overlay);
}

.ime-dialog {
  width: min(920px, calc(100vw - 64px));
  max-height: min(74vh, 640px);
  overflow: auto;
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
  font-weight: 650;
  cursor: pointer;
  transition: 0.18s ease;
}
.ime-provider-tabs button:hover { color: var(--nav-ink); background: var(--nav-segment-hover); }
.ime-provider-tabs button.active { color: var(--nav-segment-active-ink); background: var(--nav-segment-active); box-shadow: var(--nav-segment-shadow); }
.ime-close { justify-self: end; padding: 5px 11px; font-size: 12px; }

.ime-panel {
  display: grid;
  grid-template-columns: minmax(0, 1.05fr) minmax(280px, .95fr);
  gap: 24px;
  align-items: stretch;
  padding: 24px;
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
.ime-status { padding: 3px 7px; border: 1px solid var(--info-border); border-radius: 999px; color: var(--info-text); background: var(--info-bg); font-size: 11px; font-weight: 650; }
.ime-callout strong, .ime-placeholder-action strong { color: var(--text); font-size: 17px; }
.ime-callout p { color: var(--text-secondary); font-size: 12px; }
.ime-callout .btn { margin-top: 6px; }
.ime-apply-hint { color: var(--success-text); font-size: 12px; line-height: 1.45; }

.ime-wechat-guide { gap: 14px; padding: 14px; }
.ime-wechat-guide img { width: 100%; max-height: 255px; object-fit: contain; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); }
.ime-action-row { display: flex; flex-wrap: wrap; align-items: center; gap: 9px; }

.ime-placeholder-copy { opacity: .82; }
.ime-placeholder-action { align-items: center; gap: 12px; text-align: center; }
.ime-placeholder-mark { display: grid; width: 48px; height: 48px; place-items: center; border-radius: 14px; color: var(--primary); background: var(--surface-selected); font-size: 22px; font-weight: 800; }
.ime-placeholder-action .btn:disabled { cursor: not-allowed; opacity: .65; }

@media (max-width: 780px) {
  .ime-backdrop { padding: 18px; }
  .ime-dialog { width: min(100%, calc(100vw - 36px)); }
  .ime-dialog-head { grid-template-columns: 1fr auto; }
  .ime-provider-tabs { grid-column: 1 / -1; grid-row: 2; justify-self: center; }
  .ime-panel { grid-template-columns: 1fr; gap: 18px; padding: 20px; }
}

@media (max-width: 520px) {
  .ime-provider-tabs button { min-width: 0; padding: 0 10px; }
  .ime-title-block p { display: none; }
  .ime-panel-copy h4 { font-size: 18px; }
}
</style>
