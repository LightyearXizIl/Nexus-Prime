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

仓库不包含 `WinUHid.dll`，只保留放置说明文件。开发者自行提供该 DLL 时，应确认来源和许可；`.gitignore` 已阻止其被意外提交。

## 商标

VB-Audio、Frida、小米、Windows 等名称和商标归各自权利人所有。项目对这些名称的使用仅用于说明兼容性和第三方来源。
