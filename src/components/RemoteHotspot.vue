<script setup lang="ts">
import { ref } from "vue";
import remoteImage from "../assets/xiaomi-remote-cutout.png";

defineProps<{
  selectedId?: string | null;
  hoverId?: string | null;
}>();

const emit = defineEmits<{
  select: [buttonId: string];
  hover: [buttonId: string | null];
}>();

const rootRef = ref<HTMLElement | null>(null);

/** Coordinates are normalized to the cropped product photograph. */
const HOTSPOTS = [
  { id: "power", label: "电源键", x: 24.5, y: 5.8, w: 21, h: 4.6, shape: "circle" },
  { id: "mic", label: "语音键", x: 75.5, y: 5.8, w: 21, h: 4.6, shape: "circle" },
  { id: "up", label: "上键", x: 50, y: 11.2, w: 42, h: 6.7, shape: "dpad dpad-up" },
  { id: "left", label: "左键", x: 27.7, y: 18.2, w: 13, h: 15.4, shape: "dpad dpad-left" },
  { id: "right", label: "右键", x: 72.3, y: 18.2, w: 13, h: 15.4, shape: "dpad dpad-right" },
  { id: "down", label: "下键", x: 50, y: 25.2, w: 42, h: 6.7, shape: "dpad dpad-down" },
  { id: "ok", label: "确认键", x: 50, y: 18.2, w: 30, h: 10.8, shape: "circle dpad-ok" },
  { id: "back", label: "返回键", x: 25.5, y: 30.5, w: 28, h: 6.1, shape: "circle" },
  { id: "volume_up", label: "音量加键", x: 70.3, y: 30.5, w: 28, h: 6.1, shape: "pill" },
  { id: "home", label: "主页键", x: 25.5, y: 37.5, w: 28, h: 6.1, shape: "circle" },
  { id: "volume_down", label: "音量减键", x: 70.3, y: 37.5, w: 28, h: 6.1, shape: "pill" },
  { id: "menu", label: "菜单键", x: 25.5, y: 44.5, w: 28, h: 6.1, shape: "circle" },
  { id: "tv", label: "TV 键", x: 70.3, y: 44.5, w: 28, h: 6.1, shape: "circle" },
] as const;

function keyEl(id: string): HTMLElement | null {
  return rootRef.value?.querySelector(`[data-key-id="${id}"]`) as HTMLElement | null;
}

defineExpose({ keyEl, rootRef });
</script>

<template>
  <div ref="rootRef" class="remote-schematic" aria-label="小米遥控器示意图">
    <img class="remote-product-image" :src="remoteImage" alt="" draggable="false" />
    <button
      v-for="key in HOTSPOTS"
      :key="key.id"
      type="button"
      class="remote-hotspot"
      :class="[
        key.shape,
        { active: selectedId === key.id, hover: hoverId === key.id },
      ]"
      :data-key-id="key.id"
      :aria-label="key.label"
      :aria-pressed="selectedId === key.id"
      :style="{
        left: `${key.x}%`,
        top: `${key.y}%`,
        width: `${key.w}%`,
        height: `${key.h}%`,
      }"
      @mouseenter="emit('hover', key.id)"
      @mouseleave="emit('hover', null)"
      @focus="emit('hover', key.id)"
      @blur="emit('hover', null)"
      @click="emit('select', key.id)"
    />
  </div>
</template>

<style scoped>
.remote-schematic {
  position: relative;
  width: 84px;
  flex: 0 0 auto;
  aspect-ratio: 401 / 1919;
  user-select: none;
}

.remote-product-image {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  pointer-events: none;
  filter: drop-shadow(0 10px 11px rgba(15, 23, 42, 0.24));
}

.remote-hotspot {
  position: absolute;
  z-index: 1;
  box-sizing: border-box;
  margin: 0;
  padding: 0;
  border: 1.5px solid transparent;
  border-radius: 10px;
  background: transparent;
  cursor: pointer;
  transform: translate(-50%, -50%);
  transition: border-color 0.14s ease, background 0.14s ease, box-shadow 0.14s ease;
}

.remote-hotspot.circle { border-radius: 50%; }
.remote-hotspot.pill { border-radius: 999px; }
.remote-hotspot.dpad-up { border-radius: 46% 46% 28% 28%; }
.remote-hotspot.dpad-down { border-radius: 28% 28% 46% 46%; }
.remote-hotspot.dpad-left { border-radius: 46% 28% 28% 46%; }
.remote-hotspot.dpad-right { border-radius: 28% 46% 46% 28%; }
.remote-hotspot.dpad-ok { z-index: 2; }

.remote-hotspot:hover,
.remote-hotspot.hover,
.remote-hotspot.active {
  border-color: rgba(58, 132, 246, 0.94);
  background: rgba(58, 132, 246, 0.17);
  box-shadow: 0 0 0 2px rgba(58, 132, 246, 0.22);
}

.remote-hotspot:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
</style>
