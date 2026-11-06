export type ModifierSide = "none" | "generic" | "left" | "right";
export type ModifierGroup = "ctrl" | "shift" | "alt" | "win";
export type ShortcutKey = number | string;

const symbolLabels: Record<number, string> = {
  0xbc: ",", 0xbe: ".", 0xbf: "/", 0xba: ";", 0xde: "'",
  0xdb: "[", 0xdd: "]", 0xdc: "\\", 0xbd: "-", 0xbb: "=", 0xc0: "`",
};

const shiftedSymbols = new Map<string, number>([
  ["?", 0xbf], ["!", 0x31], ["<", 0xbc], [">", 0xbe], [":", 0xba], ['"', 0xde],
  ["(", 0x39], [")", 0x30], ["{", 0xdb], ["}", 0xdd], ["|", 0xdc], ["_", 0xbd],
  ["+", 0xbb], ["~", 0xc0], ["@", 0x32], ["#", 0x33], ["$", 0x34], ["%", 0x35],
  ["^", 0x36], ["&", 0x37], ["*", 0x38],
]);

const unshiftedSymbols = new Map(
  Object.entries(symbolLabels).map(([vk, label]) => [label, Number(vk)] as const),
);

export const SHORTCUT_SYMBOL_OPTIONS: { value: ShortcutKey; label: string }[] = [
  ...Array.from(unshiftedSymbols, ([label, value]) => ({ value, label })),
  ...Array.from(shiftedSymbols.keys(), (symbol) => ({ value: symbol, label: symbol })),
];

type ShortcutResult = { keys?: number[]; error?: string };

export const modifierValues: Record<ModifierGroup, Record<Exclude<ModifierSide, "none">, number>> = {
  ctrl: { generic: 0x11, left: 0xa2, right: 0xa3 },
  shift: { generic: 0x10, left: 0xa0, right: 0xa1 },
  alt: { generic: 0x12, left: 0xa4, right: 0xa5 },
  win: { generic: 0x5b, left: 0x5b, right: 0x5c },
};

const modifierGroups: Record<number, ModifierGroup> = {
  0x10: "shift",
  0x11: "ctrl",
  0x12: "alt",
  0xa0: "shift",
  0xa1: "shift",
  0xa2: "ctrl",
  0xa3: "ctrl",
  0xa4: "alt",
  0xa5: "alt",
  0x5b: "win",
  0x5c: "win",
};

const modifierLabels: Record<number, string> = {
  0x10: "Shift",
  0x11: "Ctrl",
  0x12: "Alt",
  0xa0: "左 Shift",
  0xa1: "右 Shift",
  0xa2: "左 Ctrl",
  0xa3: "右 Ctrl",
  0xa4: "左 Alt",
  0xa5: "右 Alt",
  0x5b: "左 Win",
  0x5c: "右 Win",
};

export function isModifierVk(vk: number): boolean {
  return vk in modifierGroups;
}

export function keyLabel(vk: number): string {
  const map: Record<number, string> = {
    0x08: "Backspace",
    0x09: "Tab",
    0x0d: "Enter",
    0x13: "Pause",
    0x14: "CapsLock",
    0x1b: "Esc",
    0x20: "Space",
    0x21: "Page Up",
    0x22: "Page Down",
    0x23: "End",
    0x24: "Home",
    0x25: "左方向键",
    0x26: "上方向键",
    0x27: "右方向键",
    0x28: "下方向键",
    0x2c: "PrtSc",
    0x2d: "Insert",
    0x2e: "Delete",
    0x5d: "Menu",
    0x90: "NumLock",
    0x91: "ScrLk",
    0x6a: "Num *",
    0x6b: "Num +",
    0x6d: "Num -",
    0x6e: "Num .",
    0x6f: "Num /",
    ...symbolLabels,
    0xad: "静音",
    0xae: "音量 -",
    0xaf: "音量 +",
    0xb0: "下一曲",
    0xb1: "上一曲",
    0xb2: "停止播放",
    0xb3: "播放 / 暂停",
    0xa6: "浏览器后退",
    0xa7: "浏览器前进",
    0xa8: "浏览器刷新",
    0xa9: "浏览器停止",
    0xaa: "浏览器搜索",
    0xab: "浏览器收藏夹",
    0xac: "浏览器主页",
    0xb4: "邮件",
    0xb5: "媒体播放器",
    0xb6: "应用 1",
    0xb7: "应用 2",
    ...modifierLabels,
  };
  if (map[vk]) return map[vk];
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
  if (vk >= 0x60 && vk <= 0x69) return `Num ${vk - 0x60}`;
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  return `VK_0x${vk.toString(16).toUpperCase()}`;
}

/** Parse a hand-written shortcut without relying on the Windows keyboard hook. */
export function parseShortcutText(input: string): ShortcutResult {
  const normalized = input.trim();
  if (!normalized) return { error: "请输入至少一个按键" };
  const tokens = normalized.split("+").map((item) => item.trim().toLowerCase());
  if (tokens.some((item) => !item)) return { error: "请使用 Plus 表示加号键" };

  const modifiers: Record<string, number> = {
    ctrl: 0x11, control: 0x11, lctrl: 0xa2, leftctrl: 0xa2, 左ctrl: 0xa2,
    rctrl: 0xa3, rightctrl: 0xa3, 右ctrl: 0xa3,
    shift: 0x10, lshift: 0xa0, leftshift: 0xa0, 左shift: 0xa0,
    rshift: 0xa1, rightshift: 0xa1, 右shift: 0xa1,
    alt: 0x12, lalt: 0xa4, leftalt: 0xa4, 左alt: 0xa4,
    ralt: 0xa5, rightalt: 0xa5, 右alt: 0xa5,
    win: 0x5b, lwin: 0x5b, leftwin: 0x5b, 左win: 0x5b,
    rwin: 0x5c, rightwin: 0x5c, 右win: 0x5c,
  };
  const named: Record<string, number> = {
    backspace: 0x08, tab: 0x09, enter: 0x0d, esc: 0x1b, escape: 0x1b,
    space: 0x20, pageup: 0x21, pagedown: 0x22, end: 0x23, home: 0x24,
    left: 0x25, up: 0x26, right: 0x27, down: 0x28, insert: 0x2d, delete: 0x2e,
    numplus: 0x6b, numpadplus: 0x6b, numminus: 0x6d,
    numdivide: 0x6f, numdecimal: 0x6e, mute: 0xad, volumedown: 0xae,
    volumeup: 0xaf, medianext: 0xb0, mediaprevious: 0xb1, mediastop: 0xb2,
    mediaplaypause: 0xb3,
  };
  const keys: ShortcutKey[] = [];
  for (const token of tokens) {
    const symbol = token === "plus" ? "+" : token;
    if (shiftedSymbols.has(symbol)) {
      keys.push(symbol);
      continue;
    }
    let key = modifiers[token] ?? named[token] ?? unshiftedSymbols.get(token);
    if (key == null && /^[a-z]$/.test(token)) key = token.toUpperCase().charCodeAt(0);
    if (key == null && /^\d$/.test(token)) key = token.charCodeAt(0);
    const fn = /^f([1-9]|1\d|2[0-4])$/.exec(token);
    if (key == null && fn) key = 0x6f + Number(fn[1]);
    if (typeof key !== "number") return { error: `不支持的按键：${token}` };
    keys.push(key);
  }
  return resolveShortcutKeys(keys);
}

function resolveShortcutKeys(selection: readonly ShortcutKey[]): ShortcutResult {
  const keys: number[] = [];
  const selectedModifiers = new Set<ModifierGroup>();
  let needsShift = false;

  for (const key of selection) {
    const vk = typeof key === "number" ? key : shiftedSymbols.get(key);
    if (vk == null) return { error: `不支持的按键：${key}` };
    needsShift ||= typeof key === "string";

    const group = modifierGroups[vk];
    if (group) {
      if (selectedModifiers.has(group)) return { error: `重复的 ${group} 修饰键` };
      selectedModifiers.add(group);
    }
    if (keys.includes(vk)) return { error: `重复的按键：${keyLabel(vk)}` };
    keys.push(vk);
  }

  if (!needsShift) return { keys };
  const modifiers = keys.filter(isModifierVk);
  if (!selectedModifiers.has("shift")) modifiers.push(modifierValues.shift.generic);
  return { keys: [...modifiers, ...keys.filter((vk) => !isModifierVk(vk))] };
}

/** 录入 UI 常驻媒体/系统键兜底（对齐上游 MEDIA_PICK_KEYS，标签复用 keyLabel） */
export const MEDIA_PICK_KEYS: { vk: number; label: string }[] = [
  { vk: 0xaf, label: keyLabel(0xaf) },
  { vk: 0xae, label: keyLabel(0xae) },
  { vk: 0xad, label: keyLabel(0xad) },
  { vk: 0xb7, label: keyLabel(0xb7) },
];

/**
 * Keep the user-visible shortcut semantically unique. A generic Ctrl/Shift/Alt
 * is redundant when the same modifier already has an explicit side selected.
 */
export function normalizeShortcutVks(vks: readonly number[]): number[] {
  const values = [...new Set(vks.map(Number).filter(Number.isFinite))];
  const explicitGroups = new Set<ModifierGroup>();
  for (const value of values) {
    if ([0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0x5b, 0x5c].includes(value)) {
      explicitGroups.add(modifierGroups[value]);
    }
  }
  return values.filter((value) => {
    const group = modifierGroups[value];
    const isGeneric = value === 0x10 || value === 0x11 || value === 0x12;
    return !(isGeneric && group && explicitGroups.has(group));
  });
}

export function modifierSideForKeys(keys: readonly number[], group: ModifierGroup): ModifierSide {
  const normalized = normalizeShortcutVks(keys);
  const values = modifierValues[group];
  if (group !== "win" && normalized.includes(values.generic)) return "generic";
  if (normalized.includes(values.left)) return "left";
  if (normalized.includes(values.right)) return "right";
  return "none";
}

export function composeShortcutVks(
  modifiers: Record<ModifierGroup, ModifierSide>,
  keys: readonly ShortcutKey[],
): ShortcutResult {
  const selected = (Object.keys(modifierValues) as ModifierGroup[])
    .map((group) => {
      const side = modifiers[group];
      return side === "none" ? null : modifierValues[group][side];
    })
    .filter((value): value is number => value !== null);
  return resolveShortcutKeys([...selected, ...keys]);
}

export function splitShortcutVks(keys: readonly number[]) {
  const normalized = normalizeShortcutVks(keys);
  return {
    modifiers: {
      ctrl: modifierSideForKeys(normalized, "ctrl"),
      shift: modifierSideForKeys(normalized, "shift"),
      alt: modifierSideForKeys(normalized, "alt"),
      win: modifierSideForKeys(normalized, "win"),
    } satisfies Record<ModifierGroup, ModifierSide>,
    keys: normalized.filter((key) => !isModifierVk(key)),
  };
}

/** The only historical truncated preset produced by the old Codex capture flow. */
export function isLegacyIncompleteCodexShortcut(keys: readonly number[]): boolean {
  const normalized = normalizeShortcutVks(keys);
  return normalized.length === 2 && normalized[0] === 0xa2 && normalized[1] === 0xa0;
}

export function vksToHotkeyNames(vks: readonly number[]): string[] {
  const map: Record<number, string> = {
    0xa2: "leftctrl", 0xa3: "rightctrl", 0x11: "ctrl",
    0xa0: "leftshift", 0xa1: "rightshift", 0x10: "shift",
    0xa4: "leftalt", 0xa5: "rightalt", 0x12: "alt",
    0x5b: "leftwin", 0x5c: "rightwin",
    0x08: "backspace", 0x09: "tab", 0x0d: "enter", 0x1b: "esc", 0x20: "space",
    0x13: "pause", 0x14: "capslock", 0x2c: "printscreen", 0x5d: "menu",
    0x90: "numlock", 0x91: "scrolllock",
    0x6a: "numpadmult", 0x6b: "numpadadd", 0x6d: "numpadsubtract",
    0x6e: "numpaddecimal", 0x6f: "numpaddivide",
    0xba: "semicolon", 0xbb: "equal", 0xbc: "comma", 0xbd: "minus",
    0xbe: "period", 0xbf: "slash", 0xc0: "grave",
    0xdb: "bracketleft", 0xdc: "backslash", 0xdd: "bracketright", 0xde: "apostrophe",
  };
  return normalizeShortcutVks(vks).map((vk) => {
    if (map[vk]) return map[vk];
    if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk).toLowerCase();
    if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
    if (vk >= 0x60 && vk <= 0x69) return `numpad${vk - 0x60}`;
    if (vk >= 0x70 && vk <= 0x87) return `f${vk - 0x6f}`;
    return `vk_${vk.toString(16)}`;
  });
}
