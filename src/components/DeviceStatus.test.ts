import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { i18n } from "../i18n";
import DeviceStatus from "./DeviceStatus.vue";

describe("DeviceStatus", () => {
  it("uses a short red disconnected status", () => {
    const wrapper = mount(DeviceStatus, {
      props: { status: "Disconnected", loading: false },
      global: { plugins: [i18n] },
    });

    expect(wrapper.get(".status-indicator").classes()).toContain("disconnected");
    expect(wrapper.text()).toContain("未连接");
  });

  it("keeps the technical error in the tooltip instead of the visible label", () => {
    const wrapper = mount(DeviceStatus, {
      props: { status: "Error|打开 BLE 设备失败：设备对象为空", loading: false },
      global: { plugins: [i18n] },
    });
    const indicator = wrapper.get(".status-indicator");

    expect(indicator.text()).toBe("连接失败");
    expect(indicator.attributes("title")).toBe("打开 BLE 设备失败：设备对象为空");
  });
});
describe("automatic recovery controls", () => {
  it("shows paired waiting without green and allows stopping automatic recovery", async () => {
    const wrapper = mount(DeviceStatus, {
      props: { status: "Connecting", loading: false, device: {
        bridge_type: "xiaomi", status: "Connecting", device_name: "paired", device_address: null,
        battery_level: null, battery_charging: null, bluetooth_paired: true,
        bluetooth_connected: false, auto_connect_enabled: true,
      } },
      global: { plugins: [i18n] },
    });
    expect(wrapper.get(".status-indicator").classes()).not.toContain("connected");
    expect(wrapper.text()).toContain("已配对，等待遥控器");
    const button = wrapper.get("button");
    expect(button.attributes("disabled")).toBeUndefined();
    expect(button.text()).toBe("断开连接");
    await button.trigger("click");
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });
});
