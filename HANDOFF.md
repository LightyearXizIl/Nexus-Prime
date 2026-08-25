# 交接记录

更新时间：2026-08-25

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

