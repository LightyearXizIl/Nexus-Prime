<script setup lang="ts">
import { computed, onBeforeUnmount, ref, shallowRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "vue-i18n";
import AppIcon from "./AppIcon.vue";
import VoiceCueWaveform from "./VoiceCueWaveform.vue";
import {
  voiceFeedbackPresets,
  type ImportedVoiceCue,
  type VoiceCue,
  type VoiceCueSegment,
  type VoiceCueSong,
  type VoiceFeedbackPreset,
  type VoiceFeedbackSettings,
  type VoiceFeedbackSource,
} from "../types";
import {
  decodeVoiceSource, encodeVoiceFeedback, formatVoiceTime, initialVoiceRange, MAX_CUE_MS, MIN_CUE_MS, parseVoiceTime,
} from "../utils/voiceFeedbackAudio";

type CueRange = Pick<VoiceCueSegment, "start_ms" | "duration_ms">;
const MAX_VOLUME_PERCENT = 150;

const props = defineProps<{ modelValue?: VoiceFeedbackSettings; saving?: boolean }>();
const emit = defineEmits<{ "update:modelValue": [settings: VoiceFeedbackSettings] }>();
const { t } = useI18n();
const settings = computed<VoiceFeedbackSettings>(() => props.modelValue ?? { enabled: false, preset: "guitar" });
const volume = ref(100);
const mode = ref<VoiceFeedbackSource>(settings.value.source_mode ?? (settings.value.preset === "custom" ? "custom" : "preset"));
const preset = computed(() => settings.value.preset === "custom" ? "soft" : settings.value.preset);
const cues = ["start", "end"] as const;
const modes = ["preset", "custom"] as const;
const songs = computed(() => settings.value.songs?.length
  ? settings.value.songs
  : settings.value.song ? [settings.value.song] : []);
const song = computed(() => songs.value.find(item => item.source_id === settings.value.active_song_id) ?? songs.value[0]);
const busy = ref(false);
const loading = ref(false);
const error = ref("");
const deleteDialog = ref<HTMLDialogElement | null>(null);
const pendingDeletion = shallowRef<VoiceCueSong>();
const decoded = shallowRef<{ id: string; audio: AudioBuffer }>();
const songAudio = computed(() => decoded.value?.id === song.value?.source_id ? decoded.value?.audio : undefined);
const custom = computed(() => mode.value === "custom");
const editingSong = computed(() => custom.value ? song.value : undefined);
const decodedPreset = shallowRef<{ preset: VoiceFeedbackPreset; start: AudioBuffer; end: AudioBuffer }>();
const cards = computed(() => cues.map(cue => {
  const audio = custom.value ? songAudio.value : decodedPreset.value?.preset === preset.value ? decodedPreset.value?.[cue] : undefined;
  const totalMs = editingSong.value?.source_ms ?? Math.round((audio?.duration ?? 0) * 1000);
  const segment = editingSong.value?.[cue] ?? { id: "", start_ms: 0, duration_ms: totalMs };
  return { cue, audio, totalMs, segment };
}));
let disposed = false;

watch(() => settings.value.volume_percent, value => {
  volume.value = Math.max(0, Math.min(MAX_VOLUME_PERCENT, value ?? 100));
}, { immediate: true });

function update(changes: Partial<VoiceFeedbackSettings>) {
  if (!disposed) {
    emit("update:modelValue", {
      ...settings.value,
      songs: songs.value,
      active_song_id: song.value?.source_id,
      song: null,
      ...changes,
    });
  }
}

async function readAudio(id: string) {
  if (decoded.value?.id === id) return decoded.value.audio;
  return decodeVoiceSource(await invoke<ArrayBuffer>("read_voice_feedback_source", { id }));
}

watch(() => mode.value === "custom" ? song.value?.source_id : undefined, async (id, _, onCleanup) => {
  loading.value = false;
  if (!id || decoded.value?.id === id) return;
  let stale = false;
  onCleanup(() => { stale = true; });
  loading.value = true;
  try {
    const audio = await readAudio(id);
    if (!stale) decoded.value = { id, audio };
  } catch {
    if (!stale) error.value = t("voiceFeedback.readFailed");
  } finally {
    if (!stale) loading.value = false;
  }
}, { immediate: true });

watch(() => custom.value ? undefined : preset.value, async (selection, _, onCleanup) => {
  if (!selection || decodedPreset.value?.preset === selection) return;
  let stale = false;
  onCleanup(() => { stale = true; });
  const read = async (cue: VoiceCue) => decodeVoiceSource(await invoke<ArrayBuffer>("read_voice_feedback_preset", { preset: selection, cue }));
  try {
    const [start, end] = await Promise.all([read("start"), read("end")]);
    if (!stale) decodedPreset.value = { preset: selection, start, end };
  } catch {
    if (!stale) error.value = t("voiceFeedback.presetWaveFailed");
  }
}, { immediate: true });

function legacySource(cue: ImportedVoiceCue | null | undefined) {
  return cue?.source_id || cue?.id;
}

async function renderClip(audio: AudioBuffer, range: CueRange): Promise<VoiceCueSegment> {
  const bytes = await encodeVoiceFeedback(audio, range.start_ms, range.duration_ms);
  const clip = await invoke<ImportedVoiceCue>("import_voice_feedback", bytes);
  return { id: clip.id, ...range };
}

async function prepareSong(source: { id: string; name: string }) {
  const audio = await readAudio(source.id);
  if (disposed) return;
  const sourceMs = Math.floor(audio.length * 1000 / audio.sampleRate);
  async function prepareCue(cue: VoiceCue): Promise<VoiceCueSegment> {
    const old = settings.value[cue];
    const start = old?.start_ms ?? 0;
    if (old && legacySource(old) === source.id && start >= 0
      && old.duration_ms >= MIN_CUE_MS && old.duration_ms <= MAX_CUE_MS && start + old.duration_ms <= sourceMs) {
      return { id: old.id, start_ms: start, duration_ms: old.duration_ms };
    }
    const { startMs, durationMs } = initialVoiceRange(audio, cue);
    return renderClip(audio, { start_ms: startMs, duration_ms: durationMs });
  }
  const start = await prepareCue("start");
  const end = await prepareCue("end");

  if (disposed) return;
  decoded.value = { id: source.id, audio };
  const entry: VoiceCueSong = { source_id: source.id, name: source.name, source_ms: sourceMs, start, end };
  update({ songs: [...songs.value, entry], active_song_id: entry.source_id, source_mode: "custom", preset: preset.value });
}

async function selectMode(selection: VoiceFeedbackSource) {
  mode.value = selection;
  error.value = "";
  if (selection === "preset") {
    update({ source_mode: "preset", preset: preset.value });
    return;
  }
  if (song.value) {
    if (settings.value.source_mode !== "custom") update({ source_mode: "custom" });
    return;
  }
  const old = settings.value.start ?? settings.value.end;
  const id = legacySource(old);
  if (!old || !id || (settings.value.start && settings.value.end && id !== legacySource(settings.value.end))) return;
  busy.value = true;
  try {
    await prepareSong({ id, name: old.name });
  } catch {
    error.value = t("voiceFeedback.readFailed");
  } finally {
    busy.value = false;
  }
}

function selectAudioSource(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  if (custom.value) {
    selectSong(value);
    return;
  }
  error.value = "";
  update({ source_mode: "preset", preset: value as VoiceFeedbackPreset });
}

function selectSong(id: string) {
  error.value = "";
  update({ active_song_id: id, source_mode: "custom", preset: preset.value });
}

async function importSong() {
  if (busy.value || props.saving) return;
  busy.value = true;
  error.value = "";
  try {
    const source = await invoke<{ id: string; name: string } | null>("pick_voice_feedback_source");
    if (!source || disposed) return;
    if (songs.value.some(item => item.source_id === source.id)) {
      if (source.id === song.value?.source_id && !songAudio.value) {
        const audio = await readAudio(source.id);
        if (disposed) return;
        decoded.value = { id: source.id, audio };
      }
      selectSong(source.id);
    } else {
      await prepareSong(source);
    }
  } catch {
    error.value = t("voiceFeedback.importFailed");
  } finally {
    busy.value = false;
  }
}

function openDeleteDialog() {
  const dialog = deleteDialog.value;
  const current = song.value;
  if (!dialog || dialog.open || !current || busy.value || props.saving) return;
  error.value = "";
  pendingDeletion.value = current;
  dialog.returnValue = "";
  dialog.showModal();
}

function onDeleteDialogClose() {
  const current = pendingDeletion.value;
  pendingDeletion.value = undefined;
  if (deleteDialog.value?.returnValue !== "delete" || !current || disposed || busy.value || props.saving) return;
  const remaining = songs.value.filter(item => item.source_id !== current.source_id);
  const legacyCues = {
    start: legacySource(settings.value.start) === current.source_id ? null : settings.value.start,
    end: legacySource(settings.value.end) === current.source_id ? null : settings.value.end,
  };
  if (decoded.value?.id === current.source_id) decoded.value = undefined;
  if (!remaining.length) mode.value = "preset";
  update({
    ...legacyCues,
    songs: remaining,
    active_song_id: remaining[0]?.source_id ?? null,
    source_mode: remaining.length ? "custom" : "preset",
    preset: preset.value,
  });
}

function resetRange(cue: VoiceCue) {
  if (!songAudio.value) return;
  const { startMs, durationMs } = initialVoiceRange(songAudio.value, cue);
  void saveRange(cue, { start_ms: startMs, duration_ms: durationMs });
}

async function saveRange(cue: VoiceCue, range: CueRange) {
  const currentSong = song.value;
  const current = currentSong?.[cue];
  const audio = songAudio.value;
  if (busy.value || !currentSong || !current || !audio
    || (current.start_ms === range.start_ms && current.duration_ms === range.duration_ms)) return;
  busy.value = true;
  error.value = "";
  try {
    const clip = await renderClip(audio, range);
    update({ songs: songs.value.map(item =>
      item.source_id === currentSong.source_id ? { ...item, [cue]: clip } : item) });
  } catch {
    error.value = t("voiceFeedback.rangeFailed");
  } finally {
    busy.value = false;
  }
}

function timeAt(range: CueRange, edge: VoiceCue) {
  return edge === "start" ? range.start_ms : range.start_ms + range.duration_ms;
}

async function changeTime(event: Event, cue: VoiceCue, edge: VoiceCue) {
  if (!song.value) return;
  const sourceId = song.value.source_id;
  const input = event.target as HTMLInputElement;
  const current = song.value[cue];
  const total = song.value.source_ms;
  const previous = timeAt(current, edge);
  const entered = parseVoiceTime(input.value) ?? previous;
  let start = current.start_ms;
  let stop = timeAt(current, "end");
  if (edge === "start") {
    start = Math.min(Math.max(0, entered), total - MIN_CUE_MS);
    if (stop - start < MIN_CUE_MS || stop - start > MAX_CUE_MS) stop = Math.min(total, start + current.duration_ms);
  } else {
    stop = Math.min(Math.max(entered, start + MIN_CUE_MS), total, start + MAX_CUE_MS);
  }
  await saveRange(cue, { start_ms: start, duration_ms: stop - start });
  if (song.value?.source_id === sourceId) input.value = formatVoiceTime(timeAt(song.value[cue], edge));
}

function durationLabel(ms: number) {
  if (!ms) return "—";
  return ms < 1000
    ? t("voiceFeedback.durationMs", { length: ms })
    : t("voiceFeedback.durationSeconds", { length: ms / 1000 });
}

async function preview(cue: VoiceCue) {
  error.value = "";
  try {
    const playback = { ...settings.value, source_mode: mode.value, preset: preset.value, volume_percent: volume.value };
    await invoke("preview_voice_feedback", { settings: playback, cue });
  } catch {
    error.value = t("voiceFeedback.previewFailed");
  }
}

onBeforeUnmount(() => {
  disposed = true;
  deleteDialog.value?.close();
});
</script>

<template>
  <fieldset class="cue-settings" :aria-busy="busy || loading">
    <div class="cue-row">
      <label for="voice-feedback-enabled">{{ t("voiceFeedback.enabled") }}</label>
      <span class="cue-toggle">
        <input id="voice-feedback-enabled" type="checkbox" role="switch" :checked="settings.enabled" :disabled="busy"
          @change="update({ enabled: ($event.target as HTMLInputElement).checked })" />
        <span aria-hidden="true"></span>
      </span>
    </div>

    <div class="cue-row cue-volume">
      <label for="voice-feedback-volume">{{ t("voiceFeedback.volume") }}</label>
      <div class="cue-volume-controls">
        <input id="voice-feedback-volume" v-model.number="volume" type="range" min="0" :max="MAX_VOLUME_PERCENT" step="1"
          :disabled="busy" :aria-valuetext="volume + '%'" :style="{ '--cue-volume-fill': volume / MAX_VOLUME_PERCENT * 100 + '%' }"
          @change="update({ volume_percent: Number(($event.target as HTMLInputElement).value) })" />
        <span>{{ volume }}%</span>
      </div>
    </div>

    <section class="cue-source" aria-labelledby="voice-feedback-source">
      <header class="cue-line">
        <h3 id="voice-feedback-source">{{ t("voiceFeedback.sourceHeading") }}</h3>
        <div class="cue-choices" role="group" :aria-label="t('voiceFeedback.sourceHeading')">
          <button v-for="choice in modes" :key="choice" class="cue-choice" type="button"
            :disabled="busy" :aria-pressed="mode === choice" @click="selectMode(choice)">
            {{ t(choice === "preset" ? "voiceFeedback.presetMode" : "voiceFeedback.custom") }}
          </button>
        </div>
      </header>

      <div class="cue-source-line cue-line">
        <div class="cue-song-picker">
          <label for="voice-feedback-selection">{{ t(custom ? "voiceFeedback.savedSongs" : "voiceFeedback.preset") }}</label>
          <select id="voice-feedback-selection" class="settings-select settings-select--below"
            :value="custom ? (song?.source_id ?? '') : preset" :title="custom ? song?.name : undefined"
            :disabled="busy || (custom && !song)" @change="selectAudioSource">
            <template v-if="custom">
              <option v-if="!song" value="">{{ t("voiceFeedback.noSongs") }}</option>
              <option v-for="item in songs" :key="item.source_id" :value="item.source_id">{{ item.name }}</option>
            </template>
            <template v-else>
              <option v-for="item in voiceFeedbackPresets" :key="item" :value="item">{{ t("voiceFeedback.presets." + item) }}</option>
            </template>
          </select>
        </div>
        <div v-if="custom" class="cue-inline cue-source-actions">
          <span v-if="editingSong" class="cue-length">{{ t("voiceFeedback.totalTime", { time: formatVoiceTime(editingSong.source_ms) }) }}</span>
          <button class="cue-action" type="button" :disabled="busy || saving" @click="importSong">{{ t("voiceFeedback.importSong") }}</button>
          <button v-if="song" class="cue-action cue-delete" type="button" :disabled="busy || saving"
            aria-haspopup="dialog" aria-controls="delete-song-dialog" @click="openDeleteDialog">
            <AppIcon name="trash" :size="14" />
            {{ t("voiceFeedback.deleteSong") }}
          </button>
        </div>
      </div>

      <section v-for="card in cards" :key="card.cue" class="cue-editor" :aria-labelledby="'voice-editor-' + card.cue">
        <div class="cue-line">
          <h4 :id="'voice-editor-' + card.cue">{{ t("voiceFeedback." + card.cue) }}</h4>
          <div class="cue-inline">
            <button v-if="editingSong" class="cue-action" type="button" :disabled="busy || !songAudio"
              :aria-label="t('voiceFeedback.resetCue', { cue: t('voiceFeedback.' + card.cue) })" @click="resetRange(card.cue)">
              <AppIcon name="update" :size="13" />
              {{ t("voiceFeedback.reset") }}
            </button>
            <button class="cue-action" type="button" :disabled="custom && !editingSong"
              :aria-label="t('voiceFeedback.preview_' + card.cue)" @click="preview(card.cue)">
              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 1.6a.7.7 0 0 0-1 .6v7.6a.7.7 0 0 0 1 .6l6.5-3.8a.7.7 0 0 0 0-1.2Z" fill="currentColor"/></svg>
              {{ t("voiceFeedback.preview") }}
            </button>
          </div>
        </div>
        <VoiceCueWaveform :key="editingSong?.source_id ?? mode" :audio="card.audio" :total-ms="card.totalMs" :segment="card.segment"
          :read-only="!editingSong" :disabled="busy || !songAudio" @commit="saveRange(card.cue, $event)" />
        <div class="cue-line cue-time-row">
          <div v-if="editingSong" class="cue-inline">
            <label v-for="edge in cues" :key="edge" class="cue-time">
              {{ t("voiceFeedback.range_" + edge) }}
              <input type="text" :value="formatVoiceTime(timeAt(card.segment, edge))" :readonly="busy" :disabled="!songAudio"
                :aria-label="t('voiceFeedback.rangeNumber', { cue: t('voiceFeedback.' + card.cue), edge: t('voiceFeedback.range_' + edge) })"
                @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
                @change="changeTime($event, card.cue, edge)" />
            </label>
          </div>
          <span class="cue-length">{{ t("voiceFeedback.clipLength", { duration: durationLabel(card.segment.duration_ms) }) }}</span>
        </div>
      </section>

      <p v-if="busy || loading" class="cue-hint" role="status">{{ t("voiceFeedback.importing") }}</p>
      <p v-else-if="custom" class="cue-hint">
        {{ t(editingSong ? "voiceFeedback.dragHint" : settings.start || settings.end ? "voiceFeedback.legacySong" : "voiceFeedback.importHint") }}
      </p>
      <p v-if="error" class="cue-error" role="alert">{{ error }}</p>
    </section>
  </fieldset>

  <Teleport to="body">
    <dialog id="delete-song-dialog" ref="deleteDialog" class="cue-delete-dialog" role="alertdialog" aria-modal="true"
      aria-labelledby="delete-song-title" aria-describedby="delete-song-message" @close="onDeleteDialogClose">
      <div class="cue-delete-heading">
        <span class="cue-delete-icon"><AppIcon name="trash" :size="18" /></span>
        <h2 id="delete-song-title">{{ t("voiceFeedback.deleteSong") }}</h2>
      </div>
      <p id="delete-song-message">{{ t("voiceFeedback.deleteConfirm", { name: pendingDeletion?.name ?? "" }) }}</p>
      <form method="dialog" class="cue-inline cue-delete-actions">
        <button class="cue-action" type="submit" value="cancel" autofocus>{{ t("common.cancel") }}</button>
        <button class="cue-action cue-delete-confirm" type="submit" value="delete">{{ t("voiceFeedback.deleteSong") }}</button>
      </form>
    </dialog>
  </Teleport>
</template>

<style scoped>
.cue-settings { display: grid; gap: 14px; min-width: 0; padding: 0; border: 0; }
.cue-row, .cue-source { border: 1px solid var(--border); border-radius: 9px; background: var(--surface-soft); }
.cue-row, .cue-line { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 10px; }
.cue-row { min-height: 44px; padding: 8px 12px; font-size: 13px; font-weight: 600; }
.cue-toggle { position: relative; display: inline-block; width: 42px; height: 23px; }
.cue-toggle input { position: absolute; inset: 0; width: 100%; height: 100%; opacity: 0; cursor: pointer; }
.cue-toggle span { display: grid; align-items: center; width: 100%; height: 100%; padding: 0 3px; border-radius: 999px; background: var(--border-strong); pointer-events: none; }
.cue-toggle span::before { width: 17px; height: 17px; border-radius: 50%; background: var(--control-thumb); content: ""; }
.cue-toggle input:checked + span { background: var(--primary); justify-items: end; }
.cue-toggle input:focus-visible + span { outline: 2px solid var(--primary); outline-offset: 2px; }
.cue-volume-controls { display: flex; flex: 0 1 230px; align-items: center; gap: 10px; min-width: 0; }
.cue-volume-controls span { flex: 0 0 40px; font-size: 12px; font-variant-numeric: tabular-nums; text-align: end; }
.cue-volume-controls input { flex: 1; min-width: 0; width: 100%; height: 22px; appearance: none; background: transparent; cursor: pointer; }
.cue-volume-controls input::-webkit-slider-runnable-track { height: 4px; border-radius: 999px; background: linear-gradient(to right, var(--primary) var(--cue-volume-fill), var(--border-strong) var(--cue-volume-fill)); }
.cue-volume-controls input::-webkit-slider-thumb { width: 14px; height: 14px; margin-top: -5px; appearance: none; border: 1px solid var(--primary); border-radius: 50%; background: var(--control-thumb); }
.cue-volume-controls input:focus-visible { border-radius: 4px; outline: 2px solid var(--primary); outline-offset: 2px; }
.cue-volume-controls input:disabled { opacity: .55; cursor: default; }
.cue-source { display: grid; gap: 12px; min-width: 0; padding: 13px; }
.cue-source h3 { margin: 0; font-size: 14px; font-weight: 600; }
.cue-choices, .cue-inline, .cue-time { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; min-width: 0; }
.cue-choices { gap: 3px; padding: 3px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-soft); }
.cue-choice { min-height: 26px; padding: 3px 13px; border: 1px solid transparent; border-radius: 5px; background: transparent; color: var(--text-secondary); font-size: 12px; }
.cue-choice[aria-pressed="true"] { border-color: var(--border); background: var(--surface-selected); color: var(--text); font-weight: 600; }
.cue-action, .cue-time input { min-height: 30px; padding: 4px 10px; border: 1px solid var(--border); border-radius: 7px; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 12px; }
.cue-action { display: inline-flex; align-items: center; justify-content: center; gap: 5px; }
.cue-action:disabled { opacity: .55; cursor: default; }
.cue-delete { color: var(--danger-text); }
.cue-delete:hover:not(:disabled) { border-color: var(--danger); background: var(--danger-bg); }
.cue-action, .cue-choice { cursor: pointer; }
.cue-action:hover:not(:disabled), .cue-choice:hover:not(:disabled) { background: var(--surface-hover); }
.cue-source-line { padding-top: 11px; border-top: 1px solid var(--border); font-size: 12px; }
.cue-source-actions .cue-action { min-height: 34px; }
.cue-song-picker { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; flex: 1 1 200px; max-width: 350px; gap: 10px; min-width: 0; font-size: 12px; }
.cue-song-picker select { width: 100%; text-overflow: ellipsis; }
.cue-editor { display: grid; gap: 8px; min-width: 0; padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--card-bg); }
.cue-editor h4 { margin: 0; font-size: 13px; font-weight: 600; }
.cue-time-row { min-height: 30px; justify-content: flex-end; color: var(--text-secondary); font-size: 12px; }
.cue-time-row > .cue-inline { margin-inline-end: auto; }
.cue-time { gap: 5px; }
.cue-time input { width: 88px; font-variant-numeric: tabular-nums; }
.cue-length { color: var(--text-secondary); font-variant-numeric: tabular-nums; }
.cue-hint, .cue-error { margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
.cue-error { color: var(--danger); }
.cue-delete-dialog { width: min(420px, calc(100vw - 40px)); max-height: calc(100dvh - 40px); margin: auto; padding: 20px; overflow: auto; border: 1px solid var(--border); border-radius: 14px; color: var(--text); background: var(--card-bg); box-shadow: var(--dialog-shadow); }
.cue-delete-dialog::backdrop { background: var(--overlay); }
.cue-delete-heading { display: flex; align-items: center; gap: 10px; }
.cue-delete-heading h2 { margin: 0; font-size: 16px; font-weight: 700; }
.cue-delete-icon { display: grid; width: 34px; height: 34px; place-items: center; border-radius: 8px; color: var(--danger-text); background: var(--danger-bg); }
.cue-delete-dialog p { margin: 12px 0 18px; color: var(--text-secondary); font-size: 13px; line-height: 1.6; overflow-wrap: anywhere; }
.cue-delete-actions { justify-content: flex-end; }
.cue-delete-actions .cue-action { min-height: 34px; padding-inline: 13px; }
.cue-delete-actions .cue-action:focus-visible { outline: 2px solid var(--primary); outline-offset: 2px; }
.cue-delete-actions .cue-delete-confirm { border-color: var(--danger-border); color: var(--danger-text); background: var(--danger-bg); }
.cue-delete-actions .cue-delete-confirm:hover { border-color: var(--danger); }
</style>
