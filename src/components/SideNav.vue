<script setup lang="ts">
import AppIcon from "./AppIcon.vue";
import { computed, ref, onMounted, onUnmounted } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { loggedInvoke as invoke } from "../utils/appLogger";
import { useRoute, useRouter } from "vue-router";
import { useBridgeStore } from "../stores/bridge";
import { useThemeStore } from "../stores/theme";
import { useUpdateStore } from "../stores/update";
import { useI18n } from "vue-i18n";
import type { BridgeStatus } from "../types";
import { connectedDeviceName, connectionStatusPresentation } from "../utils/connectionStatus";

const maximized = ref(false);
let unlistenResize: UnlistenFn | undefined;
async function controlWindow(action: string) {
  if (!isTauri()) return;
  try { maximized.value = await invoke<boolean>("control_main_window", { action }); }
  catch (error) { console.error("Window control failed", error); }
}
onMounted(async () => {
  if (!isTauri()) return;
  await controlWindow("state");
  unlistenResize = await listen("tauri://resize", () => { void controlWindow("state"); });
});
onUnmounted(() => unlistenResize?.());

const route = useRoute();
const router = useRouter();
const bridge = useBridgeStore();
const theme = useThemeStore();
const update = useUpdateStore();
const { t } = useI18n();

const appVersion = computed(() => update.currentVersion);

const device = computed(() => bridge.devices.xiaomi);
const isDevicePage = computed(() => route.path === "/xiaomi" || route.path === "/xiaomi/");
const isMappingPage = computed(() => route.path === "/xiaomi/mapping");
const themeToggleLabel = computed(() =>
  theme.effectiveTheme === "dark" ? t("nav.light") : t("nav.dark")
);
const devicePresentation = computed(() => connectionStatusPresentation(device.value.status, device.value));
const deviceLabel = computed(() => {
  const name = connectedDeviceName(device.value.status, device.value.device_name);
  return devicePresentation.value.tone === "connected" && name ? name : t(devicePresentation.value.labelKey === "status.disconnected" ? "status.deviceNotConnected" : devicePresentation.value.labelKey);
});

function statusClass(status: BridgeStatus): string {
  return connectionStatusPresentation(status, device.value).tone;
}

function navigate(path: "/xiaomi" | "/xiaomi/mapping" | "/settings") {
  if (route.path !== path) void router.push(path);
}

function toggleTheme() {
  void theme.toggle();
}

</script>

<template>
  <div class="titlebar" data-tauri-drag-region aria-label="应用标题栏">
    <div class="titlebar-drag" data-tauri-drag-region></div>
    <div class="window-controls" role="group" aria-label="窗口控制">
      <button type="button" class="window-button" aria-label="最小化" title="最小化" @click="controlWindow('minimize')"><AppIcon name="minus" :size="14" /></button>
      <button type="button" class="window-button" :aria-label="maximized ? '还原窗口' : '最大化'" :title="maximized ? '还原窗口' : '最大化'" @click="controlWindow('maximize')"><AppIcon :name="maximized ? 'restore' : 'maximize'" :size="13" /></button>
      <button type="button" class="window-button window-close" aria-label="关闭窗口" title="关闭窗口" @click="controlWindow('close')"><AppIcon name="close" :size="16" /></button>
    </div>
  </div>
  <header class="topnav">
    <div class="brand-block">
      <div class="brand-symbol" aria-hidden="true">
        <img src="/favicon.png" alt="" />
      </div>
      <div class="brand-copy">
        <strong>Nexus Prime</strong>
        <button v-if="update.canOpen" type="button" class="brand-version is-update" :aria-label="`发现新版本 ${update.release?.version}，查看更新详情`" @click="update.openDialog">
          {{ appVersion }}<i aria-hidden="true"></i>
        </button>
        <span v-else class="brand-version">{{ appVersion }}</span>
      </div>
      <div
        :class="['device-chip', statusClass(device.status)]"
        :title="devicePresentation.detail || bridge.statusLabel(device.status)"
      >
        <span class="status-dot status-light" :class="statusClass(device.status)" aria-hidden="true" />
        <span>{{ deviceLabel }}</span>
      </div>
    </div>

    <nav class="main-nav" :aria-label="t('nav.main')">
      <button
        type="button"
        :class="{ active: isDevicePage }"
        :aria-current="isDevicePage ? 'page' : undefined"
        @click="navigate('/xiaomi')"
      >
        {{ t("nav.home") }}
      </button>
      <button
        type="button"
        :class="{ active: isMappingPage }"
        :aria-current="isMappingPage ? 'page' : undefined"
        @click="navigate('/xiaomi/mapping')"
      >
        {{ t("nav.mapping") }}
      </button>
    </nav>

    <div class="top-actions">
      <button
        type="button"
        class="icon-button"
        :disabled="theme.saving"
        :title="themeToggleLabel"
        :aria-label="themeToggleLabel"
        @click="toggleTheme"
      >
        <AppIcon :name="theme.effectiveTheme === 'dark' ? 'sun' : 'moon'" />
      </button>
      <button
        type="button"
        :class="['icon-button', { active: route.path === '/settings' }]"
        :title="t('nav.settings')"
        :aria-label="t('nav.settings')"
        @click="navigate('/settings')"
      >
        <AppIcon name="settings" />
      </button>
    </div>
  </header>
</template>

<style scoped>
.topnav {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
  align-items: center;
  gap: 16px;
  min-height: 68px;
  padding: 0 28px;
  background: var(--nav);
  color: var(--nav-ink);
  border-bottom: 1px solid var(--nav-border);
  box-shadow: 0 1px 0 var(--nav-shadow);
  user-select: none;
  flex-shrink: 0;
}

.brand-block,
.top-actions,
.main-nav,
.device-chip {
  display: flex;
  align-items: center;
}

.brand-block {
  min-width: 0;
  gap: 12px;
}

.brand-symbol {
  width: 34px;
  height: 34px;
  display: grid;
  flex: 0 0 auto;
  place-items: center;
  overflow: hidden;
  border: 0;
  border-radius: 10px;
  background: transparent;
  box-shadow: none;
}

.brand-symbol img { display: block; width: 100%; height: 100%; object-fit: contain; }

.brand-copy {
  min-width: 0;
  flex: 0 0 auto;
}

.brand-copy strong,
.brand-version {
  display: block;
  white-space: nowrap;
}

.brand-copy strong { font-size: 15px; line-height: 1; letter-spacing: 0.1px; }
.brand-version { position: relative; margin-top: 5px; padding: 0; border: 0; color: var(--nav-muted); background: transparent; font-size: 10px; text-align: left; }
.brand-version.is-update { color: var(--primary); cursor: pointer; }
.brand-version.is-update:hover { color: var(--primary-dark); }
.brand-version.is-update i { position: absolute; right: -8px; bottom: -3px; width: 6px; height: 6px; border: 1px solid var(--nav); border-radius: 50%; background: var(--danger); box-shadow: 0 0 7px rgb(var(--danger-rgb) / 65%); box-shadow: 0 0 7px color-mix(in srgb, var(--danger) 65%, transparent); }

.device-chip {
  min-width: 0;
  height: 42px;
  gap: 7px;
  margin-left: 2px;
  padding: 0 14px;
  border: 1px solid var(--nav-segment-border);
  border-radius: 999px;
  color: var(--nav-device-ink);
  background: var(--nav-segment-bg);
  font-weight: 600;
  font-size: 13px;
}

.device-chip > span:last-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.status-dot {
  --status-light-color: #8792a7;
}
.device-chip.connected .status-dot { --status-light-color: var(--success); }
.device-chip.connecting .status-dot { --status-light-color: var(--warning); }
.device-chip.connecting { color: var(--warning-text); }
.device-chip.disconnected,
.device-chip.error { color: var(--danger-text); border-color: var(--nav-segment-border); border-color: color-mix(in srgb, var(--danger) 35%, var(--nav-segment-border)); background: var(--nav-segment-bg); background: color-mix(in srgb, var(--danger) 13%, var(--nav-segment-bg)); }
.device-chip.disconnected .status-dot,
.device-chip.error .status-dot { --status-light-color: var(--danger); }

.main-nav {
  gap: 3px;
  padding: 3px;
  border: 1px solid var(--nav-segment-border);
  border-radius: 999px;
  background: var(--nav-segment-bg);
}

.main-nav button {
  height: 34px;
  min-width: 84px;
  padding: 0 17px;
  border: 0;
  border-radius: 999px;
  color: var(--nav-muted);
  background: transparent;
  font: inherit;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  transition: 0.18s ease;
}

.main-nav button:hover { color: var(--nav-ink); background: var(--nav-segment-hover); }
.main-nav button.active {
  color: var(--nav-segment-active-ink);
  background: var(--nav-segment-active);
  box-shadow: var(--nav-segment-shadow);
}

.top-actions {
  justify-self: end;
  width: max-content;
  justify-content: flex-end;
  gap: 3px;
  padding: 3px;
  border: 1px solid var(--nav-segment-border);
  border-radius: 999px;
  background: var(--nav-segment-bg);
}

.icon-button {
  display: grid;
  place-items: center;
  height: 34px;
  border: 0;
  border-radius: 999px;
  color: var(--nav-icon);
  background: transparent;
  cursor: pointer;
  transition: color 0.18s ease, background 0.18s ease;
}

.icon-button { width: 34px; padding: 0; }
.icon-button svg { width: 17px; height: 17px; }
.icon-button:hover:not(:disabled) {
  color: var(--nav-icon-hover);
  background: var(--nav-icon-hover-bg);
}
.icon-button.active { color: var(--nav-segment-active-ink); background: var(--nav-segment-active); box-shadow: var(--nav-segment-shadow); }
.icon-button:disabled { cursor: wait; opacity: 0.58; }

@media (max-width: 1019px) {
  .topnav { gap: 10px; padding: 0 18px; }
  .brand-copy span { display: none; }
  .device-chip { max-width: 170px; }
}

@media (max-width: 900px) {
  .topnav { padding: 0 14px; }
  .brand-copy { display: none; }
  .device-chip { max-width: 154px; margin-left: 0; }
  .main-nav button { min-width: 76px; padding: 0 12px; }
}
</style>

<style scoped>
.titlebar { display: flex; height: 32px; flex: 0 0 32px; background: var(--nav); color: var(--nav-ink); user-select: none; }
.titlebar-drag { flex: 1; min-width: 0; }
.window-controls { display: flex; height: 32px; }
.window-button { display: grid; place-items: center; width: 46px; height: 32px; padding: 0; border: 0; border-radius: 0; background: transparent; color: var(--nav-icon); cursor: pointer; }
.window-button:hover { background: var(--nav-icon-hover-bg); color: var(--nav-ink); }
.window-close:hover { color: white; background: #c42b1c; }
.window-button:focus-visible { outline: 2px solid var(--primary); outline-offset: -4px; }
</style>
