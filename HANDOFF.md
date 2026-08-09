# 交接记录

更新时间：2026-08-09

## 本次范围

修复小米遥控器语音键在断线、重连或重启桥接后偶发失效，以及异常情况下 Ctrl、Win 等修饰键可能残留的问题；并在“小米设置 → 输入法设置”中加入千问输入法语音预设。

本次未修改版本号、未生成安装包、未发布 Release。

## 已完成的实现

### 语音输入会话与重连

- 将“桥接 worker 正在运行”与“当前设备输入会话有效”分离；连接会话使用递增令牌，过期回调不能结束新会话。
- 断线、停止桥接、手动重启、托盘断开、应用退出和订阅清理都会先取消当前会话，等待 GATT、VK 轮询和 Raw Input 工作线程退出后才允许建立下一会话。
- 手动启动与重启通过同一生命周期锁串行化，避免连续点击产生多个连接循环或多组 ATVV 重试。
- ATVV 改为每会话单路订阅；首次订阅完成后再启动 HID Tap。遇到 `AccessDenied` 时暂停 HID Tap，执行一次受控重订阅，成功后恢复。

涉及文件：

- `src-tauri/src/bridges/xiaomi/connect.rs`
- `src-tauri/src/bridges/xiaomi/session_state.rs`
- `src-tauri/src/bridges/xiaomi/key_log.rs`
- `src-tauri/src/bridges/xiaomi/input_session.rs`
- `src-tauri/src/bridges/xiaomi/raw_mapping.rs`
- `src-tauri/src/ipc/commands.rs`
- `src-tauri/src/ipc/tray.rs`
- `src-tauri/src/lib.rs`

### 语音快捷键安全释放

- 用实际已按下的虚拟键列表取代单一的“语音键已按住”布尔状态；释放时使用按下当时保存的组合，不读取可能已变更的配置。
- 所有清理路径均反向释放保存的组合、取消手势定时器、清除 F5 抑制状态、停止语音会话和电平状态。
- `SendInput` 现在检查实际发送数量：按下部分失败立即补偿释放；抬键失败只进行一次有限重试并记录错误。
- 新增平台无关的组合键状态机测试，覆盖正常按下/松开、断线强制松开、配置变化、部分注入失败和重复松开的幂等性。

涉及文件：

- `src-tauri/src/bridges/xiaomi/key_mapping.rs`
- `src-tauri/src/bridges/xiaomi/voice_chord_state.rs`
- `src-tauri/examples/voice_chord_state_check.rs`

### 千问输入法预设

“小米设置 → 输入法设置”已新增“千问输入法”卡片和“快速设置语音键映射为：右 Alt”按钮。

- 快速设置会同时写入 `mic`、`voice`、`voice_hotkey`，并启用“按住”触发模式。
- 千问输入法 Windows 默认按住右 Alt 录音、松开上屏；若用户在千问中修改了快捷键，需要在 Nexus Prime 的按键映射中录入同一组合。
- 右 Alt 为 `VK_RMENU (0xA5)`，现有 `SendInput` 路径已将其作为扩展键处理。

涉及文件：

- `src/views/XiaomiSettings.vue`

### 电池充电动效

- 后端在原有电量百分比外，读取标准 BLE Battery Service 1.1 的可选 `Battery Level Status` 特征（`0x2B05`）。只有该特征明确报告“充电中”时，才将充电状态传给前端；不根据电量上升或下降推测充电状态。
- 电池图标不增加文字。充电中时，图标内显示闪电，并以从左到右的流光表达正在充电；启用“减少动态效果”时保留静态闪电与柔和高光。
- 若遥控器仅提供旧版 `Battery Level`（`0x2A19`）而未提供充电状态，则继续显示原有绿色电量图标。此前日志只确认了 `0x2A19` 电量百分比，尚未从真机确认 `0x2B05` 是否可用。

涉及文件：

- `src-tauri/src/bridges/mod.rs`
- `src-tauri/src/bridges/xiaomi/input_session.rs`
- `src/types/index.ts`
- `src/stores/bridge.ts`
- `src/views/XiaomiSettings.vue`

## 验证记录

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `npm.cmd run build` | 通过 | 2026-08-09；包含 Vue 类型检查和 Vite 生产构建。 |
| `git diff --check` | 通过 | 语音修复与千问预设改动提交前无空白错误。 |
| `cargo check --workspace` | 通过 | 在语音生命周期改动完成后执行。 |
| `cargo test --example voice_chord_state_check` | 曾通过（5/5） | 覆盖新的组合键状态机；同样早于后述 `build.rs` 变更。 |
| `cargo test --lib bridges::xiaomi::input_session::tests` | 通过（6/6） | 2026-08-09；包括 Battery Level Status 充电位解析和截断数据拒绝测试。 |
| `cargo test --workspace` | 65 通过、1 失败、1 忽略 | 2026-08-09；测试可正常启动，唯一失败项详见“当前阻塞”。 |
| 遥控器实机验收 | 未执行 | 需要连接真实小米遥控器、VB-CABLE 与目标输入法后完成。 |

## 当前阻塞

2026-08-09，`cargo test --workspace` 已能完整启动；此前的 `TaskDialogIndirect` 入口点缺失和重复资源链接错误没有复现。

当前唯一失败项为 `config::manager::tests::test_long_press_bindings_are_canonicalized_and_sanitized`：`src-tauri/src/config/manager.rs:581` 断言期望 1，实际为 2。本次未修改该文件，需由后续处理人单独确认期望的长按映射去重规则；在此之前不能把完整 Rust 测试标记为通过。

修复后依次运行：

```powershell
cd src-tauri
cargo test --workspace
cargo test --example voice_chord_state_check
cd ..
npm.cmd run build
git diff --check
```

## 待完成的实机验收

1. 冷启动后分别短按、长按语音键各 20 次，确认每次松开后都能再次唤起。
2. 按住语音键时关闭遥控器或制造断线，确认日志出现强制抬键，且 Ctrl、Win、Shift、Alt 没有残留。
3. 连续完成至少 10 次休眠、断线和自动重连，确认日志没有并行会话或交错的多组 `attempt=0`。
4. 连续点击“重启按键桥接”，确认只有一个重启流程。
5. 首次制造 ATVV `AccessDenied` 后，确认单一恢复流程能够恢复，且不会无限重试。
6. 在千问输入法中完成一次“快速设置右 Alt”，连续按住/松开遥控器语音键，确认文字能重复上屏；若千问快捷键已被自定义，改用相同的自定义映射测试。

## 工作区注意事项

- 当前基线提交为 `34dd71d feat: release v0.1.2`。本次未提交的文件仅为电池充电状态/动效相关的 5 个源码文件和本文档。
- 语音键生命周期与千问输入法预设已包含在当前基线中；提交或回退本次电池改动时，仍需按功能审阅。
- 不要使用 `git reset --hard`、`git checkout --` 等方式清理当前工作区，以免丢失本次语音修复内容。
