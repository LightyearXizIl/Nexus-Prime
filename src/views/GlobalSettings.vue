<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useThemeStore } from "../stores/theme";
import { useUpdateStore } from "../stores/update";
import { useLocaleStore } from "../stores/locale";
import { useI18n } from "vue-i18n";
import type { AppLocale } from "../i18n";
import type { GlobalSettings, ThemePreference } from "../types";
import lightYearAuthor from "../assets/light_year_author.jpg";

type SectionId = "general" | "appearance" | "about";

const settings = ref<GlobalSettings>({
  autostart: false,
  language: "zh-CN",
  minimize_to_tray: true,
  auto_check_updates: true,
  theme: "system",
});
const theme = useThemeStore();
const update = useUpdateStore();
const locale = useLocaleStore();
const { t } = useI18n();
const saved = ref(true);
const saving = ref(false);
const saveError = ref("");
const appVersion = ref("v0.1.7");
const activeSection = ref<SectionId>("general");
const manualUpdateChecked = ref(false);
const settingsReady = ref(false);

const navigation = computed<Array<{ id: SectionId; label: string; caption: string }>>(() => [
  { id: "general", label: t("settings.general"), caption: t("settings.generalHint") },
  { id: "appearance", label: t("settings.appearance"), caption: t("settings.appearanceHint") },
  { id: "about", label: t("settings.about"), caption: t("settings.aboutHint") },
]);

onMounted(async () => {
  const [settingsResult, versionResult] = await Promise.allSettled([
    invoke<GlobalSettings>("get_global_settings"),
    getVersion(),
  ]);

  if (settingsResult.status === "fulfilled") {
    settings.value = settingsResult.value;
  } else {
    console.error("Failed to load settings:", settingsResult.reason);
  }
  if (versionResult.status === "fulfilled") {
    appVersion.value = `v${versionResult.value.replace(/\.0$/, "")}`;
  } else {
    console.warn("Failed to read app version:", versionResult.reason);
  }
  settingsReady.value = true;
});

const updateControlState = computed(() => {
  if (update.phase === "checking") {
    return { label: "正在检查…", tone: "checking", progress: 48, indeterminate: true, disabled: true };
  }
  if (update.phase === "downloading") {
    return { label: `下载 ${update.progress.percent}%`, tone: "downloading", progress: update.progress.percent, indeterminate: false, disabled: false };
  }
  if (update.phase === "ready") {
    return { label: "准备安装", tone: "ready", progress: 100, indeterminate: false, disabled: false };
  }
  if (update.phase === "installing") {
    return { label: "正在安装…", tone: "installing", progress: 100, indeterminate: true, disabled: true };
  }
  if (update.phase === "error") {
    return { label: update.release ? "下载失败，重试" : "检查失败，重试", tone: "error", progress: 100, indeterminate: false, disabled: false };
  }
  if (update.release) {
    return { label: `发现 ${update.release.version}`, tone: "available", progress: 100, indeterminate: false, disabled: false };
  }
  if (update.error) {
    return { label: "检查失败，重试", tone: "error", progress: 100, indeterminate: false, disabled: false };
  }
  if (manualUpdateChecked.value) {
    return { label: "已是最新版本", tone: "latest", progress: 100, indeterminate: false, disabled: false };
  }
  return { label: "检查更新", tone: "idle", progress: 14, indeterminate: false, disabled: false };
});

async function saveSettings() {
  if (saved.value || saving.value) return;
  saving.value = true;
  saveError.value = "";
  try {
    const settingsToSave = { ...settings.value, language: locale.preference, theme: theme.preference };
    await invoke("save_global_settings", { settings: settingsToSave });
    settings.value = settingsToSave;
    saved.value = true;
    if (settingsToSave.auto_check_updates) void update.checkAfterEnabled();
  } catch (error) {
    console.error("Failed to save settings:", error);
    saveError.value = "保存失败，请检查应用权限后重试。";
  } finally {
    saving.value = false;
  }
}

function onSettingChange() {
  saved.value = false;
  saveError.value = "";
}

async function setTheme(nextPreference: ThemePreference) {
  await theme.setPreference(nextPreference);
}

function setLanguage(event: Event) {
  void locale.setPreference((event.target as HTMLSelectElement).value as AppLocale);
}

async function handleUpdateControl() {
  if (update.phase === "installing") return;
  if (update.canOpen) {
    update.openDialog();
    return;
  }

  manualUpdateChecked.value = false;
  await update.checkForUpdate(false);
  manualUpdateChecked.value = true;
  if (update.canOpen) update.openDialog();
}

async function openExternal(url: string) {
  try {
    await openUrl(url);
  } catch (error) {
    console.warn("open url failed:", error);
    window.open(url, "_blank");
  }
}
</script>

<template>
  <div class="settings-page">
    <header class="settings-header">
      <div>
        <h1>{{ t("settings.title") }}</h1>
        <p>{{ t("settings.subtitle") }}</p>
      </div>
      <div class="save-area">
        <p v-if="saveError" class="save-error" role="alert">{{ saveError }}</p>
        <p v-else-if="saved" class="saved-state" role="status"><span aria-hidden="true">✓</span>{{ t("common.saved") }}</p>
        <button class="save-button" :class="{ 'is-saving': saving }" :disabled="saved || saving" @click="saveSettings">
          <span aria-hidden="true">{{ saving ? '↻' : '↓' }}</span>
          {{ saving ? t("common.saving") : t("common.save") }}
        </button>
      </div>
    </header>

    <div class="settings-layout">
      <aside class="settings-nav" role="tablist" :aria-label="t('settings.categories')">
        <p>{{ t("settings.categories") }}</p>
        <button
          v-for="item in navigation"
          :key="item.id"
          type="button"
          role="tab"
          :id="`settings-tab-${item.id}`"
          :aria-controls="`settings-panel-${item.id}`"
          :aria-selected="activeSection === item.id"
          :class="{ active: activeSection === item.id }"
          @click="activeSection = item.id"
        >
          <span>{{ item.label }}</span>
          <small>{{ item.caption }}</small>
        </button>
      </aside>

      <main class="settings-content" :aria-busy="!settingsReady">
        <section v-if="!settingsReady" class="settings-card settings-loading-card" aria-label="正在加载设置">
          <div class="loading-card-head">
            <span></span><span></span>
          </div>
          <div class="loading-settings-list">
            <span v-for="index in 3" :key="index"></span>
          </div>
        </section>

        <template v-else>
        <section v-if="activeSection === 'general'" id="settings-panel-general" class="settings-card" role="tabpanel" aria-labelledby="settings-tab-general">
          <div class="card-head">
            <div>
              <h2>{{ t("settings.generalTitle") }}</h2>
              <p>{{ t("settings.generalDesc") }}</p>
            </div>
          </div>
          <div class="settings-list">
            <div class="preference-row">
              <div class="preference-icon" aria-hidden="true">↗</div>
              <div class="preference-copy">
                <strong>{{ t("settings.autostart") }}</strong>
                <span>{{ t("settings.autostartHint") }}</span>
              </div>
              <label class="toggle" :title="t('settings.autostart')">
                <input v-model="settings.autostart" type="checkbox" :aria-label="t('settings.autostart')" @change="onSettingChange" />
                <span class="toggle-slider"></span>
              </label>
            </div>
            <div class="preference-row">
              <div class="preference-icon" aria-hidden="true">⊟</div>
              <div class="preference-copy">
                <strong>{{ t("settings.tray") }}</strong>
                <span>{{ t("settings.trayHint") }}</span>
              </div>
              <label class="toggle" :title="t('settings.tray')">
                <input v-model="settings.minimize_to_tray" type="checkbox" :aria-label="t('settings.tray')" @change="onSettingChange" />
                <span class="toggle-slider"></span>
              </label>
            </div>
            <div class="preference-row">
              <div class="preference-icon" aria-hidden="true">↑</div>
              <div class="preference-copy">
                <strong>{{ t("settings.updates") }}</strong>
                <span>{{ t("settings.updatesHint") }}</span>
              </div>
              <label class="toggle" :title="t('settings.updates')">
                <input v-model="settings.auto_check_updates" type="checkbox" :aria-label="t('settings.updates')" @change="onSettingChange" />
                <span class="toggle-slider"></span>
              </label>
            </div>
          </div>
        </section>

        <section v-else-if="activeSection === 'appearance'" id="settings-panel-appearance" class="settings-card" role="tabpanel" aria-labelledby="settings-tab-appearance">
          <div class="card-head">
            <div>
              <h2>{{ t("settings.appearanceTitle") }}</h2>
              <p>{{ t("settings.appearanceDesc") }}</p>
            </div>
          </div>
          <div class="settings-list appearance-list">
            <div class="preference-row language-row">
              <div class="preference-icon" aria-hidden="true">文</div>
              <div class="preference-copy">
                <strong>{{ t("settings.language") }}</strong>
                <span>{{ t("settings.languageHint") }}</span>
              </div>
              <select :value="locale.preference" class="language-select" :disabled="locale.saving" :aria-label="t('settings.language')" @change="setLanguage">
                <option value="zh-CN">简体中文</option>
                <option value="zh-TW">繁體中文</option>
                <option value="en">English</option>
              </select>
            </div>
            <div class="theme-preference">
              <div class="theme-copy">
                <div class="preference-icon" aria-hidden="true">◐</div>
                <div class="preference-copy">
                  <strong>{{ t("settings.theme") }}</strong>
                  <span>{{ t("settings.themeHint") }}</span>
                </div>
              </div>
              <div class="theme-segmented" role="group" :aria-label="t('settings.theme')">
                <button v-for="option in ([['light', t('settings.light')], ['system', t('settings.system')], ['dark', t('settings.dark')]] as const)" :key="option[0]" type="button" :disabled="theme.saving" :class="{ active: theme.preference === option[0] }" @click="setTheme(option[0])">
                  {{ option[1] }}
                </button>
              </div>
              <p v-if="theme.error" class="theme-error" role="alert">{{ theme.error }}</p>
              <p v-if="locale.error" class="theme-error" role="alert">{{ t("settings.languageSaveFailed") }}</p>
            </div>
          </div>
        </section>

        <section v-else id="settings-panel-about" class="settings-card about-card" role="tabpanel" aria-labelledby="settings-tab-about">
          <div class="card-head">
            <div>
              <h2>{{ t("settings.aboutTitle") }}</h2>
              <p>{{ t("settings.aboutDesc") }}</p>
            </div>
          </div>
          <div class="about-overview">
            <div><span>当前版本</span><strong>{{ appVersion }}</strong></div>
            <div><span>技术栈</span><strong>Rust · Tauri 2 · Vue 3</strong></div>
            <div><span>支持设备</span><strong>小米蓝牙遥控器 2 Pro</strong></div>
          </div>
          <div class="credits-grid">
            <section class="author-list" aria-labelledby="author-list-title">
              <p id="author-list-title" class="credit-kicker">作者与维护</p>
              <article class="author-card">
                <div class="author-identity">
                  <img class="author-avatar" :src="lightYearAuthor" alt="Light year" />
                  <div class="author-card-copy">
                    <p class="credit-kicker">NEXUS PRIME</p>
                    <h3>Light year</h3>
                    <p class="author-description">Windows 版重构与维护</p>
                    <span class="author-contact">微信号：XizllHZ_007</span>
                  </div>
                </div>
                <button
                  class="credit-link author-project-link"
                  type="button"
                  aria-label="打开 Nexus Prime GitHub 项目：LightyearXizIl/Nexus-Prime"
                  @click="openExternal('https://github.com/LightyearXizIl/Nexus-Prime')"
                >
                  <span>GitHub：</span>
                  <small>LightyearXizIl/Nexus-Prime</small>
                </button>
                <button
                  type="button"
                  :class="['update-orbit', `is-${updateControlState.tone}`]"
                  :disabled="updateControlState.disabled"
                  :aria-busy="update.phase === 'checking' || update.phase === 'installing'"
                  :aria-label="`应用更新：${updateControlState.label}`"
                  @click="handleUpdateControl"
                >
                  <span class="update-orbit-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round">
                      <path d="m13 2-9 12h6l-1 8 11-14h-6l-1-6Z" />
                    </svg>
                  </span>
                  <span class="update-orbit-track" aria-hidden="true"><i :class="{ indeterminate: updateControlState.indeterminate }" :style="{ width: `${updateControlState.progress}%` }"></i></span>
                  <span class="update-orbit-label">{{ updateControlState.label }}</span>
                </button>
              </article>
            </section>
            <section class="source-list" aria-labelledby="source-list-title">
              <p id="source-list-title" class="credit-kicker">源码与致谢</p>
              <div class="source-cards">
                <article class="source-card source-card-featured">
                  <h3>Voice VibeCoding版</h3>
                  <p class="source-author"><span>作者：</span><strong>mwlt</strong></p>
                  <div class="source-links">
                    <button
                      class="credit-link"
                      type="button"
                      aria-label="打开 Voice VibeCoding 版 Gitee 项目：mwlt/remote-voice-vibe-coding"
                      @click="openExternal('https://gitee.com/mwlt/remote-voice-vibe-coding')"
                    >
                      <span>Gitee：</span>
                      <small>mwlt/remote-voice-vibe-coding</small>
                    </button>
                    <button
                      class="credit-link"
                      type="button"
                      aria-label="打开 Voice VibeCoding 版 GitHub 项目：mwlt/Voice_VibeCoding"
                      @click="openExternal('https://github.com/mwlt/Voice_VibeCoding')"
                    >
                      <span>GitHub：</span>
                      <small>mwlt/Voice_VibeCoding</small>
                    </button>
                  </div>
                </article>
                <article class="source-card">
                  <h3>Python Windows版</h3>
                  <p class="source-author"><span>作者：</span><strong>xxb26553663-star</strong></p>
                  <div class="source-links">
                    <button
                      class="credit-link"
                      type="button"
                      aria-label="打开 Python Windows 版 GitHub 项目：xxb26553663-star/remote-bridge-hub"
                      @click="openExternal('https://github.com/xxb26553663-star/remote-bridge-hub')"
                    >
                      <span>GitHub：</span>
                      <small>xxb26553663-star/remote-bridge-hub</small>
                    </button>
                  </div>
                </article>
                <article class="source-card">
                  <h3>macOS版</h3>
                  <p class="source-author"><span>作者：</span><strong>nijez</strong></p>
                  <div class="source-links">
                    <button
                      class="credit-link"
                      type="button"
                      aria-label="打开 macOS 版 GitHub 项目：nijez/open-voice-bridge"
                      @click="openExternal('https://github.com/nijez/open-voice-bridge')"
                    >
                      <span>GitHub：</span>
                      <small>nijez/open-voice-bridge</small>
                    </button>
                  </div>
                </article>
              </div>
            </section>
          </div>
        </section>
        </template>
      </main>
    </div>
  </div>
</template>

<style scoped>
.settings-page { width: 100%; max-width: 1260px; margin: 0 auto; box-sizing: border-box; }
.settings-header { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; margin-bottom: 20px; }
.credit-kicker { margin: 0 0 7px; color: var(--primary); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 10px; font-weight: 800; letter-spacing: 0.13em; }
.settings-header h1 { margin: 0; color: var(--text); font-size: 27px; font-weight: 800; letter-spacing: -0.65px; }
.settings-header > div > p:last-child { margin: 7px 0 0; color: var(--text-secondary); font-size: 13px; }
.save-area { display: flex; align-items: center; justify-content: flex-end; gap: 10px; min-height: 36px; }
.saved-state, .save-error { margin: 0; font-size: 12px; font-weight: 700; white-space: nowrap; }
.saved-state { display: inline-flex; align-items: center; gap: 6px; color: var(--success-text); }
.saved-state span { display: grid; width: 17px; height: 17px; place-items: center; border-radius: 50%; color: var(--success); background: var(--success-bg); }
.save-error { color: var(--danger); }
.save-button { display: inline-flex; align-items: center; justify-content: center; gap: 7px; min-width: 108px; min-height: 36px; padding: 0 13px; border: 1px solid transparent; border-radius: 8px; color: #fff; background: var(--primary); box-shadow: 0 5px 13px rgb(var(--primary-rgb) / 22%); box-shadow: 0 5px 13px color-mix(in srgb, var(--primary) 22%, transparent); font: inherit; font-size: 12px; font-weight: 700; cursor: pointer; transition: transform 0.16s ease, background 0.16s ease, opacity 0.16s ease; }
.save-button:hover:not(:disabled) { background: var(--primary-dark); transform: translateY(-1px); }
.save-button:disabled { opacity: 0.55; cursor: default; }
.save-button.is-saving > span { animation: settings-spin 1s linear infinite; }
@keyframes settings-spin { to { transform: rotate(360deg); } }

.settings-layout { display: grid; grid-template-columns: 208px minmax(0, 1fr); align-items: start; gap: 20px; }
.settings-nav { position: sticky; top: 18px; padding: 12px 9px; border: 1px solid var(--border); border-radius: 13px; background: var(--card-bg); box-shadow: 0 8px 22px var(--shadow); }
.settings-nav > p { margin: 0 9px 8px; color: var(--text-muted); font-size: 10px; font-weight: 800; letter-spacing: 0.11em; text-transform: uppercase; }
.settings-nav button { display: grid; width: 100%; gap: 3px; padding: 10px 11px; border: 0; border-radius: 8px; color: var(--text-secondary); background: transparent; font: inherit; text-align: left; cursor: pointer; transition: background 0.16s ease, color 0.16s ease; }
.settings-nav button:hover { color: var(--text); background: var(--surface-soft); }
.settings-nav button.active { color: var(--primary-dark); background: var(--surface-selected); }
.settings-nav button span { font-size: 13px; font-weight: 700; }
.settings-nav button small { font-size: 10px; }
.settings-content { display: grid; gap: 16px; min-width: 0; }
.settings-card { padding: 22px; border: 1px solid var(--border); border-radius: 14px; background: var(--card-bg); box-shadow: 0 10px 28px var(--shadow); }
.card-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 14px; padding-bottom: 14px; border-bottom: 1px solid var(--border); }
.card-head h2 { margin: 0; color: var(--text); font-size: 17px; font-weight: 800; letter-spacing: -0.25px; }
.card-head p { margin: 6px 0 0; color: var(--text-secondary); font-size: 12px; }
.settings-list { display: grid; }
.preference-row { display: grid; grid-template-columns: 34px minmax(0, 1fr) auto; align-items: center; gap: 12px; min-height: 74px; border-bottom: 1px solid var(--border); }
.preference-row:last-child { border-bottom: 0; }
.preference-icon { display: grid; width: 34px; height: 34px; place-items: center; border-radius: 9px; color: var(--primary); background: var(--surface-selected); font-size: 15px; font-weight: 800; }
.preference-copy { display: grid; gap: 4px; min-width: 0; }
.preference-copy strong { color: var(--text); font-size: 13px; font-weight: 700; }
.preference-copy span { color: var(--text-secondary); font-size: 11px; line-height: 1.4; }
.preference-copy em { display: inline-block; margin-left: 6px; padding: 2px 5px; border-radius: 4px; color: var(--text-muted); background: var(--surface-muted); font-size: 9px; font-style: normal; font-weight: 700; vertical-align: 1px; }
.toggle { position: relative; display: inline-block; width: 42px; height: 23px; flex: 0 0 auto; }
.toggle input { position: absolute; width: 1px; height: 1px; margin: -1px; opacity: 0; }
.toggle-slider { position: absolute; top: 0; right: 0; bottom: 0; left: 0; border-radius: 999px; background: var(--border-strong); cursor: pointer; transition: background 0.2s ease; }
.toggle-slider::before { position: absolute; top: 3px; left: 3px; width: 17px; height: 17px; border-radius: 50%; background: var(--control-thumb); box-shadow: 0 1px 2px var(--overlay); content: ""; transition: transform 0.2s ease; }
.toggle input:checked + .toggle-slider { background: var(--primary); }
.toggle input:checked + .toggle-slider::before { transform: translateX(19px); }
.toggle input:focus-visible + .toggle-slider { outline: 2px solid var(--primary); outline-offset: 2px; }
.is-disabled { opacity: 0.55; }
.disabled-toggle { width: 42px; height: 23px; border-radius: 999px; background: var(--border); }
.disabled-toggle::before { display: block; width: 17px; height: 17px; margin: 3px; border-radius: 50%; background: var(--control-thumb); content: ""; }

.appearance-list { gap: 0; }
.language-select { min-width: 130px; height: 34px; padding: 0 27px 0 10px; border: 1px solid var(--border); border-radius: 7px; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 12px; }
.theme-preference { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 12px; min-height: 92px; }
.theme-copy { display: flex; align-items: center; gap: 12px; }
.theme-segmented { display: inline-flex; padding: 3px; border: 1px solid var(--border); border-radius: 9px; background: var(--surface-soft); }
.theme-segmented button { min-height: 29px; padding: 0 10px; border: 0; border-radius: 6px; color: var(--text-secondary); background: transparent; font: inherit; font-size: 11px; font-weight: 700; cursor: pointer; }
.theme-segmented button.active { color: var(--text); background: var(--card-bg); box-shadow: 0 1px 4px var(--shadow); }
.theme-segmented button:disabled { cursor: default; opacity: 0.7; }
.theme-error { grid-column: 1 / -1; margin: -8px 0 12px; color: var(--danger); font-size: 11px; }

.settings-loading-card { min-height: 282px; pointer-events: none; }
.loading-card-head { display: grid; gap: 9px; padding-bottom: 17px; border-bottom: 1px solid var(--border); }
.loading-card-head span, .loading-settings-list > span { display: block; border-radius: 999px; background: linear-gradient(90deg, var(--surface-soft), var(--surface-hover), var(--surface-soft)); background-size: 200% 100%; animation: settings-loading-shimmer 1.25s ease-in-out infinite; }
.loading-card-head span:first-child { width: 116px; height: 10px; }
.loading-card-head span:last-child { width: 188px; height: 19px; }
.loading-settings-list { display: grid; }
.loading-settings-list > span { height: 74px; border-bottom: 1px solid var(--border); border-radius: 0; }
.loading-settings-list > span:last-child { border-bottom: 0; }
@keyframes settings-loading-shimmer { to { background-position: -200% 0; } }
.about-overview { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin: 17px 0; }
.about-overview > div { padding: 11px; border-radius: 9px; background: var(--surface-soft); }
.about-overview span { display: block; margin-bottom: 6px; color: var(--text-secondary); font-size: 10px; }
.about-overview strong { display: block; overflow: hidden; color: var(--text); font-size: 12px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
.credits-grid { display: grid; grid-template-columns: minmax(210px, 0.78fr) minmax(0, 1.22fr); align-items: stretch; gap: 14px; padding-top: 17px; border-top: 1px solid var(--border); }
.author-list, .source-list { display: grid; grid-template-rows: auto minmax(0, 1fr); min-width: 0; }
.author-list > .credit-kicker, .source-list > .credit-kicker { margin-bottom: 8px; }
.author-card { display: grid; grid-template-rows: auto auto minmax(0, 1fr); gap: 12px; min-width: 0; padding: 14px; border-radius: 10px; background: var(--surface-soft); }
.author-identity { display: flex; align-items: flex-start; gap: 12px; min-width: 0; }
.author-avatar { width: 54px; height: 54px; flex: 0 0 54px; border: 1px solid var(--border); border-radius: 10px; object-fit: cover; background: #000; }
.author-card-copy { min-width: 0; flex: 1; }
.author-card h3 { margin: 0; color: var(--text); font-size: 14px; }
.author-description, .author-contact { margin: 4px 0 0; color: var(--text-secondary); font-size: 10px; line-height: 1.4; }
.author-contact { display: block; color: var(--text-muted); }
.source-cards { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-items: stretch; gap: 7px; }
.source-card { display: flex; min-width: 0; padding: 11px; border: 1px solid var(--border); border-radius: 9px; background: var(--surface-soft); flex-direction: column; }
.source-card-featured { grid-column: 1 / -1; }
.source-card h3 { margin: 0; color: var(--text); font-size: 12px; line-height: 1.35; }
.source-author { display: flex; min-width: 0; margin: 5px 0 10px; color: var(--text-secondary); font-size: 10px; line-height: 1.4; flex-wrap: wrap; }
.source-author strong { color: var(--text); font-weight: 700; overflow-wrap: anywhere; }
.source-links { display: grid; gap: 6px; margin-top: auto; }
.credit-link { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: baseline; gap: 2px; width: 100%; min-width: 0; min-height: 34px; padding: 8px; border: 1px solid var(--border); border-radius: 7px; color: var(--text); background: var(--surface-raised); font: inherit; text-align: left; cursor: pointer; transition: border-color 0.15s ease, transform 0.15s ease, background 0.15s ease; }
.credit-link:hover { border-color: var(--border); border-color: color-mix(in srgb, var(--primary) 45%, var(--border)); background: var(--card-bg); transform: translateY(-1px); }
.credit-link:focus-visible { border-color: var(--primary); outline: 2px solid rgb(var(--primary-rgb) / 60%); outline: 2px solid color-mix(in srgb, var(--primary) 60%, transparent); outline-offset: 2px; }
.credit-link span { color: var(--text); font-size: 10px; font-weight: 700; white-space: nowrap; }
.credit-link small { min-width: 0; color: var(--text-secondary); font-size: 9px; line-height: 1.4; overflow-wrap: anywhere; }
.author-project-link { margin: 0; }
.update-orbit { --update-accent: var(--primary); --update-accent-rgb: var(--primary-rgb); position: relative; display: grid; grid-template-columns: 26px minmax(44px, 1fr) auto; align-items: center; gap: 8px; width: 100%; min-height: 46px; align-self: end; overflow: hidden; padding: 8px 11px; border: 1px solid var(--border); border: 1px solid color-mix(in srgb, var(--update-accent) 38%, var(--border)); border-radius: 999px; color: var(--text); background: var(--surface-raised); box-shadow: inset 0 1px 0 rgb(var(--text-inverse-rgb) / 24%), 0 8px 18px var(--shadow); box-shadow: inset 0 1px 0 color-mix(in srgb, var(--text-inverse) 24%, transparent), 0 8px 18px var(--shadow); font: inherit; cursor: pointer; isolation: isolate; transition: border-color .16s ease, transform .16s ease, box-shadow .16s ease, background .16s ease; }
.update-orbit::before { position: absolute; z-index: 0; top: 50%; left: 13px; width: 44px; height: 44px; border-radius: 50%; background: rgb(var(--update-accent-rgb) / 24%); background: color-mix(in srgb, var(--update-accent) 24%, transparent); filter: blur(14px); transform: translateY(-50%); content: ""; }
.update-orbit > span { position: relative; z-index: 1; }
.update-orbit:hover:not(:disabled) { border-color: var(--border); border-color: color-mix(in srgb, var(--update-accent) 65%, var(--border)); background: var(--card-bg); box-shadow: inset 0 1px 0 rgb(var(--text-inverse-rgb) / 28%), 0 10px 21px rgb(var(--update-accent-rgb) / 17%); box-shadow: inset 0 1px 0 color-mix(in srgb, var(--text-inverse) 28%, transparent), 0 10px 21px color-mix(in srgb, var(--update-accent) 17%, transparent); transform: translateY(-1px); }
.update-orbit:focus-visible { outline: 2px solid var(--update-accent); outline-offset: 3px; }
.update-orbit:disabled { cursor: wait; opacity: .8; }
.update-orbit.is-latest, .update-orbit.is-ready { --update-accent: var(--success); --update-accent-rgb: var(--success-rgb); }
.update-orbit.is-error { --update-accent: var(--danger); --update-accent-rgb: var(--danger-rgb); }
.update-orbit.is-available { --update-accent: var(--primary); --update-accent-rgb: var(--primary-rgb); }
.update-orbit-icon { display: grid; width: 26px; height: 26px; place-items: center; border-radius: 50%; color: #fff; background: var(--update-accent); box-shadow: 0 0 14px rgb(var(--update-accent-rgb) / 82%); box-shadow: 0 0 14px color-mix(in srgb, var(--update-accent) 82%, transparent); }
.update-orbit-icon svg { width: 14px; height: 14px; }
.update-orbit-track { height: 6px; overflow: hidden; border-radius: 999px; background: rgb(var(--text-rgb) / 14%); background: color-mix(in srgb, var(--text) 14%, transparent); box-shadow: inset 0 1px 2px rgb(var(--text-rgb) / 18%); box-shadow: inset 0 1px 2px color-mix(in srgb, var(--text) 18%, transparent); }
.update-orbit-track i { display: block; height: 100%; border-radius: inherit; background: var(--update-accent); box-shadow: 0 0 10px rgb(var(--update-accent-rgb) / 78%); box-shadow: 0 0 10px color-mix(in srgb, var(--update-accent) 78%, transparent); transition: width .2s ease; }
.update-orbit-track i.indeterminate { width: 48% !important; animation: update-orbit-scan 1.1s ease-in-out infinite; }
.update-orbit-label { min-width: max-content; color: var(--text); font-size: 10px; font-weight: 800; white-space: nowrap; }
@keyframes update-orbit-scan { 0%, 100% { transform: translateX(-48%); } 50% { transform: translateX(112%); } }

@media (max-width: 900px) {
  .settings-layout { grid-template-columns: 1fr; gap: 13px; }
  .settings-nav { position: static; display: flex; align-items: stretch; gap: 5px; padding: 7px; overflow-x: auto; }
  .settings-nav > p { display: none; }
  .settings-nav button { min-width: 130px; }
}
@media (max-width: 680px) {
  .settings-header { align-items: flex-start; flex-direction: column; margin-bottom: 14px; }
  .settings-header h1 { font-size: 24px; }
  .save-area { justify-content: flex-start; flex-wrap: wrap; }
  .settings-card { padding: 16px; }
  .preference-row { grid-template-columns: 32px minmax(0, 1fr); padding: 12px 0; }
  .preference-row > :last-child { grid-column: 2; justify-self: start; margin-top: 2px; }
  .language-row .language-select { min-width: 0; width: 100%; }
  .theme-preference { grid-template-columns: 1fr; padding: 13px 0; }
  .theme-segmented { justify-self: stretch; }
  .theme-segmented button { flex: 1; }
  .about-overview, .credits-grid { grid-template-columns: 1fr; }
  .author-list, .source-list { grid-template-rows: auto auto; }
  .source-cards { grid-template-columns: 1fr; }
  .source-card-featured { grid-column: auto; }
  .credit-link { min-height: 38px; }
}
@media (max-width: 420px) {
  .author-identity { align-items: stretch; flex-direction: column; }
}
</style>
