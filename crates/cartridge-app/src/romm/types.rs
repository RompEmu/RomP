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
