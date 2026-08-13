import { describe, expect, it } from "vitest";
import { connectedDeviceName, connectionStatusPresentation } from "./connectionStatus";

describe("connection status presentation", () => {
  it("uses short, semantic presentations for all bridge states", () => {
    expect(connectionStatusPresentation("Connected")).toEqual({
      tone: "connected",
      labelKey: "status.connected",
      detail: null,
    });
    expect(connectionStatusPresentation("Connecting")).toEqual({
      tone: "connecting",
      labelKey: "status.connectingDevice",
      detail: null,
    });
    expect(connectionStatusPresentation("Disconnected")).toEqual({
      tone: "disconnected",
      labelKey: "status.disconnected",
      detail: null,
    });
  });

  it("keeps connection errors out of the label while preserving their detail", () => {
    expect(connectionStatusPresentation("Error|打开 BLE 设备失败：设备对象为空")).toEqual({
      tone: "error",
      labelKey: "status.connectionFailed",
      detail: "打开 BLE 设备失败：设备对象为空",
    });
    expect(connectionStatusPresentation("Error: adapter unavailable")).toMatchObject({
      tone: "error",
      detail: "adapter unavailable",
    });
  });

  it("only exposes a device name while connected", () => {
    expect(connectedDeviceName("Connected", "小米蓝牙遥控器 2 Pro")).toBe("小米蓝牙遥控器 2 Pro");
    expect(connectedDeviceName("Connecting", "小米蓝牙遥控器 2 Pro")).toBeNull();
    expect(connectedDeviceName("Disconnected", "小米蓝牙遥控器 2 Pro")).toBeNull();
    expect(connectedDeviceName("Error|连接失败", "小米蓝牙遥控器 2 Pro")).toBeNull();
  });
});
