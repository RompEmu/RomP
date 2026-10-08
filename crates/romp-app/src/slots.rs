use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tr::tr;

/// The automatic save made when a game closes, which "continue" loads.
pub const AUTO: u8 = 0;
pub const THUMBNAIL_WIDTH: u32 = 256;

fn stem(slot: u8) -> String {
    if slot == AUTO {
        "auto".into()
    } else {
        format!("slot-{slot}")
    }
}

pub fn state_path(dir: &Path, slot: u8) -> PathBuf {
    dir.join(format!("{}.state", stem(slot)))
}

pub fn thumbnail_path(dir: &Path, slot: u8) -> PathBuf {
    dir.join(format!("{}.png", stem(slot)))
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlotInfo {
    pub slot: u8,
    pub saved_at: Option<SystemTime>,
    pub thumbnail: Option<PathBuf>,
}

impl SlotInfo {
    pub fn is_empty(&self) -> bool {
        self.saved_at.is_none()
    }

    pub fn label(&self) -> String {
        if self.slot == AUTO {
            tr!("Where you left off")
        } else {
            tr!("Slot {}", self.slot)
        }
    }
}

/// What's in a game's save folder for `slots`, with when each was saved and its picture.
pub fn read(dir: &Path, slots: impl IntoIterator<Item = u8>) -> Vec<SlotInfo> {
    slots
        .into_iter()
        .map(|slot| {
            let saved_at = std::fs::metadata(state_path(dir, slot))
                .and_then(|m| m.modified())
                .ok();
            let thumbnail =
                Some(thumbnail_path(dir, slot)).filter(|p| saved_at.is_some() && p.is_file());
            SlotInfo {
                slot,
                saved_at,
                thumbnail,
            }
        })
        .collect()
}

/// When a slot was saved, said the way people do: "5 minutes ago", "Yesterday".
pub fn when(saved_at: SystemTime, now: SystemTime) -> String {
    let age = now
        .duration_since(saved_at)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    match age {
        0..60 => tr!("Just now"),
        60..3_600 => tr!("{n} minute ago" | "{n} minutes ago" % age / 60),
        3_600..86_400 => tr!("{n} hour ago" | "{n} hours ago" % age / 3_600),
        86_400..172_800 => tr!("Yesterday"),
        172_800..2_592_000 => tr!("{n} day ago" | "{n} days ago" % age / 86_400),
        _ => {
            let date = crate::sync::iso_utc(saved_at);
            tr!("On {}", &date[..10])
        }
    }
}

/// How a slot shows in the game menu and on the game's page, marked when it's the current one.
pub fn card(info: &SlotInfo, now: SystemTime, current: Option<u8>) -> crate::SlotCard {
    let thumbnail = info
        .thumbnail
        .as_ref()
        .and_then(|p| slint::Image::load_from_path(p).ok());
    crate::SlotCard {
        slot: i32::from(info.slot),
        label: info.label().into(),
        detail: info
            .saved_at
            .map_or_else(|| tr!("Empty"), |t| when(t, now))
            .into(),
        has_thumbnail: thumbnail.is_some(),
        thumbnail: thumbnail.unwrap_or_default(),
        empty: info.is_empty(),
        current: Some(info.slot) == current,
        in_game: false,
    }
}

/// The in-game save's card on the game's page, saying where it stands with RomM.
pub fn in_game_card(status: &crate::saves::InGameSaveStatus, now: SystemTime) -> crate::SlotCard {
    use crate::saves::InGameSaveStatus::*;
    let detail = match status {
        Synced { at } => {
            let at = SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(*at).unwrap_or(0));
            tr!("Synced with RomM · {}", when(at, now))
        }
        NotSynced => tr!("Not synced yet"),
        Waiting => tr!("Will sync when the server is reachable"),
        OnServer => tr!("On RomM · comes down when you play"),
        ThisComputerOnly => tr!("On this computer only · pair RomP to sync it"),
    };
    crate::SlotCard {
        slot: -1,
        label: tr!("In-game save").into(),
        detail: detail.into(),
        thumbnail: slint::Image::default(),
        has_thumbnail: false,
        empty: false,
        current: false,
        in_game: true,
    }
}

/// A small copy of the frame for a slot's picture, sampled so it costs next to nothing.
pub fn thumbnail(rgba: &[u8], width: u32, height: u32, aspect: f32) -> Option<image::RgbaImage> {
    if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
        return None;
    }
    let aspect = if aspect > 0.0 {
        aspect
    } else {
        width as f32 / height as f32
    };
    let out_w = THUMBNAIL_WIDTH;
    let out_h = ((out_w as f32 / aspect).round() as u32).max(1);
    let mut out = image::RgbaImage::new(out_w, out_h);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let sx = (u64::from(x) * u64::from(width) / u64::from(out_w)) as usize;
        let sy = (u64::from(y) * u64::from(height) / u64::from(out_h)) as usize;
        let i = (sy * width as usize + sx) * 4;
        *pixel = image::Rgba([rgba[i], rgba[i + 1], rgba[i + 2], 255]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_in_game_save_card_says_where_the_save_stands() {
        use crate::saves::InGameSaveStatus::*;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let card = in_game_card(&Synced { at: 10_000 - 120 }, now);
        assert!(card.in_game);
        assert_eq!(card.label, "In-game save");
        assert_eq!(card.detail, "Synced with RomM · 2 minutes ago");
        assert_eq!(
            in_game_card(&OnServer, now).detail,
            "On RomM · comes down when you play"
        );
        assert_eq!(
            in_game_card(&Waiting, now).detail,
            "Will sync when the server is reachable"
        );
    }

    #[test]
    fn slots_report_what_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(state_path(dir.path(), 2), b"state").unwrap();
        std::fs::write(thumbnail_path(dir.path(), 2), b"png").unwrap();
        std::fs::write(thumbnail_path(dir.path(), 3), b"stray picture").unwrap();
        std::fs::write(state_path(dir.path(), AUTO), b"state").unwrap();
        let slots = read(dir.path(), [AUTO, 1, 2, 3]);
        assert_eq!(slots[0].label(), "Where you left off");
        assert!(!slots[0].is_empty() && slots[0].thumbnail.is_none());
        assert!(slots[1].is_empty());
        assert_eq!(slots[2].thumbnail, Some(dir.path().join("slot-2.png")));
        assert!(slots[3].is_empty() && slots[3].thumbnail.is_none());
        assert_eq!(slots[2].label(), "Slot 2");
    }

    #[test]
    fn save_times_read_naturally() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_850_000);
        let ago = |secs| when(now - Duration::from_secs(secs), now);
        assert_eq!(ago(10), "Just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(5 * 60), "5 minutes ago");
        assert_eq!(ago(2 * 3_600), "2 hours ago");
        assert_eq!(ago(30 * 3_600), "Yesterday");
        assert_eq!(ago(4 * 86_400), "4 days ago");
        assert_eq!(ago(90 * 86_400), "On 2026-07-03");
    }

    #[test]
    fn thumbnails_keep_the_picture_shape() {
        let rgba = vec![200u8; 320 * 240 * 4];
        let picture = thumbnail(&rgba, 320, 240, 4.0 / 3.0).unwrap();
        assert_eq!(picture.dimensions(), (256, 192));
        assert!(picture.pixels().all(|p| p.0 == [200, 200, 200, 255]));
        assert!(thumbnail(&rgba[..10], 320, 240, 0.0).is_none());
    }
}
