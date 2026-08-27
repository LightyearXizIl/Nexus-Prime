import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { createMemoryHistory, createRouter } from "vue-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "../i18n";
import XiaomiSettingsView from "./XiaomiSettings.vue";

const { invoke, listen } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const config = {
  button_aliases: {},
  button_bindings: {},
  voice_hotkey: ["leftctrl", "leftwin"],
  trigger_mode: "Hold" as const,
  bluetooth_address: null,
  gain_db: 10,
  voice_shortcut_enabled: true,
};

const host = {
  bridge_alive: true,
  audio_alive: true,
  cable_ready: true,
  atvv_ok: true,
  status_text: "正常",
  detail: "",
  tone: "ok",
  items: [
    { id: "cable", label: "虚拟声卡", state_label: "已安装", tone: "ok" },
    { id: "audio", label: "语音路由", state_label: "运行中", tone: "ok" },
    { id: "bridge", label: "按键桥接", state_label: "监听中", tone: "ok" },
    { id: "injection", label: "键盘注入", state_label: "硬件键盘已验证", tone: "ok" },
  ],
};

function buttonByText(wrapper: ReturnType<typeof mount>, text: string) {
  const button = wrapper.findAll("button").find((candidate) => candidate.text().includes(text));
  if (!button) throw new Error(`button not found: ${text}`);
  return button;
}

async function mountView() {
  invoke.mockImplementation((command: string) => {
    if (command === "get_device_status") {
      return Promise.resolve({
        bridge_type: "xiaomi",
        status: "Disconnected",
        device_name: null,
        device_address: null,
        battery_level: null,
        battery_charging: null,
      });
    }
    if (command === "get_config") return Promise.resolve({ ...config });
    if (command === "get_xiaomi_host_status") return Promise.resolve({ ...host });
    if (command === "get_xiaomi_voice_meter") {
      return Promise.resolve({ bleState: "idle", waveform: [], cableActive: false, cableLevel: 0, atvvOk: true });
    }
    return Promise.resolve(undefined);
  });
  const pinia = createPinia();
  setActivePinia(pinia);
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/xiaomi", name: "xiaomi", component: XiaomiSettingsView }],
  });
  await router.push("/xiaomi");
  await router.isReady();
  const wrapper = mount(XiaomiSettingsView, {
    global: {
      plugins: [pinia, i18n, router],
      stubs: { DeviceStatus: true, KeyMappingStage: true, InputMethodSettingsDialog: true },
    },
  });
  await flushPromises();
  return wrapper;
}

describe("XiaomiSettings virtual keyboard repair", () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockClear();
    i18n.global.locale.value = "zh-CN";
  });

  it("invokes the repair command and reports a restart-required result", async () => {
    const wrapper = await mountView();
    invoke.mockImplementation((command: string) => {
      if (command === "repair_xiaomi_virtual_keyboard") {
        return Promise.resolve({ ready: false, restartRequired: true, message: "请重启 Windows" });
      }
      return Promise.resolve(undefined);
    });

    const virtualKeyboardButton = buttonByText(wrapper, "修复虚拟键盘");
    await virtualKeyboardButton.trigger("click");
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith("repair_xiaomi_virtual_keyboard");
    expect(wrapper.text()).toContain("请重启 Windows");
  });

  it("disables every repair action while virtual keyboard repair is pending", async () => {
    const wrapper = await mountView();
    let finishRepair: ((result: { ready: boolean; restartRequired: boolean; message: string }) => void) | undefined;
    invoke.mockImplementation((command: string) => {
      if (command === "repair_xiaomi_virtual_keyboard") {
        return new Promise((resolve) => { finishRepair = resolve; });
      }
      return Promise.resolve(undefined);
    });

    const virtualKeyboardButton = buttonByText(wrapper, "修复虚拟键盘");
    await virtualKeyboardButton.trigger("click");
    await flushPromises();

    for (const label of ["声卡检测与修复", "修复 ATVV 连接", "重启按键桥接"]) {
      expect((buttonByText(wrapper, label).element as HTMLButtonElement).disabled).toBe(true);
    }
    expect((virtualKeyboardButton.element as HTMLButtonElement).disabled).toBe(true);

    finishRepair?.({ ready: true, restartRequired: false, message: "虚拟键盘已修复" });
    await flushPromises();
    expect((virtualKeyboardButton.element as HTMLButtonElement).disabled).toBe(false);
  });
});

describe("XiaomiSettings injection health", () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockClear();
    i18n.global.locale.value = "zh-CN";
  });

  it("shows the verified SendInput fallback layer when WinUHid is unavailable", async () => {
    const fallbackHost = {
      ...host,
      items: host.items.map((item) => item.id === "injection"
        ? { ...item, state_label: "SendInput 兜底已验证", tone: "warn" }
        : item),
    };
    invoke.mockImplementation((command: string) => {
      if (command === "get_device_status") return Promise.resolve({ bridge_type: "xiaomi", status: "Disconnected" });
      if (command === "get_config") return Promise.resolve({ ...config });
      if (command === "get_xiaomi_host_status") return Promise.resolve(fallbackHost);
      if (command === "get_xiaomi_voice_meter") return Promise.resolve({ bleState: "idle", waveform: [], cableActive: false, cableLevel: 0, atvvOk: true });
      return Promise.resolve(undefined);
    });
    const pinia = createPinia();
    setActivePinia(pinia);
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/xiaomi", name: "xiaomi", component: XiaomiSettingsView }] });
    await router.push("/xiaomi");
    await router.isReady();
    const wrapper = mount(XiaomiSettingsView, { global: { plugins: [pinia, i18n, router], stubs: { DeviceStatus: true, KeyMappingStage: true, InputMethodSettingsDialog: true } } });
    await flushPromises();

    expect(wrapper.text()).toContain("键盘注入");
    expect(wrapper.text()).toContain("SendInput 兜底已验证");
  });
});
