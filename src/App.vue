<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { RouterView, useRoute, useRouter } from "vue-router";
import { listen, emit, type UnlistenFn } from "@tauri-apps/api/event";
import { loggedInvoke as invoke } from "./utils/appLogger";
import SideNav from "./components/SideNav.vue";
import UpdateDialog from "./components/UpdateDialog.vue";
import { useUpdateStore } from "./stores/update";
import { useI18n } from "vue-i18n";

interface ConflictProcess {
  pid: number;
  name: string;
  reasons: string[];
}

interface ConflictSnapshot {
  trigger: string;
  detail: string;
  processes: ConflictProcess[];
  pcmPort: number;
  hidTapPort: number;
}

const router = useRouter();
const route = useRoute();
const update = useUpdateStore();
const { t } = useI18n();
let unlistenNav: UnlistenFn | null = null;
let unlistenConflict: UnlistenFn | null = null;

const showConflict = ref(false);
const conflict = ref<ConflictSnapshot | null>(null);
const busy = ref(false);
const actionMsg = ref("");

function triggerLabel(trigger: string): string {
  switch (trigger) {
    case "pcm_port":
      return t("conflict.pcm");
    case "hid_tap_port":
      return t("conflict.hid");
    case "atvv":
      return t("conflict.atvv");
    case "atvv_repair":
      return t("conflict.repair");
    default:
      return t("conflict.generic");
  }
}

function openConflict(snap: ConflictSnapshot) {
  if (!snap.processes?.length) return;
  conflict.value = snap;
  actionMsg.value = "";
  showConflict.value = true;
}

async function killOne(pid: number) {
  if (busy.value || !conflict.value) return;
  busy.value = true;
  actionMsg.value = "";
  try {
    await invoke<number[]>("kill_xiaomi_conflicts", { pids: [pid] });
    conflict.value.processes = conflict.value.processes.filter((p) => p.pid !== pid);
    if (conflict.value.processes.length === 0) {
      await autoRetry();
    }
  } catch (e) {
    actionMsg.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function killAll() {
  if (busy.value || !conflict.value) return;
  const pids = conflict.value.processes.map((p) => p.pid);
  if (!pids.length) return;
  busy.value = true;
  actionMsg.value = "";
  try {
    await invoke<number[]>("kill_xiaomi_conflicts", { pids });
    conflict.value.processes = [];
    await autoRetry();
  } catch (e) {
    actionMsg.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function autoRetry() {
  const trigger = conflict.value?.trigger ?? "";
  try {
    const msg = await invoke<string>("retry_xiaomi_after_conflict_clear");
    actionMsg.value = msg;
    showConflict.value = false;
    if (trigger === "atvv_repair") {
      actionMsg.value = t("conflict.cleared");
      try {
        const result = await invoke<{
          phase: string;
          message: string;
          atvvOk: boolean;
        }>("repair_xiaomi_atvv", { force: true });
        actionMsg.value = result.message;
      } catch (e) {
        actionMsg.value = String(e);
      }
    }
  } catch (e) {
    actionMsg.value = String(e);
  }
}

async function dismissConflict() {
  const trigger = conflict.value?.trigger;
  showConflict.value = false;
  if (trigger === "atvv_repair") {
    await emit("xiaomi-atvv-repair-cancelled", {
      message: "已取消：未结束占用进程，ATVV 修复中止",
    });
  }
}

onMounted(async () => {
  void update.initialize();
  unlistenNav = await listen<string>("navigate", (ev) => {
    if (ev.payload) router.push(ev.payload);
  });
  try {
    unlistenConflict = await listen<ConflictSnapshot>("xiaomi-conflict", (ev) => {
      if (ev.payload) openConflict(ev.payload);
    });
  } catch (e) {
    console.warn("listen xiaomi-conflict failed:", e);
  }
});

onUnmounted(() => {
  unlistenNav?.();
  unlistenConflict?.();
  update.dispose();
});
</script>

<template>
  <div class="app-container">
    <SideNav />
    <main class="main-content" :class="{ 'mapping-content': route.name === 'xiaomi-mapping' }">
      <RouterView />
    </main>
  </div>

  <Teleport to="body">
    <div
      v-if="showConflict && conflict"
      class="conflict-backdrop"
      role="presentation"
      @click.self="dismissConflict"
    >
      <div
        class="conflict-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="conflict-title"
      >
        <h3 id="conflict-title">{{ triggerLabel(conflict.trigger) }}</h3>
        <p class="conflict-detail">
          {{ conflict.detail || t("conflict.detail") }}
        </p>
        <p class="conflict-ports">
          {{ t("conflict.ports", { pcm: conflict.pcmPort, hid: conflict.hidTapPort }) }}
        </p>

        <ul class="conflict-list">
          <li v-for="p in conflict.processes" :key="p.pid" class="conflict-item">
            <div class="conflict-item-main">
              <span class="conflict-name">{{ p.name }}</span>
              <span class="conflict-pid">PID {{ p.pid }}</span>
              <span class="conflict-reasons">{{ p.reasons.join(" · ") }}</span>
            </div>
            <button
              type="button"
              class="conflict-btn conflict-btn-row"
              :disabled="busy"
              @click="killOne(p.pid)"
            >
              {{ t("conflict.endOne") }}
            </button>
          </li>
        </ul>

        <p class="conflict-hint">
          {{ t("conflict.hint") }}
        </p>

        <p v-if="actionMsg" class="conflict-msg">{{ actionMsg }}</p>

        <div class="conflict-actions">
          <button
            type="button"
            class="conflict-btn conflict-btn-ghost"
            :disabled="busy"
            @click="dismissConflict"
          >
            {{ t("common.cancel") }}
          </button>
          <button
            type="button"
            class="conflict-btn conflict-btn-danger"
            :disabled="busy || !conflict.processes.length"
            @click="killAll"
          >
            {{ busy ? t("common.processing") : t("conflict.endAll") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
  <UpdateDialog />
</template>

<style>
:root {
  --primary: #3478f6;
  --primary-rgb: 52 120 246;
  --primary-dark: #2463d4;
  --bg: #f5f7fb;
  --canvas: #f5f7fb;
  --nav: #ffffff;
  --nav-ink: #172033;
  --nav-muted: #6f7b91;
  --nav-border: #e3e8f0;
  --nav-shadow: rgba(28, 39, 60, 0.05);
  --nav-device-bg: #f6f8fb;
  --nav-device-border: #dce3ed;
  --nav-device-ink: #344056;
  --nav-segment-bg: #f0f3f7;
  --nav-segment-border: #e0e6ee;
  --nav-segment-hover: #e5eaf1;
  --nav-segment-active: #3478f6;
  --nav-segment-active-ink: #ffffff;
  --nav-segment-shadow: 0 4px 12px rgba(52, 120, 246, 0.3);
  --nav-icon: #657187;
  --nav-icon-hover: #172033;
  --nav-icon-hover-bg: #edf1f6;
  --nav-account-bg: #e8edf5;
  --nav-account-ink: #344056;
  --sidebar-bg: var(--nav);
  --sidebar-text: #cbd5e1;
  --sidebar-active: var(--primary-dark);
  --card-bg: #ffffff;
  --surface-soft: #f8fafc;
  --border: #e3e8f0;
  --border-strong: #cbd5e1;
  --text: #172033;
  --text-rgb: 23 32 51;
  --text-secondary: #6f7b91;
  --text-muted: #94a3b8;
  --success: #18b979;
  --success-rgb: 24 185 121;
  --warning: #ee9b34;
  --danger: #ef5b61;
  --danger-rgb: 239 91 97;
  --radius: 12px;
  --surface-raised: #ffffff;
  --surface-muted: #f6f8fc;
  --surface-hover: #edf2f8;
  --surface-selected: #edf4ff;
  --input-bg: #ffffff;
  --text-inverse: #ffffff;
  --text-inverse-rgb: 255 255 255;
  --control-thumb: #ffffff;
  --overlay: rgba(15, 23, 42, 0.48);
  --shadow: 0 12px 32px rgba(28, 39, 60, 0.07);
  --shadow-sm: 0 4px 14px rgba(28, 39, 60, 0.06);
  --dialog-shadow: 0 16px 42px rgba(28, 39, 60, 0.24);
  --focus-ring: rgba(26, 115, 232, 0.35);
  --ease-out: cubic-bezier(0.23, 1, 0.32, 1);
  --info-bg: #eff6ff;
  --info-border: #bfdbfe;
  --info-text: #1d4ed8;
  --success-bg: #ecfdf5;
  --success-border: #bbf7d0;
  --success-text: #15803d;
  --warning-bg: #fffbeb;
  --warning-border: #fde68a;
  --warning-text: #b45309;
  --danger-bg: #fef2f2;
  --danger-border: #fecaca;
  --danger-text: #b91c1c;
}

:root[data-theme="dark"] {
  --primary: #60a5fa;
  --primary-rgb: 96 165 250;
  --primary-dark: #3b82f6;
  /* Neutral charcoal surfaces; semantic and brand accents keep their colors. */
  --bg: oklch(0.209 0 0);
  --canvas: oklch(0.209 0 0);
  --nav: oklch(0.187 0 0);
  --nav-ink: oklch(0.97 0 0);
  --nav-muted: oklch(0.72 0 0);
  --nav-border: oklch(0.309 0 0);
  --nav-shadow: oklch(0 0 0 / 0.24);
  --nav-device-bg: oklch(0.26 0 0);
  --nav-device-border: oklch(0.34 0 0);
  --nav-device-ink: oklch(0.92 0 0);
  --nav-segment-bg: oklch(0.243 0 0);
  --nav-segment-border: oklch(0.321 0 0);
  --nav-segment-hover: oklch(0.289 0 0);
  --nav-segment-active: oklch(0.34 0 0);
  --nav-segment-active-ink: #ffffff;
  --nav-segment-shadow: 0 4px 14px oklch(0 0 0 / 0.24);
  --nav-icon: oklch(0.77 0 0);
  --nav-icon-hover: #ffffff;
  --nav-icon-hover-bg: oklch(0.289 0 0);
  --nav-account-bg: oklch(0.34 0 0);
  --nav-account-ink: #ffffff;
  --sidebar-bg: var(--nav);
  --sidebar-text: oklch(0.87 0 0);
  --sidebar-active: oklch(0.95 0 0);
  --card-bg: oklch(0.239 0 0);
  --surface-soft: oklch(0.217 0 0);
  --border: oklch(0.321 0 0);
  --border-strong: oklch(0.4 0 0);
  --text: oklch(0.95 0 0);
  --text-rgb: 238 238 238;
  --text-secondary: oklch(0.75 0 0);
  --text-muted: oklch(0.68 0 0);
  --text-inverse: #ffffff;
  --text-inverse-rgb: 255 255 255;
  --success: #4ade80;
  --success-rgb: 74 222 128;
  --warning: #fbbf24;
  --danger: #fb7185;
  --danger-rgb: 251 113 133;
  --surface-raised: oklch(0.269 0 0);
  --surface-muted: oklch(0.217 0 0);
  --surface-hover: oklch(0.309 0 0);
  --surface-selected: oklch(0.289 0 0);
  --input-bg: oklch(0.217 0 0);
  --control-thumb: oklch(0.92 0 0);
  --overlay: oklch(0 0 0 / 0.72);
  --shadow: 0 12px 32px oklch(0 0 0 / 0.22);
  --shadow-sm: 0 4px 14px oklch(0 0 0 / 0.2);
  --dialog-shadow: 0 16px 46px oklch(0 0 0 / 0.48);
  --focus-ring: rgba(96, 165, 250, 0.42);
  --info-bg: #102c52;
  --info-border: #1d4ed8;
  --info-text: #93c5fd;
  --success-bg: #123526;
  --success-border: #166534;
  --success-text: #86efac;
  --warning-bg: #3a2b08;
  --warning-border: #a16207;
  --warning-text: #fcd34d;
  --danger-bg: #3b1720;
  --danger-border: #9f1239;
  --danger-text: #fda4af;
}

/* Shared status light: a solid state-colored core with a restrained translucent ring. */
.status-light {
  --status-light-color: var(--text-muted);
  display: inline-block;
  width: 7px;
  height: 7px;
  flex: 0 0 auto;
  border-radius: 50%;
  background: var(--status-light-color);
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--status-light-color) 12%, transparent);
}

.status-light.is-success { --status-light-color: var(--success); }
.status-light.is-warning { --status-light-color: var(--warning); }
.status-light.is-danger { --status-light-color: var(--danger); }
.status-light.is-info { --status-light-color: var(--primary); }

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  background: var(--bg);
  color: var(--text);
  overflow: hidden;
  height: 100vh;
}

button,
input,
select,
textarea {
  font: inherit;
}

button:focus-visible,
input:focus-visible,
select:focus-visible,
textarea:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}

.status-capsule {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 3px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface-soft);
  box-shadow: var(--shadow-sm);
}

#app {
  height: 100vh;
}

.app-container {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.main-content {
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  padding: 26px 30px 32px;
  background: var(--canvas);
}

.main-content.mapping-content { scrollbar-width: none; }
.main-content.mapping-content::-webkit-scrollbar { display: none; width: 0; height: 0; }

@media (max-width: 900px) {
  .main-content { padding: 20px; }
}

.conflict-backdrop {
  position: fixed;
  top: 0; right: 0; bottom: 0; left: 0;
  z-index: 4000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
  background: var(--overlay);
}

.conflict-dialog {
  width: min(480px, 100%);
  padding: 18px 18px 14px;
  border-radius: 10px;
  background: var(--card-bg);
  box-shadow: var(--dialog-shadow);
  color: var(--text);
}

.conflict-dialog h3 {
  margin: 0 0 8px;
  font-size: 16px;
  font-weight: 600;
}

.conflict-detail,
.conflict-ports,
.conflict-hint {
  margin: 0 0 10px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-secondary);
}

.conflict-list {
  list-style: none;
  margin: 0 0 12px;
  padding: 0;
  max-height: 220px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 8px;
}

.conflict-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
}

.conflict-item:last-child {
  border-bottom: none;
}

.conflict-item-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.conflict-name {
  font-size: 13px;
  font-weight: 600;
  word-break: break-all;
}

.conflict-pid,
.conflict-reasons {
  font-size: 12px;
  color: var(--text-secondary);
}

.conflict-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}

.conflict-btn {
  height: 32px;
  padding: 0 14px;
  border-radius: 6px;
  border: 1px solid transparent;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}

.conflict-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.conflict-btn-ghost {
  background: var(--card-bg);
  border-color: var(--border);
  color: var(--text);
}

.conflict-btn-ghost:hover:not(:disabled) {
  background: var(--surface-hover);
}

.conflict-btn-danger {
  background: var(--danger);
  color: #fff;
}

.conflict-btn-danger:hover:not(:disabled) {
  filter: brightness(0.95);
}

.conflict-btn-row {
  flex-shrink: 0;
  height: 28px;
  padding: 0 10px;
  background: var(--card-bg);
  border-color: var(--border);
  color: var(--text);
}

.conflict-btn-row:hover:not(:disabled) {
  border-color: var(--danger);
  color: var(--danger);
}

.conflict-msg {
  margin: 0 0 10px;
  font-size: 12px;
  color: var(--warning);
}
</style>
