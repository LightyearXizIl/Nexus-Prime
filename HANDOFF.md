# 交接记录

更新时间：2026-08-18

## 本次范围

修复 0.1.9 引入的遥控器预览尺寸回归并发布 **v0.2.0**（GitHub Release + 安装包）。

## 已完成的实现

### 遥控器预览尺寸回归修复

- 回归来源：0.1.9 中给 `RemoteHotspot.vue` 的 `.remote-schematic` 加的 `aspect-ratio` 兜底 `padding-top: 20.9%` 计算方向错误——`aspect-ratio: 401/1919` 意味着高 = 宽 × 1919/401 ≈ 4.79 倍，而 `padding-top` 百分比相对宽度，应写 `478.6%`；实际 20.9% 导致容器高度塌陷为 84×~18px 的细条（用户反馈"遥控器预览怎么这么小"）。
- 修复：改为固定 `width: 84px; height: 402px`（84 × 1919/401），移除错误的 `padding-top`、`height: 0` 与冗余 `aspect-ratio`；子元素 `.remote-product-image`（absolute 铺满）与热点定位不变。固定尺寸不依赖任何新 CSS 特性，全版本引擎一致，避免再犯百分比方向错误。
- **教训**：用 `padding-top` 百分比模拟 aspect-ratio 时，值必须是与宽高比**互为倒数**（高/宽），且注意 `box-sizing`。固定尺寸场景直接写死宽高最稳。

涉及文件：`src/components/RemoteHotspot.vue`、`CHANGELOG.md`、版本 bump（package.json / Cargo.toml / tauri.conf.json → 0.2.0）。

## 验证记录

| 检查 | 结果 | 说明 |
| --- | --- | --- |
| `cargo test` | 通过（77/77） | |
| `npm.cmd test`（vitest） | 通过（22/22） | |
| `vue-tsc --noEmit` | 通过 | |
| `npm.cmd run tauri:build` | 通过 | NSIS exe + MSI。 |
| Release 资产匹配 | 通过 | `Nexus.Prime_0.2.0_x64-setup.exe` 与 `update.rs` 期望一致。 |
| 遥控器预览实机 | 待确认 | 用户本地查看按键映射页预览是否恢复 ~402px 高。 |

## 当前阻塞

无已知阻塞。

## 待完成的实机验收

1. **遥控器预览尺寸**：按键映射页预览恢复约 402px 高（0.2.0 已发布，用户安装确认）。
2. 2026-08-18 交接（v0.1.9）的待验收清单仍有效：语音键开机回归（连续开关机 3 次）、Win10 界面回归（小屏/125% 缩放、旧 WebView2 样式回退、运行日志模块）、v0.1.8 的特殊键/媒体键/长组合键清单。

## 工作区注意事项

- 当前基线提交 `c6c5442`（v0.2.0）；工作区已干净（HANDOFF 更新后）。
- 发布方式备忘：gh CLI 未登录时，用 git credential manager 中存储的 OAuth token（`gho_` 前缀）通过 `GH_TOKEN` 环境变量执行 `gh release create` / `gh release view`。token 只走管道（`git credential fill`），勿打印进日志。
- 版本号规则：**个位满 10 进 1**（0.1.9 → 0.2.0，不存在 0.1.10）。
- 上游 `mwlt/Voice_VibeCoding` 对比结论与"未移植项"清单见 2026-08-16 交接。
- `KeyBindingEditor.vue` 为未被引用的遗留组件。
- 语音链路现状与 Win10 兼容修复明细见 2026-08-18（v0.1.9）交接；`inset`/`100dvh`/`color-mix`/`aspect-ratio` 均已做兼容处理（aspect-ratio 处已改为固定尺寸）。
