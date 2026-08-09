import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  GlobalSettings,
  UpdateCheckResult,
  UpdateDownloadProgress,
  UpdateRelease,
} from "../types";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "error"
  | "installing";

export const useUpdateStore = defineStore("update", () => {
  const currentVersion = ref("v0.1.5");
  const release = ref<UpdateRelease | null>(null);
  const phase = ref<UpdatePhase>("idle");
  const showDialog = ref(false);
  const error = ref("");
  const progress = ref<UpdateDownloadProgress>({
    downloadedBytes: 0,
    totalBytes: 0,
    percent: 0,
  });
  const hasUpdate = computed(() => release.value !== null && (phase.value === "available" || phase.value === "ready" || phase.value === "error"));
  const canOpen = computed(() => hasUpdate.value || phase.value === "downloading" || phase.value === "installing");
  let initialized = false;
  let unlistenProgress: UnlistenFn | undefined;

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
    try {
      const settings = await invoke<GlobalSettings>("get_global_settings");
      if (settings.auto_check_updates) await checkForUpdate(true);
    } catch (cause) {
      console.warn("Automatic update check skipped:", cause);
    }
  }

  async function checkForUpdate(silent = false) {
    if (phase.value === "checking" || phase.value === "downloading" || phase.value === "installing") return;
    phase.value = "checking";
    error.value = "";
    try {
      const result = await invoke<UpdateCheckResult>("check_for_update");
      currentVersion.value = result.currentVersion;
      release.value = result.update;
      phase.value = result.update?.downloaded ? "ready" : result.update ? "available" : "idle";
    } catch (cause) {
      release.value = null;
      phase.value = "idle";
      if (!silent) error.value = String(cause);
      console.warn("Update check failed:", cause);
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

  async function checkAfterEnabled() {
    if (!release.value && phase.value === "idle") await checkForUpdate(true);
  }

  function dispose() {
    unlistenProgress?.();
    unlistenProgress = undefined;
    initialized = false;
  }

  return {
    currentVersion,
    release,
    phase,
    showDialog,
    error,
    progress,
    hasUpdate,
    canOpen,
    initialize,
    checkForUpdate,
    checkAfterEnabled,
    openDialog,
    closeDialog,
    download,
    cancelDownload,
    install,
    dispose,
  };
});
