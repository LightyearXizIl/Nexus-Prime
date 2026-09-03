pub mod bridges;
pub mod config;
pub mod ipc;
pub mod audio;
pub mod logging;
pub mod update;
pub mod windows_command;

pub const AUTOSTART_ARGUMENT: &str = "--autostart";
const LEGACY_AUTOSTART_ARGUMENT: &str = "--minimized";

// Tauri's dialog dependencies import TaskDialogIndirect from Common Controls
// v6. tauri-build embeds that activation manifest into application binaries,
// but not into Rust's unit-test executable. Force the same generated resource
// into Windows test harnesses so they do not fall back to Common Controls v5.
#[cfg(all(test, target_os = "windows"))]
#[link(name = "resource", kind = "static")]
unsafe extern "C" {}

use tauri::{Manager, RunEvent};

/// 退出前统一清理：停桥接 + HID Tap + 卸键盘钩子，避免进程残留
fn cleanup_on_exit(app: &tauri::AppHandle) {
    if let Some(runtime) =
        app.try_state::<std::sync::Arc<bridges::xiaomi::connect::XiaomiRuntime>>()
    {
        runtime.request_stop();
        runtime.cancel_active_session("app_exit");
    }
    bridges::xiaomi::key_mapping::reset_voice_input_state("app_exit");
    bridges::xiaomi::hid_report_tap::stop_and_join();
    bridges::xiaomi::special_keys::stop_special_key_hook();
    audio::pcm_router::stop_audio_router_process();
}

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        set_main_webview_visible(app, true);
        let _ = window.set_focus();
    }
}

fn is_autostart_invocation(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            AUTOSTART_ARGUMENT | LEGACY_AUTOSTART_ARGUMENT
        )
    })
}

fn should_start_hidden(args: &[String], autostart_minimized_to_tray: bool) -> bool {
    autostart_minimized_to_tray && is_autostart_invocation(args)
}

fn should_focus_existing_instance(args: &[String]) -> bool {
    // Windows may still run a legacy Startup shortcut beside the new Run entry.
    // Ignore either marker so a duplicate login launch cannot reveal a hidden app.
    !is_autostart_invocation(args)
}

/// 托盘隐藏时停止 WebView2 渲染：不隐藏子 WebView 时 Chromium 会继续
/// 全速合成动画并保持 1s 轮询定时器不节流（实测占 ~5.6% 单核）
pub fn set_main_webview_visible(app: &tauri::AppHandle, visible: bool) {
    #[cfg(desktop)]
    if let Some(wv) = app.get_webview_window("main") {
        let result = if visible {
            wv.as_ref().show()
        } else {
            wv.as_ref().hide()
        };
        if let Err(error) = result {
            log::warn!("failed to set main webview visible={visible}: {error}");
        }
    }
    #[cfg(not(desktop))]
    let _ = (app, visible);
}

#[cfg_attr(mobile, mobile_entry_point)]
pub fn run() {
    // single-instance 必须最先注册：二次启动时激活已有窗口并退出新进程
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if should_focus_existing_instance(&args) {
                focus_main_window(app);
            } else {
                log::info!("Ignoring duplicate autostart activation");
            }
        }));
    }

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize configuration + 单文件日志
            let config_manager = config::manager::ConfigManager::new(app.handle().clone())?;
            let log_retention_days = config_manager
                .get_global_settings()
                .map(|settings| settings.log_retention_days as usize)
                .unwrap_or(7);
            let log_path = logging::init(&config_manager.logs_dir(), log_retention_days);
            std::env::set_var("REMOTE_BRIDGE_LOG_PATH", &log_path);
            std::env::set_var("REMOTE_BRIDGE_LOG_DIR", config_manager.logs_dir());
            std::env::set_var("REMOTE_BRIDGE_LOG_RETENTION_DAYS", log_retention_days.to_string());
            app.manage(config_manager);
            app.manage(update::UpdateManager::default());

            log::info!("Nexus Prime starting...");
            #[cfg(debug_assertions)]
            log::info!("build_profile=debug (开发包)");
            #[cfg(not(debug_assertions))]
            log::info!("build_profile=release");

            // Initialize bridge state
            let bridge_state = bridges::BridgeState::new();
            app.manage(bridge_state);

            // Xiaomi 连接运行时（停止信号）
            app.manage(std::sync::Arc::new(
                bridges::xiaomi::connect::XiaomiRuntime::new(),
            ));

            // 快捷键录制会话
            app.manage(bridges::shared::shortcut_capture::ShortcutCaptureSession::new());

            // Setup tray menu（必须 manage，否则 TrayIcon Drop 会摘掉托盘）
            let tray = ipc::tray::setup_tray(app.handle())?;
            app.manage(tray);

            // 语音电平/波形 UI 事件
            bridges::xiaomi::voice_meter::bind_app(app.handle().clone());
            bridges::xiaomi::conflict_guard::bind_app(app.handle().clone());

            // Deploy the bundled WinUHid SDK, then install the bundled virtual
            // keyboard driver on first run. This is required for IME global
            // hotkeys such as Doubao and WeChat, which filter SendInput events.
            bridges::xiaomi::winuhid_env::ensure_runtime_quiet();
            std::thread::Builder::new()
                .name("winuhid-bootstrap".into())
                .spawn(|| {
                    if let Err(error) = bridges::xiaomi::winuhid_env::install_if_needed() {
                        log::warn!("WinUHid bootstrap failed: {error}");
                    }
                })?;

            let launch_args: Vec<String> = std::env::args().collect();
            if let Some(window) = app.get_webview_window("main") {
                // WIN10 兼容：默认窗口 1080x814 可能超出小屏/高缩放的工作区
                // （如 1366x768@125% 逻辑工作区约 1093x614），底部内容会被裁出屏幕。
                // 启动时按主显示器工作区 clamp 窗口尺寸与最小尺寸，并重新居中。
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let scale = monitor.scale_factor();
                    let size = monitor.size(); // 物理像素
                    let work_w = size.width as f64 / scale;
                    let work_h = size.height as f64 / scale;
                    let cap_w = work_w * 0.96; // 留 4% 边距，避免贴边
                    let cap_h = work_h * 0.96;
                    let (default_w, default_h) = (1080.0f64, 814.0f64);
                    let (min_w, min_h) = (880.0f64, 720.0f64);
                    let clamp_w = default_w.min(cap_w);
                    let clamp_h = default_h.min(cap_h);
                    if default_w > cap_w || default_h > cap_h {
                        let _ = window.set_size(tauri::LogicalSize::new(clamp_w, clamp_h));
                        let _ = window.center();
                        log::info!(
                            "WINDOW size clamped to {clamp_w:.0}x{clamp_h:.0} (work {work_w:.0}x{work_h:.0} scale={scale})"
                        );
                    }
                    // 最小尺寸同样不能超过工作区，否则用户缩小时仍会溢出
                    let _ = window.set_min_size(Some(tauri::LogicalSize::new(
                        min_w.min(cap_w),
                        min_h.min(cap_h),
                    )));
                }

                // 关闭窗口：minimize_to_tray=true 则隐藏；false 则真正退出
                let app_handle = app.handle().clone();
                let window_ = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let minimize = app_handle
                            .try_state::<config::manager::ConfigManager>()
                            .and_then(|m| m.get_global_settings().ok())
                            .map(|s| s.minimize_to_tray)
                            .unwrap_or(true);
                        if minimize {
                            api.prevent_close();
                            set_main_webview_visible(&app_handle, false);
                            let _ = window_.hide();
                        }
                        // else: 允许关闭 → 触发 Exit → cleanup_on_exit
                    }
                });

                let start_hidden = match app
                    .try_state::<config::manager::ConfigManager>()
                    .and_then(|manager| manager.get_global_settings().ok())
                {
                    Some(settings) => should_start_hidden(
                        &launch_args,
                        settings.autostart_minimized_to_tray,
                    ),
                    None => {
                        log::warn!("Unable to read startup visibility preference; showing main window");
                        false
                    }
                };
                if start_hidden {
                    set_main_webview_visible(app.handle(), false);
                    let _ = window.hide();
                    log::info!("Main window hidden for autostart launch");
                } else {
                    focus_main_window(app.handle());
                }
            }

            // 独立 audio_router 子进程（对齐 Python --role audio）
            std::env::set_var("REMOTE_BRIDGE_PCM_PORT", "31680");
            if let Err(e) = audio::pcm_router::spawn_audio_router_process() {
                log::warn!("audio router spawn failed: {e}");
                bridges::xiaomi::conflict_guard::emit_if_conflicts(
                    "pcm_port",
                    &format!("语音路由启动失败: {e}"),
                    true,
                );
            } else {
                // 路由起来后立刻预热 UDP，避免首句语音才 PING
                bridges::xiaomi::voice_pcm::warmup_async();
                bridges::xiaomi::hid_injector::warmup_async();
                bridges::xiaomi::conflict_guard::check_audio_router_after_spawn(app.handle());
            }

            // 启动后自动连接 + 断线重连（对齐 Python worker 循环）
            let auto_app = app.handle().clone();
            std::thread::Builder::new()
                .name("xiaomi-auto-connect".into())
                .spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if let (Some(config_manager), Some(runtime)) = (
                        auto_app.try_state::<config::manager::ConfigManager>(),
                        auto_app
                            .try_state::<std::sync::Arc<bridges::xiaomi::connect::XiaomiRuntime>>(),
                    ) {
                        if runtime.running.load(std::sync::atomic::Ordering::SeqCst) {
                            return;
                        }
                        let cfg = config_manager.get_device_config("xiaomi").ok();
                        let retry = std::time::Duration::from_secs_f32(
                            cfg.as_ref().map(|c| c.retry_delay).unwrap_or(3.0).max(0.5),
                        );
                        let configured = cfg.and_then(|c| c.bluetooth_address);
                        runtime.clear_stop();
                        runtime
                            .running
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        ipc::commands::xiaomi_reconnect_loop_public(
                            auto_app.clone(),
                            std::sync::Arc::clone(&runtime),
                            configured,
                            retry,
                        );
                    }
                })?;

            log::info!("Nexus Prime started successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::get_device_status,
            ipc::commands::start_bridge,
            ipc::commands::stop_bridge,
            ipc::commands::get_config,
            ipc::commands::save_config,
            ipc::commands::get_key_mappings,
            ipc::commands::update_key_mapping,
            ipc::commands::capture_shortcut_start,
            ipc::commands::capture_shortcut_stop,
            ipc::commands::capture_shortcut_poll,
            ipc::commands::get_audio_devices,
            ipc::commands::get_bridge_logs,
            ipc::commands::set_autostart,
            ipc::commands::get_autostart,
            ipc::commands::get_global_settings,
            ipc::commands::save_global_settings,
            ipc::commands::set_theme_preference,
            ipc::commands::set_language_preference,
            ipc::commands::get_xiaomi_host_status,
            ipc::commands::get_xiaomi_voice_meter,
            ipc::commands::restart_xiaomi_bridge,
            ipc::commands::check_xiaomi_voice_env,
            ipc::commands::get_xiaomi_voice_env_status,
            ipc::commands::repair_xiaomi_voice_env,
            ipc::commands::download_xiaomi_vbcable_zip,
            ipc::commands::cancel_xiaomi_vbcable_zip_download,
            ipc::commands::repair_xiaomi_virtual_keyboard,
            ipc::commands::open_logs_folder,
            ipc::commands::get_app_log,
            ipc::commands::open_app_log,
            ipc::commands::clear_old_app_logs,
            ipc::commands::append_app_events,
            ipc::commands::quit_application,
            ipc::commands::get_xiaomi_conflicts,
            ipc::commands::kill_xiaomi_conflicts,
            ipc::commands::retry_xiaomi_after_conflict_clear,
            ipc::commands::repair_xiaomi_atvv,
            update::check_for_update,
            update::download_update,
            update::cancel_update_download,
            update::install_downloaded_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                RunEvent::ExitRequested { .. } | RunEvent::Exit => {
                    cleanup_on_exit(app_handle);
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn startup_visibility_requires_marker_and_preference() {
        assert!(!should_start_hidden(&args(&["nexus-prime.exe"]), true));
        assert!(!should_start_hidden(&args(&["nexus-prime.exe", "--autostart"]), false));
        assert!(should_start_hidden(&args(&["nexus-prime.exe", "--autostart"]), true));
        assert!(should_start_hidden(&args(&["nexus-prime.exe", "--minimized"]), true));
    }

    #[test]
    fn only_manual_secondary_launches_restore_the_window() {
        assert!(should_focus_existing_instance(&args(&["nexus-prime.exe"])));
        assert!(!should_focus_existing_instance(&args(&["nexus-prime.exe", "--autostart"])));
        assert!(!should_focus_existing_instance(&args(&["nexus-prime.exe", "--minimized"])));
    }
}
