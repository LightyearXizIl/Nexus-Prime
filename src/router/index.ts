import { createRouter, createWebHashHistory } from "vue-router";
import XiaomiSettings from "../views/XiaomiSettings.vue";
import GlobalSettings from "../views/GlobalSettings.vue";

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      redirect: "/xiaomi",
    },
    {
      path: "/xiaomi",
      name: "xiaomi",
      component: XiaomiSettings,
    },
    {
      path: "/xiaomi/mapping",
      name: "xiaomi-mapping",
      component: XiaomiSettings,
    },
    {
      path: "/settings",
      name: "settings",
      component: GlobalSettings,
    },
    {
      path: "/:pathMatch(.*)*",
      redirect: "/xiaomi",
    },
  ],
});

export default router;
