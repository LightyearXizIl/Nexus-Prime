pub mod shared;
pub mod xiaomi;

use parking_lot::RwLock;

/// Represents the type of bridge device
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BridgeType {
    Xiaomi,
}

impl std::fmt::Display for BridgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeType::Xiaomi => write!(f, "小米遥控器"),
        }
    }
}

/// Status of a bridge connection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl serde::Serialize for BridgeStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            BridgeStatus::Disconnected => serializer.serialize_str("Disconnected"),
            BridgeStatus::Connecting => serializer.serialize_str("Connecting"),
            BridgeStatus::Connected => serializer.serialize_str("Connected"),
            BridgeStatus::Error(msg) => serializer.serialize_str(&format!("Error|{}", msg)),
        }
    }
}

impl<'de> serde::Deserialize<'de> for BridgeStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "Disconnected" => BridgeStatus::Disconnected,
            "Connecting" => BridgeStatus::Connecting,
            "Connected" => BridgeStatus::Connected,
            _ if s.starts_with("Error|") => BridgeStatus::Error(s[6..].to_string()),
            _ => {
                log::warn!("Unknown BridgeStatus value: {s}, defaulting to Disconnected");
                BridgeStatus::Disconnected
            }
        })
    }
}

impl std::fmt::Display for BridgeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeStatus::Disconnected => write!(f, "未连接"),
            BridgeStatus::Connecting => write!(f, "连接中..."),
            BridgeStatus::Connected => write!(f, "已连接"),
            BridgeStatus::Error(e) => write!(f, "错误: {}", e),
        }
    }
}

/// Device information returned to the frontend
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceInfo {
    pub bridge_type: BridgeType,
    pub status: BridgeStatus,
    pub device_name: Option<String>,
    pub device_address: Option<String>,
    pub battery_level: Option<u8>,
    /// `Some(true)` only when the device explicitly reports that it is charging.
    /// `None` means the BLE peripheral does not expose, or cannot determine, charge state.
    pub battery_charging: Option<bool>,
}

/// Global bridge state shared across the application
pub struct BridgeState {
    pub xiaomi: RwLock<DeviceInfo>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            xiaomi: RwLock::new(DeviceInfo {
                bridge_type: BridgeType::Xiaomi,
                status: BridgeStatus::Disconnected,
                device_name: None,
                device_address: None,
                battery_level: None,
                battery_charging: None,
            }),
        }
    }

    pub fn update_status(&self, bridge_type: BridgeType, status: BridgeStatus) {
        let info = match bridge_type {
            BridgeType::Xiaomi => &self.xiaomi,
        };
        let mut guard = info.write();
        let should_clear_device = status != BridgeStatus::Connected;
        guard.status = status;
        if should_clear_device {
            guard.device_name = None;
            guard.device_address = None;
            guard.battery_level = None;
            guard.battery_charging = None;
        }
    }

    /// Update full device info (name, address, battery) after successful connection.
    /// Also sets the status to Connected.
    pub fn update_device_info(
        &self,
        bridge_type: BridgeType,
        name: Option<String>,
        address: Option<String>,
        battery: Option<u8>,
    ) {
        let info = match bridge_type {
            BridgeType::Xiaomi => &self.xiaomi,
        };
        let mut guard = info.write();
        guard.status = BridgeStatus::Connected;
        if let Some(n) = name { guard.device_name = Some(n); }
        if let Some(a) = address { guard.device_address = Some(a); }
        if let Some(b) = battery { guard.battery_level = Some(b); }
    }

    /// Updates only the charge state reported by the Battery Level Status characteristic.
    pub fn update_battery_charging(&self, bridge_type: BridgeType, charging: Option<bool>) {
        let info = match bridge_type {
            BridgeType::Xiaomi => &self.xiaomi,
        };
        let mut guard = info.write();
        guard.battery_charging = charging;
    }

    pub fn get_info(&self, bridge_type: BridgeType) -> DeviceInfo {
        match bridge_type {
            BridgeType::Xiaomi => self.xiaomi.read().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_connected_statuses_clear_stale_device_metadata() {
        for status in [
            BridgeStatus::Connecting,
            BridgeStatus::Disconnected,
            BridgeStatus::Error("BLE open failed".into()),
        ] {
            let state = BridgeState::new();
            state.update_device_info(
                BridgeType::Xiaomi,
                Some("小米蓝牙遥控器 2 Pro".into()),
                Some("00:11:22:33:44:55".into()),
                Some(80),
            );
            state.update_battery_charging(BridgeType::Xiaomi, Some(true));
            state.update_status(BridgeType::Xiaomi, status);

            let info = state.get_info(BridgeType::Xiaomi);
            assert!(info.device_name.is_none());
            assert!(info.device_address.is_none());
            assert!(info.battery_level.is_none());
            assert!(info.battery_charging.is_none());
        }
    }
}
