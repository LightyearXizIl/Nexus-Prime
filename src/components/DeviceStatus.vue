<script setup lang="ts">
import type { BridgeStatus } from "../types";

const props = defineProps<{
  status: BridgeStatus;
  loading: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
}>();

function statusText(status: BridgeStatus): string {
  if (status.startsWith("Error|")) {
    return status.slice("Error|".length) || "错误";
  }
  if (status.startsWith("Error")) return status;
  const map: Record<string, string> = {
    Disconnected: "未连接",
    Connecting: "连接中...",
    Connected: "已连接",
  };
  return map[status] || status;
}

function statusClass(status: BridgeStatus): string {
  if (status === "Connected") return "connected";
  if (status === "Connecting") return "connecting";
  if (status.startsWith("Error")) return "error";
  return "disconnected";
}

function buttonText(status: BridgeStatus): string {
  if (status === "Connected") return "断开连接";
  if (status === "Connecting") return "连接中...";
  return "连接设备";
}
</script>

<template>
  <div class="device-status">
    <span :class="['status-indicator', statusClass(status)]">
      <span class="dot"></span>
      {{ statusText(status) }}
    </span>
    <button
      :class="['connection-action', status === 'Connected' ? 'disconnect' : 'connect']"
      :disabled="loading || status === 'Connecting'"
      @click="emit('toggle')"
    >
      {{ loading ? "处理中..." : buttonText(status) }}
    </button>
  </div>
</template>

<style scoped>
.device-status {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 3px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface-soft);
  box-shadow: var(--shadow-sm);
}

.status-indicator {
  min-height: 34px;
  padding: 0 12px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  font-size: 13px;
  font-weight: 650;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}

.status-indicator.connected { color: var(--success-text); background: var(--success-bg); border-color: var(--success-border); }
.status-indicator.connected .dot { background: var(--success); box-shadow: 0 0 0 4px rgba(24, 185, 121, 0.1); }

.status-indicator.connecting { color: var(--warning-text); background: var(--warning-bg); border-color: var(--warning-border); }
.status-indicator.connecting .dot {
  background: var(--warning);
  animation: pulse 1s ease-in-out infinite;
}

.status-indicator.disconnected { color: var(--text-secondary); }
.status-indicator.disconnected .dot { background: var(--text-muted); }

.status-indicator.error { color: var(--danger-text); background: var(--danger-bg); border-color: var(--danger-border); }
.status-indicator.error .dot { background: var(--danger); }

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

.connection-action {
  height: 34px;
  padding: 0 13px;
  border: 0;
  border-radius: 999px;
  font-size: 13px;
  font-weight: 650;
  cursor: pointer;
  transition: color 0.15s ease, background 0.15s ease, border-color 0.15s ease;
}

.connection-action:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.connect {
  background: var(--primary);
  border-color: var(--primary);
  color: #fff;
}

.connect:hover:not(:disabled) {
  background: var(--primary-dark);
  border-color: var(--primary-dark);
}

.disconnect {
  background: var(--danger-bg);
  color: var(--danger);
}

.disconnect:hover:not(:disabled) {
  background: color-mix(in srgb, var(--danger) 18%, var(--danger-bg));
}
</style>
