# 参与贡献

感谢你愿意改进 Nexus Prime。项目是 Windows 专用的 Tauri 2 桌面应用，前端使用 Vue 3 + TypeScript，后端使用 Rust。

## 开始之前

请先搜索现有 Issue，确认问题尚未被报告。涉及新设备、协议调整、进程注入、端口或音频路由的大改，建议先创建 Issue 说明动机、设计和影响范围。

提交 Issue、日志或截图前，请删除以下内容：

- 蓝牙 MAC、设备序列号和其他设备标识；
- Windows 用户名、绝对路径和日志目录；
- API Key、Token、密码、Cookie 和私钥；
- 与问题无关的个人联系方式、进程信息和录音数据。

## 开发环境

需要 Windows 10/11、Node.js 18 或 20+、Rust stable MSVC、Microsoft C++ Build Tools 和 WebView2。完整说明见 [README](./README.md#开发运行)。

```powershell
npm ci
npm run tauri:dev
```

如果 PowerShell 的执行策略阻止 `npm.ps1`，可将命令中的 `npm` 换成 `npm.cmd`。

## 提交修改

1. 从 `main` 创建简短、聚焦的分支，例如 `fix/atvv-reconnect`。
2. 一个 Pull Request 只解决一类问题，避免夹带无关格式化。
3. 保持现有 UI 和行为兼容；新增环境变量必须同步更新 `.env.example` 和 README。
4. 不要提交生成目录、用户配置、日志、录音、真实设备信息或未获再分发许可的二进制文件。
5. 新增第三方依赖或资源时，说明来源、版本、许可证和再分发条件。

建议使用清晰的提交信息，例如：

```text
fix: retry ATVV subscription after HID tap restart
docs: clarify VB-CABLE setup
```

## 提交前检查

至少运行：

```powershell
npm run build
cargo check --locked --all-targets --manifest-path src-tauri/Cargo.toml
npm run tauri -- build --debug --no-bundle
```

如果修改了 Windows API、音频、BLE 或进程注入代码，还应在真实 Windows 设备上验证连接、按键映射、语音路由、托盘退出和重连流程。

## Pull Request 清单

- [ ] 修改范围单一，未改变无关功能；
- [ ] 前端构建和 Rust 全目标检查通过；
- [ ] 新行为有测试或清晰的手工验证说明；
- [ ] 文档、截图和 `.env.example` 已同步；
- [ ] 未包含凭据、个人数据或真实设备标识；
- [ ] 第三方代码与资源的许可证已核对。

## Windows Release 清单

发布 Windows 版本时，除版本号、更新日志、安装包和 GitHub Release 外，还必须同步根目录 `latest.json`：其中的版本、安装包文件名、字节大小、SHA-256 和 GitHub 下载地址必须来自本次实际构建产物。将同一份 `latest.json` 作为 Release 资产上传，并确认 Release 清单端点、`main/latest.json` 和安装包公开下载均可访问；不要将仅本地计算的摘要写成已发布状态。

## 许可证

向本项目提交贡献即表示你同意以 [MIT License](./LICENSE) 发布你的贡献。第三方组件仍受各自许可证约束。
