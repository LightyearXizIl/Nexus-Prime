import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import router from "./router";
import { applyCachedTheme, useThemeStore } from "./stores/theme";

applyCachedTheme();
const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
app.use(router);
void useThemeStore(pinia).initialize();
app.mount("#app");
