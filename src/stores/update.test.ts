import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUpdateStore } from "./update";

const { invoke, getVersion, listen, recordAppEvent } = vi.hoisted(() => ({
  invoke: vi.fn(),
  getVersion: vi.fn(),
  listen: vi.fn(),
  recordAppEvent: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("../utils/appLogger", () => ({ loggedInvoke: invoke, recordAppEvent }));

const autoSettings = {
  autostart: false,
  autostart_minimized_to_tray: false,
  language: "zh-CN",
  minimize_to_tray: true,
  auto_check_updates: true,
  theme: "system",
  log_retention_days: 7,
};

const noUpdate = { currentVersion: "v0.3.8", update: null, source: "release-manifest" };

describe("update check coordinator", () => {
  let store: ReturnType<typeof useUpdateStore>;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-03T12:00:00Z"));
    setActivePinia(createPinia());
    store = useUpdateStore();
    getVersion.mockResolvedValue("0.3.8");
    listen.mockResolvedValue(() => {});
    invoke.mockImplementation((command: string) => {
      if (command === "get_global_settings") return Promise.resolve({ ...autoSettings });
      if (command === "check_for_update") return Promise.resolve({ ...noUpdate });
      return Promise.resolve(undefined);
    });
  });

  afterEach(() => {
    store.dispose();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("checks on startup, then once again after six hours", async () => {
    await store.initialize();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenLastCalledWith("check_for_update");

    await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);
    expect(invoke.mock.calls.filter(([command]) => command === "check_for_update")).toHaveLength(2);
  });

  it("does not recheck on focus or visibility restoration before it is due", async () => {
    await store.initialize();
    window.dispatchEvent(new Event("focus"));
    document.dispatchEvent(new Event("visibilitychange"));
    await Promise.resolve();

    expect(invoke.mock.calls.filter(([command]) => command === "check_for_update")).toHaveLength(1);
  });

  it("retries after the rate-limit recovery time without waiting six hours", async () => {
    let attempts = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "get_global_settings") return Promise.resolve({ ...autoSettings });
      if (command === "check_for_update") {
        attempts += 1;
        return attempts === 1
          ? Promise.reject({ stage: "http", message: "额度用尽", retryAfterSeconds: 90 })
          : Promise.resolve({ ...noUpdate });
      }
      return Promise.resolve(undefined);
    });

    await store.initialize();
    await vi.advanceTimersByTimeAsync(90_000);

    expect(attempts).toBe(2);
  });

  it("joins an automatic check when the user clicks manual check", async () => {
    let resolveCheck: ((value: typeof noUpdate) => void) | undefined;
    invoke.mockImplementation((command: string) => {
      if (command === "get_global_settings") return Promise.resolve({ ...autoSettings });
      if (command === "check_for_update") return new Promise<typeof noUpdate>((resolve) => { resolveCheck = resolve; });
      return Promise.resolve(undefined);
    });

    const initializing = store.initialize();
    await vi.runAllTicks();
    const manual = store.checkForUpdate("manual");
    resolveCheck?.({ ...noUpdate });
    await Promise.all([initializing, manual]);

    expect(invoke.mock.calls.filter(([command]) => command === "check_for_update")).toHaveLength(1);
    expect(store.manualCheckSucceeded).toBe(true);
  });

  it("keeps a manual failure retryable and visible", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_global_settings") return Promise.resolve({ ...autoSettings, auto_check_updates: false });
      if (command === "check_for_update") return Promise.reject({ stage: "http", message: "GitHub 请求额度暂时用尽。" });
      return Promise.resolve(undefined);
    });
    await store.initialize();
    await store.checkForUpdate("manual");

    expect(store.phase).toBe("idle");
    expect(store.checkError).toBe("GitHub 请求额度暂时用尽。");
    expect(store.manualCheckSucceeded).toBe(false);
  });
});
