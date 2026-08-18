# 交接记录

更新时间：2026-08-18

## 本次范围（v0.2.1，2026-08-18 追加）

修复 0.2.0 引入的遥控器预览热点错位，并完成全面 UI 布局审计与功能自检，发布 **v0.2.1**。

### 遥控器预览热点错位（0.2.0 回归，二次修复）

- 0.2.0 把 `aspect-ratio` 换成固定 `height: 402px`，但 KeyMappingStage 用 `:deep(.remote-schematic)` 覆盖宽度（默认 82px、≤880px 时 75px、≤760px 时 78px），宽度变化时高度不联动 → 容器比例与图片 401:1919 失配 → `object-fit: contain` 上下留白（75px 宽时约 ±21px）→ 热点错位。
- 修复：`height: 0; padding-top: 478.55%; aspect-ratio: 401/1919`——padding-top 百分比相对宽度，高度自动跟随外部覆盖的宽度（84/82/75/78px → ≈402/392/359/373px），与图片 contain 一致、无留白。新旧引擎行为一致（height:0 显式时 aspect-ratio 不参与，由 padding 撑起）。
- **教训（第二次）**：aspect-ratio 兼容兜底必须与"可能被外部覆盖的维度"联动，固定值只在单维度不变时安全。

### 全面 UI 审计与功能自检（0.2.1）

- 主题变量完整性：62 个 var() 引用全部有效（`--accent` 带 fallback；暗色缺的 `--radius/--shadow/--ease-out` 回退到 `:root` 亮色值，仍生效）。
- `:deep()` 覆盖联动：全项目 4 处，仅 RemoteHotspot 有尺寸联动风险（已修），其余为对齐/宽度覆盖安全。
- 窄窗口（880px 及自适应后更小）布局：媒体查询断点 900/1019（SideNav）、840/760（XiaomiSettings）可正常触发，`minmax(0, 1fr)` 防 grid 溢出。
- 弹窗：IME 弹窗 `100dvh` 已加 `vh` 回退；release-notes/IME 列表 `overflow-y: auto` + `max-height`。
- 滚动：`.main-content { overflow-y: auto }`，body 不滚，无双滚动条。
- 命令对齐：前端 18 个 `invoke` 命令全部在后端 `invoke_handler` 注册。
- 测试：`cargo test` 77/77、vitest 22/22、`vue-tsc` 通过。

## 验证记录（0.2.1）

| 检查 | 结果 |
| --- | --- |
| `cargo test` | 通过（77/77） |
| `npm.cmd test` | 通过（22/22） |
| `vue-tsc --noEmit` | 通过 |
| `tauri:build` | 通过（NSIS + MSI） |
| Release 资产匹配 | `Nexus.Prime_0.2.1_x64-setup.exe` ✓ |
| 实机验收 | 待用户安装确认（预览热点对齐、窄窗口） |

