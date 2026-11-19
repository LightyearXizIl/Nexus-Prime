<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { VoiceCueSegment } from "../types";
import { formatVoiceTime, MAX_CUE_MS, MIN_CUE_MS, voiceWaveform } from "../utils/voiceFeedbackAudio";

type Range = Pick<VoiceCueSegment, "start_ms" | "duration_ms">;
type Boundary = "start" | "end" | "move";

const props = defineProps<{
  audio?: AudioBuffer;
  totalMs: number;
  segment: VoiceCueSegment;
  disabled?: boolean;
  readOnly?: boolean;
}>();
const emit = defineEmits<{ commit: [range: Range] }>();
const { t } = useI18n();
const editor = ref<HTMLElement>();
const draft = ref<Range>();
const overviewDraft = ref<Range>();
let drag: { pointerId: number; boundary: Boundary; anchor: number; original: Range } | undefined;
let overviewPointer: number | undefined;

const viewSize = computed(() => {
  const duration = props.segment.duration_ms;
  const detail = Math.max(duration * 2.4, Math.min(2400, duration * 8));
  return Math.min(props.totalMs, Math.ceil(detail));
});
const viewStart = computed(() => Math.round(Math.max(0, Math.min(
  props.totalMs - viewSize.value,
  (overviewDraft.value?.start_ms ?? props.segment.start_ms) + props.segment.duration_ms / 2 - viewSize.value / 2,
))));
const viewEnd = computed(() => Math.min(props.totalMs, viewStart.value + viewSize.value));
const viewLength = computed(() => viewEnd.value - viewStart.value);
const current = computed(() => draft.value ?? overviewDraft.value ?? props.segment);
const hasOverview = computed(() => !props.readOnly && viewLength.value < props.totalMs);
const end = computed(() => current.value.start_ms + current.value.duration_ms);
const editorBars = computed(() => props.audio ? voiceWaveform(props.audio, viewStart.value, viewEnd.value, 112) : []);
const overviewBars = computed(() => props.audio ? voiceWaveform(props.audio, 0, props.totalMs, 144) : []);
const startPct = computed(() => percentAt(current.value.start_ms));
const endPct = computed(() => percentAt(end.value));

watch(() => [props.segment, props.disabled] as const, ([, disabled]) => {
  if (!disabled && overviewPointer === undefined) overviewDraft.value = undefined;
});

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function percentAt(ms: number) {
  return clamp((ms - viewStart.value) / viewLength.value, 0, 1) * 100;
}

function position(event: PointerEvent, element: HTMLElement, start: number, length: number) {
  const bounds = element.getBoundingClientRect();
  return Math.round(start + clamp((event.clientX - bounds.left) / bounds.width, 0, 1) * length);
}

function adjusted(original: Range, boundary: Boundary, at: number, anchor: number): Range {
  const oldEnd = original.start_ms + original.duration_ms;
  if (boundary === "start") {
    const start = clamp(at, Math.max(viewStart.value, oldEnd - MAX_CUE_MS), oldEnd - MIN_CUE_MS);
    return { start_ms: start, duration_ms: oldEnd - start };
  }
  if (boundary === "end") {
    const stop = clamp(at, original.start_ms + MIN_CUE_MS, Math.min(viewEnd.value, original.start_ms + MAX_CUE_MS));
    return { start_ms: original.start_ms, duration_ms: stop - original.start_ms };
  }
  return {
    start_ms: clamp(original.start_ms + at - anchor, viewStart.value, viewEnd.value - original.duration_ms),
    duration_ms: original.duration_ms,
  };
}

function commit(range: Range) {
  if (range.start_ms !== props.segment.start_ms || range.duration_ms !== props.segment.duration_ms) {
    emit("commit", range);
  }
}

function begin(event: PointerEvent, boundary: Boundary) {
  if (event.button !== 0 || props.disabled || drag || !editor.value) return;
  event.preventDefault();
  if (event.currentTarget instanceof HTMLButtonElement) event.currentTarget.focus({ preventScroll: true });
  const original = { start_ms: props.segment.start_ms, duration_ms: props.segment.duration_ms };
  drag = { pointerId: event.pointerId, boundary, anchor: position(event, editor.value, viewStart.value, viewLength.value), original };
  draft.value = original;
  editor.value.setPointerCapture(event.pointerId);
}

function move(event: PointerEvent) {
  if (!drag || event.pointerId !== drag.pointerId || !editor.value) return;
  const at = position(event, editor.value, viewStart.value, viewLength.value);
  draft.value = adjusted(drag.original, drag.boundary, at, drag.anchor);
}

function finish(event: PointerEvent, cancelled = false) {
  if (!drag || drag.pointerId !== event.pointerId) return;
  const finished = draft.value;
  drag = undefined;
  draft.value = undefined;
  if (editor.value?.hasPointerCapture(event.pointerId)) editor.value.releasePointerCapture(event.pointerId);
  if (!cancelled && finished) commit(finished);
}

function adjustWithKey(event: KeyboardEvent, boundary: "start" | "end") {
  if (props.disabled || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  const origin = boundary === "start" ? props.segment.start_ms : props.segment.start_ms + props.segment.duration_ms;
  const step = event.shiftKey ? 100 : 10;
  const at = event.key === "Home" ? viewStart.value : event.key === "End" ? viewEnd.value
    : origin + (event.key === "ArrowLeft" ? -step : step);
  commit(adjusted(props.segment, boundary, at, origin));
}

function rangeAt(at: number) {
  const duration = props.segment.duration_ms;
  return { start_ms: clamp(Math.round(at - duration / 2), 0, props.totalMs - duration), duration_ms: duration };
}

function moveOverview(event: PointerEvent) {
  if (overviewPointer !== event.pointerId) return;
  overviewDraft.value = rangeAt(position(event, event.currentTarget as HTMLElement, 0, props.totalMs));
}

function beginOverview(event: PointerEvent) {
  if (props.disabled || event.button !== 0 || overviewPointer !== undefined) return;
  const button = event.currentTarget as HTMLButtonElement;
  overviewPointer = event.pointerId;
  button.setPointerCapture(event.pointerId);
  moveOverview(event);
}

function finishOverview(event: PointerEvent, cancelled = false) {
  if (overviewPointer !== event.pointerId) return;
  const range = overviewDraft.value;
  overviewPointer = undefined;
  if (cancelled || !range || range.start_ms === props.segment.start_ms) {
    overviewDraft.value = undefined;
  } else {
    commit(range);
  }
}

function shiftOverview(event: KeyboardEvent) {
  if (props.disabled || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  const middle = props.segment.start_ms + props.segment.duration_ms / 2;
  commit(rangeAt(event.key === "Home" ? 0 : event.key === "End" ? props.totalMs
    : middle + viewLength.value * (event.key === "ArrowLeft" ? -1 : 1)));
}
</script>

<template>
  <div class="voice-wave" :class="{ 'is-dragging': draft, 'has-overview': hasOverview }">
    <button
      v-if="hasOverview"
      class="wave-overview"
      type="button"
      :aria-disabled="disabled"
      :aria-label="t('voiceFeedback.overview')"
      @pointerdown="beginOverview" @pointermove="moveOverview"
      @pointerup="finishOverview($event)" @pointercancel="finishOverview($event, true)"
      @lostpointercapture="finishOverview($event, true)"
      @keydown="shiftOverview"
    >
      <svg viewBox="0 0 720 24" preserveAspectRatio="none" aria-hidden="true">
        <rect v-for="(peak, i) in overviewBars" :key="i" :x="i * 5 + 1.35" :y="12 - Math.max(1, peak * 7)" width="2.3" :height="Math.max(2, peak * 14)" rx="1.15" />
      </svg>
      <span class="wave-window" :style="{ left: `${viewStart / totalMs * 100}%`, width: `${viewLength / totalMs * 100}%` }"></span>
    </button>
    <div class="wave-axis" aria-hidden="true">
      <template v-if="audio"><span>{{ formatVoiceTime(viewStart) }}</span><span>{{ formatVoiceTime(viewEnd) }}</span></template>
    </div>
    <div
      ref="editor" class="wave-editor"
      @pointermove="move" @pointerup="finish($event)" @pointercancel="finish($event, true)"
      @lostpointercapture="finish($event, true)"
    >
      <svg class="wave-bars" viewBox="0 0 672 96" preserveAspectRatio="none" aria-hidden="true">
        <rect
          v-for="(peak, i) in editorBars" :key="i"
          :class="{ inside: readOnly || ((i + .5) / editorBars.length * 100 >= startPct && (i + .5) / editorBars.length * 100 <= endPct) }"
          :x="i * 6 + 1.45" :y="48 - Math.max(1.7, peak * 32)" width="3.1" :height="Math.max(3.4, peak * 64)" rx="1.55" ry="2"
        />
      </svg>
      <template v-if="!readOnly && audio">
      <div class="wave-selection" :style="{ left: `${startPct}%`, width: `${endPct - startPct}%` }" @pointerdown="begin($event, 'move')"></div>
      <button
        v-for="boundary in (['start', 'end'] as const)" :key="boundary"
        type="button" class="wave-handle" role="slider"
        :style="{ left: `${boundary === 'start' ? startPct : endPct}%` }"
        :aria-disabled="disabled"
        :aria-label="t(`voiceFeedback.range_${boundary}`)"
        :aria-valuemin="boundary === 'start' ? Math.max(viewStart, end - MAX_CUE_MS) : current.start_ms + MIN_CUE_MS"
        :aria-valuemax="boundary === 'start' ? end - MIN_CUE_MS : Math.min(viewEnd, current.start_ms + MAX_CUE_MS)"
        :aria-valuenow="boundary === 'start' ? current.start_ms : end"
        :aria-valuetext="formatVoiceTime(boundary === 'start' ? current.start_ms : end)"
        @pointerdown="begin($event, boundary)" @keydown="adjustWithKey($event, boundary)"
      ></button>
      </template>
    </div>
  </div>
</template>

<style scoped>
.voice-wave { --wave-accent: color-mix(in srgb, var(--primary) 68%, var(--text-secondary)); display: grid; gap: 6px; min-width: 0; }
.wave-overview { position: relative; display: block; width: 100%; height: 23px; padding: 0; overflow: hidden; border: 1px solid var(--border); border-radius: 7px; background: var(--surface-soft); cursor: grab; touch-action: none; }
.wave-overview:active { cursor: grabbing; }
.wave-overview svg, .wave-bars { position: absolute; inset: 0; display: block; width: 100%; height: 100%; pointer-events: none; fill: color-mix(in srgb, var(--text-secondary) 48%, transparent); }
.wave-overview svg { opacity: .76; }
.wave-window { position: absolute; top: 2px; bottom: 2px; min-width: 3px; border: 1px solid color-mix(in srgb, var(--wave-accent) 60%, transparent); border-radius: 3px; background: color-mix(in srgb, var(--wave-accent) 15%, transparent); pointer-events: none; }
.wave-axis { display: flex; justify-content: space-between; gap: 8px; min-height: 15px; color: var(--text-secondary); font-size: 11px; font-variant-numeric: tabular-nums; }
.wave-editor { position: relative; height: 106px; overflow: hidden; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-soft); touch-action: none; user-select: none; }
.has-overview .wave-editor { height: 77px; }
.wave-bars rect.inside { fill: var(--wave-accent); }
.wave-selection { position: absolute; top: 6px; bottom: 6px; border: 1px solid color-mix(in srgb, var(--wave-accent) 32%, transparent); border-radius: 6px; background: color-mix(in srgb, var(--wave-accent) 12%, transparent); cursor: grab; }
.is-dragging .wave-selection { cursor: grabbing; }
.wave-handle { position: absolute; top: 0; bottom: 0; width: 14px; margin-left: -7px; padding: 0; border: 0; background: transparent; cursor: ew-resize; }
.wave-handle::before { position: absolute; inset: 6px auto 6px 6px; width: 2px; border-radius: 2px; background: var(--wave-accent); opacity: .78; content: ""; }
.wave-handle::after { position: absolute; top: calc(50% - 7px); left: 4px; width: 6px; height: 14px; border-radius: 4px; background: var(--wave-accent); content: ""; }
.wave-handle:focus-visible { outline-offset: -2px; }
</style>
