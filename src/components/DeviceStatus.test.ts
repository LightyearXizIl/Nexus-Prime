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
