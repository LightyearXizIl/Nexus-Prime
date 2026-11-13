import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { createMemoryHistory, createRouter } from "vue-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "../i18n";
const { windowInvoke } = vi.hoisted(() => ({ windowInvoke: vi.fn().mockResolvedValue(false) }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: windowInvoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../utils/appLogger", () => ({ loggedInvoke: windowInvoke }));
import { useBridgeStore } from "../stores/bridge";
import SideNav from "./SideNav.vue";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setTheme: vi.fn().mockResolvedValue(undefined) }),
}));

function mountSideNav(status: "Connected" | "Connecting" | "Disconnected" | `Error|${string}`) {
  const pinia = createPinia();
  setActivePinia(pinia);
  const bridge = useBridgeStore();
  bridge.devices.xiaomi = {
    bridge_type: "xiaomi",
    status,
    device_name: "小米蓝牙遥控器 2 Pro",
    device_address: "00:11:22:33:44:55",
    battery_level: 80,
    battery_charging: false,
  };
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: "/", component: { template: "<div />" } },
      { path: "/xiaomi", component: { template: "<div />" } },
      { path: "/xiaomi/mapping", component: { template: "<div />" } },
      { path: "/settings", component: { template: "<div />" } },
    ],
  });

  return mount(SideNav, { global: { plugins: [pinia, router, i18n] } });
}

describe("SideNav device chip", () => {
  beforeEach(() => {
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({ matches: false }));
  });

  it("does not show a stale device name while disconnected", () => {
    const wrapper = mountSideNav("Disconnected");
    const chip = wrapper.get(".device-chip");

    expect(chip.classes()).toContain("disconnected");
    expect(chip.text()).toContain("设备未连接");
    expect(chip.text()).not.toContain("小米蓝牙遥控器");
  });

  it("routes custom window controls to the native window and has one brand", async () => {
    const wrapper = mountSideNav("Connected");
    await wrapper.get('[aria-label="最小化"]').trigger("click");
    await wrapper.get('[aria-label="最大化"]').trigger("click");
    await wrapper.get('[aria-label="关闭窗口"]').trigger("click");
    for (const action of ["minimize", "maximize", "close"]) {
      expect(windowInvoke).toHaveBeenCalledWith("control_main_window", { action });
    }
    expect(wrapper.findAll(".brand-symbol")).toHaveLength(1);
    expect(wrapper.get(".titlebar").attributes()).toHaveProperty("data-tauri-drag-region");
    expect(wrapper.findAll(".titlebar .window-button")).toHaveLength(3);
    expect(wrapper.find(".top-actions .window-controls").exists()).toBe(false);
    expect(wrapper.find(".topnav").attributes()).not.toHaveProperty("data-tauri-drag-region");
    wrapper.unmount();
  });

  it("only shows the real device name after a connection succeeds", () => {
    const wrapper = mountSideNav("Connected");
    expect(wrapper.get(".device-chip").text()).toContain("小米蓝牙遥控器 2 Pro");
  });
});
