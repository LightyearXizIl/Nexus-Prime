<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { useBridgeStore } from "../stores/bridge";
import { useThemeStore } from "../stores/theme";
import { useUpdateStore } from "../stores/update";
import type { BridgeStatus } from "../types";

const route = useRoute();
const router = useRouter();
const bridge = useBridgeStore();
const theme = useThemeStore();
const update = useUpdateStore();

const showQuitConfirm = ref(false);
const quitting = ref(false);
const accountMenuOpen = ref(false);
const accountMenuRef = ref<HTMLElement | null>(null);
const appVersion = computed(() => update.currentVersion);

const device = computed(() => bridge.devices.xiaomi);
const isDevicePage = computed(() => route.path === "/xiaomi" || route.path === "/xiaomi/");
const isMappingPage = computed(() => route.path === "/xiaomi/mapping");
const themeToggleLabel = computed(() =>
  theme.effectiveTheme === "dark" ? "切换为浅色模式" : "切换为深色模式"
);
const deviceLabel = computed(() => device.value.device_name || "小米 2 Pro");

function statusClass(status: BridgeStatus): string {
  if (status === "Connected") return "connected";
  if (status === "Connecting") return "connecting";
  if (status.startsWith("Error")) return "error";
  return "disconnected";
}

function navigate(path: "/xiaomi" | "/xiaomi/mapping" | "/settings") {
  accountMenuOpen.value = false;
  if (route.path !== path) void router.push(path);
}

function toggleTheme() {
  void theme.toggle();
}

function openQuitConfirm() {
  accountMenuOpen.value = false;
  showQuitConfirm.value = true;
}

function cancelQuit() {
  if (quitting.value) return;
  showQuitConfirm.value = false;
}

async function confirmQuit() {
  if (quitting.value) return;
  quitting.value = true;
  try {
    await invoke("quit_application");
  } catch (error) {
    console.error("quit_application failed:", error);
    quitting.value = false;
  }
}

function closeAccountMenuOnOutsidePointer(event: PointerEvent) {
  if (!accountMenuRef.value?.contains(event.target as Node)) {
    accountMenuOpen.value = false;
  }
}

onMounted(() => window.addEventListener("pointerdown", closeAccountMenuOnOutsidePointer));
onBeforeUnmount(() => window.removeEventListener("pointerdown", closeAccountMenuOnOutsidePointer));
</script>

<template>
  <header class="topnav">
    <div class="brand-block">
      <div class="brand-symbol" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none">
          <path d="M7 6.5h3.5v11H7zM13.5 6.5H17v11h-3.5z" fill="currentColor" opacity=".96" />
          <path d="M10.5 9.2h3v5.6h-3z" fill="currentColor" opacity=".62" />
        </svg>
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
        :title="bridge.statusLabel(device.status)"
      >
        <span class="status-dot" aria-hidden="true" />
        <span>{{ deviceLabel }}</span>
      </div>
    </div>

    <nav class="main-nav" aria-label="主导航">
      <button
        type="button"
        :class="{ active: isDevicePage }"
        :aria-current="isDevicePage ? 'page' : undefined"
        @click="navigate('/xiaomi')"
      >
        首页
      </button>
      <button
        type="button"
        :class="{ active: isMappingPage }"
        :aria-current="isMappingPage ? 'page' : undefined"
        @click="navigate('/xiaomi/mapping')"
      >
        按键映射
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
        <svg v-if="theme.effectiveTheme !== 'dark'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <path d="M20 15.4A8.2 8.2 0 0 1 8.6 4a8.2 8.2 0 1 0 11.4 11.4Z" />
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <circle cx="12" cy="12" r="4" />
          <path d="M12 2v2.2M12 19.8V22M4.93 4.93l1.56 1.56M17.51 17.51l1.56 1.56M2 12h2.2M19.8 12H22M4.93 19.07l1.56-1.56M17.51 6.49l1.56-1.56" />
        </svg>
      </button>
      <button
        type="button"
        :class="['icon-button', { active: route.path === '/settings' }]"
        title="设置"
        aria-label="设置"
        @click="navigate('/settings')"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.09a1.7 1.7 0 0 0-1.03-1.56 1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.56-1.03H3v-4h.09A1.7 1.7 0 0 0 4.65 8.9a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06a1.7 1.7 0 0 0 1.88.34A1.7 1.7 0 0 0 10.05 3h4v.09a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06a1.7 1.7 0 0 0-.34 1.88A1.7 1.7 0 0 0 21 10.05v4h-.09A1.7 1.7 0 0 0 19.4 15Z" />
        </svg>
      </button>
      <div ref="accountMenuRef" class="account-wrap">
        <button
          type="button"
          class="account-button"
          aria-label="应用菜单"
          :aria-expanded="accountMenuOpen"
          @click="accountMenuOpen = !accountMenuOpen"
        >
          NP
        </button>
        <div v-if="accountMenuOpen" class="account-menu" role="menu">
          <button type="button" role="menuitem" @click="openQuitConfirm">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
              <path d="M10 5H5v14h5M14 8l4 4-4 4M18 12H9" />
            </svg>
            退出应用
          </button>
        </div>
      </div>
    </div>
  </header>

  <Teleport to="body">
    <div
      v-if="showQuitConfirm"
      class="quit-backdrop"
      role="presentation"
      @click.self="cancelQuit"
    >
      <div class="quit-dialog" role="dialog" aria-modal="true" aria-labelledby="quit-title">
        <h3 id="quit-title">退出应用？</h3>
        <p>将彻底关闭软件（不会最小化到托盘）。确定要退出吗？</p>
        <div class="quit-actions">
          <button type="button" class="quit-btn quit-btn-ghost" :disabled="quitting" @click="cancelQuit">
            取消
          </button>
          <button type="button" class="quit-btn quit-btn-danger" :disabled="quitting" @click="confirmQuit">
            {{ quitting ? "退出中..." : "退出" }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.topnav {
  display: grid;
  grid-template-columns: minmax(280px, 1fr) auto minmax(180px, 1fr);
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
.device-chip,
.account-menu button {
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
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 10px;
  color: var(--text-inverse);
  background: linear-gradient(145deg, #2f7cf8, #6b5cf4);
  box-shadow: 0 8px 18px rgba(52, 120, 246, 0.25);
}

.brand-symbol svg { width: 19px; height: 19px; }

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
.brand-version.is-update i { position: absolute; right: -8px; bottom: -3px; width: 6px; height: 6px; border: 1px solid var(--nav); border-radius: 50%; background: var(--danger); box-shadow: 0 0 7px color-mix(in srgb, var(--danger) 65%, transparent); }

.device-chip {
  min-width: 0;
  height: 34px;
  gap: 7px;
  margin-left: 2px;
  padding: 0 11px;
  border: 1px solid var(--nav-device-border);
  border-radius: 9px;
  color: var(--nav-device-ink);
  background: var(--nav-device-bg);
  font-weight: 650;
  font-size: 13px;
}

.device-chip > span:last-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.status-dot {
  width: 7px;
  height: 7px;
  flex: 0 0 auto;
  border-radius: 999px;
  background: #8792a7;
}
.device-chip.connected .status-dot { background: var(--success); box-shadow: 0 0 0 4px rgba(24, 185, 121, 0.11); }
.device-chip.connecting .status-dot { background: var(--warning); }
.device-chip.error .status-dot { background: var(--danger); }

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
  font-weight: 650;
  cursor: pointer;
  transition: 0.18s ease;
}

.main-nav button:hover { color: var(--nav-ink); background: var(--nav-segment-hover); }
.main-nav button.active {
  color: var(--nav-segment-active-ink);
  background: var(--nav-segment-active);
  box-shadow: var(--nav-segment-shadow);
}

.top-actions { justify-content: flex-end; gap: 6px; }

.icon-button,
.account-button {
  display: grid;
  place-items: center;
  height: 34px;
  border: 0;
  border-radius: 9px;
  color: var(--nav-icon);
  background: transparent;
  cursor: pointer;
  transition: color 0.18s ease, background 0.18s ease;
}

.icon-button { width: 34px; padding: 0; }
.icon-button svg { width: 17px; height: 17px; }
.icon-button:hover:not(:disabled),
.icon-button.active,
.account-button:hover,
.account-button[aria-expanded="true"] {
  color: var(--nav-icon-hover);
  background: var(--nav-icon-hover-bg);
}
.icon-button:disabled { cursor: wait; opacity: 0.58; }

.account-wrap { position: relative; }
.account-button {
  width: 34px;
  height: 34px;
  padding: 0;
  border: 5px solid transparent;
  border-radius: 12px;
  color: var(--nav-account-ink);
  background: var(--nav-account-bg);
  font-size: 10px;
  font-weight: 800;
}

.account-menu {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  z-index: 100;
  min-width: 150px;
  padding: 6px;
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  background: var(--card-bg);
  box-shadow: var(--dialog-shadow);
}

.account-menu button {
  width: 100%;
  gap: 8px;
  height: 34px;
  padding: 0 9px;
  border: 0;
  border-radius: 6px;
  color: var(--text);
  background: transparent;
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}
.account-menu button:hover { color: var(--danger); background: var(--danger-bg); }
.account-menu svg { width: 16px; height: 16px; }

.quit-backdrop {
  position: fixed;
  inset: 0;
  z-index: 3000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
  background: var(--overlay);
}

.quit-dialog {
  width: min(360px, 100%);
  padding: 18px 18px 14px;
  border-radius: 10px;
  background: var(--card-bg);
  box-shadow: var(--dialog-shadow);
  color: var(--text);
}
.quit-dialog h3 { margin: 0 0 8px; font-size: 16px; font-weight: 600; }
.quit-dialog p { margin: 0 0 16px; font-size: 13px; line-height: 1.5; color: var(--text-secondary); }
.quit-actions { display: flex; justify-content: flex-end; gap: 8px; }
.quit-btn { height: 32px; padding: 0 14px; border-radius: 6px; border: 1px solid transparent; font: inherit; font-size: 13px; font-weight: 500; cursor: pointer; }
.quit-btn:disabled { opacity: 0.6; cursor: wait; }
.quit-btn-ghost { background: var(--card-bg); border-color: var(--border); color: var(--text); }
.quit-btn-ghost:hover:not(:disabled) { background: var(--surface-hover); }
.quit-btn-danger { background: var(--danger); color: #fff; }
.quit-btn-danger:hover:not(:disabled) { filter: brightness(0.95); }

@media (max-width: 1019px) {
  .topnav { grid-template-columns: minmax(0, 1fr) auto auto; gap: 10px; padding: 0 18px; }
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
