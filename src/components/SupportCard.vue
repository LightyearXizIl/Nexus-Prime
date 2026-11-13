<script setup lang="ts">
import AppIcon from "./AppIcon.vue";
import { onBeforeUnmount, ref } from "vue";
import { useI18n } from "vue-i18n";
import wechatQr from "../assets/support/wechat-pay.png";
import alipayQr from "../assets/support/alipay.png";
// Render these local SVGs inline so Tauri's self-only image policy remains intact.
import wechatLogo from "../assets/support/wechat-logo.svg?raw";
import alipayLogo from "../assets/support/alipay-logo.svg?raw";

const { t } = useI18n();
const trigger = ref<HTMLButtonElement | null>(null);
const dialog = ref<HTMLDialogElement | null>(null);
const isOpen = ref(false);

function openSupport() {
  if (!dialog.value || dialog.value.open) return;
  dialog.value.showModal();
  isOpen.value = true;
}

function onClose() {
  isOpen.value = false;
  trigger.value?.focus({ preventScroll: true });
}

function closeOnBackdrop(event: MouseEvent) {
  const element = dialog.value;
  if (!element || event.target !== element) return;
  const bounds = element.getBoundingClientRect();
  if (event.clientX < bounds.left || event.clientX > bounds.right
    || event.clientY < bounds.top || event.clientY > bounds.bottom) {
    element.close();
  }
}

onBeforeUnmount(() => dialog.value?.close());
</script>

<template>
  <button
    ref="trigger"
    type="button"
    class="support-card"
    aria-haspopup="dialog"
    aria-controls="support-dialog"
    :aria-expanded="isOpen"
    @click="openSupport"
  >
    <span class="support-heart" aria-hidden="true">
      <AppIcon name="heart" />
    </span>
    <span class="support-copy">
      <strong>{{ t("support.button") }}</strong>
      <small>{{ t("support.hint") }}</small>
    </span>
    <AppIcon class="support-arrow" name="chevron" />
  </button>

  <Teleport to="body">
    <dialog
      id="support-dialog"
      ref="dialog"
      class="support-dialog"
      aria-labelledby="support-title"
      aria-describedby="support-description"
      @close="onClose"
      @click="closeOnBackdrop"
    >
      <header class="support-header">
        <div>
          <h2 id="support-title">{{ t("support.title") }}</h2>
          <p id="support-description">{{ t("support.description") }}</p>
        </div>
        <button type="button" class="support-close" :aria-label="t('common.close')" autofocus @click="dialog?.close()">
          <AppIcon name="close" />
        </button>
      </header>

      <div class="support-methods">
        <section class="support-method wechat" aria-labelledby="support-wechat">
          <h3 id="support-wechat">
            <span class="support-brand support-brand-wechat" aria-hidden="true" v-html="wechatLogo"></span>
            {{ t("support.wechat") }}
          </h3>
          <div class="support-qr"><img :src="wechatQr" :alt="t('support.wechatQr')" width="566" height="566" /></div>
        </section>
        <section class="support-method alipay" aria-labelledby="support-alipay">
          <h3 id="support-alipay">
            <span class="support-brand support-brand-alipay" aria-hidden="true" v-html="alipayLogo"></span>
            {{ t("support.alipay") }}
          </h3>
          <div class="support-qr"><img :src="alipayQr" :alt="t('support.alipayQr')" width="1068" height="1068" /></div>
        </section>
      </div>
      <p class="support-scan-hint">{{ t("support.scanHint") }}</p>
    </dialog>
  </Teleport>
</template>

<style scoped>
.support-card { display: flex; align-items: center; gap: 11px; width: 100%; min-height: 76px; padding: 14px; border: 1px solid var(--border); border-radius: 13px; color: var(--text); background: var(--card-bg); box-shadow: 0 8px 22px var(--shadow); font: inherit; text-align: left; cursor: pointer; transition: border-color .16s ease, background .16s ease; }
.support-card:hover { border-color: var(--primary); background: var(--surface-raised); }
.support-heart { display: grid; place-items: center; flex: 0 0 34px; width: 34px; height: 34px; border-radius: 10px; color: #ef4444; background: rgb(239 68 68 / 10%); }
.support-heart svg { width: 19px; height: 19px; fill: currentColor; transform-origin: center; animation: support-heartbeat 1.4s ease-in-out infinite; }
@keyframes support-heartbeat {
  0%, 32%, 100% { transform: scale(1); }
  12% { transform: scale(1.18); }
  20% { transform: scale(1); }
  26% { transform: scale(1.1); }
}
.support-copy { display: grid; gap: 4px; flex: 1; min-width: 0; }
.support-copy strong { font-size: 13px; font-weight: 700; }
.support-copy small { color: var(--text-secondary); font-size: 10px; line-height: 1.4; }
.support-arrow { flex: 0 0 14px; width: 14px; height: 14px; color: var(--text-muted); }
.support-card:focus-visible, .support-close:focus-visible { outline: 2px solid var(--primary); outline-offset: 3px; }
.support-dialog { width: min(600px, calc(100vw - 40px)); max-width: none; max-height: calc(100dvh - 40px); margin: auto; padding: 24px; overflow-y: auto; overscroll-behavior: contain; border: 1px solid var(--border); border-radius: 18px; color: var(--text); background: var(--card-bg); box-shadow: var(--dialog-shadow); }
.support-dialog::backdrop { background: var(--overlay); }
.support-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
.support-header h2 { margin: 0; font-size: 20px; font-weight: 800; line-height: 1.4; }
.support-header p { margin: 8px 0 0; color: var(--text-secondary); font-size: 12px; line-height: 1.6; }
.support-close { display: grid; place-items: center; flex: 0 0 32px; width: 32px; height: 32px; padding: 6px; border: 1px solid var(--border); border-radius: 9px; color: var(--text-secondary); background: var(--surface-soft); cursor: pointer; }
.support-close:hover { color: var(--text); background: var(--surface-hover); }
.support-close svg { width: 18px; height: 18px; }
.support-methods { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; margin-top: 22px; }
.support-method { min-width: 0; padding: 14px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface-soft); }
.support-method h3 { display: flex; align-items: center; justify-content: center; gap: 7px; margin: 0 0 12px; font-size: 13px; font-weight: 700; }
.support-brand { display: inline-flex; align-items: center; justify-content: center; flex: 0 0 22px; width: 22px; height: 22px; overflow: hidden; border-radius: 4px; }
.support-brand :deep(svg) { display: block; width: 100%; height: 100%; }
.support-brand-wechat { background: #07c160; }
.support-brand-alipay { background: #fff; }
.support-qr { padding: 12px; border-radius: 8px; background: #fff; }
.support-qr img { display: block; width: 100%; height: auto; aspect-ratio: 1; object-fit: contain; }
.support-scan-hint { margin: 18px 0 0; color: var(--text-secondary); font-size: 11px; line-height: 1.6; text-align: center; }
@media (max-width: 520px) {
  .support-dialog { padding: 20px; }
  .support-methods { grid-template-columns: 1fr; }
  .support-method { width: 100%; max-width: 280px; margin: 0 auto; }
}
@media (prefers-reduced-motion: reduce) {
  .support-card { transition: none; }
  .support-heart svg { animation: none; }
}
</style>
