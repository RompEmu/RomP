use serde::{Deserialize, Serialize};

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
    Approved { token: String, scopes: Vec<String> },
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub username: String,
    pub current_device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Platform {
    pub id: i64,
    pub slug: String,
    pub display_name: String,
    pub rom_count: i64,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RomMetadata {
    pub genres: Vec<String>,
    pub franchises: Vec<String>,
    pub companies: Vec<String>,
    pub publishers: Vec<String>,
    pub developers: Vec<String>,
    pub player_count: Option<String>,
    pub first_release_date: Option<i64>,
    pub average_rating: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rom {
    pub id: i64,
    pub platform_id: i64,
    pub name: Option<String>,
    pub fs_name: String,
    pub summary: Option<String>,
    pub updated_at: String,
    pub path_cover_small: Option<String>,
    pub path_cover_large: Option<String>,
    pub fs_size_bytes: i64,
    #[serde(default)]
    pub metadatum: Option<RomMetadata>,
    #[serde(default)]
    pub merged_screenshots: Vec<String>,
}

impl Rom {
    pub fn title(&self) -> &str {
        match self.name.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name,
            _ => self
                .fs_name
                .rsplit_once('.')
                .map_or(self.fs_name.as_str(), |(stem, _)| stem),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RomPage {
    pub items: Vec<Rom>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RomFile {
    pub id: i64,
    pub file_name: String,
    pub file_path: String,
    pub file_size_bytes: i64,
    pub sha1_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RomDetail {
    pub id: i64,
    pub platform_slug: String,
    pub fs_name: String,
    pub fs_path: String,
    pub has_multiple_files: bool,
    pub files: Vec<RomFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Firmware {
    pub id: i64,
    pub file_name: String,
    pub sha1_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClientSave {
    pub rom_id: i64,
    pub file_name: String,
    pub slot: Option<String>,
    pub emulator: Option<String>,
    pub content_hash: Option<String>,
    pub updated_at: String,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SyncOp {
    pub action: String,
    pub rom_id: i64,
    pub save_id: Option<i64>,
    #[serde(default)]
    pub slot: Option<String>,
    pub file_name: String,
    pub server_updated_at: Option<String>,
    pub server_content_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Negotiation {
    pub session_id: i64,
    pub operations: Vec<SyncOp>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RemoteSave {
    pub id: i64,
    pub rom_id: i64,
    pub file_name: String,
    pub slot: Option<String>,
    pub updated_at: String,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RemoteState {
    pub id: i64,
    pub rom_id: i64,
    pub file_name: String,
    pub updated_at: String,
}

#[cfg(test)]
pub(crate) fn rom(id: i64, platform_id: i64, name: &str, updated_at: &str) -> Rom {
    Rom {
        id,
        platform_id,
        name: Some(name.into()),
        fs_name: format!("{name}.bin"),
        summary: None,
        updated_at: updated_at.into(),
        path_cover_small: Some(format!(
            "/assets/romm/resources/roms/{platform_id}/{id}/cover/small.png"
        )),
        path_cover_large: None,
        fs_size_bytes: 1024,
        metadatum: None,
        merged_screenshots: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_metadata_and_screenshots_parse() {
        let rom: Rom = serde_json::from_value(serde_json::json!({
            "id": 1, "platform_id": 2, "name": "Alien Soldier", "fs_name": "a.md",
            "summary": null, "updated_at": "t", "path_cover_small": null,
            "path_cover_large": null, "fs_size_bytes": 5,
            "metadatum": {"rom_id": 1, "genres": ["Shooter"], "developers": ["Treasure"],
                "player_count": "1", "first_release_date": 761961600000_i64,
                "average_rating": 81.5, "age_ratings": []},
            "merged_screenshots": ["/assets/romm/resources/roms/2/1/screenshots/0.jpg"]
        }))
        .unwrap();
        let meta = rom.metadatum.unwrap();
        assert_eq!(meta.genres, ["Shooter"]);
        assert_eq!(meta.first_release_date, Some(761_961_600_000));
        assert_eq!(meta.average_rating, Some(81.5));
        assert!(meta.publishers.is_empty());
        assert_eq!(rom.merged_screenshots.len(), 1);
    }

    #[test]
    fn platform_category_is_optional() {
        let p: Platform = serde_json::from_value(serde_json::json!({
            "id": 1, "slug": "snes", "display_name": "SNES", "rom_count": 3, "category": "Console"
        }))
        .unwrap();
        assert_eq!(p.category.as_deref(), Some("Console"));
        let p: Platform = serde_json::from_value(serde_json::json!({
            "id": 1, "slug": "snes", "display_name": "SNES", "rom_count": 3
        }))
        .unwrap();
        assert_eq!(p.category, None);
    }

    #[test]
    fn title_falls_back_to_file_name_without_extension() {
        let mut r = rom(1, 1, "Tir Na Nog", "t");
        assert_eq!(r.title(), "Tir Na Nog");
        r.name = Some("  ".into());
        r.fs_name = "Tir Na Nog (1984).tzx".into();
        assert_eq!(r.title(), "Tir Na Nog (1984)");
        r.name = None;
        assert_eq!(r.title(), "Tir Na Nog (1984)");
    }
}
