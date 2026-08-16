# 交接记录

更新时间：2026-08-16

## 本次范围

对照上游原创仓库 [mwlt/Voice_VibeCoding](https://github.com/mwlt/Voice_VibeCoding)（v1.3.9 键盘适配）为按键录入补充特殊键与小键盘支持（纯增量，不改变本地既有行为）；修复映射列表快捷键胶囊长组合键截断遮挡问题；新增媒体键录入兜底按钮。已完成 **v0.1.8** 发布（GitHub Release + 安装包）。

## 已完成的实现

### 特殊键与小键盘键名适配（对照上游 v1.3.9）

- 上游对比结论：上游的"键盘适配"核心是键名标签表扩展（`vkDisplay.ts` + `vk_to_label`）+ 录入规则变更。本地只按"纯增量"原则移植标签部分；**未移植**上游的行为改动：主键按下即提交的录入规则、`feed_capture_key` 非阻塞喂键、`ShortcutPollSnapshot` 轮询进度、媒体键 Consumer HID 直提。原因：本地"全部抬起提交"规则有测试覆盖且刻意为之，行为改动需单独评审。
- Rust `vk_to_label` 新增：Pause、CapsLock、PrtSc、Insert、Menu、NumLock、ScrLk、Num0-9、Num\*、Num+、Num-、Num.、Num/、标点（`; = , - . / \` [ \ ] '`）、F13-F24。本地既有标签（媒体键英文、浏览器键、左右修饰键）全部保留，未照搬上游的删除。
- 前端 `src/utils/shortcut.ts` 的 `keyLabel` / `vksToHotkeyNames` 同步扩展，与 Rust 名字完全一致。
- `voice_hotkey` 配置：新键存盘使用可读名字（`numpad0`、`pause`、`capslock`、`printscreen`、`f13`、`semicolon` 等）；`name_to_vk` 保留 `vk_xx` 十六进制兼容（旧配置照常回读），f 解析从 f1-12 扩到 f1-24。
- **未动**：`KeyBindingEditor.vue` 的独立 `vkName` 表（该组件未被任何文件引用，遗留组件，若重新启用需补同步）。

涉及文件：

- `src-tauri/src/bridges/shared/shortcut_capture.rs`
- `src-tauri/src/bridges/xiaomi/key_mapping.rs`
- `src/utils/shortcut.ts`

### 映射列表快捷键自适应换行

- `KeyMappingStage.vue` 的 `.mapping-row-keycap`（映射列表每行的快捷键胶囊）：由 `white-space: nowrap + text-overflow: ellipsis + max-width: 104px` 改为 `white-space: normal + overflow-wrap: anywhere + max-width: min(200px, 100%) + line-height: 1.45`，长组合键（如"左Ctrl + 左Alt + Shift + F13"）完整换行显示，不再截断遮挡。
- 选中卡片 `.mapping-selection-card .mapping-keycap` 原本已支持换行（`overflow-wrap: anywhere`），未改。

### 媒体键录入兜底

- 背景：媒体键（音量、静音等）是 WM_APPCOMMAND 消息，系统键盘 LL 钩子收不到，无法"按真实键盘录入"；本地发送侧本就完整支持（`hid_injector.rs` consumer usage）。
- 实现：录入中且**非语音键**时显示"媒体键录不上？直接设置为：音量+/音量-/静音/应用 2"按钮；点击后先 `cancelCapture()` 结束吞键会话，再走既有 `applyCapturedKeys` 保存链路（emit save）。
- 对齐上游 `MEDIA_PICK_KEYS`，常量导出在 `src/utils/shortcut.ts`。

涉及文件：

- `src/components/KeyMappingStage.vue`
- `src/utils/shortcut.ts`

### 发布 v0.1.8

- 版本 bump：`package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` → 0.1.8；`CHANGELOG.md` 新增 [0.1.8] 条目。
- 提交 `50790c2`（feat: release v0.1.8），tag `v0.1.8` 已推送。
- GitHub Release：https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.1.8 ，唯一资产 `Nexus.Prime_0.1.8_x64-setup.exe`（与 `update.rs` 的 `expected_asset_name` 完全匹配）。
- **发布方式备忘**（gh CLI 未登录时）：用 git credential manager 中存储的 OAuth token（`gho_` 前缀，owner 的凭据）通过 `GH_TOKEN` 环境变量执行 `gh release create` / `gh release upload`。注意该 token 缺少 `read:org` scope，无法通过 `gh auth login --with-token` 登录 gh CLI，但直接以 `GH_TOKEN` 方式调用 Release API 可行。token 只走管道，勿打印进日志。

## 验证记录

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `cargo test` | 通过（73/73） | 2026-08-16；含新增 `test_extended_key_labels`、`extended_keys_round_trip`。 |
| `npm.cmd test`（vitest） | 通过（22/22） | 9 个测试文件全部通过。 |
| `vue-tsc --noEmit` | 通过 | 与 build 脚本一致的严格类型检查。 |
| `npm.cmd run tauri:build` | 通过 | NSIS exe 12.8 MB + MSI 14.9 MB。 |
| Release 资产匹配 | 通过 | `Nexus.Prime_0.1.8_x64-setup.exe` 与 `update.rs` 期望一致。 |
| 遥控器实机验收 | 未执行 | 需要连接真实小米遥控器与目标输入法。 |

## 当前阻塞

无已知阻塞。

> 注：2026-08-09 交接记录中的唯一失败项 `config::manager::tests::test_long_press_bindings_are_canonicalized_and_sanitized` 在本次全量 `cargo test` 中已通过（73/73），说明该问题已随 v0.1.7 迭代修复。

## 待完成的实机验收

1. 真实键盘录入特殊键与小键盘（CapsLock、NumLock、ScrLk、小键盘 0-9、运算键、标点、F13-F24），确认捕获与显示正确、`voice_hotkey` 保存为可读名字。
2. 媒体键兜底按钮：点击后绑定立即保存、录入状态退出、系统不再吞键。
3. 长组合键（如 左Ctrl + 左Alt + Shift + F13）在映射列表完整换行显示，窄窗口下无遮挡。
4. 语音键回归：语音快捷键绑定、触发模式（按住/点按）、输入法预设、断线重连后语音唤起——本次零改动，但建议按 2026-08-09 交接的清单回归一遍。

## 工作区注意事项

- 当前基线提交 `50790c2`（v0.1.8）；工作区已干净（HANDOFF 与 CHANGELOG 日期修正提交后）。
- 上游 `mwlt/Voice_VibeCoding` 的完整对比结论（键名表、录入规则差异、Alt+Tab 机制本地独有、上游 hanvon/T1 设备桥等）见会话记录；后续若移植更多上游功能（录入提交规则、poll 进度、`feed_capture_key`、媒体键 Consumer 直提），需单独评审，勿直接覆盖本地行为。
- `KeyBindingEditor.vue` 为未被引用的遗留组件，其 `vkName` 键名表未同步新键；若将来重新启用需补同步。
- 语音键（mic/voice）绑定、`voice_hotkey` 同步、`voice_chord_state.rs`、`session_state.rs` 本次零改动；媒体键兜底按钮在模板层用 `!isVoiceButton()` 排除语音键。
