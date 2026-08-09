<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  composeShortcutVks,
  keyLabel,
  splitShortcutVks,
  type ModifierSide,
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
const mainKey = ref<number | null>(null);
const preservedExtraKeys = ref<number[]>([]);

type KeyOption = { value: number; label: string };

const keyGroups = computed(() => [
  {
    label: "字母",
    options: Array.from({ length: 26 }, (_, index) => ({ value: 0x41 + index, label: String.fromCharCode(0x41 + index) })),
  },
  {
    label: "数字",
    options: Array.from({ length: 10 }, (_, index) => ({ value: 0x30 + index, label: String(index) })),
  },
  {
    label: "功能键",
    options: Array.from({ length: 12 }, (_, index) => ({ value: 0x70 + index, label: `F${index + 1}` })),
  },
  {
    label: "编辑与导航",
    options: [0x08, 0x09, 0x0d, 0x1b, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2d, 0x2e].map((value) => ({ value, label: keyLabel(value) })),
  },
  {
    label: "媒体与系统",
    options: [0xad, 0xae, 0xaf, 0xb0, 0xb1, 0xb2, 0xb3, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xb4, 0xb5, 0xb6, 0xb7].map((value) => ({ value, label: keyLabel(value) })),
  },
] as Array<{ label: string; options: KeyOption[] }>);

const mainKeyOptions = computed(() => {
  const known = new Set(keyGroups.value.flatMap((group) => group.options.map((option) => option.value)));
  if (mainKey.value != null && !known.has(mainKey.value)) {
    return [{ label: "已保存的未知键值", options: [{ value: mainKey.value, label: keyLabel(mainKey.value) }] }, ...keyGroups.value];
  }
  return keyGroups.value;
});

const composedKeys = computed(() => {
  return composeShortcutVks(
    { ctrl: ctrl.value, shift: shift.value, alt: alt.value, win: win.value },
    mainKey.value,
    preservedExtraKeys.value,
  );
});

const preview = computed(() => composedKeys.value.map(keyLabel).join(" + ") || "尚未选择按键");

function resetFromKeys(keys: number[]) {
  const shortcut = splitShortcutVks(keys);
  ctrl.value = shortcut.modifiers.ctrl;
  shift.value = shortcut.modifiers.shift;
  alt.value = shortcut.modifiers.alt;
  win.value = shortcut.modifiers.win;
  mainKey.value = shortcut.mainKey;
  preservedExtraKeys.value = shortcut.extraKeys;
}

function apply() {
  if (!composedKeys.value.length) return;
  emit("apply", composedKeys.value);
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

    <p class="shortcut-composer-note">不依赖系统键盘钩子。可只选修饰键，或为修饰键加一个主键。</p>

    <div class="shortcut-composer-fields">
      <label><span>Ctrl</span><select v-model="ctrl"><option value="none">不使用</option><option value="generic">任意 Ctrl</option><option value="left">左 Ctrl</option><option value="right">右 Ctrl</option></select></label>
      <label><span>Shift</span><select v-model="shift"><option value="none">不使用</option><option value="generic">任意 Shift</option><option value="left">左 Shift</option><option value="right">右 Shift</option></select></label>
      <label><span>Alt</span><select v-model="alt"><option value="none">不使用</option><option value="generic">任意 Alt</option><option value="left">左 Alt</option><option value="right">右 Alt</option></select></label>
      <label><span>Win</span><select v-model="win"><option value="none">不使用</option><option value="left">左 Win</option><option value="right">右 Win</option></select></label>
      <label class="shortcut-main-key"><span>主键</span><select v-model.number="mainKey"><option :value="null">不使用</option><optgroup v-for="group in mainKeyOptions" :key="group.label" :label="group.label"><option v-for="option in group.options" :key="option.value" :value="option.value">{{ option.label }}</option></optgroup></select></label>
    </div>

    <p v-if="preservedExtraKeys.length" class="shortcut-composer-preserved">已保留的额外键值：{{ preservedExtraKeys.map(keyLabel).join(' + ') }}</p>

    <div class="shortcut-composer-preview">
      <span>组合预览</span>
      <strong>{{ preview }}</strong>
    </div>

    <div class="shortcut-composer-actions">
      <button type="button" class="selection-action" @click="emit('cancel')">取消</button>
      <button type="button" class="selection-action primary" :disabled="!composedKeys.length" @click="apply">应用组合</button>
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
.shortcut-composer-fields { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 8px; }
.shortcut-composer-fields label { display: grid; gap: 5px; min-width: 0; color: var(--text-secondary); font-size: 10px; font-weight: 700; }
.shortcut-composer-fields select { width: 100%; min-width: 0; height: 32px; padding: 0 8px; border: 1px solid var(--border-strong); border-radius: 7px; outline: 0; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 11px; }
.shortcut-composer-fields select:focus { border-color: var(--primary); box-shadow: 0 0 0 3px var(--focus-ring); }
.shortcut-main-key { grid-column: span 2; }
.shortcut-composer-preserved { margin: 10px 0 0; color: var(--text-secondary); font-size: 11px; line-height: 1.45; }
.shortcut-composer-preview { display: grid; gap: 4px; margin-top: 12px; padding: 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-raised); }
.shortcut-composer-preview strong { overflow-wrap: anywhere; color: var(--text); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; line-height: 1.45; }
.shortcut-composer-actions { display: flex; justify-content: flex-end; gap: 7px; margin-top: 12px; }
.selection-action { min-height: 30px; padding: 0 10px; border: 1px solid var(--border-strong); border-radius: 7px; color: var(--text); background: var(--surface-raised); font: inherit; font-size: 11px; font-weight: 700; cursor: pointer; }
.selection-action:hover:not(:disabled) { background: var(--surface-hover); }
.selection-action.primary { border-color: var(--primary); color: #fff; background: var(--primary); }
.selection-action.primary:hover:not(:disabled) { background: var(--primary-dark); }
.selection-action:disabled { opacity: .55; cursor: not-allowed; }
@media (max-width: 760px) { .shortcut-composer-fields { grid-template-columns: repeat(2, minmax(0, 1fr)); } .shortcut-main-key { grid-column: span 2; } }
</style>
