//! 配置管理器 — 管理所有设备配置
//!
//! 配置文件存放于 %APPDATA%\RemoteBridgeHub\
//! - xiaomi.json   — 小米遥控器配置
//! - settings.json — 全局设置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

// ============================================================
// 数据类型定义
// ============================================================

/// 按键动作类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum KeyAction {
    /// 单个虚拟键码
    SingleKey(u16),
    /// 组合键（修饰符 + 键）
    ComboKey(Vec<u16>),
    /// 文本输入
    TextInput(String),
    /// 启动应用
    LaunchApp(String),
    /// 无动作
    None,
}

impl Default for KeyAction {
    fn default() -> Self {
        KeyAction::None
    }
}

/// 触发模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TriggerMode {
    /// 点击型：点一下开始（松手继续），再点结束并提交（需 MIC_OPEN 保活）
    Toggle,
    /// 按住型：按下说话，松开结束
    Hold,
}

impl Default for TriggerMode {
    fn default() -> Self {
        // 对齐 Python DEFAULT：voice_trigger_mode=toggle
        TriggerMode::Toggle
    }
}

/// 设备配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// 按键别名：button_id → 显示名称
    pub button_aliases: HashMap<String, String>,
    /// 按键绑定：button_id → 动作
    pub button_bindings: HashMap<String, KeyAction>,
    /// 独立长按绑定：button_id → 动作
    #[serde(default)]
    pub long_press_bindings: HashMap<String, KeyAction>,
    #[serde(default)]
    pub multi_click_bindings: HashMap<String, HashMap<u8, KeyAction>>,
    #[serde(default = "default_multi_click_interval_ms")]
    pub multi_click_interval_ms: u64,
    /// 语音快捷键
    pub voice_hotkey: Option<Vec<String>>,
    /// 语音触发模式
    #[serde(default)]
    pub trigger_mode: TriggerMode,
    /// 蓝牙地址（仅小米）
    pub bluetooth_address: Option<String>,
    /// 语音增益 dB（对齐 Python gain_db）
    #[serde(default = "default_gain_db")]
    pub gain_db: f32,
    /// 断线重连间隔秒
    #[serde(default = "default_retry_delay")]
    pub retry_delay: f32,
    /// 是否启用语音快捷键
    #[serde(default = "default_true")]
    pub voice_shortcut_enabled: bool,
    /// TV 键就绪延迟秒
    #[serde(default = "default_tv_delay")]
    pub tv_action_ready_delay: f32,
    /// 特殊键抑制
    #[serde(default = "default_true")]
    pub special_key_hook_enabled: bool,
    /// HID Tap
    #[serde(default = "default_true")]
    pub hid_report_tap_enabled: bool,
}

fn default_gain_db() -> f32 {
    10.0
}
fn default_retry_delay() -> f32 {
    3.0
}
fn default_tv_delay() -> f32 {
    2.0
}
fn default_multi_click_interval_ms() -> u64 {
    300
}
fn default_true() -> bool {
    true
}

impl DeviceConfig {
    pub fn new() -> Self {
        Self {
            button_aliases: HashMap::new(),
            button_bindings: HashMap::new(),
            long_press_bindings: HashMap::new(),
            multi_click_bindings: HashMap::new(),
            multi_click_interval_ms: default_multi_click_interval_ms(),
            voice_hotkey: Some(vec!["rightalt".into()]),
            trigger_mode: TriggerMode::Toggle,
            bluetooth_address: None,
            gain_db: default_gain_db(),
            retry_delay: default_retry_delay(),
            voice_shortcut_enabled: true,
            tv_action_ready_delay: default_tv_delay(),
            special_key_hook_enabled: true,
            hid_report_tap_enabled: true,
        }
    }
}

/// 全局设置
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

impl Default for ThemePreference {
    fn default() -> Self {
        Self::System
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettings {
    /// 开机自启
    pub autostart: bool,
    /// 界面语言
    pub language: String,
    /// 最小化到托盘
    pub minimize_to_tray: bool,
    /// 界面主题偏好；缺失时兼容旧配置并跟随系统
    #[serde(default)]
    pub theme: ThemePreference,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            autostart: false,
            language: "zh-CN".to_string(),
            minimize_to_tray: true,
            theme: ThemePreference::System,
        }
    }
}

// ============================================================
// ConfigManager
// ============================================================

pub struct ConfigManager {
    config_dir: PathBuf,
    /// 设备配置内存缓存：按键热路径避免每次读盘+JSON
    device_cache: Mutex<HashMap<String, DeviceConfig>>,
    /// 全局设置读改写需要串行，避免主题快捷切换覆盖表单保存。
    global_settings_lock: Mutex<()>,
}

impl ConfigManager {
    /// 创建配置管理器，自动创建配置目录
    pub fn new(app_handle: AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = get_config_dir(&app_handle)?;
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(config_dir.join("logs")).ok();
        Ok(Self {
            config_dir,
            device_cache: Mutex::new(HashMap::new()),
            global_settings_lock: Mutex::new(()),
        })
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.config_dir.join("logs")
    }

    /// 获取设备配置文件路径
    fn device_config_path(&self, device: &str) -> PathBuf {
        self.config_dir.join(format!("{}.json", device))
    }

    /// 获取全局设置文件路径
    fn settings_path(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    // ---- 设备配置 ----

    fn load_device_config_from_disk(&self, device: &str) -> Result<DeviceConfig, String> {
        let path = self.device_config_path(device);
        if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("读取配置失败: {}", e))?;
            let mut config: DeviceConfig = serde_json::from_str(&content)
                .map_err(|e| format!("解析配置失败: {}", e))?;
            if device == "xiaomi" {
                Self::merge_xiaomi_defaults(&mut config);
            }
            Ok(config)
        } else {
            Ok(Self::default_config_for(device))
        }
    }

    /// 获取设备配置（内存缓存；未命中再读盘）
    pub fn get_device_config(&self, device: &str) -> Result<DeviceConfig, String> {
        if let Some(cached) = self.device_cache.lock().get(device).cloned() {
            return Ok(cached);
        }
        let config = self.load_device_config_from_disk(device)?;
        self.device_cache
            .lock()
            .insert(device.to_string(), config.clone());
        Ok(config)
    }

    /// 使缓存失效（外部改文件时可选调用）
    pub fn invalidate_device_config(&self, device: &str) {
        self.device_cache.lock().remove(device);
    }

    /// 对齐 Python schema 升级：补齐缺失的默认绑定/别名，不覆盖用户已有项
    fn merge_xiaomi_defaults(config: &mut DeviceConfig) {
        let defaults = Self::default_config_for("xiaomi");
        for (k, v) in defaults.button_aliases {
            config.button_aliases.entry(k).or_insert(v);
        }
        for (k, v) in defaults.button_bindings {
            config.button_bindings.entry(k).or_insert(v);
        }
        if config.voice_hotkey.as_ref().map(|v| v.is_empty()).unwrap_or(true) {
            config.voice_hotkey = defaults.voice_hotkey;
        }
        Self::sanitize_xiaomi_gesture_bindings(config);
    }

    fn sanitize_xiaomi_gesture_bindings(config: &mut DeviceConfig) {
        let mut sanitized: HashMap<String, HashMap<u8, KeyAction>> = HashMap::new();
        for (button_id, slots) in std::mem::take(&mut config.multi_click_bindings) {
            let canonical = crate::bridges::xiaomi::key_mapping::canonical_button_id(&button_id);
            if canonical == "mic" || canonical == "voice" || canonical == "unknown" {
                if !slots.is_empty() {
                    log::warn!("XIAOMI CONFIG ignored multi-click binding for {button_id}");
                }
                continue;
            }
            for (count, action) in slots {
                if !(2..=4).contains(&count) {
                    log::warn!(
                        "XIAOMI CONFIG ignored invalid multi-click count {count} for {button_id}"
                    );
                    continue;
                }
                if matches!(action, KeyAction::None) {
                    continue;
                }
                sanitized
                    .entry(canonical.to_string())
                    .or_default()
                    .insert(count, action);
            }
        }
        config.multi_click_bindings = sanitized;

        let mut sanitized_long_press: HashMap<String, KeyAction> = HashMap::new();
        for (button_id, action) in std::mem::take(&mut config.long_press_bindings) {
            let canonical = crate::bridges::xiaomi::key_mapping::canonical_button_id(&button_id);
            if canonical == "mic" || canonical == "voice" || canonical == "unknown" {
                log::warn!("XIAOMI CONFIG ignored long-press binding for {button_id}");
                continue;
            }
            if matches!(action, KeyAction::None) {
                continue;
            }
            sanitized_long_press.insert(canonical.to_string(), action);
        }
        config.long_press_bindings = sanitized_long_press;

        if !(150..=800).contains(&config.multi_click_interval_ms) {
            log::warn!(
                "XIAOMI CONFIG clamped invalid multi_click_interval_ms={}",
                config.multi_click_interval_ms
            );
            config.multi_click_interval_ms = config.multi_click_interval_ms.clamp(150, 800);
        }
    }

    /// 保存设备配置（写临时文件 → sync → rename；并更新缓存）
    pub fn save_device_config(&self, device: &str, config: &DeviceConfig) -> Result<(), String> {
        let mut config = config.clone();
        if device == "xiaomi" {
            Self::sanitize_xiaomi_gesture_bindings(&mut config);
            crate::bridges::xiaomi::key_mapping::cancel_pending_gestures();
            crate::bridges::xiaomi::key_mapping::sync_voice_from_mic_binding(&mut config);
        }
        let path = self.device_config_path(device);
        let tmp_path = path.with_extension("json.tmp");

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("序列化配置失败: {}", e))?;

        {
            let mut file = fs::File::create(&tmp_path)
                .map_err(|e| format!("写入临时文件失败: {}", e))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("写入临时文件失败: {}", e))?;
            file.sync_all()
                .map_err(|e| format!("同步临时文件失败: {}", e))?;
        }

        fs::rename(&tmp_path, &path).map_err(|e| format!("替换配置文件失败: {}", e))?;

        self.device_cache
            .lock()
            .insert(device.to_string(), config);
        Ok(())
    }

    // ---- 全局设置 ----

    fn load_global_settings_unlocked(&self) -> Result<GlobalSettings, String> {
        let path = self.settings_path();
        if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("读取设置失败: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("解析设置失败: {}", e))
        } else {
            Ok(GlobalSettings::default())
        }
    }

    fn save_global_settings_unlocked(&self, settings: &GlobalSettings) -> Result<(), String> {
        let path = self.settings_path();
        let tmp_path = path.with_extension("json.tmp");

        let content = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("序列化设置失败: {}", e))?;

        fs::write(&tmp_path, &content)
            .map_err(|e| format!("写入临时文件失败: {}", e))?;

        fs::rename(&tmp_path, &path)
            .map_err(|e| format!("替换设置文件失败: {}", e))?;

        Ok(())
    }

    pub fn get_global_settings(&self) -> Result<GlobalSettings, String> {
        let _guard = self.global_settings_lock.lock();
        self.load_global_settings_unlocked()
    }

    /// 保存表单中的全局设置，但主题始终以主题快捷保存的最新值为准。
    pub fn save_global_settings(&self, settings: &GlobalSettings) -> Result<(), String> {
        let _guard = self.global_settings_lock.lock();
        let mut merged = settings.clone();
        merged.theme = self.load_global_settings_unlocked()?.theme;
        self.save_global_settings_unlocked(&merged)
    }

    /// 只更新主题字段，保留可能尚未被其它界面重新读取的全局设置。
    pub fn set_theme_preference(&self, theme: ThemePreference) -> Result<(), String> {
        let _guard = self.global_settings_lock.lock();
        let mut settings = self.load_global_settings_unlocked()?;
        settings.theme = theme;
        self.save_global_settings_unlocked(&settings)
    }

    // ---- 默认配置 ----

    /// 返回各设备的默认按键映射
    fn default_config_for(device: &str) -> DeviceConfig {
        match device {
            "xiaomi" => DeviceConfig {
                button_aliases: Self::xiaomi_button_aliases(),
                button_bindings: Self::xiaomi_default_bindings(),
                long_press_bindings: HashMap::new(),
                multi_click_bindings: HashMap::new(),
                multi_click_interval_ms: default_multi_click_interval_ms(),
                voice_hotkey: Some(vec!["rightalt".into()]),
                trigger_mode: TriggerMode::Toggle,
                bluetooth_address: None,
                gain_db: 10.0,
                retry_delay: 3.0,
                voice_shortcut_enabled: true,
                tv_action_ready_delay: 2.0,
                special_key_hook_enabled: true,
                hid_report_tap_enabled: true,
            },
            _ => DeviceConfig::new(),
        }
    }

    // ---- 小米遥控器默认按键 ----
    fn xiaomi_button_aliases() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("power".into(), "电源".into());
        m.insert("volume_up".into(), "音量+".into());
        m.insert("volume_down".into(), "音量-".into());
        m.insert("up".into(), "上".into());
        m.insert("down".into(), "下".into());
        m.insert("left".into(), "左".into());
        m.insert("right".into(), "右".into());
        m.insert("dpad_up".into(), "上".into());
        m.insert("dpad_down".into(), "下".into());
        m.insert("dpad_left".into(), "左".into());
        m.insert("dpad_right".into(), "右".into());
        m.insert("ok".into(), "确定".into());
        m.insert("back".into(), "返回".into());
        m.insert("home".into(), "主页".into());
        m.insert("menu".into(), "菜单".into());
        m.insert("mic".into(), "语音".into());
        m.insert("voice".into(), "语音".into());
        m.insert("volume_mute".into(), "静音".into());
        m.insert("mute".into(), "静音".into());
        m.insert("tv".into(), "TV".into());
        m
    }

    fn xiaomi_default_bindings() -> HashMap<String, KeyAction> {
        // 对齐 Python DEFAULT_BUTTON_BINDINGS
        let mut m = HashMap::new();
        m.insert("power".into(), KeyAction::SingleKey(0x1B)); // Esc
        m.insert("mic".into(), KeyAction::SingleKey(0xA5)); // Right Alt
        m.insert("up".into(), KeyAction::SingleKey(0x26));
        m.insert("down".into(), KeyAction::SingleKey(0x28));
        m.insert("left".into(), KeyAction::SingleKey(0x25));
        m.insert("right".into(), KeyAction::SingleKey(0x27));
        m.insert("ok".into(), KeyAction::SingleKey(0x0D));
        m.insert("back".into(), KeyAction::SingleKey(0x08));
        m.insert("volume_up".into(), KeyAction::SingleKey(0xAF));
        m.insert("volume_down".into(), KeyAction::SingleKey(0xAE));
        m.insert("home".into(), KeyAction::ComboKey(vec![0x5B, 0x44])); // Win+D
        m.insert("menu".into(), KeyAction::ComboKey(vec![0x10, 0x79])); // Shift+F10
        m.insert("tv".into(), KeyAction::ComboKey(vec![0x12, 0x1B])); // Alt+Esc
        m.insert("volume_mute".into(), KeyAction::SingleKey(0xAD));
        // 兼容旧 UI id
        m.insert("dpad_up".into(), KeyAction::SingleKey(0x26));
        m.insert("dpad_down".into(), KeyAction::SingleKey(0x28));
        m.insert("dpad_left".into(), KeyAction::SingleKey(0x25));
        m.insert("dpad_right".into(), KeyAction::SingleKey(0x27));
        m.insert("voice".into(), KeyAction::SingleKey(0xA5));
        m.insert("mute".into(), KeyAction::SingleKey(0xAD));
        m
    }
}

// ============================================================
// 辅助函数
// ============================================================

/// 获取配置目录路径
fn get_config_dir(app_handle: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let appdata = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {}", e))?;
    Ok(appdata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_xiaomi_config() {
        let config = ConfigManager::default_config_for("xiaomi");
        assert!(config.button_aliases.contains_key("menu"));
        assert!(config.button_aliases.contains_key("dpad_up"));
        assert!(config.button_aliases.contains_key("volume_mute"));
        assert!(config.button_bindings.contains_key("volume_up"));
        assert!(config.long_press_bindings.is_empty());
        assert_eq!(config.voice_hotkey, Some(vec!["rightalt".to_string()]));
    }

    #[test]
    fn test_legacy_device_config_defaults_gesture_fields() {
        let config: DeviceConfig = serde_json::from_str(
            r#"{
                "button_aliases": {},
                "button_bindings": {},
                "voice_hotkey": null,
                "bluetooth_address": null
            }"#,
        )
        .unwrap();
        assert!(config.long_press_bindings.is_empty());
        assert!(config.multi_click_bindings.is_empty());
        assert_eq!(config.multi_click_interval_ms, 300);
    }

    #[test]
    fn test_long_press_bindings_are_canonicalized_and_sanitized() {
        let mut config = DeviceConfig::new();
        config
            .long_press_bindings
            .insert("dpad_up".into(), KeyAction::SingleKey(0x26));
        config
            .long_press_bindings
            .insert("voice".into(), KeyAction::SingleKey(0x74));
        config
            .long_press_bindings
            .insert("not_a_button".into(), KeyAction::SingleKey(0x41));
        config
            .long_press_bindings
            .insert("menu".into(), KeyAction::None);

        ConfigManager::sanitize_xiaomi_gesture_bindings(&mut config);

        assert_eq!(
            config.long_press_bindings.get("up"),
            Some(&KeyAction::SingleKey(0x26))
        );
        assert_eq!(config.long_press_bindings.len(), 1);
    }

    #[test]
    fn test_global_settings_default() {
        let settings = GlobalSettings::default();
        assert!(!settings.autostart);
        assert_eq!(settings.language, "zh-CN");
        assert!(settings.minimize_to_tray);
        assert_eq!(settings.theme, ThemePreference::System);
    }

    #[test]
    fn test_legacy_global_settings_default_to_system_theme() {
        let settings: GlobalSettings = serde_json::from_str(
            r#"{"autostart":false,"language":"zh-CN","minimize_to_tray":true}"#,
        )
        .unwrap();
        assert_eq!(settings.theme, ThemePreference::System);
    }

    #[test]
    fn test_theme_preference_serialization() {
        for (theme, expected) in [
            (ThemePreference::System, "\"system\""),
            (ThemePreference::Light, "\"light\""),
            (ThemePreference::Dark, "\"dark\""),
        ] {
            let json = serde_json::to_string(&theme).unwrap();
            assert_eq!(json, expected);
            let decoded: ThemePreference = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, theme);
        }
    }

    #[test]
    fn test_key_action_serialization() {
        let action = KeyAction::SingleKey(0x41);
        let json = serde_json::to_string(&action).unwrap();
        let decoded: KeyAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, decoded);

        let combo = KeyAction::ComboKey(vec![0x11, 0x41]);
        let json = serde_json::to_string(&combo).unwrap();
        let decoded: KeyAction = serde_json::from_str(&json).unwrap();
        assert_eq!(combo, decoded);
    }
}
