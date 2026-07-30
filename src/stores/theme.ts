import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { defineStore } from "pinia";
import { ref } from "vue";
import type { GlobalSettings, ThemePreference } from "../types";

export type EffectiveTheme = Exclude<ThemePreference, "system">;

const THEME_CACHE_KEY = "nexus-prime.theme-preference";

function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

function readCachedPreference(): ThemePreference {
  try {
    const cached = window.localStorage.getItem(THEME_CACHE_KEY);
    return isThemePreference(cached) ? cached : "system";
  } catch {
    return "system";
  }
}

function cachePreference(preference: ThemePreference) {
  try {
    window.localStorage.setItem(THEME_CACHE_KEY, preference);
  } catch {
    // 本地缓存仅用于首帧；settings.json 仍是主题的权威来源。
  }
}

function systemTheme(): EffectiveTheme {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

function resolveTheme(preference: ThemePreference): EffectiveTheme {
  return preference === "system" ? systemTheme() : preference;
}

function applyDocumentTheme(theme: EffectiveTheme) {
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

function syncNativeTheme(preference: ThemePreference) {
  void getCurrentWindow()
    .setTheme(preference === "system" ? null : preference)
    .catch((error) => {
    // 浏览器预览环境不具备 Tauri 原生标题栏，此时仅保留网页主题。
    console.warn("Failed to sync native theme:", error);
      console.warn("Failed to sync native theme:", error);
    });
}

/** 在 Vue 挂载前应用缓存，避免已保存的手动主题出现闪白。 */
export function applyCachedTheme(): ThemePreference {
  const preference = readCachedPreference();
  applyDocumentTheme(resolveTheme(preference));
  return preference;
}

export const useThemeStore = defineStore("theme", () => {
  const preference = ref<ThemePreference>(applyCachedTheme());
  const effectiveTheme = ref<EffectiveTheme>(resolveTheme(preference.value));
  const initialized = ref(false);
  const saving = ref(false);
  const error = ref("");
  let mediaQuery: MediaQueryList | null = null;

  function apply(preferenceToApply: ThemePreference) {
    effectiveTheme.value = resolveTheme(preferenceToApply);
    applyDocumentTheme(effectiveTheme.value);
    syncNativeTheme(preferenceToApply);
  }

  function ensureSystemListener() {
    if (mediaQuery || !window.matchMedia) return;
    mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    mediaQuery.addEventListener("change", () => {
      if (preference.value === "system") apply(preference.value);
    });
  }

  async function initialize() {
    ensureSystemListener();
    apply(preference.value);
    const initialPreference = preference.value;
    try {
      const settings = await invoke<GlobalSettings>("get_global_settings");
      const persisted = isThemePreference(settings.theme) ? settings.theme : "system";
      // 如果用户已经在初始请求期间切换了主题，保留其新选择。
      if (!saving.value && preference.value === initialPreference) {
        preference.value = persisted;
        apply(persisted);
      }
      cachePreference(persisted);
    } catch (loadError) {
      console.warn("Failed to load theme preference:", loadError);
    } finally {
      initialized.value = true;
    }
  }

  async function setPreference(nextPreference: ThemePreference): Promise<boolean> {
    if (saving.value || nextPreference === preference.value) return true;

    const previousPreference = preference.value;
    error.value = "";
    preference.value = nextPreference;
    apply(nextPreference);
    saving.value = true;

    try {
      await invoke("set_theme_preference", { theme: nextPreference });
      cachePreference(nextPreference);
      return true;
    } catch (saveError) {
      preference.value = previousPreference;
      apply(previousPreference);
      error.value = `主题保存失败：${String(saveError)}`;
      console.error("Failed to save theme preference:", saveError);
      return false;
    } finally {
      saving.value = false;
    }
  }

  function toggle() {
    return setPreference(effectiveTheme.value === "dark" ? "light" : "dark");
  }

  return {
    preference,
    effectiveTheme,
    initialized,
    saving,
    error,
    initialize,
    setPreference,
    toggle,
  };
});
