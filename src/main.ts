import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import { applyCachedTheme, useThemeStore } from "./stores/theme";
import { applyCachedLocale, useLocaleStore } from "./stores/locale";
import { i18n } from "./i18n";
import { installUiLogging, recordAppEvent } from "./utils/appLogger";

applyCachedTheme();
applyCachedLocale();
const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
app.use(i18n);
app.use(router);
installUiLogging();
router.afterEach((to, from) => {
  recordAppEvent({ category: "navigation", action: "route_change", target: to.fullPath, details: { from: from.fullPath } });
});
void useThemeStore(pinia).initialize();
void useLocaleStore(pinia).initialize();
app.mount("#app");
