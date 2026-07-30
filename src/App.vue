<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { RouterView, useRouter } from "vue-router";
import { listen, emit, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import SideNav from "./components/SideNav.vue";

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
let unlistenNav: UnlistenFn | null = null;
let unlistenConflict: UnlistenFn | null = null;

const showConflict = ref(false);
const conflict = ref<ConflictSnapshot | null>(null);
const busy = ref(false);
const actionMsg = ref("");

function triggerLabel(t: string): string {
  switch (t) {
    case "pcm_port":
      return "语音端口冲突";
    case "hid_tap_port":
      return "HID Tap 端口冲突";
    case "atvv":
      return "ATVV 语音通道失败";
    case "atvv_repair":
      return "修复 ATVV：请先结束占用";
    default:
      return "桥接进程冲突";
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
      actionMsg.value = "占用已清理，正在继续修复 ATVV…";
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
});
</script>

<template>
  <div class="app-container">
    <SideNav />
    <main class="main-content">
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
          {{ conflict.detail || "检测到其它遥控桥接进程，可能占用端口或 BLE。" }}
        </p>
        <p class="conflict-ports">
          关注端口：PCM UDP {{ conflict.pcmPort }}、HID Tap TCP {{ conflict.hidTapPort }}
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
              结束此进程
            </button>
          </li>
        </ul>

        <p class="conflict-hint">
          也可手动打开任务管理器（Ctrl+Shift+Esc）结束上列进程。仅允许结束已知桥接程序。
        </p>

        <p v-if="actionMsg" class="conflict-msg">{{ actionMsg }}</p>

        <div class="conflict-actions">
          <button
            type="button"
            class="conflict-btn conflict-btn-ghost"
            :disabled="busy"
            @click="dismissConflict"
          >
            取消
          </button>
          <button
            type="button"
            class="conflict-btn conflict-btn-danger"
            :disabled="busy || !conflict.processes.length"
            @click="killAll"
          >
            {{ busy ? "处理中…" : "关掉上列全部" }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style>
:root {
  --primary: #3478f6;
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
  --sidebar-active: var(--primary);
  --card-bg: #ffffff;
  --surface-soft: #f8fafc;
  --border: #e3e8f0;
  --border-strong: #cbd5e1;
  --text: #172033;
  --text-secondary: #6f7b91;
  --text-muted: #94a3b8;
  --success: #18b979;
  --warning: #ee9b34;
  --danger: #ef5b61;
  --radius: 12px;
  --surface-raised: #ffffff;
  --surface-muted: #f6f8fc;
  --surface-hover: #edf2f8;
  --surface-selected: #edf4ff;
  --input-bg: #ffffff;
  --text-inverse: #ffffff;
  --control-thumb: #ffffff;
  --overlay: rgba(15, 23, 42, 0.48);
  --shadow: 0 12px 32px rgba(28, 39, 60, 0.07);
  --shadow-sm: 0 4px 14px rgba(28, 39, 60, 0.06);
  --dialog-shadow: 0 16px 42px rgba(28, 39, 60, 0.24);
  --focus-ring: rgba(26, 115, 232, 0.35);
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
  --primary-dark: #3b82f6;
  --bg: #0d1523;
  --canvas: #0d1523;
  --nav: #101827;
  --nav-ink: #eef4ff;
  --nav-muted: #91a0b8;
  --nav-border: #243249;
  --nav-shadow: rgba(0, 0, 0, 0.24);
  --nav-device-bg: #182338;
  --nav-device-border: #2b3a50;
  --nav-device-ink: #dfe7f5;
  --nav-segment-bg: #182338;
  --nav-segment-border: #29384e;
  --nav-segment-hover: #223047;
  --nav-segment-active: #3b82f6;
  --nav-segment-active-ink: #ffffff;
  --nav-segment-shadow: 0 4px 14px rgba(37, 99, 235, 0.36);
  --nav-icon: #aeb8c9;
  --nav-icon-hover: #ffffff;
  --nav-icon-hover-bg: #223047;
  --nav-account-bg: #354056;
  --nav-account-ink: #ffffff;
  --sidebar-bg: var(--nav);
  --sidebar-text: #cbd5e1;
  --sidebar-active: #2563eb;
  --card-bg: #172235;
  --surface-soft: #111c2e;
  --border: #2b3a50;
  --border-strong: #40516a;
  --text: #e8eef8;
  --text-secondary: #aab8cc;
  --text-muted: #94a3b8;
  --text-inverse: #ffffff;
  --success: #4ade80;
  --warning: #fbbf24;
  --danger: #fb7185;
  --surface-raised: #1b2940;
  --surface-muted: #111c2e;
  --surface-hover: #26364d;
  --surface-selected: #102c52;
  --input-bg: #101b2d;
  --control-thumb: #e2e8f0;
  --overlay: rgba(2, 6, 23, 0.72);
  --dialog-shadow: 0 16px 46px rgba(0, 0, 0, 0.48);
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

@media (max-width: 900px) {
  .main-content { padding: 20px; }
}

.conflict-backdrop {
  position: fixed;
  inset: 0;
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
