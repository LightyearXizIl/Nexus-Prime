import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { i18n } from "../i18n";
import type { DeviceConfig } from "../types";
import KeyMappingStage from "./KeyMappingStage.vue";
import VoiceShortcutComposer from "./VoiceShortcutComposer.vue";

const invokeMock = vi.hoisted(() => vi.fn().mockResolvedValue([]));
const dialogOpenMock = vi.hoisted(() => vi.fn());
const dialogSaveMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialogOpenMock, save: dialogSaveMock }));

function createConfig(): DeviceConfig {
  return {
    button_aliases: {},
    button_bindings: {
      mic: { type: "ComboKey", value: [0xa2, 0xa0] },
      voice: { type: "ComboKey", value: [0xa2, 0xa0] },
    },
    long_press_bindings: {},
    multi_click_bindings: {},
    multi_click_interval_ms: 300,
    voice_hotkey: ["leftctrl", "leftshift"],
    trigger_mode: "Hold",
    bluetooth_address: null,
  };
}

function latestSave(wrapper: ReturnType<typeof mount>) {
  const saves = wrapper.emitted("save") ?? [];
  return saves[saves.length - 1]?.[0] as DeviceConfig;
}

describe("KeyMappingStage voice mapping", () => {
  afterEach(() => {
    vi.clearAllMocks();
    i18n.global.locale.value = "zh-CN";
  });

  it("only flags the known truncated Codex mapping and synchronizes the single-click aliases", async () => {
    const wrapper = mount(KeyMappingStage, {
      props: { config: createConfig() },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });
    const voiceRow = wrapper.findAll("button.mapping-row").find((row) => row.text().includes("语音键"));
    expect(voiceRow).toBeDefined();
    await voiceRow!.trigger("click");
    expect(wrapper.text()).toContain("可能缺少主键 D");

    await wrapper.findAll("button.selection-action").find((button) => button.text().includes("手动组合"))!.trigger("click");
    wrapper.findComponent(VoiceShortcutComposer).vm.$emit("apply", [0xa2, 0xa0, 0x44]);
    await nextTick();

    expect(latestSave(wrapper)).toMatchObject({
      button_bindings: {
        mic: { type: "ComboKey", value: [0xa2, 0xa0, 0x44] },
        voice: { type: "ComboKey", value: [0xa2, 0xa0, 0x44] },
      },
      voice_hotkey: ["leftctrl", "leftshift", "d"],
    });
    wrapper.unmount();
  });

  it("keeps voice dedicated and clears legacy gesture mappings", async () => {
    const config = createConfig();
    config.button_bindings.mic = { type: "SingleKey", value: 0xa5 };
    config.button_bindings.voice = { type: "SingleKey", value: 0xa5 };
    config.voice_hotkey = ["rightalt"];
    config.long_press_bindings = { mic: { type: "SingleKey", value: 0xa5 } };
    config.multi_click_bindings = {
      mic: { 2: { type: "SingleKey", value: 0xa5 } },
      menu: { 2: { type: "SingleKey", value: 0x41 } },
    };
    const wrapper = mount(KeyMappingStage, {
      props: { config },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });
    const voiceRow = wrapper.findAll("button.mapping-row").find((row) => row.text().includes("语音键"));
    await voiceRow!.trigger("click");
    expect(wrapper.text()).not.toContain("可能缺少主键 D");
    expect(wrapper.text()).toContain("语音快捷键");
    expect(wrapper.text()).not.toContain("连击间隔");
    expect(wrapper.findAll('[role="menuitemradio"]')).toHaveLength(0);

    await wrapper.findAll("button.selection-action").find((button) => button.text().includes("手动组合"))!.trigger("click");
    wrapper.findComponent(VoiceShortcutComposer).vm.$emit("apply", [0xa5]);
    await nextTick();

    expect(latestSave(wrapper)).toMatchObject({
      button_bindings: {
        mic: { type: "SingleKey", value: 0xa5 },
        voice: { type: "SingleKey", value: 0xa5 },
      },
      voice_hotkey: ["rightalt"],
      long_press_bindings: {},
      multi_click_bindings: {
        menu: { 2: { type: "SingleKey", value: 0x41 } },
      },
    });
    wrapper.unmount();
  });

  it("shows mouse controls without shortcut capture and saves a left click to the selected slot", async () => {
    const wrapper = mount(KeyMappingStage, {
      props: { config: createConfig() },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });
    const upRow = wrapper.findAll("button.mapping-row").find((row) => row.text().includes("上键"));
    expect(upRow).toBeDefined();
    await upRow!.trigger("click");

    expect(wrapper.find('[aria-label="鼠标操作"]').exists()).toBe(true);
    expect(wrapper.text()).not.toContain("取消录入");
    await wrapper.find('[aria-label="鼠标操作"]').trigger("click");
    await wrapper.findAll('[role="menuitemradio"]').find((item) => item.text().includes("鼠标左键"))!.trigger("click");

    expect(latestSave(wrapper).button_bindings.up).toEqual({ type: "MouseClick", value: null });
    wrapper.unmount();
  });

  it("clamps mouse movement step, limits acceleration to long press, and localizes its controls", async () => {
    const wrapper = mount(KeyMappingStage, {
      props: { config: createConfig() },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });
    const upRow = wrapper.findAll("button.mapping-row").find((row) => row.text().includes("上键"));
    await upRow!.trigger("click");

    await wrapper.find('[aria-label="鼠标操作"]').trigger("click");
    await wrapper.findAll('[role="menuitemradio"]').find((item) => item.text().includes("鼠标向上移动"))!.trigger("click");
    expect(latestSave(wrapper).button_bindings.up).toEqual({
      type: "MouseMove",
      value: { dx: 0, dy: -1, step: 20, accelerate: false },
    });

    await wrapper.findAll("button.selection-action").find((button) => button.text().includes("单击"))!.trigger("click");
    await wrapper.findAll('[role="menuitemradio"]').find((item) => item.text().includes("长按"))!.trigger("click");
    await nextTick();
    await wrapper.find('[aria-label="鼠标操作"]').trigger("click");
    await wrapper.findAll('[role="menuitemradio"]').find((item) => item.text().includes("鼠标向上移动"))!.trigger("click");
    await wrapper.find(".mouse-pick-step input").setValue(999);
    await wrapper.find(".mouse-pick-accel input").setValue(false);

    expect(latestSave(wrapper).long_press_bindings?.up).toEqual({
      type: "MouseMove",
      value: { dx: 0, dy: -1, step: 100, accelerate: false },
    });

    i18n.global.locale.value = "en";
    await nextTick();
    expect(wrapper.text()).toContain("Mouse controls");
    expect(wrapper.findAll('[role="menuitemradio"]').some((item) => item.text().includes("Move mouse up"))).toBe(true);
    wrapper.unmount();
  });

  it("keeps the mouse and click-count menus mutually exclusive and closes them with Escape", async () => {
    const wrapper = mount(KeyMappingStage, {
      props: { config: createConfig() },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });
    const upRow = wrapper.findAll("button.mapping-row").find((row) => row.text().includes("上键"));
    await upRow!.trigger("click");

    await wrapper.find('[aria-label="鼠标操作"]').trigger("click");
    expect(wrapper.find(".mouse-menu").exists()).toBe(true);
    await wrapper.findAll("button.selection-action").find((button) => button.text().includes("单击"))!.trigger("click");
    expect(wrapper.find(".mouse-menu").exists()).toBe(false);
    expect(wrapper.find(".click-menu").exists()).toBe(true);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await nextTick();
    expect(wrapper.find(".click-menu").exists()).toBe(false);
    wrapper.unmount();
  });

  it("shows reset risk help without resetting, then saves the backend reset result", async () => {
    const resetConfig = createConfig();
    resetConfig.button_bindings.power = { type: "SingleKey", value: 0x1b };
    resetConfig.long_press_bindings = {};
    resetConfig.multi_click_bindings = {};
    invokeMock.mockImplementation((command: string) =>
      Promise.resolve(command === "reset_xiaomi_standard_key_bindings" ? resetConfig : []),
    );
    const wrapper = mount(KeyMappingStage, {
      props: { config: createConfig() },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });

    const tip = wrapper.get('[aria-label="一键重置风险说明"]');
    await tip.trigger("mouseenter");
    await nextTick();
    expect(document.body.textContent).toContain("重置前请注意");
    expect(invokeMock).not.toHaveBeenCalledWith("reset_xiaomi_standard_key_bindings");

    await wrapper.get("button.mapping-reset-all").trigger("click");
    await nextTick();
    expect(invokeMock).toHaveBeenCalledWith("reset_xiaomi_standard_key_bindings");
    expect(latestSave(wrapper)).toBe(resetConfig);
    expect(wrapper.text()).toContain("音量和语音设置已保留");
    wrapper.unmount();
  });

  it("exports the mapping-only configuration through the native save dialog", async () => {
    dialogSaveMock.mockResolvedValue("C:/temp/mappings.json");
    invokeMock.mockResolvedValue(undefined);
    const wrapper = mount(KeyMappingStage, {
      props: { config: createConfig() },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });

    await wrapper.findAll("button").find((button) => button.text() === "导出配置")!.trigger("click");
    await nextTick();
    expect(dialogSaveMock).toHaveBeenCalled();
    expect(invokeMock).toHaveBeenCalledWith("export_xiaomi_mappings", { path: "C:/temp/mappings.json" });
    expect(wrapper.text()).toContain("映射配置已导出");
    wrapper.unmount();
  });

  it("imports a validated mapping configuration and emits the returned device config", async () => {
    dialogOpenMock.mockResolvedValue("C:/temp/mappings.json");
    const imported = {
      ...createConfig(),
      button_bindings: { power: { type: "SingleKey", value: 0x1b } },
      long_press_bindings: {},
      multi_click_bindings: {},
      multi_click_interval_ms: 450,
    };
    invokeMock.mockResolvedValue(imported);
    const wrapper = mount(KeyMappingStage, {
      props: { config: createConfig() },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });

    await wrapper.findAll("button").find((button) => button.text() === "导入配置")!.trigger("click");
    await nextTick();
    expect(dialogOpenMock).toHaveBeenCalled();
    expect(invokeMock).toHaveBeenCalledWith("import_xiaomi_mappings", { path: "C:/temp/mappings.json" });
    const saves = wrapper.emitted("save") || [];
    expect(saves[saves.length - 1]?.[0]).toEqual(imported);
    expect(wrapper.text()).toContain("映射配置已导入");
    wrapper.unmount();
  });
});
