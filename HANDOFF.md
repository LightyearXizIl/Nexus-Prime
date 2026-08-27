# 交接记录

更新时间：2026-08-27

## 本次范围（v0.3.0，2026-08-27）

- 运行日志已改为 `logs/app-YYYY-MM-DD.log` 按日后台写入；设置页可选 1、3、7、14、30 天，默认 7 天。界面、命令、设备、按键注入和语音会话均写语义事件；日志保留实际设备地址、路径、快捷键和配置值。
- Click/Hold 均在 `AUDIO_START` 立即发送语音快捷键。语音的纯 Ctrl+Win、Win 或 Alt 组合会先按 F24 中和再释放，避免空按弹出系统菜单。
- `VoiceChordState` 不会再在释放失败后丢弃原始持键记录；同路由重试三次后，虚拟 HID 会执行仅限本会话键位的 SendInput KEYUP 恢复。普通组合键的 SendInput KEYUP 已逐键发送。
- 已通过 `cargo test --workspace`（96 项）、`cargo check --workspace --all-targets`、`npm.cmd test`（31 项）、`npm.cmd run build`、PowerShell 脚本语法检查、`git diff --check` 和 `npm.cmd run tauri:build`。本地正式 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.0_x64-setup.exe`，13,298,359 bytes，SHA-256 `ADF18A6692D57C5B65BEAD0D7586C5284902BE1C77F835BF35859C26EFD657FC`，文件版本和产品版本均为 `0.3.0`。真机豆包、微信悬浮语音条与 Windows 菜单表现按用户指示暂缓验收；GitHub Release 上传完成后补录远端资产校验信息。

## 本次范围（v0.2.9，2026-08-27）

修复已发布 v0.2.8 在真实 Windows 11 中“修复虚拟键盘失败”的根因，并以新的正式版本交付：

- **根因**：旧 PowerShell 脚本以 ANSI 形式调用 `SetupDiSetDeviceRegistryProperty`，却传入 UTF-16 `MULTI_SZ`。Windows 因此把 `Root\\WinUHid` 写成 12 个单字符硬件 ID；驱动可以成功暂存为 `oem103.inf`，但永远不会匹配该设备，`\\.\\WinUHid` 不存在。
- **修复**：所有涉及字符串结构的 SetupAPI 调用显式使用 Unicode；创建或复用正确根设备后，列出兼容驱动、选中 WinUHid 并调用 `DIF_INSTALLDEVICE`，随后执行 PnP 扫描。安装成功只在 `\\.\\WinUHid` 实际可打开时报告“已就绪”。
- **强制修复**：保留 `-Force` 入口；仅删除旧版脚本所产生的“逐字符硬件 ID”WinUHid 根设备，再重新绑定正确节点，不影响其它物理或虚拟设备。安装过程会记录 `install.log`，提升权限子进程失败时把末条原因带回应用日志。
- **真实 Windows 11 验证**：强制修复返回 0；设备 `ROOT\\WINUHID_VIRTUAL_HID_ENUMERATOR\\0008` 状态为 Started、驱动为 `oem103.inf`、提供方 WinUHid Project；`\\.\\WinUHid` 可打开。`cargo run --example diag_voice_tap` 显示 `WinUHid available=true`，右 Alt、Ctrl+Win、普通按键和连续 20 次点击均已释放，无粘键。
- **本地正式安装包**：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.2.9_x64-setup.exe`，13,272,757 bytes，SHA-256 `D7406F6B8F7AC68CC20AB9D5236147BA21623E5D39164E7B51AC54BDE26A12DE`；文件版本和产品版本均为 `0.2.9`，构建配置将 WinUHid DLL、证书、INF、CAT、驱动 DLL 和修复脚本作为安装资源打包。
- **GitHub 正式发布**：[`v0.2.9 Release`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.2.9) 已公开发布；资产 `Nexus.Prime_0.2.9_x64-setup.exe` 状态为 `uploaded`、大小 13,272,757 bytes，GitHub digest 为 `sha256:d7406f6b8f7ac68cc20ab9d5236147ba21623e5d39164e7b51ac54bde26a12de`。

### 仍待用户输入法验收

- 请安装 v0.2.9 后在实际豆包和微信中分别验证 Click/Hold。驱动与虚拟键盘注入已在本机通过，但是否触发第三方输入法的听写界面仍需以用户实际配置为准。
- 回归 Alt+Tab、Space、音量、方向与返回；这些路径未改为 WinUHid。

## 本次范围（v0.2.8，2026-08-26）

本次把截至当前工作区的图标、启动行为与虚拟键盘可靠性改动统一整理为 v0.2.8：

- **输入法虚拟键盘修复**：豆包右 Alt、微信旧版 Ctrl + Win 和新版 Ctrl + Shift + D 的语音快捷键，在 Click、Click 后长按和 Hold 三条路径都会优先使用 WinUHid；一次按下后会记住实际注入路由，抬起、重试与异常补偿只使用同一路由，防止混用 SendInput、重复触发或粘键。WinUHid 不可用时才回退 SendInput，并写入 `route=virtual_hid` 或 `route=send_input_fallback` 日志。
- **不扩大影响范围**：Alt 组合、Space、静音与音量保留原有分流；ATVV、返回、方向、音量及其它普通遥控器映射不改为虚拟键盘。新增回归覆盖固定路由释放、注入失败补偿与普通 Alt 组合不优先走 WinUHid。
- **驱动与修复入口**：NSIS 资源含 WinUHid SDK DLL、证书、INF、CAT、驱动 DLL 和 PowerShell 安装脚本。首次启动后台检测并请求 UAC 安装；首页“修复虚拟键盘”会先释放正在按住的语音组合键，强制重新部署/安装，并明确显示“已就绪”“需要重启”“UAC 已取消”或失败信息。声卡、虚拟键盘、ATVV 和桥接修复互斥执行。
- **启动与桌面体验**：开机启动使用当前用户 Run 注册表并迁移清理旧 Startup 快捷方式；“登录时最小化到托盘”只影响 Windows 登录启动，手动打开仍显示窗口。端口检测和 HID Tap 的后台系统命令不再弹出控制台窗口。图标资源链路继续使用透明 `N.` 图标，覆盖桌面、任务栏、托盘、安装器、网页和移动端资产。
- **文档与版本**：`package.json`、Cargo、Tauri 配置均为 `0.2.8`；README、更新日志和第三方声明已写明虚拟键盘用途、修复方法和 WinUHid 来源。

### 已完成验证

- `npm.cmd test`：30/30 通过；`npm.cmd run build`：通过。
- `cargo test --workspace`：89/89 通过；`cargo check --workspace --all-targets`：通过。
- PowerShell 安装脚本语法检查、`git diff --check` 与 `npm.cmd run tauri:build` 均通过。
- 本地正式 NSIS 安装包：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.2.8_x64-setup.exe`，13,271,770 bytes，SHA-256 `D205316D3D927949146FC1CFDD83CF470678F6046EF7D7D93E06ED937E35F499`。已检查 NSIS 脚本包含 WinUHid DLL、证书、INF、CAT、驱动 DLL 和修复脚本。

### GitHub 发布记录

- 源码提交：`8d7fd06e977fbb70a0794a0a560916661ac279ca`（`main`）。
- [`v0.2.7 Release`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.2.7)：`Nexus.Prime_0.2.7_x64-setup.exe`，13,266,457 bytes，SHA-256 `8C5D004DDACC1F78FBF347289462A8FDDCDF23ECBDE0BE396519000E5C6826C8`，GitHub 资产状态为 `uploaded`。
- [`v0.2.8 Release`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.2.8)：`Nexus.Prime_0.2.8_x64-setup.exe`，13,271,770 bytes，SHA-256 `D205316D3D927949146FC1CFDD83CF470678F6046EF7D7D93E06ED937E35F499`，GitHub 资产状态为 `uploaded`。
- v0.2.8 已公开发布且是 `releases/latest`；应用内更新器会读取它并匹配同名的 x64 NSIS 安装包。

### 待真实 Windows 验收

- 当前机器仍在运行已安装的 v0.2.6，`\\.\WinUHid` 尚未创建；本轮没有把 v0.2.8 安装到真实环境，也没有完成 UAC/驱动设备/豆包或微信的端到端验收。
- 安装 v0.2.8 后，先接受首次 WinUHid 安装提示；若未就绪，在首页执行“修复虚拟键盘”，按结果重启 Windows。随后分别在 Click/Hold 下验证豆包右 Alt、微信 Ctrl + Win 和 Ctrl + Shift + D，确认日志为 `route=virtual_hid`；再回归 Alt+Tab、Space、音量、方向与返回。
- 两个 Release 已包含正式安装包；仍需在真实 Windows 设备安装后完成驱动、输入法与遥控器端到端验收。

## 本次范围（v0.2.6，2026-08-25）

以用户提供的 2048×2048 图像为唯一母版，完成全平台应用图标替换：

- **透明母版**：`src-tauri/icons/app-icon-transparent.png` 已改为 2048×2048 RGBA。仅去除与画布边界相连的外围黑底；内部深色面板与白色 `N.` 保持原图，外围保留收窄的半透明彩色光晕。
- **全链路资源**：使用项目内 Tauri CLI 重生 Windows、Windows Store、macOS、Android、iOS 与通用 PNG 图标资源；`tray-icon.png` 单独更新为 64×64，小尺寸网页图标新增为 `public/favicon.png`，`index.html` 不再引用失效的 `/vite.svg`。
- **版本与文档**：`package.json`/锁文件、Cargo 包与锁文件、Tauri 配置、README、CHANGELOG 均同步至 0.2.6；不改变业务 API、配置格式、用户数据或运行逻辑。
- **验证与发布**：`npm.cmd test` 25/25、`npm.cmd run build`、`cargo test --workspace` 82/82、`cargo check --workspace --all-targets`、`npm.cmd run tauri:build` 与 `git diff --check` 均通过。母版与桌面 PNG 已验证 RGBA、四角 Alpha=0、中心面板 Alpha=255，ICO 含 16/24/32/48/64/256 六层；主程序和 NSIS 安装器已提取并目检为新图标，卸载器配置同样绑定 `icons/icon.ico`。本地 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.2.6_x64-setup.exe`（13,219,595 bytes，SHA-256 `093B5C9FC23F732051B023EE5A2B65A53AC69428B17ACD1785F877837587A6D2`）。为保护正在运行的用户安装和真实配置，未启动本地构建产物或临时安装；开始菜单、窗口/任务栏/托盘图标及从 v0.2.5 覆盖安装后的图标与用户配置保留，仍需用户在真实 Windows 环境完成验收。

## 本次范围（v0.2.4，2026-08-19）

修正 v0.2.3 直接替换微信输入法预设的问题，兼容不同输入法版本：

- **旧版预设保留**：`wechat` 恢复为左 Ctrl + 左 Win，保持 `Hold` 触发模式。
- **新版预设新增**：`wechat-current` 使用本机微信输入法 2.1.2.12 已验证的左 Ctrl + 左 Shift + D，保持 `Hold` 触发模式。
- **设置页**：微信页同时显示“新版（本机 2.1.2.12 已验证）”与“旧版”两个按钮，并提示用户以微信输入法“设置 → 语音输入”中的实际快捷键为准。
- **验证**：`npm.cmd test` 25/25、`npm.cmd run build`、`cargo test --workspace` 77/77、`cargo check --workspace --all-targets`、`npm.cmd run tauri:build` 与 `git diff --check` 均通过。NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.2.4_x64-setup.exe`，SHA-256 为 `EF451E4CFBBD58B080AF2E5867DB23258786FE40C895A48B9AEC354962A0C5C4`；实机验收待用户确认。

## 本次范围（v0.2.3，2026-08-19）

修复微信输入法升级后遥控器语音键无法唤起听写的问题：

- **根因确认**：微信输入法已升级至 2.1.2.12，旧预设左 Ctrl + 左 Win 不再响应；手动按旧快捷键同样无反应。
- **链路确认**：遥控器 ATVV 音频、Nexus Prime `SendInput`、PCM 路由和 VB-CABLE 默认录音设备均正常，故障仅为输入法快捷键不匹配。
- **修复**：微信预设改为“按住说话”的左 Ctrl + 左 Shift + D，保持 `Hold` 触发模式；同时更新输入法设置文案，移除过期截图。
- **验证**：`npm.cmd test` 23/23、`npm.cmd run build`、`cargo test --workspace` 77/77、`cargo check --workspace --all-targets`、`npm.cmd run tauri:build` 与 `git diff --check` 均通过。NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.2.3_x64-setup.exe`，SHA-256 为 `0BAAC3A84F5B6CA4A666B7132A7B566B643FBEEF30A3F0E6EE4D707D826CA9EB`；实机验收待用户确认。

## 本次范围（v0.2.2，2026-08-18 追加）

遥控器预览第三次回归修复并发布 **v0.2.2**：**完全回退到 0.1.8 原版实现**。

### 遥控器预览(0.1.9~0.2.1 三次改动全部回退)

- **教训（第三次）**：为了"兼容旧引擎"给 `.remote-schematic` 加兼容层，连续三次引入回归：
  - 0.1.9：`padding-top: 20.9%` 方向错误 → 预览塌陷成 84×18 细条；
  - 0.2.0：固定 `height: 402px` → 窄窗口(`:deep` 覆盖宽度 75/78px)下比例失配 → 蓝框错位；
  - 0.2.1：`padding-top: 478.55%` → **padding 百分比相对包含块宽度(不是元素自身宽度)**，当元素宽度被 `:deep` 覆盖而包含块宽度不同时仍失配 → 蓝框仍错位。
- **用户关键信息**：0.1.8 显示正常 → 用户环境 WebView2 支持 `aspect-ratio`(Chromium 88+)。原版 `aspect-ratio: 401/1919` 相对元素自身宽度，任何宽度下恒与图片比例一致，本就正确。
- **最终修复**：`git checkout v0.1.8 -- src/components/RemoteHotspot.vue` 整文件还原(含 `.remote-product-image` 恢复 display:block + 100%)，与 v0.1.8 零差异。
- **结论**：不要为了旧引擎兼容而改动正常工作的实现，除非有该引擎的真实复现证据；`aspect-ratio` 兜底优先用 `height: 0 + padding-top`(相对元素自身宽度的替代)必须确认包含块宽度=元素宽度，或直接不兼容旧引擎。

## 验证记录（0.2.2）

| 检查 | 结果 |
| --- | --- |
| `git diff v0.1.8 -- RemoteHotspot.vue` | 无差异 |
| `cargo test` | 77/77 |
| `npm.cmd test` | 22/22 |
| `vue-tsc --noEmit` | 通过 |
| `tauri:build` | 通过 |
| Release 资产匹配 | `Nexus.Prime_0.2.2_x64-setup.exe` ✓ |
| 实机验收 | 待用户安装确认(预览与蓝框恢复正常) |

