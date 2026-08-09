import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import VoiceShortcutComposer from "./VoiceShortcutComposer.vue";

function mountComposer(initialKeys: number[]) {
  return mount(VoiceShortcutComposer, {
    props: { initialKeys, buttonLabel: "语音", slotLabel: "单击" },
  });
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

  it("keeps unknown secondary keys when editing the primary key", async () => {
    const wrapper = mountComposer([0xa2, 0x44, 0x91]);
    const selects = wrapper.findAll("select");
    await selects[4].setValue("69");
    await wrapper.get("button.selection-action.primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([[[0xa2, 0x45, 0x91]]]);
  });
});
