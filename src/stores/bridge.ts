import { defineStore } from "pinia";
import { ref } from "vue";
import { loggedInvoke as invoke } from "../utils/appLogger";
import type { DeviceInfo, BridgeType, BridgeStatus } from "../types";
import { connectionStatusPresentation } from "../utils/connectionStatus";

export const useBridgeStore = defineStore("bridge", () => {
  const devices = ref<Record<BridgeType, DeviceInfo>>({
    xiaomi: {
      bridge_type: "xiaomi",
      status: "Disconnected",
      device_name: null,
      device_address: null,
      battery_level: null,
      battery_charging: null,
    },
  });

  const loading = ref<Record<BridgeType, boolean>>({
    xiaomi: false,
  });

  async function refreshStatus(type: BridgeType) {
    try {
      const info = await invoke<DeviceInfo>("get_device_status", {
        bridgeType: type,
      });
      devices.value[type] = info;
    } catch (e) {
      // Silently fail in browser dev mode (Tauri API not available)
      console.warn(`Failed to get ${type} status (expected in browser dev):`, e);
    }
  }

  async function refreshAll() {
    await refreshStatus("xiaomi");
  }

  async function startBridge(type: BridgeType) {
    loading.value[type] = true;
    devices.value[type].status = "Connecting";
    try {
      await invoke("start_bridge", { bridgeType: type });
      await refreshStatus(type);
    } catch (e) {
      devices.value[type].status = `Error: ${e}` as BridgeStatus;
      console.error(`Failed to start ${type}:`, e);
    } finally {
      loading.value[type] = false;
    }
  }

  async function stopBridge(type: BridgeType) {
    loading.value[type] = true;
    try {
      await invoke("stop_bridge", { bridgeType: type });
      devices.value[type].status = "Disconnected";
    } catch (e) {
      console.error(`Failed to stop ${type}:`, e);
    } finally {
      loading.value[type] = false;
    }
  }

  function statusLabel(status: BridgeStatus): string {
    if (status.startsWith("Error|")) {
      return status.slice("Error|".length) || "错误";
    }
    if (status.startsWith("Error")) return status;
    const map: Record<string, string> = {
      Disconnected: "未连接",
      Connecting: "连接中...",
      Connected: "已连接",
      Error: "错误",
    };
    return map[status] || status;
  }

  function statusColor(status: BridgeStatus): string {
    const tone = connectionStatusPresentation(status).tone;
    if (tone === "connected") return "var(--success)";
    if (tone === "connecting") return "var(--warning)";
    return "var(--danger)";
  }

  return {
    devices,
    loading,
    refreshStatus,
    refreshAll,
    startBridge,
    stopBridge,
    statusLabel,
    statusColor,
  };
});
