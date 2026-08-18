# 交接记录

更新时间：2026-08-18

## 本次范围

修复两个用户反馈问题并发布 **v0.1.9**（GitHub Release + 安装包）：

1. **开机后语音键失灵**（"输入法完全不唤起、其他键正常、显示已连接、只能重启电脑恢复"）——根因是 HID Tap 注入与 ATVV 语音通道并发抢占 WUDFHost。
2. **Win10 界面兼容**（"截图日志模块丢失、所有功能模块圆角不对"）——根因是旧版 WebView2 下 `inset`/`100dvh`/`color-mix`/`aspect-ratio` 等新特性失效。

## 已完成的实现

### 开机后语音键失灵（核心修复）

- 根因：`key_log.rs` 原固定等 6 秒后无条件附着 HID Tap（注入 DLL 到 WUDFHost），若 ATVV 语音通道未订阅成功就被抢占，语音键永久无信号；重连走同一流程、重启应用注入同一 WUDFHost，只有重启电脑重置 WUDFHost 才恢复。日志证据：8-18 开机后 input_session 连续 5 次"遥控器已断开连接"重试（蓝牙慢），期间 6 秒窗口已过。
- 修复：HID Tap 附着以 ATVV 订阅成功为前提（`connect.rs` 新增 `ATVV_DIAGNOSED_FAILED` 标志）；ATVV 诊断失败时给后台重试 15 秒宽限窗口，仍失败才附着（此时蓝牙栈已稳定，竞争风险低）；30 秒硬上限兜底；ATVV 未就绪时提示"语音优先，返回/音量走系统原生键"。
- 附带：`hid_tap_runtime.rs` 的 find_host 日志降级为 debug（8-16 实测每秒一条、持续 2 小时撑爆日志，导致早期"失灵"证据轮转丢失）。
- 新增测试：`key_log.rs` 的 `tap_attach_due` 决策函数 4 个单元测试（ATVV 成功立即附着/诊断中等待/失败后 15 秒宽限/30 秒硬上限）。

### Win10 界面兼容

- 窗口尺寸自适应（`src-tauri/src/lib.rs`）：启动时按主显示器工作区（逻辑像素，留 4% 边距）clamp 默认 1080x814 与最小 880x720，超出时缩小并居中——1366x768@125%（逻辑工作区约 1093x614）下底部内容不再被裁。
- `inset:` 11 处（7 个文件）替换为 `top/right/bottom/left`（`inset` 需 Chromium 87+）——修"运行日志模块丢失"（`.log-card` 绝对定位靠 `inset: 0` 铺满，失效即塌陷）。`.log-aside` 另加 `min-height: 260px` 兜底。
- `100dvh` 2 处（InputMethodSettingsDialog）加 `100vh` 回退（Chromium 108+）。
- `color-mix()` 27 处（7 个文件）加 `rgb(var(--X-rgb) / NN%)` 回退（Chromium 65+ 支持），App.vue 亮/暗两套 `:root` 新增 `--primary-rgb/--danger-rgb/--success-rgb/--text-rgb/--text-inverse-rgb` 通道变量；`--update-accent` 配套 `--update-accent-rgb`（跟随 is-latest/is-error/is-available 状态）。
- `aspect-ratio` 1 处（RemoteHotspot 遥控器示意图）加 `padding-top: 20.9%` 兜底，子图片改 absolute 铺满。
- 非常规 `font-weight`（650/720/730/740/750/760/770/780，共 27 处）标准化为 600/700/800。
- **未改**：flex `gap` 119 处（需 Chromium 84+）——改动成本/风险高，若反馈者 WebView2 低于 84（2020-07 前，概率低），界面间距仍会塌陷，需更新 WebView2 Runtime（NSIS 安装器会触发 bootstrapper 更新）。`100vw` 5 处为低危横向溢出，未动。

涉及文件：`src-tauri/src/lib.rs`、`src-tauri/src/bridges/xiaomi/{connect,input_session,key_log,hid_tap_runtime}.rs`、`src/App.vue`、`src/views/{XiaomiSettings,GlobalSettings}.vue`、`src/components/{DeviceStatus,InputMethodSettingsDialog,KeyMappingStage,SideNav,UpdateDialog,RemoteHotspot,KeyBindingEditor}.vue`、`CHANGELOG.md`、版本 bump（package.json / Cargo.toml / tauri.conf.json → 0.1.9）。

## 验证记录

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `cargo test` | 通过（77/77） | 含新增 `tap_attach_due` 4 测试。 |
| `npm.cmd test`（vitest） | 通过（22/22） | 9 个测试文件。 |
| `vue-tsc --noEmit` | 通过 | 与 build 脚本一致。 |
| `npm.cmd run tauri:build` | 通过 | NSIS exe 13.4 MB + MSI。 |
| Release 资产匹配 | 通过 | `Nexus.Prime_0.1.9_x64-setup.exe` 与 `update.rs` 期望一致。 |
| 遥控器实机验收 | 未执行 | 需要真实遥控器（开机场景语音键、Win10 界面）。 |

## 当前阻塞

无已知阻塞。

## 待完成的实机验收

1. **语音键开机回归**：安装 0.1.9 后重启电脑，开机后直接按语音键应能唤起输入法；连续开关机 3 次；失灵时先别重启电脑，把 `%APPDATA%\com.lightyear.nexusprime\logs\app.log` 发开发者（日志已不再被 HID TAP 刷屏冲掉）。
2. **Win10 界面回归**：小屏/125% 缩放下窗口完整可见；旧 WebView2（<111）下样式回退正常；运行日志模块不再丢失。
3. 2026-08-16 交接（v0.1.8）的待验收清单（特殊键录入、媒体键兜底、长组合键换行、语音键回归）仍有效。

## 工作区注意事项

- 当前基线提交 `9d594de`（v0.1.9）；工作区已干净（HANDOFF 更新后）。
- 发布方式备忘：gh CLI 未登录时，用 git credential manager 中存储的 OAuth token（`gho_` 前缀）通过 `GH_TOKEN` 环境变量执行 `gh release create` / `gh release view`。token 只走管道（`git credential fill`），勿打印进日志。token 缺 `read:org` scope，无法 `gh auth login --with-token`。
- 上游 `mwlt/Voice_VibeCoding` 对比结论与"未移植项"清单见 2026-08-16 交接；后续移植需单独评审。
- `KeyBindingEditor.vue` 为未被引用的遗留组件（本轮顺手同步了其 `inset` 写法），若重新启用需补 `vkName` 键名表同步。
- 语音链路现状：HID Tap 附着 = ATVV 订阅成功（或失败后 15 秒宽限）；`special_key_hook`（F5 抑制）与 ATVV 解耦，独立启动。
