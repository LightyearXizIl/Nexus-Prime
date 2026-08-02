<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { useUpdateStore } from "../stores/update";
import { useI18n } from "vue-i18n";

const update = useUpdateStore();
const { t } = useI18n();
const dialog = ref<HTMLElement | null>(null);

watch(
  () => update.showDialog,
  async (visible) => {
    if (visible) {
      await nextTick();
      dialog.value?.focus();
    }
  },
);

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const amount = value / 1024 ** index;
  return `${amount >= 10 || index === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[index]}`;
}

function closeOnBackdrop() {
  update.closeDialog();
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="update.showDialog && update.release"
      class="update-backdrop"
      role="presentation"
      @click.self="closeOnBackdrop"
    >
      <section
        ref="dialog"
        class="update-dialog"
        tabindex="-1"
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-title"
        @keydown.esc.prevent="update.closeDialog"
      >
        <div class="update-accent" aria-hidden="true"><span></span><i></i><i></i><i></i></div>
        <div class="update-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 4v11m0 0 4-4m-4 4-4-4M5 20h14" />
          </svg>
        </div>

        <template v-if="update.phase === 'downloading'">
          <h2 id="update-title">{{ t("update.downloading", { version: update.release.version }) }}</h2>
          <p class="update-subtitle">{{ t("update.keepOpen") }}</p>
          <div class="progress-wrap" aria-live="polite">
            <div class="progress-label"><span>{{ t("update.progress") }}</span><strong>{{ update.progress.percent }}%</strong></div>
            <div class="progress-track" role="progressbar" :aria-label="t('update.progress')" :aria-valuenow="update.progress.percent" aria-valuemin="0" aria-valuemax="100">
              <span :style="{ width: `${update.progress.percent}%` }"></span>
            </div>
            <p>{{ formatBytes(update.progress.downloadedBytes) }} / {{ formatBytes(update.progress.totalBytes || update.release.assetSize) }}</p>
          </div>
          <div class="update-actions single-action"><button type="button" class="update-btn update-btn-ghost" @click="update.cancelDownload">{{ t("update.cancel") }}</button></div>
        </template>

        <template v-else-if="update.phase === 'ready'">
          <h2 id="update-title">{{ t("update.ready") }}</h2>
          <p class="update-subtitle">{{ t("update.verified", { version: update.release.version }) }}</p>
          <div class="version-compare"><span>{{ update.currentVersion }}</span><b>→</b><strong>{{ update.release.version }}</strong></div>
          <div class="update-actions"><button type="button" class="update-btn update-btn-ghost" @click="update.closeDialog">{{ t("update.later") }}</button><button type="button" class="update-btn update-btn-primary" @click="update.install">{{ t("update.install") }}</button></div>
        </template>

        <template v-else-if="update.phase === 'installing'">
          <h2 id="update-title">{{ t("update.installing") }}</h2>
          <p class="update-subtitle">{{ t("update.closing") }}</p>
        </template>

        <template v-else>
          <h2 id="update-title">{{ t("update.found", { version: update.release.version }) }}</h2>
          <p class="update-subtitle">{{ update.release.title }}</p>
          <div class="version-compare"><span>{{ update.currentVersion }}</span><b>→</b><strong>{{ update.release.version }}</strong></div>
          <div class="release-notes"><p>{{ t("update.notes") }}</p><pre>{{ update.release.notes }}</pre></div>
          <p v-if="update.error" class="update-error" role="alert">{{ update.error }}</p>
          <div class="update-actions"><button type="button" class="update-btn update-btn-ghost" @click="update.closeDialog">{{ t("update.laterTalk") }}</button><button type="button" class="update-btn update-btn-primary" @click="update.download">{{ update.phase === 'error' ? t("update.retryDownload") : t("update.update") }}</button></div>
        </template>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
.update-backdrop { position: fixed; inset: 0; z-index: 4100; display: grid; place-items: center; padding: 22px; background: var(--overlay); }
.update-dialog { width: min(450px, 100%); padding: 23px 24px 20px; border: 1px solid var(--border); border-radius: 18px; color: var(--text); background: var(--card-bg); box-shadow: var(--dialog-shadow); outline: none; }
.update-accent { display: flex; justify-content: center; gap: 6px; margin-bottom: 19px; }
.update-accent span, .update-accent i { display: block; width: 26px; height: 3px; border-radius: 99px; background: var(--border-strong); }
.update-accent span { background: var(--success); box-shadow: 0 0 10px color-mix(in srgb, var(--success) 55%, transparent); }
.update-icon { display: grid; width: 48px; height: 48px; margin: 0 auto 15px; place-items: center; border: 1px solid color-mix(in srgb, var(--primary) 30%, var(--border)); border-radius: 15px; color: var(--primary); background: var(--surface-selected); box-shadow: 0 9px 20px color-mix(in srgb, var(--primary) 15%, transparent); }
.update-icon svg { width: 24px; height: 24px; }
.update-dialog h2 { margin: 0; color: var(--text); font-size: 20px; letter-spacing: -.35px; text-align: center; }
.update-subtitle { max-width: 340px; margin: 8px auto 0; color: var(--text-secondary); font-size: 12px; line-height: 1.55; text-align: center; }
.version-compare { display: flex; align-items: center; justify-content: center; gap: 11px; margin: 18px 0; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; }
.version-compare span, .version-compare strong { padding: 6px 9px; border-radius: 7px; }
.version-compare span { color: var(--text-secondary); background: var(--surface-muted); }
.version-compare strong { color: var(--primary-dark); background: var(--surface-selected); }
.version-compare b { color: var(--text-muted); }
.release-notes { margin: 0 0 16px; overflow: hidden; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-soft); }
.release-notes > p { margin: 0; padding: 9px 12px; border-bottom: 1px solid var(--border); color: var(--text); font-size: 12px; font-weight: 760; }
.release-notes pre { max-height: 185px; margin: 0; padding: 11px 12px; overflow: auto; color: var(--text-secondary); font: inherit; font-size: 11px; line-height: 1.55; white-space: pre-wrap; word-break: break-word; }
.progress-wrap { margin: 22px 0 20px; padding: 13px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-soft); }
.progress-label { display: flex; justify-content: space-between; margin-bottom: 9px; font-size: 12px; }.progress-label span { color: var(--text-secondary); }.progress-label strong { color: var(--primary); }
.progress-track { height: 8px; overflow: hidden; border-radius: 99px; background: var(--border); }.progress-track span { display: block; height: 100%; border-radius: inherit; background: linear-gradient(90deg, var(--primary), var(--success)); transition: width .16s ease; }
.progress-wrap > p { margin: 8px 0 0; color: var(--text-muted); font-size: 11px; text-align: right; }
.update-error { margin: -6px 0 13px; color: var(--danger); font-size: 11px; line-height: 1.45; }
.update-actions { display: flex; justify-content: flex-end; gap: 8px; }.single-action { justify-content: center; }
.update-btn { min-height: 34px; padding: 0 14px; border: 1px solid transparent; border-radius: 8px; font: inherit; font-size: 12px; font-weight: 750; cursor: pointer; transition: transform .16s ease, background .16s ease; }.update-btn:hover { transform: translateY(-1px); }
.update-btn-ghost { color: var(--text); border-color: var(--border); background: var(--card-bg); }.update-btn-ghost:hover { background: var(--surface-hover); }.update-btn-primary { color: #fff; background: var(--primary); box-shadow: 0 5px 13px color-mix(in srgb, var(--primary) 24%, transparent); }.update-btn-primary:hover { background: var(--primary-dark); }
@media (max-width: 480px) { .update-dialog { padding: 20px 17px 16px; border-radius: 15px; }.release-notes pre { max-height: 155px; } }
</style>
