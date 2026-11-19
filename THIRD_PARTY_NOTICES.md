# 第三方组件声明

项目自有源码采用 MIT License。下列第三方组件不属于 MIT 授权范围，仍受其各自许可证和分发条款约束。

## VB-Audio VB-CABLE

- 文件：`src-tauri/assets/xiaomi/VBCABLE_Driver_Pack45.zip`
- SHA-256：`b950e39f01af1d04ea623c8f6d8eb9b6ea5c477c637295fabf20631c85116bfb`
- 来源：[VB-Audio VB-CABLE](https://vb-audio.com/Cable/)
- 条款：[VB-Audio Licensing](https://vb-audio.com/Services/licensing.htm)

VB-CABLE 是 Donationware。仓库中的驱动包应保持供应商原始 ZIP 不变。再分发时必须明确说明其来源为 VB-Audio，并告知最终用户可以捐赠或购买许可证。

驱动包内的许可说明还指出：未经作者同意，不得把 VB-CABLE 集成进另一软件的安装流程。公开源码或发布安装包前，请根据实际分发方式再次核对最新条款；需要时向 VB-Audio 获取授权。

## Frida Gadget

- 文件：`src-tauri/assets/xiaomi/frida-gadget-17.15.3-windows-x86_64.dll.xz`
- 版本：17.15.3
- SHA-256：`b566d70189b6d551ad8f4e0bea24de08a3d4c0f559bb35b2bdb67d45182240c2`
- 来源：[Frida Releases](https://github.com/frida/frida/releases)
- 项目主页：[Frida](https://frida.re/)
- 许可证副本：[`third-party-licenses/FRIDA-COPYING.txt`](./third-party-licenses/FRIDA-COPYING.txt)

该文件是 Frida 官方预编译二进制的压缩包，不属于 Nexus Prime 的 MIT 授权范围。

## WinUHid

- 文件：`src-tauri/assets/winuhid/WinUHid.dll` 与 `driver/WinUHidDriver.*`。
- 来源：用户指定的原版 [Voice VibeCoding](https://github.com/mwlt/Voice_VibeCoding) 仓库，其中的 WinUHid 实现基于 [cgutman/WinUHid](https://github.com/cgutman/WinUHid)。
- 安装：首次启动会经 UAC 安装该包内的虚拟键盘驱动，供豆包、微信等输入法快捷键按硬件 HID 路径发送。

## Voice VibeCoding 语音按键实现参考

- 来源：[mwlt/Voice_VibeCoding](https://github.com/mwlt/Voice_VibeCoding)
- 固定提交：`c76056d597c9fb2e504f79313a8cbffd8d648535`
- 参考范围：语音按压顺序、低级键盘钩子置顶确认、F5 配对抑制、WinUHid/SendInput 后端锁定和受控修饰键释放。
- 许可证：MIT License，Copyright (c) 2026 mwlt。

本项目只选择性改写并适配上述机制，未合并上游的微信 F5 映射或界面方案。上游 MIT 许可证文本及版权声明应随任何实质性复用或再分发一并保留。

## 吉他提示音

- 文件：`src-tauri/assets/voice-feedback/start.wav`、`end.wav`。
- 原素材为 Meta Creative Audio 的 Meta Steel Guitar。固定提交、加工说明和样本哈希见 [素材清单](./src-tauri/assets/voice-feedback/sources.json)及[来源说明](./src-tauri/assets/voice-feedback/NOTICE.txt)。
- 素材包提供的 [CC0 1.0 文本](./src-tauri/assets/voice-feedback/LICENSE-AUDIO-CC0-1.0.txt)随源码保存。

## 轻按和消息叮

- 来源：[Kenney Interface Sounds 1.0](https://kenney.nl/assets/interface-sounds)，CC0 1.0 Universal。原文件名、镜像和 SHA-256 见[来源说明](./src-tauri/assets/voice-feedback/NOTICE.txt)。
- 应用内的四段 WAV 经过裁剪、音量匹配和淡化处理。

## 商标

### 输入法图标

- 文件：`src/assets/input-methods/qianwen.jpg`、`wechat.jpg`、`doubao.jpg`。
- 来源：各发布者的 App Store 产品页图标。[千问输入法](https://apps.apple.com/cn/app/id6781589798)、[微信输入法](https://apps.apple.com/cn/app/id1618175312)、[豆包输入法](https://apps.apple.com/cn/app/id6752316550)。
- 图标归各自权利人所有，仅用于在设置中标识对应的输入法。

### 支付渠道图标

- 文件：`src/assets/support/wechat-logo.svg`、`src/assets/support/alipay-logo.svg`。
- 来源：[Ant Design Icons](https://github.com/ant-design/ant-design-icons/tree/7f2516ac91226d2b41f93b35cb5197c8d94f7189/packages/icons-svg/svg/filled) 的 `wechat.svg` 与 `alipay-square.svg`。
- 调整：设置显示颜色，裁去支付宝图标外围留白；保留原始图形路径。
- 许可证：MIT，Copyright (c) 2018-present Ant UED；许可证随应用打包在 `public/licenses/ant-design-icons.txt`。

微信、微信支付、支付宝名称及品牌图形归各自权利人所有，仅用于标明赞赏收款渠道。

VB-Audio、Frida、小米、Windows 等名称和商标归各自权利人所有。项目对这些名称的使用仅用于说明兼容性和第三方来源。
