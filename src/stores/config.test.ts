import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConfigStore } from "./config";
import { applyImePresetConfig } from "../utils/imePreset";
import type { DeviceConfig } from "../types";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../utils/appLogger", () => ({ loggedInvoke: invoke }));

const base: DeviceConfig = {
  button_aliases: {}, button_bindings: {}, voice_hotkey: [],
  trigger_mode: "Hold", bluetooth_address: null,
};

describe("saved input-method configuration", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    invoke.mockReset();
  });

  it("does not let an older load clear a newly saved WeChat preset", async () => {
    const store = useConfigStore();
    let finishLoad!: (config: DeviceConfig) => void;
    invoke.mockImplementation((command) => command === "get_config"
      ? new Promise((resolve) => { finishLoad = resolve; })
      : Promise.resolve());
    const pendingLoad = store.loadConfig("xiaomi");
    const next = applyImePresetConfig(base, "wechat");
    expect(await store.saveConfig("xiaomi", next)).toBe(true);
    finishLoad(base);
    await pendingLoad;
    expect(store.configs.xiaomi?.voice_input_profile).toBe("wechat");
    expect(store.configs.xiaomi?.trigger_mode).toBe("Toggle");
  });

  it("retains the saved preset on write failure and reports failure to the UI", async () => {
    const store = useConfigStore();
    store.configs.xiaomi = applyImePresetConfig(base, "qianwen");
    invoke.mockRejectedValue(new Error("write failed"));
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await store.saveConfig("xiaomi", applyImePresetConfig(base, "wechat"))).toBe(false);
    expect(store.configs.xiaomi.voice_input_profile).toBe("qianwen");
    expect(store.saving).toBe(false);
    log.mockRestore();
  });
});
