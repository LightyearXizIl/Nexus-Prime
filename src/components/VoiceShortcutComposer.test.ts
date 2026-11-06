import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import VoiceShortcutComposer from "./VoiceShortcutComposer.vue";

function mountComposer(initialKeys: number[]) {
  return mount(VoiceShortcutComposer, {
    props: { initialKeys, buttonLabel: "语音", slotLabel: "单击" },
  });
}

async function selectKey(wrapper: ReturnType<typeof mountComposer>, index: number, label: string) {
  const select = wrapper.findAll(".shortcut-key-row")[index].get("select");
  const option = select.findAll("option").find((item) => item.text() === label);
  expect(option).toBeDefined();
  await select.setValue(option!.element.value);
}

describe("VoiceShortcutComposer", () => {
  it("preserves a valid right-Alt-only shortcut", async () => {
    const wrapper = mountComposer([0xa5]);
    expect(wrapper.text()).toContain("右 Alt");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0xa5]]]);
  });

  it("offers and preserves a generic Ctrl without adding a duplicate left Ctrl", async () => {
    const wrapper = mountComposer([0x11, 0x44]);
    const selects = wrapper.findAll("select");
    expect((selects[0].element as HTMLSelectElement).value).toBe("generic");
    await selects[0].setValue("left");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0xa2, 0x44]]]);
  });

  it("reopens saved extra keys and allows replacing or removing them", async () => {
    const wrapper = mountComposer([0xa2, 0x44, 0x91]);
    expect(wrapper.get(".shortcut-composer-preview strong").text()).toBe("左 Ctrl + D + ScrLk");
    await selectKey(wrapper, 0, "E");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0xa2, 0x45, 0x91]]]);

    await selectKey(wrapper, 1, ".");
    expect(wrapper.get(".shortcut-composer-preview strong").text()).toBe("左 Ctrl + E + .");
    await wrapper.get('button[aria-label="移除附加键 1"]').trigger("click");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0xa2, 0x45, 0x91]], [[0xa2, 0x45]]]);
  });

  it.each([[".", 0xbe], [",", 0xbc]] as const)("offers %s as a standalone symbol", async (label, vk) => {
    const wrapper = mountComposer([]);
    expect(wrapper.get("button.selection-action.primary").attributes("disabled")).toBeDefined();
    expect(wrapper.find('optgroup[label="符号"]').exists()).toBe(true);
    await selectKey(wrapper, 0, label);
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[vk]]]);
  });

  it("adds Shift for a question mark and rejects a duplicate physical key", async () => {
    const wrapper = mountComposer([]);
    await selectKey(wrapper, 0, "?");
    expect(wrapper.get(".shortcut-composer-preview strong").text()).toBe("Shift + /");
    await wrapper.get(".shortcut-add-key").trigger("click");
    await selectKey(wrapper, 1, "/");
    expect(wrapper.get(".shortcut-composer-preview strong").text()).toContain("重复的按键");
    expect(wrapper.get("button.selection-action.primary").attributes("disabled")).toBeDefined();

    await selectKey(wrapper, 1, "A");
    await wrapper.findAll("select")[1].setValue("right");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0xa1, 0xbf, 0x41]]]);
  });

  it("transfers valid symbol text to dropdowns for further editing", async () => {
    const wrapper = mountComposer([0x41]);
    const input = wrapper.get('input[name="shortcut-text"]');
    await input.setValue("Ctrl + ? + .");
    expect(wrapper.get("fieldset").attributes("disabled")).toBeDefined();
    expect(wrapper.get(".shortcut-composer-preview strong").text()).toBe("Ctrl + Shift + / + .");
    await wrapper.findAll("button").find((button) => button.text() === "改用下拉")!.trigger("click");
    expect((input.element as HTMLInputElement).value).toBe("");
    expect(wrapper.get("fieldset").attributes("disabled")).toBeUndefined();
    expect(wrapper.findAll(".shortcut-key-row")).toHaveLength(2);
    await selectKey(wrapper, 1, ",");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0x11, 0x10, 0xbf, 0xbc]]]);
  });

  it("uses valid text input and blocks invalid text without replacing the selection", async () => {
    const wrapper = mountComposer([0x41]);
    const input = wrapper.get('input[name="shortcut-text"]');
    await input.setValue("Alt + F4");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0x12, 0x73]]]);

    await input.setValue("Alt+MissingKey");
    expect(wrapper.get("button.selection-action.primary").attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("不支持的按键");
  });
});
