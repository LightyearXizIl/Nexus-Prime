import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "../i18n";
import type { GlobalSettings } from "../types";
import GlobalSettingsView from "./GlobalSettings.vue";

const { invoke, confirm } = vi.hoisted(() => ({ invoke: vi.fn(), confirm: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn().mockResolvedValue("0.2.6") }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setTheme: vi.fn().mockResolvedValue(undefined) }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm }));

const defaultSettings: GlobalSettings = {
  autostart: false,
  autostart_minimized_to_tray: false,
  language: "zh-CN",
  minimize_to_tray: true,
  auto_check_updates: true,
  theme: "system",
  log_retention_days: 7,
};

async function mountSettings(settings = defaultSettings) {
  invoke.mockImplementation((command: string) => {
    if (command === "get_global_settings") return Promise.resolve({ ...settings });
    if (command === "check_for_update") return Promise.resolve({ currentVersion: "v0.2.6", update: null });
    return Promise.resolve(undefined);
  });
  const pinia = createPinia();
  setActivePinia(pinia);
  const wrapper = mount(GlobalSettingsView, { global: { plugins: [pinia, i18n] } });
  await flushPromises();
  return wrapper;
}

describe("GlobalSettings autostart tray preference", () => {
  beforeEach(() => {
    window.localStorage.clear();
    i18n.global.locale.value = "zh-CN";
    vi.spyOn(console, "error").mockImplementation(() => {});
    invoke.mockReset();
    confirm.mockReset();
    confirm.mockResolvedValue(true);
    invoke.mockImplementation((command: string) => {
      if (command === "get_global_settings") return Promise.resolve({ ...defaultSettings });
      return Promise.resolve(undefined);
    });
  });

  it("defaults off and keeps the existing close-to-tray preference unchanged", async () => {
    const wrapper = await mountSettings();
    const toggles = wrapper.findAll('input[type="checkbox"]');

    expect(toggles).toHaveLength(4);
    expect((toggles[0].element as HTMLInputElement).checked).toBe(false);
    expect((toggles[1].element as HTMLInputElement).checked).toBe(false);
    expect((toggles[1].element as HTMLInputElement).disabled).toBe(true);
    expect((toggles[2].element as HTMLInputElement).checked).toBe(true);
  });

  it("disables the dependent preference without clearing it and saves the full payload", async () => {
    const wrapper = await mountSettings({
      ...defaultSettings,
      autostart: true,
      autostart_minimized_to_tray: true,
      minimize_to_tray: false,
    });
    const toggles = wrapper.findAll('input[type="checkbox"]');

    await toggles[0].setValue(false);
    expect((toggles[1].element as HTMLInputElement).disabled).toBe(true);
    expect((toggles[1].element as HTMLInputElement).checked).toBe(true);
    await toggles[0].setValue(true);
    await wrapper.get("button.save-button").trigger("click");

    expect(invoke).toHaveBeenCalledWith("save_global_settings", {
      settings: expect.objectContaining({
        autostart: true,
        autostart_minimized_to_tray: true,
        minimize_to_tray: false,
      }),
    });
  });

  it("keeps the edited setting available for a retry after save failure", async () => {
    const wrapper = await mountSettings();
    const toggles = wrapper.findAll('input[type="checkbox"]');
    await toggles[0].setValue(true);
    await toggles[1].setValue(true);

    invoke.mockRejectedValueOnce(new Error("denied"));
    await wrapper.get("button.save-button").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("保存失败");
    expect((toggles[1].element as HTMLInputElement).checked).toBe(true);

    await wrapper.get("button.save-button").trigger("click");
    await flushPromises();
    expect(invoke).toHaveBeenCalledWith("save_global_settings", {
      settings: expect.objectContaining({ autostart_minimized_to_tray: true }),
    });
  });

  it("clears only old logs after confirmation without saving retention edits", async () => {
    const wrapper = await mountSettings();
    invoke.mockImplementation((command: string) => {
      if (command === "clear_old_app_logs") return Promise.resolve({ deletedFiles: 0, freedBytes: 0 });
      return Promise.resolve(undefined);
    });
    const select = wrapper.get(".log-retention-controls select");
    await select.setValue("14");
    await wrapper.get("button.clear-logs-button").trigger("click");
    await flushPromises();

    expect(confirm).toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("clear_old_app_logs");
    expect(wrapper.text()).toContain("已清理 0 个旧日志");
    expect((select.element as HTMLSelectElement).value).toBe("14");
  });

  it("reports a log cleanup failure inline", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_global_settings") return Promise.resolve({ ...defaultSettings });
      if (command === "clear_old_app_logs") return Promise.reject(new Error("denied"));
      return Promise.resolve(undefined);
    });
    const pinia = createPinia();
    setActivePinia(pinia);
    const wrapper = mount(GlobalSettingsView, { global: { plugins: [pinia, i18n] } });
    await flushPromises();
    await wrapper.get("button.clear-logs-button").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("清理日志失败");
  });
});
