<script setup lang="ts">
import { ref } from "vue";
import remoteImage from "../assets/xiaomi-remote-cutout.png";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

defineProps<{
  selectedId?: string | null;
  hoverId?: string | null;
}>();

const emit = defineEmits<{
  select: [buttonId: string];
  hover: [buttonId: string | null];
}>();

const rootRef = ref<HTMLElement | null>(null);

/**
 * Coordinates are normalized from the physical button bounds in the
 * 401 x 1919 product cutout. Keeping the source-pixel measurements here
 * prevents a future asset crop from silently shifting the hit targets.
 */
const HOTSPOTS = [
  { id: "power", x: 24.56, y: 6.54, w: 22.44, h: 4.79, shape: "circle" },
  { id: "mic", x: 75.19, y: 6.51, w: 22.44, h: 4.85, shape: "circle" },
  { id: "up", x: 50, y: 13, w: 43.39, h: 4.06, shape: "dpad dpad-up" },
  { id: "left", x: 18.7, y: 19.67, w: 18.95, h: 9.07, shape: "dpad dpad-left" },
  { id: "right", x: 81.3, y: 19.67, w: 18.95, h: 9.07, shape: "dpad dpad-right" },
  { id: "down", x: 50, y: 26.39, w: 43.39, h: 4.06, shape: "dpad dpad-down" },
  { id: "ok", x: 50, y: 19.67, w: 42.89, h: 9.07, shape: "circle dpad-ok" },
  { id: "back", x: 29.3, y: 32.78, w: 32.42, h: 7.03, shape: "circle" },
  { id: "volume_up", x: 70.32, y: 32.78, w: 32.42, h: 7.03, shape: "pill" },
  { id: "home", x: 29.3, y: 40.78, w: 32.42, h: 6.98, shape: "circle" },
  { id: "volume_down", x: 70.32, y: 40.78, w: 32.42, h: 6.98, shape: "pill" },
  { id: "menu", x: 29.3, y: 48.75, w: 32.42, h: 6.98, shape: "circle" },
  { id: "tv", x: 70.32, y: 48.75, w: 32.42, h: 6.98, shape: "circle" },
] as const;

function keyEl(id: string): HTMLElement | null {
  return rootRef.value?.querySelector(`[data-key-id="${id}"]`) as HTMLElement | null;
}

defineExpose({ keyEl, rootRef });
</script>

<template>
  <div ref="rootRef" class="remote-schematic" :aria-label="t('mapping.remotePreview')">
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
      :aria-label="t(`keys.${key.id}`)"
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
  height: 0;
  padding-top: 20.9%; /* aspect-ratio 兜底（Chromium <88）：401/1919 ≈ 20.9% */
  aspect-ratio: 401 / 1919;
  user-select: none;
}

.remote-product-image {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  left: 0;
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
