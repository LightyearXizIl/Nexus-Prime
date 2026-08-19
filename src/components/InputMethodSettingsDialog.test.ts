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

  it("applies both WeChat versions and both Doubao voice modes", async () => {
    const wrapper = mountDialog();
    const tabs = wrapper.findAll('[role="tab"]');
    expect(tabs).toHaveLength(4);

    await tabs[1].trigger("click");
    const wechatButtons = wrapper.findAll(".ime-wechat-option button");
    expect(wechatButtons).toHaveLength(2);
    await wechatButtons[0].trigger("click");
    await wechatButtons[1].trigger("click");
    expect(wrapper.emitted("apply")).toEqual([["wechat-current"], ["wechat"]]);

    await tabs[2].trigger("click");
    await wrapper.get("button.ime-button--primary").trigger("click");
    expect(wrapper.emitted("apply")).toEqual([["wechat-current"], ["wechat"], ["qianwen"]]);

    await tabs[3].trigger("click");
    expect(wrapper.text()).toContain("电脑麦克风");
    const doubaoButtons = wrapper.findAll(".ime-doubao-option button");
    expect(doubaoButtons).toHaveLength(2);
    await doubaoButtons[0].trigger("click");
    await doubaoButtons[1].trigger("click");
    expect(wrapper.emitted("apply")).toEqual([
      ["wechat-current"],
      ["wechat"],
      ["qianwen"],
      ["doubao-hold"],
      ["doubao-hands-free"],
    ]);
  });

  it("disables both WeChat version actions while saving and scopes the success hint to the selected version", async () => {
    const wrapper = mountDialog();
    await wrapper.findAll('[role="tab"]')[1].trigger("click");
    await wrapper.setProps({ saving: true });
    expect(wrapper.findAll(".ime-wechat-option button:disabled")).toHaveLength(2);

    await wrapper.setProps({ saving: false, applyHint: "已应用：语音键 = 左 Ctrl + 左 Shift + D" });
    await wrapper.findAll(".ime-wechat-option button")[0].trigger("click");
    expect(wrapper.findAll(".ime-wechat-option .ime-apply-hint")).toHaveLength(1);
    await wrapper.findAll(".ime-wechat-option button")[1].trigger("click");
    expect(wrapper.findAll(".ime-wechat-option .ime-apply-hint")).toHaveLength(1);
  });

  it("disables both Doubao actions while saving and scopes the success hint to the selected mode", async () => {
    const wrapper = mountDialog();
    await wrapper.findAll('[role="tab"]')[3].trigger("click");
    await wrapper.setProps({ saving: true });
    expect(wrapper.findAll(".ime-doubao-option button:disabled")).toHaveLength(2);

    await wrapper.setProps({ saving: false, applyHint: "已应用：豆包长按模式" });
    await wrapper.findAll(".ime-doubao-option button")[0].trigger("click");
    expect(wrapper.findAll(".ime-doubao-option .ime-apply-hint")).toHaveLength(1);
    await wrapper.findAll(".ime-doubao-option button")[1].trigger("click");
    expect(wrapper.findAll(".ime-doubao-option .ime-apply-hint")).toHaveLength(1);
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
