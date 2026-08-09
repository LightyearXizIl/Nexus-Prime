import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { i18n } from "../i18n";
import type { DeviceConfig } from "../types";
import KeyMappingStage from "./KeyMappingStage.vue";
import VoiceShortcutComposer from "./VoiceShortcutComposer.vue";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

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
  afterEach(() => vi.clearAllMocks());

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

  it("does not flag valid right-Alt and saves a manual shortcut to the selected long-press slot", async () => {
    const config = createConfig();
    config.button_bindings.mic = { type: "SingleKey", value: 0xa5 };
    config.button_bindings.voice = { type: "SingleKey", value: 0xa5 };
    config.voice_hotkey = ["rightalt"];
    const wrapper = mount(KeyMappingStage, {
      props: { config },
      global: { plugins: [i18n], stubs: { RemoteHotspot: true } },
    });
    const voiceRow = wrapper.findAll("button.mapping-row").find((row) => row.text().includes("语音键"));
    await voiceRow!.trigger("click");
    expect(wrapper.text()).not.toContain("可能缺少主键 D");

    await wrapper.findAll("button.selection-action").find((button) => button.text().includes("手动组合"))!.trigger("click");
    const slotButton = wrapper.findAll("button.selection-action").find((button) => button.text().includes("单击"));
    await slotButton!.trigger("click");
    await wrapper.findAll('[role="menuitemradio"]').find((item) => item.text().includes("长按"))!.trigger("click");
    await wrapper.findAll("button.selection-action").find((button) => button.text().includes("手动组合"))!.trigger("click");
    wrapper.findComponent(VoiceShortcutComposer).vm.$emit("apply", [0xa5]);
    await nextTick();

    expect(latestSave(wrapper).long_press_bindings?.mic).toEqual({ type: "SingleKey", value: 0xa5 });
    expect(latestSave(wrapper).voice_hotkey).toEqual(["rightalt"]);
    wrapper.unmount();
  });
});
