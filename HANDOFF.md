# 交接记录

更新时间：2026-09-21

## v0.4.7：ATVV 会话恢复与 Shift+Enter 注入路由（2026-09-21）

- PR #12 已承接并保留 SummerSec 在原 PR #11 中的 ATVV 恢复贡献：`MIC_OPEN` 看门狗现在带连接会话代号和 `Idle / MicOpenPending / Active` 状态。`AUDIO_START` 或首个有效 PCM 都经过同一启动逻辑且仅能执行一次；800ms 内没有启动事件时结束当前输入会话并复用自动重连，同时清理语音组合键、PCM 和 F5 抑制。旧连接回调不能影响后续会话；日志按会话标注启动来源、耗时及超时/重连原因。
- PR #13 独立修复 Issue #9：普通映射内任意含 Enter 的组合（通用、左、右 Shift+Enter）跳过 WinUHid，走带 `EXTRA_INFO` 的 SendInput。语音快捷键的 WinUHid 专用路径、单 Enter/方向/音量现有语义和 Alt+Tab 分流未改；`VK observe ... (no map)` 仍只是诊断。
- 自动化：Gadget 10/10、前端 60/60、Rust 150 通过且 2 项环境 smoke ignored；前端生产构建、Cargo workspace 全目标检查、Tauri release 与 `git diff --check` 通过。本地唯一正式 NSIS 资产为 `Nexus.Prime_0.4.7_x64-setup.exe`，13,368,240 bytes，SHA-256 `3A78679EABCF9A3984190A25090FC734AC025B72A8656D11158E2E3FFFE2A716`，安装包产品/文件版本均为 `0.4.7`。远端标签、Release、清单与公开下载复核将在上传后记录。
- 真机验收仍待执行：Home、菜单各绑定三种 Shift+Enter 并与语音键交替至少 30 轮；RC003 语音启停至少 50 轮，空闲 10 分钟后复测；并回归方向、确认、主页、菜单、音量、返回和微信/千问/豆包预设。若未再遇到 ATVV 丢失，只能记录状态机模拟通过，不能宣称现场故障已复现。

## v0.4.6：虚拟键盘蓝屏、普通按键与声卡全自动修复（2026-09-12）

- 用户日志与 Windows 本机证据确认：20:12:35 点击“修复虚拟键盘”后，旧实现以 `force=true` 进入 WinUHid `DIF_INSTALLDEVICE` 重绑；20:13:13 Windows 记录 `0x000000D1 DRIVER_IRQL_NOT_LESS_OR_EQUAL` 并生成 `C:\Windows\Minidump\091226-9953-01.dmp`。WER 故障桶为 `AV_vbaudio_cable64_win10!unknown_function`。9 月 11 日的另一次同类转储也是相同故障桶，因此这是 VB-CABLE 内核驱动在该 PnP 重扫链上的可重复崩溃，不是普通应用重启，也不能仅凭点击来源归因给 WinUHid。
- 虚拟键盘修复已改为安全幂等：应用层先刷新用户态 DLL/注入器并检查 WinUHid；设备已就绪时直接返回“无需重装”，不提升权限、不重绑驱动。PowerShell 安装脚本在普通入口和提升权限入口再次检查 `\\.\WinUHid`，即使旧调用方仍传 `-Force`，设备可访问时也会跳过 `DIF_INSTALLDEVICE` 与 `/scan-devices`。
- “除语音键外普通键全部失灵”是另一条证据链：升级后的日志持续停在 `XIAOMI HID TAP ATTACHED ... awaiting_io=true`，没有 `HID TAP READY`，但启动逻辑只按“Tap 线程已启动”就跳过 Raw Input 备用路由；方向、确认、主页等只能被 VK 诊断记录为 `(no map)`。语音键独立走 ATVV，所以仍可用。历史日志显示 9 月 11 日加载新版 Gadget 之前能进入 `HID TAP READY`，之后没有成功记录。
- Gadget 已恢复升级前验证过的同步成功读取规则：`NtDeviceIoControlFile` 立即返回成功且调用方请求恰为 9 字节时直接转发报告，不再强制依赖此刻可能仍旧的 `IO_STATUS_BLOCK.Information`；异步 `STATUS_PENDING` 路径仍要求完成状态与内核报告的 9 字节长度。Rust hub 同时记录 `pending/completed/failed` 遥测变化，便于下一次真机日志区分“没有命中 IOCTL”和“命中但长度/完成状态异常”。
- VB-CABLE“自动修复”改为一次 UAC 确认后的全自动流程：管理员助手再次校验官方 x64/x86 Setup 的固定 SHA-256，只识别并点击 `Install Driver` 和包含明确成功/失败文字的结果确认按钮，随后自动探测端点并校正默认麦克风。只看到 `Remove Driver` 时安全停止并提示重启，绝不自动卸载或覆盖现有驱动；未采用官方文档没有确认的静默参数。
- 自动化：Gadget 10/10、前端 60/60、Rust 146 通过且 2 项环境 smoke ignored；前端生产构建、Cargo workspace 全目标检查、Tauri release 与 NSIS/MSI 打包、`git diff --check` 通过。另在当前 WinUHid 正常的机器上，用带 `-Force` 的修复脚本实测得到 `already reachable; skip live driver rebind`、退出码 0，未新增系统错误事件。
- 本地 NSIS 包：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.4.6_x64-setup.exe`，13,367,506 bytes，SHA-256 `4485EFD12C7493E80450CCC0BAED77B60206DB6615DD2C4AF293C7BCE5C8B1FD`；主程序与安装包产品版本均为 `0.4.6`，均未签名。发布源提交/注释标签为 `b04567095b4b458bbc5ab945a2c27c31b99939de` / `v0.4.6`，远端 `main` 与解引用标签均指向该提交。
- GitHub 正式 Release [`v0.4.6`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.4.6) 已公开且不是草稿或预发布；安装包与 `latest.json` 均为 `uploaded`。Release 资产、Release 清单和 `main/latest.json` 的安装包大小与摘要一致；另从公开下载地址重新下载安装包后复算，仍为 13,367,506 bytes 和同一 SHA-256。
- 仍待真实 RC003 验收：普通方向/确认/主页/菜单/音量/返回至少各 10 次，日志应出现 `HID TAP READY` 与实际 `HID TAP key=...`；再验证语音键和自定义映射无回归。VB-CABLE 自动化已通过代码与状态机测试，但未在本轮对当前机器重新安装驱动，以免把发布构建测试和真实驱动生命周期混为一谈；不得为了验证防护而再次在已发布 v0.4.5 中点击“修复虚拟键盘”。

## v0.4.5：千问四种语音快捷键（2026-09-12）

- 千问输入法设置弹窗现可独立应用左 Ctrl、左 Ctrl + 左 Win、左 Win + 左 Alt、右 Alt 四种按住说话快捷键；不会读取或修改千问输入法自身配置，用户需手动选择两端一致的组合。
- 旧 `qianwen` profile 仍是右 Alt，以保证已有配置兼容；其余三项使用新的持久化 profile。四种千问 profile 都优先使用 WinUHid、只清理已确认且不属于目标组合的遥控器 Ctrl/Win 泄漏，并在松手时直接释放原组合，不注入 F24、Enter 或 Space。
- 自动化：Gadget 9/9、前端 60/60、Rust 143 通过；前端生产构建、Cargo workspace 全目标检查与 NSIS 打包通过。RC003/WUDFHost 与 ChatGPT 桌面版两个既有环境 smoke 为 ignored。
- 本地 NSIS 包：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.4.5_x64-setup.exe`，13,360,573 bytes，SHA-256 `60B604377A23BB16A1EF7E71EBC947178E14EA773D26361B4E20EF1201FC9B5E`；应用与安装包产品版本均为 `0.4.5`。
- 发布源提交/标签：`31075a378a596da0c1589948d44575c8e32eb1d4` / `v0.4.5`。GitHub 正式 Release [`v0.4.5`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.4.5) 已公开；资产 `Nexus.Prime_0.4.5_x64-setup.exe` 状态为 `uploaded`、大小 13,360,573 bytes、摘要 `sha256:60b604377a23bb16a1ef7e71ebc947178e14ea773d26361b4e20ef1201fc9b5e`，与本地和 GitHub CLI 下载复算结果一致。Release `latest.json` 与 `main/latest.json` 均已读取并确认包含同一安装包元数据。
- 尚未在真实遥控器 + 千问输入法上逐项完成 10 次连续按压、自动上屏、无开始菜单/系统菜单、无 F5 串入和无粘键的验收。

## v0.4.4：虚拟声卡弹窗与输入法设置统一（2026-09-11）

- 虚拟声卡修复弹窗已采用输入法设置弹窗相同的标题区、居中胶囊页签、右上角关闭按钮、双栏面板、主次按钮、主题变量和窄屏响应式布局；修正关闭按钮不再继承全宽操作按钮样式，固定为 `36px` 图标按钮。未改动 VB-CABLE 安装、下载、取消或端点校正流程。
- 自动化：Gadget 9/9、前端 55/55、前端生产构建和 `git diff --check` 通过。发布前仍不把真实 Windows 驱动、UAC、重复设备或遥控器验收表述为已完成。
- 覆盖发布后的本地 NSIS 包：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.4.4_x64-setup.exe`，13,356,601 bytes，SHA-256 `7E56747B1FED38F1D835DD61BE2D05D6442ACBFC8F62EC1555FB33805B7BC2EB`；应用与安装包产品版本均为 `0.4.4`。覆盖发布提交/标签为 `fbf0b02e937375d51e9e70c376d117f38bc6ce18` / `v0.4.4`。
- GitHub 正式 Release [`v0.4.4`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.4.4) 已覆盖。资产 `Nexus.Prime_0.4.4_x64-setup.exe` 状态为 `uploaded`，大小 13,356,601 bytes，GitHub 摘要 `sha256:7e56747b1fed38f1d835dd61be2d05d6442acbfc8f62ec1555fb33805b7bc2eb`，与本地一致；Release `latest.json` 与 `main/latest.json` 均已读取并确认包含覆盖后的 v0.4.4 安装包元数据。

## v0.4.3：VB-CABLE 官方安装器修复（2026-09-11）

- 选择性同步 `mwlt/Voice_VibeCoding@679e654` 的虚拟声卡安装行为：内嵌 Pack45 ZIP 保留固定 SHA-256 校验，解压至 `%LOCALAPPDATA%\\Nexus Prime\\VB-CABLE` 下的带散列标记暂存目录；只有临时内容已找到完整官方 Setup 后才替换旧暂存，失败不破坏可复用的有效暂存。
- 正常应用流程改为 `ShellExecuteExW` 启动 `VBCABLE_Setup_x64.exe` 的官方图形安装器并等待结束，不弹出 PowerShell 黑框。并发安装返回可重试结果；UAC 取消、普通退出、`3010` / `1641` 和 10 秒内端点仍未就绪均提供明确结果。CABLE Input/Output 就绪后，PowerShell 新增的 `EnsureMic` 模式只负责将默认麦克风校正为 CABLE Output；历史静默安装模式仅保留手工/RunOnce 兼容。
- 首页“修复声卡”不再立即执行，改为项目原有主题变量下的“修复 / 安装说明 / 常见问题”页签弹窗；自动修复与内置官方驱动共享同一官方安装器路径，下载、取消和官网入口仍可用。
- 自动化：Gadget 9/9、前端 55/55、Rust 142 通过，另有 RC003/WUDFHost 和 ChatGPT 桌面版两个既有环境依赖 smoke ignored；前端生产构建、Cargo workspace 全目标检查和 `git diff --check` 通过。
- 本地 NSIS 包已重建：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.4.3_x64-setup.exe`，13,360,977 bytes，SHA-256 `FA2A0AA94C3910C7F21BF88617E1B506EF3F4D05BDF2F835CE1E1496710418B9`；应用与安装包产品版本均为 `0.4.3`。发布提交/标签为 `d6d9623d4d8c85b82994c1ced2d2267adaaa4242` / `v0.4.3`。
- GitHub 正式 Release [`v0.4.3`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.4.3) 已公开。资产 `Nexus.Prime_0.4.3_x64-setup.exe` 状态为 `uploaded`，大小 13,360,977 bytes，GitHub 摘要 `sha256:fa2a0aa94c3910c7f21bf88617e1b506ef3f4d05bdf2f835ce1e1496710418b9`，与本地一致；Release `latest.json` 与 `main/latest.json` 均已读取并确认包含 v0.4.3 安装包元数据。
- 发布前仍待手工验收：未安装、损坏安装、重复设备和已安装状态；官方 GUI/UAC、取消、重启后的 CABLE Input/Output 与 CABLE Output 默认麦克风校正。不得把自动化结果表述为上述 Windows 驱动场景已完成验收。

## Issue #6：RC003 TV 键原生字符穿透修复（2026-09-11）

- 根因已落实：RC003 TV 键的 HID Usage `0x35` 被 Windows 同时翻译为 `VK_OEM_3`，因此英文输入框得到反引号，中文输入法得到间隔号；清空应用内映射只能停止二次动作，不能阻止遥控器原始键。
- 新增内部 `tv_native_guard`：低级键盘钩子收到 `VK_OEM_3 + scan 0x29` 时先非阻塞缓存，最多等待 120ms。HID Tap 的 TV DOWN 在配置、TV gate 和 `KeyAction::None` 前立即确认遥控器来源；确认后丢弃整次原始按压，超时则用带 `EXTRA_INFO` 的 SendInput 按原顺序回放实体键盘事件。队列满、钩子停止、断连和异常均采用 fail-open 回放，避免丢键或粘键。
- TV 的原生键抑制不再依赖是否存在单击、连击或长按映射；语音 F5、方向、菜单、音量、ATVV、WinUHid、配置格式和公开命令均未改动。发布版本已同步为 `0.4.2`。
- 本地 NSIS 安装包已重新构建：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.4.2_x64-setup.exe`，13,350,889 bytes，SHA-256 `E1430D75D52C7EB5C6BDCD4432076018943E1A6A18E1FFC95176084E86C587BB`；主程序与安装包产品版本均为 `0.4.2`。发布提交/标签为 `12bf889f8fe2fd71781c12a23a569c428f7a2688` / `v0.4.2`。
- GitHub 正式 Release [`v0.4.2`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.4.2) 已公开。资产 `Nexus.Prime_0.4.2_x64-setup.exe` 状态为 `uploaded`，大小 13,350,889 bytes，GitHub 摘要 `sha256:e1430d75d52c7eb5c6bdcd4432076018943e1a6a18e1ffc95176084e86c587bb`，与本地一致；Release `latest.json` 和 `main/latest.json` 均可读取，公开安装包下载返回 HTTP 200 与相同长度。

### 本次验证

- `npm.cmd test`：Gadget 9/9、前端 13 个文件 52/52 通过。
- `npm.cmd run build`、`cargo test --workspace --manifest-path src-tauri/Cargo.toml`、`cargo check --workspace --all-targets --manifest-path src-tauri/Cargo.toml` 和 `git diff --check`：通过；Rust 为 140 通过、2 项按环境要求 ignored（RC003/WUDFHost 与 ChatGPT 桌面版）。
- 新增状态机覆盖候选事件先到、TV 信号先到、短按回放、迟到信号、重复按键、队列满和远端释放超时清理；自动化证明逻辑，不代替真实硬件验收。

### 仍待真实设备验收

- 在记事本和聊天输入框中，分别使用中英文输入法连续按 RC003 TV 键 20 次；默认映射、自定义映射和“未绑定”三种状态均不得出现「·」或 `` ` ``，且设置的 TV 动作只能执行一次。GitHub Issue #6 当前已关闭，但这项真机验收仍未完成；若复现，须重新打开该议题并附上日志。
- 遥控器桥接活动时验证实体键盘 `` ` ``、Shift+`` ` ``、快速连按和长按可用且无粘键；允许首次按下最多 120ms 的判定延迟。
- 验证重启按键桥接、蓝牙断连重连后仍满足上述行为，并回归方向、确认、菜单、音量和语音键。真机验收完成前，不应仅以 GitHub Issue #6 已关闭作为硬件问题完全修复的依据。

## 全项目复核（2026-09-11）

- 随后合并 PR #7、#8 的功能整合已在本地完成，发布版本仍保持 `0.4.1`；未采用 PR 中的 `0.4.2` 或 `0.4.3`。
- 合并后的验证：Gadget 测试 9/9、前端 52/52、Rust 133/133 通过；2 个需要真实 RC003/ChatGPT 环境的测试按设计 ignored；前端生产构建、Cargo 全目标检查和 `git diff --check` 通过。未执行新的 Tauri 安装包发布，不能据此宣称新安装包或远程 Release 已更新。

- 本次以 `E:\Vibe coding\Nexus Prime\Nexus Prime-repo` 为交接与后续开发基准。实施本修复前的同步基线为 `c338057`（`origin/main`），版本号为 `0.4.1`；外层目录中的 `Nexus Prime` 副本仍不作为 Git 写入目标，原因是历史上出现过 `fatal: bad object HEAD`。
- 已复核项目结构：Vue/Vite 前端位于 `src/`（29 个 TypeScript、11 个 Vue 文件），Tauri/Rust 后端位于 `src-tauri/src/`（46 个 Rust 文件），核心模块覆盖配置管理、日志、更新器、BLE/UDP 遥控器桥接、HID/WinUHid 注入、按键映射、ATVV 音频、VB-CABLE、托盘与自启动；`src-tauri/assets/` 保存 WinUHid 和音频相关安装资源，`public/` 与 `src-tauri/icons/` 保存网页、桌面及移动端图标资源。
- 已对 `README.md`、`CHANGELOG.md`、`CONTRIBUTING.md`、`THIRD_PARTY_NOTICES.md`、`package.json`、Cargo 配置及当前源代码进行交叉检查。当前文档最新发布范围均为 v0.4.1；没有发现 v0.4.1 之后的代码提交或未记录的 Git 改动。

### 本次自动化复核

- `npm.cmd test -- --run`：13 个测试文件、50/50 通过。
- `npm.cmd run build`：`vue-tsc --noEmit` 与 Vite 生产构建通过。
- `cargo test --workspace --manifest-path src-tauri/Cargo.toml`：130/130 通过。
- `cargo check --workspace --all-targets --manifest-path src-tauri/Cargo.toml`：通过。
- `git diff --check`：通过。
- 本次未重新执行 `npm.cmd run tauri:build`，因此不新增或改写安装包、大小、SHA-256、Release 或 `latest.json` 结论；这些仍以 v0.4.1 发布记录为准。

### 当前交接边界

- v0.4.1 的本地构建包和 GitHub Release 已在下方原记录中完成校验；本次复核未修改真实用户配置、输入法设置、驱动安装状态或远程发布内容。
- v0.4.1 仍待真实设备验收：Alt+F4 录入与执行、Alt+Tab/Alt+Space/Alt+Esc 系统行为、Alt+字母路径，以及“一键重置”范围和保留项；未完成前不要宣称这些场景已通过真机验收。
- 后续代码或发布工作应继续在 `Nexus Prime-repo` 完成，并在交接文档中分别记录自动化、打包、远程 Release 和真实设备证据。

## 本次范围（v0.4.1，2026-09-07）

- 按键映射列表新增“一键重置”。它只恢复电源、方向、确认、返回、主页、菜单和 TV 的单击默认映射，并清除这些按键的双击、三击、四击、长按和鼠标动作；音量加减、静音、语音键、语音快捷键/预设、按键名称、连击间隔、蓝牙和音频设置均保留。后端命令 `reset_xiaomi_standard_key_bindings` 从最新配置执行最小范围原子保存，不用整份默认配置覆盖用户设置。
- 重置按钮旁新增与首页同样的悬浮说明图标：悬停、焦点或点击可查看“会重置什么 / 有什么影响 / 会保留什么”；Esc 可关闭，离开按钮和浮层 120ms 后关闭。重置不提供撤销，风险文案明确提示例如电源双击的 Alt+F4 会被清除。
- 所有映射槽位均可打开“手动组合”编辑器。支持直接输入 `Alt+F4` 等文本或选择控件，支持左右修饰键、F1–F24、导航、媒体和小键盘键；错误文本不会覆盖旧绑定，已保存未知键继续以 `VK_0xNN` 显示并保留。
- 快捷键录入增加会话 ID：开始、事件、轮询和停止只接受同一会话，迟到事件或旧会话取消不会写入当前选中的按键。日志带会话 ID；前端不再接受无关录入结果。
- 普通映射的 Alt+F4、Alt+Tab、Alt+Space 和 Alt+Esc 改走 Windows 系统 `SendInput` 路径，避免旧的窗口消息路径绕过系统行为；其他 Alt+字母仍沿用原前台窗口消息路径，语音专用注入未改。

### 当前验证

- 前端测试 13 个文件、50/50 通过；包含文本 `Alt+F4` 解析、无效文本拦截和重置风险提示/后端返回配置。
- Rust workspace 测试 130/130 通过；覆盖标准按键重置保留音量与语音、Alt 系统组合路由及既有语音回归。前端生产构建、Cargo workspace 全目标检查与 `git diff --check` 均通过。
- 本地 NSIS 安装包：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.4.1_x64-setup.exe`，13,339,714 bytes，SHA-256 `45146B9B7E30D4369BA4F762A4B63DDC09D2E6C39C063512F91A04F9C8CFC91F`；主程序文件与产品版本均为 `0.4.1`。未修改真实用户配置。
- 发布提交/标签为 `05e36b24135e6a83604f2528cc96eef654425b76` / `v0.4.1`。GitHub 正式 Release [`v0.4.1`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.4.1) 已公开；资产 `Nexus.Prime_0.4.1_x64-setup.exe` 状态为 `uploaded`、大小 13,339,714 bytes、GitHub 摘要 `sha256:45146b9b7e30d4369ba4f762a4b63ddc09d2e6c39c063512f91a04f9c8cfc91f`，与本地一致。Release `latest.json` 已上传，仓库 raw 清单为 v0.4.1；公开安装器下载返回 HTTP 200 与相同长度。

### 仍待真实设备验收

- 在隔离配置中录入 Alt+F4，确认录入窗口不会关闭；用遥控器电源键双击验证前台测试窗口只关闭一次且无粘键。
- 验证 Alt+Tab、Alt+Space、Alt+Esc 和 Alt+F4 的系统行为；确认 Alt+字母仍发送到前台应用。
- 点击一键重置后，核对普通按键默认值和所有手势清除，同时确认音量、静音、语音、输入法预设及蓝牙/音频设置未改变。

## 本次范围（v0.4.0，2026-09-04）

- 本机复现证据：微信输入法 2.1.3.18 的“启动语音输入”仍为左 Ctrl + 左 Win、“按住说话”仍为左 Ctrl + 左 Shift + D，两个开关均已启用；千问输入法配置仍启用右 Alt 唤醒。因此失效不是输入法快捷键设置被改动。
- v0.3.9 日志确认微信启动模式在同一次遥控器按压中先记录 `start-voice tap`，松开又记录 `start-voice stop tap`。第二次 Ctrl+Win 会立即关闭刚打开的微信语音面板；v0.4.0 用 `VoicePressSession` 统一记录 Tap/Hold、原始组合键、预设和实际注入路由，UP 不会再次发微信 Ctrl+Win。
- 选择性参考 `mwlt/Voice_VibeCoding` 提交 `c76056d597c9fb2e504f79313a8cbffd8d648535`：语音 DOWN 固定为“认领边沿 → F5 周期 → 确保/置顶 LL 钩子（最多 8ms）→ 等待固件前置信号最多 80ms、关闭捕获并受控修复已捕获 Ctrl/Win → 锁定注入后端 → 快捷键 DOWN → PCM/界面”。未引入其 F5+微信映射或界面方案；MIT 来源已记入 `THIRD_PARTY_NOTICES.md`。
- F5 改为完整配对状态：关联窗 120ms、前置信号等待最多 80ms、松开尾窗 3 秒、sticky 空闲上限 10 秒。若 F5 DOWN 已放行至系统，对应 UP 必须放行；固件 Ctrl/Win 只在钩子实际捕获的语音周期内补 KEYUP。捕获在 WinUHid 注入前关闭，避免虚拟 Ctrl/Win 被误吞；不再采用旧的 20ms 收尾等待或全局异步键态探测。
- 千问和豆包预设在右 Alt / 右 Alt + 空格注入前，额外检查本次已确认 F5 周期中仍按下的固件左 Ctrl/左 Win 并只对这两个已知泄漏键发送 KEYUP，避免输入法收到错误的 Ctrl+Win+Alt 组合。微信两条路径不调用该清理。
- WinUHid 在 DOWN 前优先选定并在 Hold 会话中锁定；只有不可用才降级 SendInput。千问右 Alt 的降级路径会明确提示修复虚拟键盘，不把软件模拟键成功当作千问已唤醒。
- 版本号按“末位满十进一”规则由 v0.3.9 进位到 v0.4.0，不使用 v0.3.10。当前未修改用户的 Nexus Prime 配置、微信输入法设置或千问输入法设置。千问 0.8.0.27 升级后其语音服务停滞，重启该输入法的维护服务和语音服务器后，实体右 Alt 已由用户确认恢复；这项恢复不依赖 Nexus Prime 改动。

### 当前验证

- 当前代码验证：Rust workspace 测试 127/127、前端测试 47/47、前端生产构建、Cargo workspace 全目标检查和 `git diff --check` 均通过；新增覆盖 Tap/Hold 会话、固定后端、原始键位释放、钩子置顶确认和 F5 配对/泄漏/尾窗。
- Windows Tauri 打包通过。NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.4.0_x64-setup.exe`，13,339,277 bytes，SHA-256 `7F0EE7750E029A74708793228A11653580B4091C6C0694C226BA0AA30D225B18`；文件和产品版本均为 `0.4.0`。该包包含“WinUHid 自身 Ctrl/Win 不再被固件过滤器吞掉”以及“千问/豆包右 Alt 清理已确认固件 Ctrl/Win”的修复。MSI 未随本次发布重打。
- GitHub 正式 Release [`v0.4.0`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.4.0) 已发布。资产 `Nexus.Prime_0.4.0_x64-setup.exe` 状态为 `uploaded`、大小 13,339,277 bytes、GitHub 摘要为 `sha256:7f0ee7750e029a74708793228a11653580b4091c6c0694c226ba0aa30d225b18`，与本地一致；Release `latest.json` 与标签原始清单均为 v0.4.0。
- 千问实体右 Alt 已在重启输入法服务后确认恢复；微信两种模式、豆包和完整的 10 次遥控器回归仍是发布后的待验证项。重点检查单面板、自动上屏但不发送 Enter、无开始菜单/F5 串入/粘键，以及快速连按、断连和配置切换。

## 本次范围（v0.3.9，已发布，2026-09-03）

- 已按本机微信输入法 2.1.3.18 的真实“语音输入”设置恢复两个可选项：“启动语音输入”对应左 Ctrl + 左 Win 点击触发；“按住说话”对应左 Ctrl + 左 Shift + D 按住触发。Nexus Prime 同一时间只应用其中一种，并在界面标明当前模式。
- 删除 v0.3.7/v0.3.8 界面和 README 中错误的 F5 + 左 Ctrl + 左 Win 指引。已发布版本的历史记录保留，但其操作说明由本节和最新 README 取代。
- `wechat-hold` 仅作为 Rust 持久化兼容值读取，配置加载或保存时会自动迁移到 `wechat-current`，同步修正 mic/voice 绑定、voice_hotkey 和 Hold 模式；没有预设标记的用户自定义快捷键不会迁移。
- ATVV 会话现在通过独立边沿门控明确拒绝重复 DOWN/UP；微信“启动语音输入”继续使用 Ctrl+Win 短脉冲，不会把组合键持续按住；“按住说话”继续以 Ctrl+Shift+D 的真实 DOWN/UP 覆盖遥控器按住周期。固件 F5/修饰键过滤、WinUHid 与 SendInput 降级路径保持不变。

### 自动化验证

- `npm.cmd test -- --run`：13 个测试文件、47/47 通过。
- `npm.cmd run build`：Vue TypeScript 检查和 Vite 生产构建通过。
- `cargo test --workspace --manifest-path src-tauri/Cargo.toml`：120/120 通过。
- `cargo check --workspace --all-targets --manifest-path src-tauri/Cargo.toml`：通过。
- `npm.cmd run tauri:build`：通过。本地 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.9_x64-setup.exe`，13,336,223 bytes，SHA-256 `76D74270324AD02E8F60DFAA77494F6BB7601BD819A8D1DD6D7AAC2A559A3DC5`；主程序文件版本与产品版本均为 `0.3.9`，安装包未签名。
- `git diff --check`：通过。本轮未修改微信输入法自身设置。
- GitHub 正式 Release：[`v0.3.9`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.9) 已公开；标签解引用提交为 `cdfcafa`。
- 资产 `Nexus.Prime_0.3.9_x64-setup.exe` 状态为 `uploaded`、大小 13,336,223 bytes、GitHub digest 为 `sha256:76d74270324ad02e8f60dfaa77494f6bb7601bd819a8d1dd6d7aac2a559a3dc5`，与本地 SHA-256 一致；公开下载返回 HTTP 200 与相同长度。
- `latest.json` 同时作为 Release 资产和 `main/latest.json` 提供，资产 SHA-256 为 `74b07e647a6b09e5de4b27890e56fd6f24217c1c0db8e96c666c664cbd57df92`，两个公开清单端点均返回 v0.3.9 元数据。

### 仍待真实设备验收

- 微信输入法两项同时启用时，分别在 Nexus Prime 应用“启动语音输入”和“按住说话”，每种连续按住/松开至少 10 次；每次只能出现一个语音面板，无开始菜单、F5 串入或粘键。
- “按住说话”松手后文字应进入当前输入框，但不得自动发送 Enter；分别确认 WinUHid 正常路径和 SendInput 降级路径。

## 本次范围（v0.3.8，已发布，2026-09-03）

- 已根据用户日志定位应用内更新失败的直接原因：GitHub Core API 返回 `403 rate limit exceeded`，响应头为 `X-RateLimit-Limit: 60`、`X-RateLimit-Remaining: 0`；这与 ATVV 的“遥控器不可达”告警无关。
- 更新检查改为单一协调器：自动检查开启时在启动、每 6 小时和窗口恢复且已到期时执行；`focus` 与 `visibilitychange` 在 1 秒内去重。手动检查强制刷新，但遇到正在进行的自动请求会等待该请求，不再把空返回误写为“已是最新版本”。
- 新版优先从 GitHub Release 的 `latest.json` 更新清单读取版本、安装包大小、下载地址和 SHA-256；仓库 `main/latest.json` 与 GitHub API 保留为兼容回退。下载仍只接受受信任 GitHub 地址，并在完成后进行大小和 SHA-256 校验。
- 后端将网络、HTTP、解析、校验和任务错误以结构化形式返回；GitHub 限流会给出安全的自动重试时间。前端记录触发来源、合并检查、跳过原因和错误阶段，设置页在原有更新胶囊下显示可重试的简洁说明。
- v0.3.6/v0.3.7 的旧二进制仍只使用 GitHub API，匿名额度耗尽时无法靠代码热修复；必须手动安装本轮 v0.3.8 一次，之后才获得清单优先与定时恢复机制。

### 本轮自动化验证

- `npm.cmd test`：47/47 通过。
- `cargo test --workspace --manifest-path src-tauri/Cargo.toml`：116/116 通过。
- `npm.cmd run build`、`cargo check --workspace --all-targets --manifest-path src-tauri/Cargo.toml` 与 `git diff --check`：通过。
- `npm.cmd run tauri:build`：通过。本地 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.8_x64-setup.exe`，13,335,215 bytes，SHA-256 `CF91B052491CB3B4AC05208198733EA39D1B1C6101D3CD06D5BD6431DB444B6C`；文件版本和产品版本均为 `0.3.8`。

### GitHub Release（已发布）

- GitHub 正式 Release：[`v0.3.8`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.8) 已公开，标签指向 `26f0e19`。
- 资产 `Nexus.Prime_0.3.8_x64-setup.exe` 状态 `uploaded`、大小 13,335,215 bytes、GitHub digest `sha256:cf91b052491cb3b4ac05208198733ea39d1b1c6101d3cd06d5bd6431db444b6c`，与本地 SHA-256 一致；公开下载链路返回 HTTP 200 与相同长度。
- `latest.json` 同时作为 Release 资产和 `main/latest.json` 提供，两个公开端点均返回相同的 v0.3.8 安装包元数据。

## 本次范围（v0.3.7，已发布，2026-09-03）

- 参考上游 `mwlt/Voice_VibeCoding` 的 `v1.6.7` / `c76056d` 做选择性适配；两仓库没有共同 Git 历史，未整仓合并，也未同步其电池组件、整体页面、更新器或启动修复。
- 微信界面现在只展示 `wechat-hold`：Nexus Prime 真实 Hold 注入左 Ctrl + 左 Win，用户必须先在微信输入法手动设置“按住说话”为 F5 + 左 Ctrl + 左 Win，再明确点击应用。旧 `wechat`、`wechat-current` 仍可反序列化和保留原行为，读取时不会自动迁移。
- F5 回退链路保留 ATVV 不可用时的受控处理；钩子将固件 F5 的后续注入转交串行 worker，不在键盘回调内执行等待或注入。若 F5 DOWN 已放行给 Windows，其对应 UP 必须放行，避免实体键盘 F5 被误吞。普通方向键、鼠标、千问、豆包和手动快捷键没有改成上游实现。
- 电池继续使用 Nexus Prime 原有外形和充电动效，只使用本地阈值色：≥30% 绿、10%–29% 黄、<10% 红、未知灰色空电池；后端轮询或 GATT 通知写入 `BridgeState` 后发送完整设备快照，页面立即替换对应设备状态，同时保留首次读取和 1.5 秒兜底轮询。
- 新增 VB-CABLE 官方 Pack45 应用内另存为下载：同目录临时文件、16 MiB 上限、HTTP/长度/ZIP 文件头/SHA-256 校验后才替换目标。取消或失败只清理临时文件，不覆盖或删除已有目标；完成后仅提示用户手动解压安装。内嵌包安装、CAT 签名检查和默认麦克风修复未改。

### 本轮自动化验证

- `npm.cmd test`：41/41 通过；`npm.cmd run build`：通过；`cargo check --workspace --all-targets --manifest-path src-tauri/Cargo.toml`：通过。
- `npm.cmd run tauri:build`：`0.3.7` 已通过。本地 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.7_x64-setup.exe`，13,334,604 bytes，SHA-256 `6B191A10FF4298F3CC34D0AA1FBA165E914E1102FB8E49234D38F2FC4CC00EDF`；文件版本和产品版本均为 `0.3.7`。
- GitHub 正式 Release：[`v0.3.7`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.7) 已公开。标签指向 `262cd77`；资产 `Nexus.Prime_0.3.7_x64-setup.exe` 状态 `uploaded`、大小 13,334,604 bytes、GitHub digest `sha256:6b191a10ff4298f3cc34d0aa1fba165e914e1102fb8e49234d38f2fc4cc00edf`，与本地 SHA-256 一致。为保留远端 README 提交，`main` 随后合并至 `607a11b`。

### 仍待真实设备验收（不得标为微信已完全修复）

- 微信输入法录入 F5 + 左 Ctrl + 左 Win 后，连续按住/松开至少 10 次：每次开始、结束、上屏，且无开始菜单、F5 串入或粘键。
- 记事本确认遥控语音键不会插入日期，同时实体键盘 F5 正常；分别验收 WinUHid 正常路径和 SendInput 降级路径。
- 在后端电量轮询日志出现时确认页面不切换即可刷新电量；验证 GATT 电量/充电通知同样即时更新。

## 本次范围（v0.3.6，2026-08-28）

- 在安全仓库 `Nexus Prime-repo` 修复输入法与普通键注入链路；没有在已损坏的原目录执行 Git 写操作。应后续明确授权，已推送 `main`、`v0.3.5` 与 `v0.3.6`，并创建两个 GitHub 正式 Release。
- 普通 Enter、上、下、左、右改走带应用标记的 SendInput，特殊键钩子因此放行应用注入，同时仍吞掉遥控器原始重复事件。鼠标移动、鼠标点击和长按 generation 未改路由。
- ATVV 语音事件会短暂过滤遥控器固件泄漏的非注入左 Ctrl、左 Win，F5 关联后立即停止按下过滤；语音释放窗口只放行本应用虚拟 HID 抬键，超时、重置和异常路径都会清理过滤状态。
- 输入法预设新增可选 `voice_input_profile`（旧配置缺失时保持兼容）。应用预设会写入 profile，手动改语音快捷键或触发模式会清空；按住会锁定 profile、快捷键与注入路由。千问右 Alt 使用直接释放，不再插入 F24、Enter 或 Space；豆包和其他纯修饰键预设保留原有防系统菜单释放策略。
- `tray-icon-compact.png` 保持既有黑色圆角底和白色 `N.`，将主体放大至接近 64px 可用边界；没有修改窗口、安装程序、桌面或其他平台图标。

### 自动化验证

- `npm.cmd test`：已通过（38 项）。
- `cargo test --workspace`：已通过（112 项）。
- `npm.cmd run build`、`cargo check --workspace --all-targets`、`git diff --check` 和 `npm.cmd run tauri:build`：已通过。
- 本地新构建 NSIS 安装包：`src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.6_x64-setup.exe`，13,317,579 bytes，SHA-256 `5C5290A5636C40A211975793798879ED955FCEC54DA55F176CA3505B3594F1F2`；安装包及主程序的文件版本、产品版本均为 `0.3.6`，未签名（`NotSigned`）。
- 已在覆盖前备份 `%APPDATA%\\com.lightyear.nexusprime` 到 `C:\\Users\\16054\\AppData\\Roaming\\com.lightyear.nexusprime.backup-20260828-103555`。安装包以静默覆盖方式安装至 `D:\\Software\\Nexus Prime`，退出码 `0`；重启后的主程序和音频路由进程均来自该目录。当前配置已恢复并显式标记为 `wechat-current`（左 Ctrl + 左 Shift + D、Hold）。
- GitHub 发布复核：[`v0.3.5`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.5) 资产 `Nexus.Prime_0.3.5_x64-setup.exe` 状态 `uploaded`、大小 13,317,199 bytes、digest `sha256:f1efdb0b19fcfc67f83797c3a7836e6d9b7f71c0861ff8e473f2a929a9e0a383`；[`v0.3.6`](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.6) 资产 `Nexus.Prime_0.3.6_x64-setup.exe` 状态 `uploaded`、大小 13,317,579 bytes、digest `sha256:5c5290a5636c40a211975793798879ed955fcec54da55f176ca3505b3594f1f2`。两个 digest 均与本地 SHA-256 一致。

### 仍待真实设备验收（不得标为已修复）

- 微信当前版：连续按住/松开至少 10 次，确认每次都开始与结束、无粘键、开始菜单、紧急释放日志。
- 千问：连续语音至少 10 次，确认松手自动上屏且不再停在“文字复制”。
- 记事本及常用界面：确认上下左右短按、长按连发和确认键无漏发/双发；单独确认鼠标移动和点击正常。
- Windows 隐藏托盘面板：在 100% 与当前系统缩放下确认图标视觉尺寸可用。

## 本次范围（v0.3.5，2026-08-28）

- 微信输入法预设文案与触发模式对齐：
  - `src/utils/imePreset.ts`：`wechat`（启动语音输入，左 Ctrl + 左 Win）触发模式由「按住」改为「点击」，`applyHint` 与 `logMessage` 同步微信输入法原生措辞；`wechat-current`（按住说话，左 Ctrl + 左 Shift + D）描述同步。
  - `src/utils/imePreset.test.ts`：`wechat` 预设的 `trigger_mode` 断言由 `Hold` 改为 `Toggle`。
  - `src/components/InputMethodSettingsDialog.vue`：微信面板摘要与「启动语音输入」说明同步上述措辞（卡片按钮与「新版/旧版」状态徽标仍在 v0.3.6 中改为「按住说话」「启动语音输入」）。
- 版本号同步升至 0.3.5（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 验证与构建产物 SHA-256 见本次提交完成后回填。

## 本次范围（v0.3.3，2026-08-27）

- 旧版微信 Ctrl + Win 语音映射改为「按下启动、松开结束」的两次独立点按会话：按下时先等待关联 F5 被抑制钩子吃掉，再发送约 120ms 的短脉冲；抬起时按开始时固定的注入路由再发一次结束脉冲。不再全程持有组合键，避免遥控器固件连发的 F5 与 Ctrl + Win 组合被微信再次识别。
- 点按会话与既有语音持键状态分开管理：`force_release_voice_shortcut` 会先收尾点按会话；`current_voice_route_label` 同时反映两条路径；配置变更、断开连接等 `reset_voice_input_state` 清理仍有效。F24 中和释放路径仅用于其它纯 Win/Alt 快捷键，不再覆盖旧版微信。
- 全局设置新增「清理旧日志」按钮：确认对话框通过后调用新的 `clear_old_app_logs` 命令删除历史日期与旧格式日志、保留当天，界面显示清理文件数与释放空间；三语言文案同步。
- 首页在宽屏（≥1020px）下三张卡片纵向弹性铺满窗口，窄屏维持原有滚动布局。
- 语音 F5 抑制改为一轮会话汇总一条日志（含吞掉的次数），键盘钩子不再逐条输出 voice_f5 抑制日志。
- 已通过 `npm.cmd test`（37 项）、`npm.cmd run build`、`cargo test --workspace`（106 项）、`cargo check --workspace --all-targets` 和 `git diff --check`。本地 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.3_x64-setup.exe`，13,318,599 bytes，SHA-256 `371466D572893FB0FECDB6056B035F2630181D73C2B1B232CB718ABF4C356BE8`。
- [GitHub 正式 Release v0.3.3](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.3) 已公开且为 `releases/latest`；标签解引用提交为 `1120e5fad166b5e006c03bfe8797eb4dd4580439`，资产 `Nexus.Prime_0.3.3_x64-setup.exe` 状态为 `uploaded`、大小 13,318,599 bytes、GitHub digest 为 `sha256:371466d572893fb0fecdb6056b035f2630181d73c2b1b232cb718abf4c356be8`，与本地 SHA-256 一致。

### 仍待真实设备验收

- 请在真实遥控器 + 旧版微信上验证：语音键按下弹起一次只出现一次语音窗，再次按住/点击能正常结束；快速连续按压不重复触发、无粘键。同时回归新版微信 Ctrl+Shift+D、右 Alt、普通方向键/音量映射和新增的「清理旧日志」按钮。
- CHANGELOG.md 中缺失的 [0.3.2] 条目已在本次一并补齐。

## 上次范围（v0.3.2，2026-08-27）

- 首页服务状态卡片压缩为 68px 的扁平四列布局；快捷操作改为短标题与单行说明，信息按钮固定在右上角，简体中文、繁体中文和英文文案同步。
- 顶部设备、页面连接状态、服务状态、实时提示和动态时间线统一使用 7px 实心圆与 4px 半透明外环的状态灯；不再使用方形提示或闪烁动画。
- 非语音按键的鼠标操作收为下拉菜单，支持鼠标左键及四向移动、步幅 1–100、长按加速、已有参数回填、外部点击/Escape 关闭及与连击菜单互斥。
- 旧版微信的 Hold `Ctrl+Win` 释放不再回放裸组合键：虚拟 HID 使用“原组合+F24 → F24 → 全部抬起”，SendInput 使用“F24 按下 → 原组合逐键抬起 → F24 抬起”；保留同路由重试、延迟确认和紧急 KEYUP。
- 托盘改用紧凑专用图标。16px 下有效不透明主体从约 14×14px 提升至约 16×14px，不影响窗口、安装器或网页图标。
- 已通过 `npm.cmd test`（35 项）、`npm.cmd run build`、`cargo test --workspace`（104 项）、`cargo check --workspace --all-targets`、`git diff --check` 和 `npm.cmd run tauri:build`。源码以 `v0.3.2` 标签发布；本地 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.2_x64-setup.exe`，13,315,955 bytes，SHA-256 `779F4CBDDE40680FA6E57FEF214C81431B3AE680376C2335AF1B0108C504357C`，文件版本与产品版本均为 `0.3.2`，未签名（`NotSigned`）。
- [GitHub 正式 Release v0.3.2](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.2) 已公开；标签解引用提交为 `cebdbf15a3123e990b9f2aa91bec7cd446d1f26d`，资产 `Nexus.Prime_0.3.2_x64-setup.exe` 状态为 `uploaded`、大小 13,315,955 bytes、GitHub digest 为 `sha256:779f4cbdde40680fa6e57fef214c81431b3ae680376c2335af1b0108c504357c`，与本地 SHA-256 一致。

### 仍待真实设备验收

- 请在实际遥控器上验证旧版微信 `Ctrl+Win + Hold` 只出现一次语音窗，抬起不弹开始菜单/系统菜单且无粘键；同时回归新版微信、右 Alt、Click/Hold、鼠标左键和四向移动。

## 本次范围（v0.3.1，2026-08-27）

- [PR #5](https://github.com/LightyearXizIl/Nexus-Prime/pull/5) 已以 squash 合入 `main`，源码提交与标签目标均为 `399ad359a637ce4b17901d16a1970bd051ba761d`（`v0.3.1`）。它包含鼠标左键与上下左右相对移动；[PR #4](https://github.com/LightyearXizIl/Nexus-Prime/pull/4) 已说明被 #5 包含并关闭。
- 鼠标操作为非语音键的可选映射，现有方向键默认仍发送键盘方向键。映射卡片中不必开启“录入快捷键”即可设置左键、上、下、左、右；步幅默认 20 像素并限制为 1–100，加速选项只在长按槽位生效。
- `MouseClick` 与 `MouseMove { dx, dy, step, accelerate }` 已纳入 Rust/TypeScript 配置接口。读取与保存配置会钳制步幅、拒绝非四向移动；鼠标 `SendInput` 必须完整写入才报告成功。长按移动的重复令牌在物理按压仍有效时预留，抬起、配置保存、断开或手势重置都能取消，避免快速抬起后仍持续移动。
- 已通过 `npm.cmd test`（33 项）、`npm.cmd run build`、`cargo test --workspace`（100 项）、`cargo check --workspace --all-targets`、`git diff --check` 与 `npm.cmd run tauri:build`。本地 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.1_x64-setup.exe`，13,310,065 bytes，SHA-256 `06DA1BF9DF8D75BCE3D7E236C6D5063DA4149FDD7960E4364DBCF0504D04487C`，文件版本与产品版本均为 `0.3.1`，未签名（`NotSigned`）。
- [GitHub 正式 Release v0.3.1](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.1) 已公开；资产 `Nexus.Prime_0.3.1_x64-setup.exe` 状态为 `uploaded`、大小 13,310,065 bytes、GitHub digest 为 `sha256:06da1bf9df8d75bce3d7e236c6d5063da4149fdd7960e4364dbcf0504d04487c`。已从 Release 下载并复核大小和 SHA-256，`releases/latest` 为 v0.3.1。

### 仍待真实遥控器验收

- 本轮未在活动桌面执行真实鼠标点击或移动，避免影响当前用户操作；也未用实际小米遥控器验证。请在安装 v0.3.1 后验证任意非语音键的左键、四向单击、长按匀速/加速，以及快速抬起、断开连接和保存设置后的立即停止；同时回归原有方向键、语音、音量和返回映射。

## 本次范围（v0.3.0，2026-08-27）

- 运行日志已改为 `logs/app-YYYY-MM-DD.log` 按日后台写入；设置页可选 1、3、7、14、30 天，默认 7 天。界面、命令、设备、按键注入和语音会话均写语义事件；日志保留实际设备地址、路径、快捷键和配置值。
- Click/Hold 均在 `AUDIO_START` 立即发送语音快捷键。语音的纯 Ctrl+Win、Win 或 Alt 组合会先按 F24 中和再释放，避免空按弹出系统菜单。
- `VoiceChordState` 不会再在释放失败后丢弃原始持键记录；同路由重试三次后，虚拟 HID 会执行仅限本会话键位的 SendInput KEYUP 恢复。普通组合键的 SendInput KEYUP 已逐键发送。
- 已通过 `cargo test --workspace`（96 项）、`cargo check --workspace --all-targets`、`npm.cmd test`（31 项）、`npm.cmd run build`、PowerShell 脚本语法检查、`git diff --check` 和 `npm.cmd run tauri:build`。本地正式 NSIS 安装包为 `src-tauri/target/release/bundle/nsis/Nexus Prime_0.3.0_x64-setup.exe`，13,298,359 bytes，SHA-256 `ADF18A6692D57C5B65BEAD0D7586C5284902BE1C77F835BF35859C26EFD657FC`，文件版本和产品版本均为 `0.3.0`。源码与标签提交为 `55dd5011317388179e4ed0fac45e4f184877a9c3`（`v0.3.0`）；[GitHub 正式 Release](https://github.com/LightyearXizIl/Nexus-Prime/releases/tag/v0.3.0) 已公开，资产 `Nexus.Prime_0.3.0_x64-setup.exe` 状态为 `uploaded`、大小 13,298,359 bytes、GitHub digest 为 `sha256:adf18a6692d57c5b65bead0d7586c5284902be1c77f835bf35859c26efd657fc`。真机豆包、微信悬浮语音条与 Windows 菜单表现按用户指示暂缓验收。

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
