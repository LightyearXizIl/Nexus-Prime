<script setup lang="ts">
import { onMounted, onUnmounted, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useThemeStore } from "../stores/theme";
import type { GlobalSettings, ThemePreference } from "../types";
import lightYearAuthor from "../assets/light_year_author.jpg";

type SectionId = "general" | "appearance" | "about";

const settings = ref<GlobalSettings>({
  autostart: false,
  language: "zh-CN",
  minimize_to_tray: true,
  theme: "system",
});
const theme = useThemeStore();
const saved = ref(true);
const saving = ref(false);
const saveError = ref("");
const appVersion = ref("v0.0.4");
const activeSection = ref<SectionId>("general");
const sectionRefs: Record<SectionId, Ref<HTMLElement | null>> = {
  general: ref(null),
  appearance: ref(null),
  about: ref(null),
};
let sectionObserver: IntersectionObserver | undefined;

const navigation: Array<{ id: SectionId; label: string; caption: string }> = [
  { id: "general", label: "通用", caption: "启动与窗口" },
  { id: "appearance", label: "外观与语言", caption: "显示偏好" },
  { id: "about", label: "关于", caption: "版本与致谢" },
];

onMounted(async () => {
  try {
    const loaded = await invoke<GlobalSettings>("get_global_settings");
    settings.value = loaded;
  } catch (error) {
    console.error("Failed to load settings:", error);
  }
  try {
    appVersion.value = `v${(await getVersion()).replace(/\.0$/, "")}`;
  } catch (error) {
    console.warn("Failed to read app version:", error);
  }

  sectionObserver = new IntersectionObserver(
    (entries) => {
      const visible = entries
        .filter((entry) => entry.isIntersecting)
        .sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0];
      const id = visible?.target.getAttribute("data-settings-section") as SectionId | null;
      if (id) activeSection.value = id;
    },
    { rootMargin: "-17% 0px -15% 0px", threshold: [0.05, 0.25, 0.55] },
  );
  Object.values(sectionRefs).forEach((section) => {
    if (section.value) sectionObserver?.observe(section.value);
  });
});

onUnmounted(() => sectionObserver?.disconnect());

async function saveSettings() {
  if (saved.value || saving.value) return;
  saving.value = true;
  saveError.value = "";
  try {
    const settingsToSave = { ...settings.value, theme: theme.preference };
    await invoke("save_global_settings", { settings: settingsToSave });
    settings.value = settingsToSave;
    saved.value = true;
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

function scrollToSection(id: SectionId) {
  activeSection.value = id;
  sectionRefs[id].value?.scrollIntoView({ behavior: "smooth", block: "start" });
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
        <p class="settings-eyebrow">PREFERENCES</p>
        <h1>全局设置</h1>
        <p>管理应用启动、外观与工作环境偏好。</p>
      </div>
      <div class="save-area">
        <p v-if="saveError" class="save-error" role="alert">{{ saveError }}</p>
        <p v-else-if="saved" class="saved-state" role="status"><span aria-hidden="true">✓</span>所有更改已保存</p>
        <button class="save-button" :class="{ 'is-saving': saving }" :disabled="saved || saving" @click="saveSettings">
          <span aria-hidden="true">{{ saving ? '↻' : '↓' }}</span>
          {{ saving ? "正在保存…" : "保存设置" }}
        </button>
      </div>
    </header>

    <div class="settings-layout">
      <aside class="settings-nav" aria-label="设置分类">
        <p>设置分类</p>
        <button
          v-for="item in navigation"
          :key="item.id"
          type="button"
          :class="{ active: activeSection === item.id }"
          @click="scrollToSection(item.id)"
        >
          <span>{{ item.label }}</span>
          <small>{{ item.caption }}</small>
        </button>
      </aside>

      <main class="settings-content">
        <section :ref="sectionRefs.general" class="settings-card" data-settings-section="general">
          <div class="card-head">
            <div>
              <p class="card-kicker">GENERAL</p>
              <h2>通用</h2>
              <p>应用启动方式与窗口行为。</p>
            </div>
          </div>
          <div class="settings-list">
            <div class="preference-row">
              <div class="preference-icon" aria-hidden="true">↗</div>
              <div class="preference-copy">
                <strong>开机自启</strong>
                <span>Windows 启动后自动运行 Nexus Prime</span>
              </div>
              <label class="toggle" title="开机自启">
                <input v-model="settings.autostart" type="checkbox" aria-label="开机自启" @change="onSettingChange" />
                <span class="toggle-slider"></span>
              </label>
            </div>
            <div class="preference-row">
              <div class="preference-icon" aria-hidden="true">⊟</div>
              <div class="preference-copy">
                <strong>最小化到托盘</strong>
                <span>关闭窗口时隐藏到托盘，可从托盘重新打开</span>
              </div>
              <label class="toggle" title="最小化到托盘">
                <input v-model="settings.minimize_to_tray" type="checkbox" aria-label="最小化到托盘" @change="onSettingChange" />
                <span class="toggle-slider"></span>
              </label>
            </div>
            <div class="preference-row is-disabled" aria-disabled="true">
              <div class="preference-icon" aria-hidden="true">↑</div>
              <div class="preference-copy">
                <strong>自动检查更新 <em>开发中</em></strong>
                <span>将在后续版本中提供可选的更新提醒</span>
              </div>
              <span class="disabled-toggle" aria-hidden="true"></span>
            </div>
          </div>
        </section>

        <section :ref="sectionRefs.appearance" class="settings-card" data-settings-section="appearance">
          <div class="card-head">
            <div>
              <p class="card-kicker">APPEARANCE</p>
              <h2>外观与语言</h2>
              <p>主题即时生效，语言将在后续界面中逐步覆盖。</p>
            </div>
          </div>
          <div class="settings-list appearance-list">
            <div class="preference-row language-row">
              <div class="preference-icon" aria-hidden="true">文</div>
              <div class="preference-copy">
                <strong>界面语言</strong>
                <span>选择应用程序的显示语言</span>
              </div>
              <select v-model="settings.language" class="language-select" aria-label="界面语言" @change="onSettingChange">
                <option value="zh-CN">简体中文</option>
                <option value="zh-TW">繁體中文</option>
                <option value="en">English</option>
              </select>
            </div>
            <div class="theme-preference">
              <div class="theme-copy">
                <div class="preference-icon" aria-hidden="true">◐</div>
                <div class="preference-copy">
                  <strong>外观主题</strong>
                  <span>即时应用并保存你的主题偏好</span>
                </div>
              </div>
              <div class="theme-segmented" role="group" aria-label="外观主题">
                <button v-for="option in ([['light', '浅色'], ['system', '跟随系统'], ['dark', '深色']] as const)" :key="option[0]" type="button" :disabled="theme.saving" :class="{ active: theme.preference === option[0] }" @click="setTheme(option[0])">
                  {{ option[1] }}
                </button>
              </div>
              <p v-if="theme.error" class="theme-error" role="alert">{{ theme.error }}</p>
            </div>
          </div>
        </section>

        <section :ref="sectionRefs.about" class="settings-card about-card" data-settings-section="about">
          <div class="card-head">
            <div>
              <p class="card-kicker">ABOUT NEXUS PRIME</p>
              <h2>关于</h2>
              <p>Windows 桌面版遥控器语音桥接工具。</p>
            </div>
            <span class="version-badge">{{ appVersion }}</span>
          </div>
          <div class="about-overview">
            <div><span>当前版本</span><strong>{{ appVersion }}</strong></div>
            <div><span>运行时</span><strong>Rust · Tauri 2 · Vue 3</strong></div>
            <div><span>支持设备</span><strong>小米遥控器 2 Pro</strong></div>
          </div>
          <div class="credits-grid">
            <article class="author-card">
              <img class="author-avatar" :src="lightYearAuthor" alt="Light year" />
              <div>
                <p class="credit-kicker">NEXUS PRIME</p>
                <h3>Light year</h3>
                <p>Windows 版重构与维护</p>
                <span>微信号：XizllHZ_007</span>
              </div>
            </article>
            <div class="source-list">
              <p class="credit-kicker">源码与致谢</p>
              <button type="button" @click="openExternal('https://gitee.com/mwlt/remote-voice-vibe-coding')"><span>原版 · Gitee</span><small>mwlt / remote-voice-vibe-coding</small></button>
              <button type="button" @click="openExternal('https://github.com/mwlt/Voice_VibeCoding')"><span>原版 · GitHub</span><small>mwlt / Voice_VibeCoding</small></button>
              <button type="button" @click="openExternal('https://github.com/xxb26553663-star/remote-bridge-hub')"><span>Python Windows 版</span><small>xxb26553663-star / remote-bridge-hub</small></button>
              <button type="button" @click="openExternal('https://github.com/nijez/open-voice-bridge')"><span>macOS 版</span><small>nijez / open-voice-bridge</small></button>
            </div>
          </div>
        </section>
      </main>
    </div>
  </div>
</template>

<style scoped>
.settings-page { width: 100%; max-width: 1260px; margin: 0 auto; box-sizing: border-box; }
.settings-header { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; margin-bottom: 20px; }
.settings-eyebrow, .card-kicker, .credit-kicker { margin: 0 0 7px; color: var(--primary); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 10px; font-weight: 800; letter-spacing: 0.13em; }
.settings-header h1 { margin: 0; color: var(--text); font-size: 27px; font-weight: 780; letter-spacing: -0.65px; }
.settings-header > div > p:last-child { margin: 7px 0 0; color: var(--text-secondary); font-size: 13px; }
.save-area { display: flex; align-items: center; justify-content: flex-end; gap: 10px; min-height: 36px; }
.saved-state, .save-error { margin: 0; font-size: 12px; font-weight: 700; white-space: nowrap; }
.saved-state { display: inline-flex; align-items: center; gap: 6px; color: var(--success-text); }
.saved-state span { display: grid; width: 17px; height: 17px; place-items: center; border-radius: 50%; color: var(--success); background: var(--success-bg); }
.save-error { color: var(--danger); }
.save-button { display: inline-flex; align-items: center; justify-content: center; gap: 7px; min-width: 108px; min-height: 36px; padding: 0 13px; border: 1px solid transparent; border-radius: 8px; color: #fff; background: var(--primary); box-shadow: 0 5px 13px color-mix(in srgb, var(--primary) 22%, transparent); font: inherit; font-size: 12px; font-weight: 750; cursor: pointer; transition: transform 0.16s ease, background 0.16s ease, opacity 0.16s ease; }
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
.settings-nav button span { font-size: 13px; font-weight: 750; }
.settings-nav button small { font-size: 10px; }
.settings-content { display: grid; gap: 16px; min-width: 0; }
.settings-card { scroll-margin-top: 18px; padding: 22px; border: 1px solid var(--border); border-radius: 14px; background: var(--card-bg); box-shadow: 0 10px 28px var(--shadow); }
.card-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 14px; padding-bottom: 17px; border-bottom: 1px solid var(--border); }
.card-head h2 { margin: 0; color: var(--text); font-size: 17px; font-weight: 770; letter-spacing: -0.25px; }
.card-head p:not(.card-kicker) { margin: 6px 0 0; color: var(--text-secondary); font-size: 12px; }
.settings-list { display: grid; }
.preference-row { display: grid; grid-template-columns: 34px minmax(0, 1fr) auto; align-items: center; gap: 12px; min-height: 74px; border-bottom: 1px solid var(--border); }
.preference-row:last-child { border-bottom: 0; }
.preference-icon { display: grid; width: 34px; height: 34px; place-items: center; border-radius: 9px; color: var(--primary); background: var(--surface-selected); font-size: 15px; font-weight: 800; }
.preference-copy { display: grid; gap: 4px; min-width: 0; }
.preference-copy strong { color: var(--text); font-size: 13px; font-weight: 760; }
.preference-copy span { color: var(--text-secondary); font-size: 11px; line-height: 1.4; }
.preference-copy em { display: inline-block; margin-left: 6px; padding: 2px 5px; border-radius: 4px; color: var(--text-muted); background: var(--surface-muted); font-size: 9px; font-style: normal; font-weight: 720; vertical-align: 1px; }
.toggle { position: relative; display: inline-block; width: 42px; height: 23px; flex: 0 0 auto; }
.toggle input { position: absolute; width: 1px; height: 1px; margin: -1px; opacity: 0; }
.toggle-slider { position: absolute; inset: 0; border-radius: 999px; background: var(--border-strong); cursor: pointer; transition: background 0.2s ease; }
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

.version-badge { padding: 5px 8px; border-radius: 6px; color: var(--primary-dark); background: var(--surface-selected); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 11px; font-weight: 800; }
.about-overview { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin: 17px 0; }
.about-overview > div { padding: 11px; border-radius: 9px; background: var(--surface-soft); }
.about-overview span { display: block; margin-bottom: 6px; color: var(--text-secondary); font-size: 10px; }
.about-overview strong { display: block; overflow: hidden; color: var(--text); font-size: 12px; font-weight: 730; text-overflow: ellipsis; white-space: nowrap; }
.credits-grid { display: grid; grid-template-columns: minmax(210px, 0.78fr) minmax(0, 1.22fr); gap: 14px; padding-top: 17px; border-top: 1px solid var(--border); }
.author-card { display: flex; align-items: center; gap: 12px; padding: 14px; border-radius: 10px; background: var(--surface-soft); }
.author-avatar { width: 54px; height: 54px; flex: 0 0 54px; border: 1px solid var(--border); border-radius: 10px; object-fit: cover; background: #000; }
.author-card h3 { margin: 0; color: var(--text); font-size: 14px; }
.author-card p:not(.credit-kicker), .author-card span { margin: 4px 0 0; color: var(--text-secondary); font-size: 10px; line-height: 1.4; }
.author-card span { display: block; color: var(--text-muted); }
.source-list { display: grid; grid-template-columns: 1fr 1fr; gap: 7px; }
.source-list .credit-kicker { grid-column: 1 / -1; margin-bottom: 1px; }
.source-list button { display: grid; gap: 3px; min-width: 0; padding: 9px; border: 1px solid var(--border); border-radius: 8px; color: var(--text); background: var(--surface-raised); font: inherit; text-align: left; cursor: pointer; transition: border-color 0.15s ease, transform 0.15s ease; }
.source-list button:hover { border-color: color-mix(in srgb, var(--primary) 45%, var(--border)); transform: translateY(-1px); }
.source-list button span { overflow: hidden; font-size: 11px; font-weight: 720; text-overflow: ellipsis; white-space: nowrap; }
.source-list button small { overflow: hidden; color: var(--text-secondary); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }

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
  .about-overview, .credits-grid, .source-list { grid-template-columns: 1fr; }
  .source-list .credit-kicker { grid-column: auto; }
}
</style>
