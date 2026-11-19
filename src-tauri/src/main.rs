// Windows GUI：始终无控制台窗口（含子进程 CLI 入口）。调试日志写文件；需要控制台时设 RUST_LOG 并自行 AllocConsole。
#![windows_subsystem = "windows"]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--voice-feedback-preview") => {
            if let Some(report) = args.get(2) {
                if let Ok(file) =
                    std::fs::File::create(std::path::Path::new(report).with_extension("log"))
                {
                    let _ = env_logger::Builder::new()
                        .filter_level(log::LevelFilter::Info)
                        .target(env_logger::Target::Pipe(Box::new(file)))
                        .try_init();
                }
            }
            let rendered = nexus_prime_lib::audio::feedback::preview();
            let (start_us, end_us) = nexus_prime_lib::audio::feedback::latency_metrics_us();
            if let Some(report) = args.get(2) {
                let text = format!("{{\"native_audio_rendered\":{rendered},\"app_event_to_first_frame_us\":{{\"start\":{start_us},\"end\":{end_us}}},\"audibility\":\"requires_user_confirmation\"}}\n");
                let _ = std::fs::write(report, text);
            }
            if !rendered {
                std::process::exit(1);
            }
        }
        Some("xiaomi-hid-injector") => {
            let code = nexus_prime_lib::bridges::xiaomi::hid_tap_injector::run_injector_cli(&args);
            std::process::exit(code);
        }
        Some("xiaomi-audio-router") => {
            let code = nexus_prime_lib::audio::pcm_router::run_audio_router_cli(&args);
            std::process::exit(code);
        }
        Some(nexus_prime_lib::audio::vb_cable::AUTO_INSTALL_HELPER_ARGUMENT) => {
            let code = nexus_prime_lib::audio::vb_cable::run_official_installer_helper_cli(&args);
            std::process::exit(code);
        }
        _ => nexus_prime_lib::run(),
    }
}
