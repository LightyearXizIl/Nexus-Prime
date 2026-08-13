import type { BridgeStatus } from "../types";

export type ConnectionTone = "connected" | "connecting" | "disconnected" | "error";

export interface ConnectionStatusPresentation {
  tone: ConnectionTone;
  labelKey:
    | "status.connected"
    | "status.connectingDevice"
    | "status.disconnected"
    | "status.connectionFailed";
  detail: string | null;
}

/** Keeps operational UI copy short while preserving backend diagnostics for details and logs. */
export function connectionStatusPresentation(status: BridgeStatus): ConnectionStatusPresentation {
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
