use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Heartbeat {
    #[serde(rename = "SYSTEM")]
    pub system: SystemInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SystemInfo {
    #[serde(rename = "VERSION")]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAuth {
    pub device_code: String,
    pub user_code: String,
    pub verification_path_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Approved(String),
}
