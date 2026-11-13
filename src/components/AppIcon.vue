<script setup lang="ts">
import { computed } from "vue";
const props = withDefaults(defineProps<{ name: string; size?: number }>(), { size: 20 });
// Original 24px drawings. A consistent stroke and generous spacing keep 18–24px icons legible.
const drawings: Record<string, string[]> = {
  home: ["M3 10.5 12 3l9 7.5M5 9v12h5v-7h4v7h5V9"],
  mapping: ["M4 5h6v6H4zM14 5h6v6h-6zM4 15h6v6H4zM14 15h6v6h-6z"],
  settings: ["M9 3h6v3l2 1 2-1 3 5-2 1v2l2 1-3 5-2-1-2 1v3H9v-3l-2-1-2 1-3-5 2-1v-2l-2-1 3-5 2 1 2-1Z", "M15.5 12a3.5 3.5 0 1 1-7 0 3.5 3.5 0 0 1 7 0"],
  sun: ["M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5 19 19M5 19l1.5-1.5M17.5 6.5 19 5"],
  moon: ["M20.5 14.5A8.5 8.5 0 0 1 9.5 3.5a8.5 8.5 0 1 0 11 11Z"],
  power: ["M12 3v9M6 5.5a8.5 8.5 0 1 0 12 0"],
  mic: ["M9 5a3 3 0 0 1 6 0v8a3 3 0 0 1-6 0ZM5 10v3a7 7 0 0 0 14 0v-3M12 20v3M8 23h8"],
  cable: ["M4 6h16v12H4zM7 10h4M7 14h2M15 10h2v4h-2zM9 3v3M15 18v3"],
  audio: ["M5 8v8M9 4v16M13 9v6M17 6v12M21 10v4"],
  bridge: ["m8 16-2 2a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0m2 1 2-2a4 4 0 0 1 6 6l-5 5a4 4 0 0 1-6 0M8 16l8-8"],
  keyboard: ["M3 5h18v14H3zM6 9h1m3 0h1m3 0h1m3 0h0M6 12h1m3 0h1m3 0h1m3 0h0M7 16h10"],
  bluetooth: ["M12 2v20l7-6-14-8m0 8L19 8l-7-6"],
  info: ["M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M12 11v6M12 7h.01"],
  check: ["m5 12 4.5 4.5L20 6"],
  alert: ["m12 3 10 18H2ZM12 9v5M12 17h.01"],
  close: ["m6 6 12 12M18 6 6 18"],
  minus: ["M5 12h14"], plus: ["M5 12h14M12 5v14"],
  maximize: ["M5 5h14v14H5z"], restore: ["M8 8V4h12v12h-4M4 8h12v12H4z"],
  up: ["m5 12 7-7 7 7M12 5v15"], down: ["m5 12 7 7 7-7M12 4v15"],
  left: ["m12 5-7 7 7 7M5 12h15"], right: ["m12 5 7 7-7 7M4 12h15"],
  chevron: ["m9 5 7 7-7 7"], chevronDown: ["m5 9 7 7 7-7"],
  back: ["m9 4-7 7 7 7M3 11h12a6 6 0 0 1 6 6v3"],
  menu: ["M4 6h16M4 12h16M4 18h16"],
  volume_up: ["M3 9h4l5-5v16l-5-5H3zM16 8a6 6 0 0 1 0 8M19 5a10 10 0 0 1 0 14"],
  volume_down: ["M3 9h4l5-5v16l-5-5H3zM16 8a6 6 0 0 1 0 8"],
  tv: ["M3 6h18v14H3zM8 2l4 4 4-4M8 23h8"],
  clock: ["M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M12 6v6l4 3"],
  globe: ["M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0M2 12h20M12 2c-6 5-6 15 0 20 6-5 6-15 0-20"],
  palette: ["M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 1-3.5 2 2 0 0 1 1-3.5h3a3 3 0 0 0 3-3 9 9 0 0 0-9-8ZM7 10h.01M9 6h.01M14 6h.01M17 9h.01"],
  update: ["M20 9a8 8 0 0 0-14-4L3 8m0-5v5h5M4 15a8 8 0 0 0 14 4l3-3m0 5v-5h-5"],
  save: ["M4 3h13l4 4v14H3V3h1ZM7 3v6h9V3M7 21v-8h10v8"],
  download: ["M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"],
  export: ["M12 16V3m-5 5 5-5 5 5M4 13v8h16v-8"],
  copy: ["M8 8h13v13H8zM16 8V3H3v13h5"],
  trash: ["M3 6h18M5 6l1 15h12l1-15M9 6V3h6v3M10 10v7M14 10v7"],
  folder: ["M3 5h7l2 3h9v13H3z"],
  external: ["M14 3h7v7M21 3 11 13M10 4H3v17h17v-7"],
  log: ["M5 2h10l4 4v16H5zM9 10h6M9 14h6M9 18h4"],
  tray: ["M3 4h18v15H3zM3 14h6l1 3h4l1-3h6M9 8l3 3 3-3M12 5v6"],
  search: ["M17 10a7 7 0 1 1-14 0 7 7 0 0 1 14 0m-2 5 6 6"],
  heart: ["M12 21 3.5 12.5a5.5 5.5 0 0 1 8-7.5l.5.5.5-.5a5.5 5.5 0 0 1 8 7.5Z"],
};
const paths = computed(() => drawings[props.name === "ok" ? "check" : props.name === "injection" ? "keyboard" : props.name] || drawings.info);
</script>

<template>
  <svg class="app-icon" :data-icon="name" :width="size" :height="size" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.85" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
    <path v-for="(path, index) in paths" :key="index" :d="path" />
  </svg>
</template>

<style scoped>
.app-icon { display: inline-block; flex: 0 0 auto; vertical-align: middle; }
</style>
