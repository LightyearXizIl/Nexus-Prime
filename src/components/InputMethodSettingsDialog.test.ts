import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";
import InputMethodSettingsDialog from "./InputMethodSettingsDialog.vue";

function mountDialog() {
  return mount(InputMethodSettingsDialog, {
    attachTo: document.body,
    props: { open: true, configReady: true, saving: false, applyHint: "", activePreset: null },
  });
}

describe("InputMethodSettingsDialog", () => {
  beforeEach(() => window.localStorage.clear());

  it("matches WeChat's two voice modes, all Qianwen shortcuts, and both Doubao modes", async () => {
    const wrapper = mountDialog();
    const tabs = wrapper.findAll('[role="tab"]');
    expect(tabs.map(tab => tab.text())).toEqual(["千问输入法", "微信输入法", "豆包输入法"]);

    await tabs[1].trigger("click");
    const wechatButtons = wrapper.findAll(".ime-option button");
    expect(wechatButtons).toHaveLength(2);
    expect(wrapper.text()).toContain("启动语音输入");
    expect(wrapper.text()).toContain("左 Ctrl + 左 Win");
    expect(wrapper.text()).toContain("按住说话");
    expect(wrapper.text()).toContain("左 Ctrl + 左 Shift + D");
    expect(wrapper.text()).not.toContain("F5 + 左 Ctrl + 左 Win");
    await wechatButtons[0].trigger("click");
    await wechatButtons[1].trigger("click");
    expect(wrapper.emitted("apply")).toEqual([["wechat-current"], ["wechat"]]);

    await wrapper.get("#ime-tab-qianwen").trigger("click");
    expect(wrapper.text()).toContain("左 Ctrl + 左 Win");
    expect(wrapper.text()).toContain("左 Win + 左 Alt");
    expect(wrapper.text()).toContain("右 Alt");
    const qianwenButtons = wrapper.findAll(".ime-option button");
    expect(qianwenButtons).toHaveLength(4);
    for (const button of qianwenButtons) await button.trigger("click");
    expect(wrapper.emitted("apply")).toEqual([
      ["wechat-current"],
      ["wechat"],
      ["qianwen"],
      ["qianwen-left-ctrl"],
      ["qianwen-left-ctrl-win"],
      ["qianwen-left-win-alt"],
    ]);

    await wrapper.get("#ime-tab-doubao").trigger("click");
    expect(wrapper.text()).toContain("电脑麦克风");
    const doubaoButtons = wrapper.findAll(".ime-option button");
    expect(doubaoButtons).toHaveLength(2);
    await doubaoButtons[0].trigger("click");
    await doubaoButtons[1].trigger("click");
    expect(wrapper.emitted("apply")).toEqual([
      ["wechat-current"],
      ["wechat"],
      ["qianwen"],
      ["qianwen-left-ctrl"],
      ["qianwen-left-ctrl-win"],
      ["qianwen-left-win-alt"],
      ["doubao-hold"],
      ["doubao-hands-free"],
    ]);
  });

  it("disables both WeChat actions while saving and shows the active mode", async () => {
    const wrapper = mountDialog();
    await wrapper.findAll('[role="tab"]')[1].trigger("click");
    await wrapper.setProps({ saving: true });
    expect(wrapper.findAll(".ime-option button:disabled")).toHaveLength(2);

    await wrapper.setProps({ saving: false, applyHint: "已应用：微信按住说话", activePreset: "wechat-current" });
    expect(wrapper.findAll(".ime-current")).toHaveLength(1);
    expect(wrapper.find(".ime-current").text()).toBe("当前已应用");
    expect(wrapper.findAll(".ime-option button")[0].attributes("aria-pressed")).toBe("true");
    await wrapper.findAll(".ime-option button")[0].trigger("click");
    expect(wrapper.findAll(".ime-option .ime-apply-hint")).toHaveLength(1);
  });

  it("disables both Doubao actions while saving and scopes the success hint to the selected mode", async () => {
    const wrapper = mountDialog();
    await wrapper.get("#ime-tab-doubao").trigger("click");
    await wrapper.setProps({ saving: true });
    expect(wrapper.findAll(".ime-option button:disabled")).toHaveLength(2);

    await wrapper.setProps({ saving: false, applyHint: "已应用：豆包长按模式" });
    await wrapper.findAll(".ime-option button")[0].trigger("click");
    expect(wrapper.findAll(".ime-option .ime-apply-hint")).toHaveLength(1);
    await wrapper.findAll(".ime-option button")[1].trigger("click");
    expect(wrapper.findAll(".ime-option .ime-apply-hint")).toHaveLength(1);
  });

  it("disables every Qianwen action while saving and marks only the active shortcut", async () => {
    const wrapper = mountDialog();
    await wrapper.get("#ime-tab-qianwen").trigger("click");
    await wrapper.setProps({ saving: true });
    expect(wrapper.findAll(".ime-option button:disabled")).toHaveLength(4);

    await wrapper.setProps({ saving: false, applyHint: "已应用：千问语音键 = 左 Win + 左 Alt", activePreset: "qianwen-left-win-alt" });
    expect(wrapper.findAll(".ime-option .ime-current")).toHaveLength(1);
    expect(wrapper.findAll(".ime-option button")[3].attributes("aria-pressed")).toBe("true");
    await wrapper.findAll(".ime-option button")[3].trigger("click");
    expect(wrapper.findAll(".ime-option .ime-apply-hint")).toHaveLength(1);
  });

  it("cycles tab selection with the keyboard and restores focus after closing", async () => {
    window.localStorage.setItem("nexus-prime.input-method-settings.provider", "codex");
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const wrapper = mount(InputMethodSettingsDialog, {
      attachTo: document.body,
      props: { open: false, configReady: true, saving: false, applyHint: "", activePreset: null },
    });
    await wrapper.setProps({ open: true });
    const active = wrapper.get('[role="tab"][aria-selected="true"]');
    expect(active.text()).toBe("千问输入法");
    await active.trigger("keydown", { key: "ArrowRight" });
    expect(wrapper.get('[role="tab"][aria-selected="true"]').text()).toBe("微信输入法");
    await wrapper.setProps({ open: false });
    expect(document.activeElement).toBe(opener);
    wrapper.unmount();
    opener.remove();
  });
});
