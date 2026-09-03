import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { loggedInvoke as invoke, recordAppEvent } from "../utils/appLogger";
import type {
  GlobalSettings,
  UpdateCheckResult,
  UpdateDownloadProgress,
  UpdateRelease,
} from "../types";

const AUTO_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
const RESUME_EVENT_DEDUP_MS = 1_000;

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "error"
  | "installing";

export type UpdateCheckSource = "startup" | "scheduled" | "resume" | "settings-enabled" | "manual";

type UpdateCheckFailure = {
  stage?: string;
  message: string;
  retryAfterSeconds?: number;
};

type UpdateCheckOutcome =
  | { kind: "success"; hasUpdate: boolean; source: string }
  | { kind: "failure"; failure: UpdateCheckFailure }
  | { kind: "busy" };

function normalizeCheckFailure(cause: unknown): UpdateCheckFailure {
  if (cause && typeof cause === "object") {
    const value = cause as Partial<UpdateCheckFailure>;
    if (typeof value.message === "string") {
      return {
        stage: typeof value.stage === "string" ? value.stage : undefined,
        message: value.message,
        retryAfterSeconds: typeof value.retryAfterSeconds === "number" ? value.retryAfterSeconds : undefined,
      };
    }
  }
  return { message: String(cause) };
}

export const useUpdateStore = defineStore("update", () => {
  const currentVersion = ref("v0.1.7");
  const release = ref<UpdateRelease | null>(null);
  const phase = ref<UpdatePhase>("idle");
  const showDialog = ref(false);
  const error = ref("");
  const checkError = ref("");
  const manualCheckSucceeded = ref(false);
  const progress = ref<UpdateDownloadProgress>({
    downloadedBytes: 0,
    totalBytes: 0,
    percent: 0,
  });
  const hasUpdate = computed(() => release.value !== null && (phase.value === "available" || phase.value === "ready" || phase.value === "error"));
  const canOpen = computed(() => hasUpdate.value || phase.value === "downloading" || phase.value === "installing");
  let initialized = false;
  let autoCheckEnabled = false;
  let lastCheckAttemptAt = 0;
  let retryAfterAt = 0;
  let lastResumeEventAt = 0;
  let autoCheckTimer: ReturnType<typeof window.setTimeout> | undefined;
  let inFlightCheck: Promise<UpdateCheckOutcome> | undefined;
  let unlistenProgress: UnlistenFn | undefined;

  function clearAutoCheckTimer() {
    if (autoCheckTimer !== undefined) {
      window.clearTimeout(autoCheckTimer);
      autoCheckTimer = undefined;
    }
  }

  function scheduleAutoCheck() {
    clearAutoCheckTimer();
    if (!autoCheckEnabled || !initialized) return;
    const now = Date.now();
    const elapsed = now - lastCheckAttemptAt;
    const intervalWait = lastCheckAttemptAt === 0 ? 0 : Math.max(0, AUTO_CHECK_INTERVAL_MS - elapsed);
    const rateLimitWait = retryAfterAt === 0 ? Number.POSITIVE_INFINITY : Math.max(1_000, retryAfterAt - now);
    const wait = Math.min(intervalWait, rateLimitWait);
    autoCheckTimer = window.setTimeout(() => {
      void checkIfDue("scheduled");
    }, wait);
  }

  function recordCheck(source: UpdateCheckSource, outcome: string, details: Record<string, unknown> = {}) {
    recordAppEvent({
      category: "update",
      action: "check",
      outcome,
      details: { source, ...details },
    });
  }

  function applyManualOutcome(outcome: UpdateCheckOutcome) {
    manualCheckSucceeded.value = outcome.kind === "success" && !outcome.hasUpdate;
    checkError.value = outcome.kind === "failure" ? outcome.failure.message : "";
  }

  async function runCheck(source: UpdateCheckSource): Promise<UpdateCheckOutcome> {
    lastCheckAttemptAt = Date.now();
    retryAfterAt = 0;
    phase.value = "checking";
    error.value = "";
    try {
      const result = await invoke<UpdateCheckResult>("check_for_update");
      currentVersion.value = result.currentVersion;
      release.value = result.update;
      phase.value = result.update?.downloaded ? "ready" : result.update ? "available" : "idle";
      checkError.value = "";
      manualCheckSucceeded.value = false;
      recordCheck(source, "success", { updateSource: result.source, hasUpdate: Boolean(result.update) });
      return { kind: "success", hasUpdate: Boolean(result.update), source: result.source };
    } catch (cause) {
      const failure = normalizeCheckFailure(cause);
      retryAfterAt = failure.retryAfterSeconds
        ? Date.now() + failure.retryAfterSeconds * 1_000
        : 0;
      phase.value = release.value?.downloaded ? "ready" : release.value ? "available" : "idle";
      recordCheck(source, "error", {
        stage: failure.stage ?? "unknown",
        message: failure.message,
        retryAfterSeconds: failure.retryAfterSeconds,
      });
      return { kind: "failure", failure };
    } finally {
      scheduleAutoCheck();
    }
  }

  async function checkForUpdate(source: UpdateCheckSource = "manual"): Promise<UpdateCheckOutcome> {
    if (phase.value === "downloading" || phase.value === "installing") {
      const outcome: UpdateCheckOutcome = { kind: "busy" };
      recordCheck(source, "skipped", { reason: phase.value });
      if (source === "manual") applyManualOutcome(outcome);
      return outcome;
    }

    if (inFlightCheck) {
      recordCheck(source, "joined");
      const outcome = await inFlightCheck;
      if (source === "manual") applyManualOutcome(outcome);
      return outcome;
    }

    const request = runCheck(source);
    inFlightCheck = request;
    try {
      const outcome = await request;
      if (source === "manual") applyManualOutcome(outcome);
      return outcome;
    } finally {
      if (inFlightCheck === request) inFlightCheck = undefined;
    }
  }

  async function checkIfDue(source: Exclude<UpdateCheckSource, "manual">): Promise<UpdateCheckOutcome | undefined> {
    if (!autoCheckEnabled) {
      recordCheck(source, "skipped", { reason: "disabled" });
      return undefined;
    }
    const retryDue = retryAfterAt !== 0 && Date.now() >= retryAfterAt;
    if (!retryDue && lastCheckAttemptAt !== 0 && Date.now() - lastCheckAttemptAt < AUTO_CHECK_INTERVAL_MS) {
      recordCheck(source, "skipped", { reason: "not-due" });
      scheduleAutoCheck();
      return undefined;
    }
    return checkForUpdate(source);
  }

  function onWindowActivity() {
    if (document.visibilityState === "hidden") return;
    const now = Date.now();
    if (now - lastResumeEventAt < RESUME_EVENT_DEDUP_MS) return;
    lastResumeEventAt = now;
    void checkIfDue("resume");
  }

  async function setAutoCheckEnabled(enabled: boolean) {
    autoCheckEnabled = enabled;
    if (!enabled) {
      clearAutoCheckTimer();
      return;
    }
    await checkIfDue("settings-enabled");
    scheduleAutoCheck();
  }

  async function initialize() {
    if (initialized) return;
    initialized = true;
    try {
      currentVersion.value = `v${await getVersion()}`;
    } catch (cause) {
      console.warn("Failed to read app version:", cause);
    }
    unlistenProgress = await listen<UpdateDownloadProgress>("update-download-progress", (event) => {
      progress.value = event.payload;
    });
    window.addEventListener("focus", onWindowActivity);
    document.addEventListener("visibilitychange", onWindowActivity);
    try {
      const settings = await invoke<GlobalSettings>("get_global_settings");
      await setAutoCheckEnabled(settings.auto_check_updates);
    } catch (cause) {
      console.warn("Automatic update check skipped:", cause);
      scheduleAutoCheck();
    }
  }

  function openDialog() {
    if (canOpen.value) showDialog.value = true;
  }

  function closeDialog() {
    if (phase.value !== "downloading" && phase.value !== "installing") showDialog.value = false;
  }

  async function download() {
    if (!release.value || phase.value === "downloading") return;
    error.value = "";
    progress.value = { downloadedBytes: 0, totalBytes: release.value.assetSize, percent: 0 };
    phase.value = "downloading";
    try {
      await invoke("download_update");
      release.value = { ...release.value, downloaded: true };
      phase.value = "ready";
    } catch (cause) {
      const message = String(cause);
      phase.value = message.includes("下载已取消") ? "available" : "error";
      if (phase.value === "error") error.value = message;
    }
  }

  async function cancelDownload() {
    if (phase.value !== "downloading") return;
    try {
      await invoke("cancel_update_download");
    } catch (cause) {
      error.value = String(cause);
    }
  }

  async function install() {
    if (!release.value || phase.value !== "ready") return;
    phase.value = "installing";
    error.value = "";
    try {
      await invoke("install_downloaded_update");
    } catch (cause) {
      error.value = String(cause);
      phase.value = "ready";
    }
  }

  function dispose() {
    clearAutoCheckTimer();
    window.removeEventListener("focus", onWindowActivity);
    document.removeEventListener("visibilitychange", onWindowActivity);
    unlistenProgress?.();
    unlistenProgress = undefined;
    initialized = false;
    autoCheckEnabled = false;
  }

  return {
    currentVersion,
    release,
    phase,
    showDialog,
    error,
    checkError,
    manualCheckSucceeded,
    progress,
    hasUpdate,
    canOpen,
    initialize,
    checkForUpdate,
    setAutoCheckEnabled,
    openDialog,
    closeDialog,
    download,
    cancelDownload,
    install,
    dispose,
  };
});
