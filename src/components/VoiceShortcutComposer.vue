<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import {
  SHORTCUT_SYMBOL_OPTIONS,
  composeShortcutVks,
  keyLabel,
  parseShortcutText,
  splitShortcutVks,
  type ModifierSide,
  type ShortcutKey,
} from "../utils/shortcut";

const props = defineProps<{
  initialKeys: number[];
  buttonLabel: string;
  slotLabel: string;
}>();

const emit = defineEmits<{
  apply: [keys: number[]];
  cancel: [];
}>();

const ctrl = ref<ModifierSide>("none");
const shift = ref<ModifierSide>("none");
const alt = ref<ModifierSide>("none");
const win = ref<ModifierSide>("none");
const shortcutText = ref("");

type KeyOption = { value: ShortcutKey; label: string };
type KeyGroup = { label: string; options: KeyOption[] };
type KeyRow = { id: number; value: ShortcutKey | null };

const keyRows = ref<KeyRow[]>([]);
const keyFields = ref<HTMLFieldSetElement | null>(null);
let nextRowId = 0;

const keyGroups: KeyGroup[] = [
  {
    label: "字母",
    options: Array.from({ length: 26 }, (_, index) => ({ value: 0x41 + index, label: String.fromCharCode(0x41 + index) })),
  },
  {
    label: "数字",
    options: Array.from({ length: 10 }, (_, index) => ({ value: 0x30 + index, label: String(index) })),
  },
  {
    label: "符号",
    options: SHORTCUT_SYMBOL_OPTIONS,
  },
  {
    label: "功能键",
    options: Array.from({ length: 24 }, (_, index) => ({ value: 0x70 + index, label: `F${index + 1}` })),
  },
  {
    label: "编辑与导航",
    options: [0x08, 0x09, 0x0d, 0x1b, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2d, 0x2e].map((value) => ({ value, label: keyLabel(value) })),
  },
  {
    label: "媒体与系统",
    options: [0xad, 0xae, 0xaf, 0xb0, 0xb1, 0xb2, 0xb3, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xb4, 0xb5, 0xb6, 0xb7].map((value) => ({ value, label: keyLabel(value) })),
  },
];
const knownKeys = new Set(keyGroups.flatMap((group) => group.options.map((option) => option.value)));

function keyOptions(value: ShortcutKey | null): KeyGroup[] {
  if (typeof value === "number" && !knownKeys.has(value)) {
    return [{ label: "已保存的键值", options: [{ value, label: keyLabel(value) }] }, ...keyGroups];
  }
  return keyGroups;
}

const composedShortcut = computed(() => {
  return composeShortcutVks(
    { ctrl: ctrl.value, shift: shift.value, alt: alt.value, win: win.value },
    keyRows.value.map((row) => row.value).filter((value): value is ShortcutKey => value !== null),
  );
});

const usesText = computed(() => Boolean(shortcutText.value.trim()));
const activeShortcut = computed(() => usesText.value ? parseShortcutText(shortcutText.value) : composedShortcut.value);
const effectiveKeys = computed(() => activeShortcut.value.keys ?? []);
const preview = computed(() => effectiveKeys.value.map(keyLabel).join(" + ") || "尚未选择按键");
const canAddKey = computed(() => keyRows.value.every((row) => row.value !== null));

async function focusKey(index: number) {
  await nextTick();
  keyFields.value?.querySelectorAll<HTMLSelectElement>(".shortcut-key-row select")[index]?.focus();
}

function addKey() {
  keyRows.value.push({ id: nextRowId++, value: null });
  void focusKey(keyRows.value.length - 1);
}

function removeKey(index: number) {
  keyRows.value.splice(index, 1);
  void focusKey(Math.min(index, keyRows.value.length - 1));
}

function resetFromKeys(keys: number[]) {
  const shortcut = splitShortcutVks(keys);
  ctrl.value = shortcut.modifiers.ctrl;
  shift.value = shortcut.modifiers.shift;
  alt.value = shortcut.modifiers.alt;
  win.value = shortcut.modifiers.win;
  const ordinaryKeys = shortcut.keys.length ? shortcut.keys : [null];
  keyRows.value = ordinaryKeys.map((value) => ({ id: nextRowId++, value }));
  shortcutText.value = "";
}

function useDropdowns() {
  if (!effectiveKeys.value.length) return;
  resetFromKeys(effectiveKeys.value);
  void focusKey(0);
}

function apply() {
  if (!effectiveKeys.value.length || activeShortcut.value.error) return;
  emit("apply", effectiveKeys.value);
}

watch(() => props.initialKeys, resetFromKeys, { immediate: true });
</script>

<template>
  <section class="shortcut-composer" aria-label="手动组合快捷键">
    <div class="shortcut-composer-heading">
      <div>
        <span>手动组合</span>
        <strong>{{ buttonLabel }}键 · {{ slotLabel }}</strong>
      </div>
      <button type="button" class="shortcut-composer-close" aria-label="关闭手动组合" @click="emit('cancel')">×</button>
    </div>

    <p class="shortcut-composer-note">不依赖系统键盘钩子。可直接输入 <code>Alt+F4</code>，或从下方选择按键。</p>

    <label class="shortcut-text-input">
      <span>快捷键文本</span>
      <input
        v-model="shortcutText"
        name="shortcut-text"
        autocomplete="off"
        spellcheck="false"
        placeholder="例如 Alt+F4"
        :aria-invalid="usesText && Boolean(activeShortcut.error)"
        aria-describedby="shortcut-text-help"
      />
    </label>
    <div class="shortcut-text-help-row">
      <p id="shortcut-text-help" :class="['shortcut-text-help', { error: usesText && activeShortcut.error }]">
        {{ usesText && activeShortcut.error ? activeShortcut.error : "支持符号，例如 Ctrl + ?。加号请写 Plus。Shift 对整组按键生效。" }}
      </p>
      <button v-if="usesText" type="button" class="selection-action" :disabled="!effectiveKeys.length" @click="useDropdowns">改用下拉</button>
    </div>

    <fieldset ref="keyFields" class="shortcut-composer-fields" :disabled="usesText" aria-label="下拉选择按键">
      <label><span>Ctrl</span><select v-model="ctrl"><option value="none">不使用</option><option value="generic">任意 Ctrl</option><option value="left">左 Ctrl</option><option value="right">右 Ctrl</option></select></label>
      <label><span>Shift</span><select v-model="shift"><option value="none">不使用</option><option value="generic">任意 Shift</option><option value="left">左 Shift</option><option value="right">右 Shift</option></select></label>
      <label><span>Alt</span><select v-model="alt"><option value="none">不使用</option><option value="generic">任意 Alt</option><option value="left">左 Alt</option><option value="right">右 Alt</option></select></label>
      <label><span>Win</span><select v-model="win"><option value="none">不使用</option><option value="left">左 Win</option><option value="right">右 Win</option></select></label>
      <div v-for="(row, index) in keyRows" :key="row.id" class="shortcut-key-row">
        <label>
          <span>{{ index === 0 ? "主键" : `附加键 ${index}` }}</span>
          <select v-model="row.value">
            <option :value="null">{{ index === 0 ? "不使用" : "请选择" }}</option>
            <optgroup v-for="group in keyOptions(row.value)" :key="group.label" :label="group.label">
              <option v-for="option in group.options" :key="option.value" :value="option.value">{{ option.label }}</option>
            </optgroup>
          </select>
        </label>
        <button v-if="index > 0" type="button" class="selection-action" :aria-label="`移除附加键 ${index}`" @click="removeKey(index)">移除</button>
      </div>
      <button type="button" class="selection-action shortcut-add-key" :disabled="!canAddKey" @click="addKey">添加按键</button>
    </fieldset>

    <div class="shortcut-composer-preview">
      <span>组合预览</span>
      <strong :class="{ error: !usesText && activeShortcut.error }" aria-live="polite">{{ !usesText && activeShortcut.error ? activeShortcut.error : preview }}</strong>
    </div>

    <div class="shortcut-composer-actions">
      <button type="button" class="selection-action" @click="emit('cancel')">取消</button>
      <button type="button" class="selection-action primary" :disabled="!effectiveKeys.length || Boolean(activeShortcut.error)" @click="apply">应用组合</button>
    </div>
  </section>
</template>

<style scoped>
.shortcut-composer { margin: 0 0 14px; padding: 14px; border: 1px solid var(--info-border); border-radius: 12px; background: var(--surface-selected); }
.shortcut-composer-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
.shortcut-composer-heading span, .shortcut-composer-preview > span { display: block; color: var(--text-secondary); font-size: 11px; }
.shortcut-composer-heading strong { display: block; margin-top: 3px; color: var(--text); font-size: 13px; }
.shortcut-composer-close { width: 28px; height: 28px; border: 1px solid var(--border-strong); border-radius: 8px; color: var(--text-secondary); background: var(--surface-raised); font: inherit; font-size: 18px; line-height: 1; cursor: pointer; }
.shortcut-composer-close:hover { color: var(--text); background: var(--surface-hover); }
.shortcut-composer-note { margin: 10px 0 12px; color: var(--text-secondary); font-size: 11px; line-height: 1.5; }
.shortcut-text-input { display: grid; gap: 5px; margin-bottom: 4px; color: var(--text-secondary); font-size: 10px; font-weight: 700; }
.shortcut-text-input input { width: 100%; height: 34px; box-sizing: border-box; padding: 0 9px; border: 1px solid var(--border-strong); border-radius: 7px; outline: 0; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 12px; }
.shortcut-text-input input:focus-visible { border-color: var(--primary); box-shadow: 0 0 0 3px var(--focus-ring); }
.shortcut-text-input input[aria-invalid="true"] { border-color: var(--danger, #c43b3b); }
.shortcut-text-help-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 10px; }
.shortcut-text-help { margin: 0; color: var(--text-secondary); font-size: 10px; line-height: 1.45; }
.shortcut-text-help.error, .shortcut-composer-preview strong.error { color: var(--danger, #c43b3b); }
.shortcut-composer-fields { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; min-width: 0; margin: 0; padding: 0; border: 0; }
.shortcut-composer-fields label { display: grid; gap: 5px; min-width: 0; color: var(--text-secondary); font-size: 10px; font-weight: 700; }
.shortcut-composer-fields select { width: 100%; min-width: 0; height: 32px; padding: 0 8px; border: 1px solid var(--border-strong); border-radius: 7px; outline: 0; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 11px; }
.shortcut-composer-fields select:focus { border-color: var(--primary); box-shadow: 0 0 0 3px var(--focus-ring); }
.shortcut-key-row { grid-column: 1 / -1; display: flex; align-items: end; gap: 8px; }
.shortcut-key-row label { flex: 0 1 50%; }
.shortcut-add-key { grid-column: 1 / -1; justify-self: start; }
.shortcut-composer-preview { display: grid; gap: 4px; margin-top: 12px; padding: 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); }
.shortcut-composer-preview strong { overflow-wrap: anywhere; color: var(--text); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; line-height: 1.45; }
.shortcut-composer-actions { display: flex; justify-content: flex-end; gap: 7px; margin-top: 12px; }
.selection-action { min-height: 30px; padding: 0 10px; border: 1px solid var(--border-strong); border-radius: 7px; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 11px; font-weight: 700; cursor: pointer; }
.selection-action:hover:not(:disabled) { background: var(--surface-hover); }
.selection-action.primary { border-color: var(--primary); color: #fff; background: var(--primary); }
.selection-action.primary:hover:not(:disabled) { background: var(--primary-dark); }
.selection-action:disabled { opacity: .55; cursor: not-allowed; }
@media (max-width: 760px) { .shortcut-composer-fields { grid-template-columns: repeat(2, minmax(0, 1fr)); } .shortcut-key-row label { flex: 1; } }
</style>
