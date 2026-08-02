import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import { applyCachedTheme, useThemeStore } from "./stores/theme";
import { applyCachedLocale, useLocaleStore } from "./stores/locale";
import { i18n } from "./i18n";

applyCachedTheme();
applyCachedLocale();
const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
app.use(i18n);
app.use(router);
void useThemeStore(pinia).initialize();
void useLocaleStore(pinia).initialize();
app.mount("#app");
