import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { i18n, isAppLocale, type AppLocale } from "../i18n";
import type { GlobalSettings } from "../types";

const CACHE_KEY = "nexus-prime.locale";

function cachedLocale(): AppLocale {
  try {
    const value = window.localStorage.getItem(CACHE_KEY);
    return isAppLocale(value) ? value : "zh-CN";
  } catch { return "zh-CN"; }
}

function apply(locale: AppLocale) {
  i18n.global.locale.value = locale;
  document.documentElement.lang = locale;
}

export function applyCachedLocale(): AppLocale {
  const locale = cachedLocale();
  apply(locale);
  return locale;
}

export const useLocaleStore = defineStore("locale", () => {
  const preference = ref<AppLocale>(applyCachedLocale());
  const saving = ref(false);
  const error = ref("");

  function cache(locale: AppLocale) {
    try { window.localStorage.setItem(CACHE_KEY, locale); } catch { /* optional cache */ }
  }

  async function initialize() {
    try {
      const settings = await invoke<GlobalSettings>("get_global_settings");
      if (isAppLocale(settings.language) && !saving.value) {
        preference.value = settings.language;
        apply(settings.language);
        cache(settings.language);
      }
    } catch (cause) {
      console.warn("Failed to load language preference:", cause);
    }
  }

  async function setPreference(next: AppLocale): Promise<boolean> {
    if (saving.value || next === preference.value) return true;
    const previous = preference.value;
    error.value = "";
    preference.value = next;
    apply(next);
    saving.value = true;
    try {
      await invoke("set_language_preference", { language: next });
      cache(next);
      return true;
    } catch (cause) {
      preference.value = previous;
      apply(previous);
      error.value = String(cause);
      console.error("Failed to save language preference:", cause);
      return false;
    } finally { saving.value = false; }
  }

  return { preference, saving, error, initialize, setPreference };
});
