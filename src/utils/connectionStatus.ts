import type { BridgeStatus, DeviceInfo } from "../types";

export type ConnectionTone = "connected" | "connecting" | "disconnected" | "error";

export interface ConnectionStatusPresentation {
  tone: ConnectionTone;
  labelKey:
    | "status.waitingRemote"
    | "status.detecting"
    | "status.pairRequired"
    | "status.voiceConnecting"
    | "status.connected"
    | "status.connectingDevice"
    | "status.disconnected"
    | "status.connectionFailed";
  detail: string | null;
}

/** Keeps operational UI copy short while preserving backend diagnostics for details and logs. */
export function connectionStatusPresentation(status: BridgeStatus, device?: Partial<DeviceInfo> & { atvv_ok?: boolean }): ConnectionStatusPresentation {
  if (device && device.auto_connect_enabled !== undefined) {
    if (!device.auto_connect_enabled) return { tone: "disconnected", labelKey: "status.disconnected", detail: null };
    if (device.bluetooth_connected !== true) {
      const labelKey = device.bluetooth_paired === true ? "status.waitingRemote"
        : device.bluetooth_paired === false ? "status.pairRequired" : "status.detecting";
      return { tone: "connecting", labelKey, detail: null };
    }
    if (device.atvv_ok === false) return { tone: "connected", labelKey: "status.voiceConnecting", detail: null };
  }
  if (status === "Connected") {
    return { tone: "connected", labelKey: "status.connected", detail: null };
  }
  if (status === "Connecting") {
    return { tone: "connecting", labelKey: "status.connectingDevice", detail: null };
  }
  if (status.startsWith("Error")) {
    const detail = status
      .replace(/^Error\|/, "")
      .replace(/^Error:\s*/, "")
      .replace(/^Error\s*/, "")
      .trim();
    return {
      tone: "error",
      labelKey: "status.connectionFailed",
      detail: detail || null,
    };
  }
  return { tone: "disconnected", labelKey: "status.disconnected", detail: null };
}

export function connectedDeviceName(status: BridgeStatus, deviceName: string | null): string | null {
  const name = deviceName?.trim();
  return status === "Connected" && name ? name : null;
}
