//! 独立于 Tauri/WebView2 的语音粘键状态机测试入口。
//!
//! 当本机系统缺少 TaskDialogIndirect、无法启动完整 Tauri test harness 时，
//! 可用 `cargo test --example voice_chord_state_check` 实际运行此处的关键测试。

#![allow(dead_code)]

#[path = "../src/bridges/xiaomi/voice_chord_state.rs"]
mod voice_chord_state;

#[path = "../src/bridges/xiaomi/session_state.rs"]
mod session_state;

fn main() {}
