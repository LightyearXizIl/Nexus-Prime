import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import InputMethodSettingsDialog from "./InputMethodSettingsDialog.vue";

function mountDialog() {
  return mount(InputMethodSettingsDialog, {
    attachTo: document.body,
    props: { open: true, configReady: true, saving: false, applyHint: "" },
  });
}

describe("InputMethodSettingsDialog", () => {
  beforeEach(() => window.localStorage.clear());

  it("applies each supported preset without offering an action for the placeholder", async () => {
    const wrapper = mountDialog();
    const tabs = wrapper.findAll('[role="tab"]');
    expect(tabs).toHaveLength(4);

    await tabs[1].trigger("click");
    await wrapper.get("button.ime-button--primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([["wechat"]]);

    await tabs[2].trigger("click");
    await wrapper.get("button.ime-button--primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([["wechat"], ["qianwen"]]);

    await tabs[3].trigger("click");
    expect(wrapper.get("button.ime-button--secondary[disabled]").text()).toContain("等待");
    expect(wrapper.findAll("button.ime-button--primary")).toHaveLength(0);
  });

  it("cycles tab selection with the keyboard and restores focus after closing", async () => {
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const wrapper = mount(InputMethodSettingsDialog, {
      attachTo: document.body,
      props: { open: false, configReady: true, saving: false, applyHint: "" },
    });
    await wrapper.setProps({ open: true });
    const active = wrapper.get('[role="tab"][aria-selected="true"]');
    await active.trigger("keydown", { key: "ArrowRight" });
    expect(wrapper.get('[role="tab"][aria-selected="true"]').text()).toBe("微信");
    await wrapper.setProps({ open: false });
    expect(document.activeElement).toBe(opener);
    wrapper.unmount();
    opener.remove();
  });
});
