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
    #[serde(default)]
    pub id: Option<i64>,
    pub username: String,
    pub current_device_id: Option<String>,
    #[serde(default)]
    pub avatar_path: Option<String>,
    #[serde(default)]
    pub ra_username: Option<String>,
    #[serde(default)]
    pub ra_progression: Option<RaProgression>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RaProgression {
    #[serde(default)]
    pub results: Vec<RaGameProgress>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RaGameProgress {
    pub rom_ra_id: Option<i64>,
    #[serde(default)]
    pub earned_achievements: Vec<RaEarned>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RaEarned {
    pub id: serde_json::Value,
}

impl RaEarned {
    pub fn achievement_id(&self) -> Option<u32> {
        match &self.id {
            serde_json::Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
            serde_json::Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl User {
    pub fn has_avatar(&self) -> bool {
        self.avatar_path
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty())
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum CollectionId {
    Number(i64),
    Text(String),
}

impl std::fmt::Display for CollectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Text(t) => f.write_str(t),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RemoteCollection {
    pub id: CollectionId,
    pub name: String,
    #[serde(default)]
    pub rom_ids: Vec<i64>,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub is_smart: bool,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub owner_username: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
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
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub rom_user: Option<RomUser>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RomUser {
    #[serde(default)]
    pub last_played: Option<String>,
}

impl Rom {
    pub fn added_at(&self) -> Option<i64> {
        crate::sync::parse_iso(self.created_at.as_deref()?)
    }

    pub fn last_played(&self) -> Option<i64> {
        crate::sync::parse_iso(self.rom_user.as_ref()?.last_played.as_deref()?)
    }

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
    #[serde(default)]
    pub ra_hash: Option<String>,
    #[serde(default)]
    pub ra_id: Option<i64>,
    #[serde(default)]
    pub merged_ra_metadata: Option<RaMetadata>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RaMetadata {
    #[serde(default)]
    pub achievements: Vec<RaAchievement>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RaAchievement {
    pub ra_id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub points: Option<i64>,
    #[serde(default)]
    pub badge_url: Option<String>,
    #[serde(default)]
    pub badge_url_lock: Option<String>,
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
    #[serde(default)]
    pub screenshot: Option<RemoteScreenshot>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RemoteScreenshot {
    pub id: i64,
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
        created_at: None,
        rom_user: None,
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
    fn rom_carries_when_it_was_added_and_last_played() {
        let rom: Rom = serde_json::from_value(serde_json::json!({
            "id": 1, "platform_id": 2, "name": "A", "fs_name": "a.md", "summary": null,
            "updated_at": "t", "path_cover_small": null, "path_cover_large": null,
            "fs_size_bytes": 5, "created_at": "2026-01-02T03:04:05+00:00",
            "rom_user": {"id": 9, "last_played": "2026-09-01T10:00:00+00:00", "rating": 0}
        }))
        .unwrap();
        assert_eq!(
            rom.added_at(),
            crate::sync::parse_iso("2026-01-02T03:04:05+00:00")
        );
        assert_eq!(
            rom.last_played(),
            crate::sync::parse_iso("2026-09-01T10:00:00Z")
        );
        let bare: Rom = serde_json::from_value(serde_json::json!({
            "id": 1, "platform_id": 2, "name": "A", "fs_name": "a.md", "summary": null,
            "updated_at": "t", "path_cover_small": null, "path_cover_large": null,
            "fs_size_bytes": 5, "rom_user": null
        }))
        .unwrap();
        assert_eq!(bare.added_at(), None);
        assert_eq!(bare.last_played(), None);
    }

    #[test]
    fn users_with_a_profile_picture_say_so() {
        let user: User = serde_json::from_value(serde_json::json!({
            "id": 1, "username": "beshr", "current_device_id": null,
            "avatar_path": "users/1/avatar.png"
        }))
        .unwrap();
        assert!(user.has_avatar());
        let plain: User = serde_json::from_value(serde_json::json!({
            "id": 1, "username": "beshr", "current_device_id": null, "avatar_path": ""
        }))
        .unwrap();
        assert!(!plain.has_avatar());
        let older: User = serde_json::from_value(serde_json::json!({
            "username": "beshr", "current_device_id": null
        }))
        .unwrap();
        assert!(!older.has_avatar());
    }

    #[test]
    fn collections_parse_numeric_and_virtual_ids() {
        let list: Vec<RemoteCollection> = serde_json::from_value(serde_json::json!([
            {"id": 3, "name": "Favourites", "rom_ids": [1, 2], "is_favorite": true,
             "user_id": 1, "owner_username": "me", "description": "", "rom_count": 2},
            {"id": "eyJuYW1lIjoiWmVsZGEifQ==", "name": "Zelda", "type": "franchise",
             "rom_ids": [5], "is_virtual": true}
        ]))
        .unwrap();
        assert_eq!(list[0].id.to_string(), "3");
        assert!(list[0].is_favorite);
        assert_eq!(
            list[1].id,
            CollectionId::Text("eyJuYW1lIjoiWmVsZGEifQ==".into())
        );
        assert_eq!(list[1].kind.as_deref(), Some("franchise"));
        assert_eq!(list[1].user_id, None);
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
