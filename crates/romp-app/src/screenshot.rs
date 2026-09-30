use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const FOLDER: &str = "screenshots";

/// The frame as an opaque image, stretched to the shape the game is shown at.
pub fn image(rgba: &[u8], width: u32, height: u32, aspect: f32) -> Option<image::RgbaImage> {
    let len = (width as usize) * (height as usize) * 4;
    let mut frame = image::RgbaImage::from_raw(width, height, rgba.get(..len)?.to_vec())?;
    for pixel in frame.pixels_mut() {
        pixel.0[3] = 255;
    }
    let shown = (height as f32 * aspect).round() as u32;
    if aspect <= 0.0 || shown == 0 || shown.abs_diff(width) <= 1 {
        return Some(frame);
    }
    Some(image::imageops::resize(
        &frame,
        shown,
        height,
        image::imageops::FilterType::Triangle,
    ))
}

pub fn file_name(title: &str, when: SystemTime) -> String {
    let safe: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_'()&!,.".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    let safe = safe.trim_matches('.');
    let stamp = crate::sync::iso_utc(when)
        .trim_end_matches("+00:00")
        .replace('T', " ")
        .replace(':', "-");
    if safe.is_empty() {
        format!("Screenshot {stamp}.png")
    } else {
        format!("{safe} {stamp}.png")
    }
}

/// Saves the picture in `dir`, adding a number when a shot was already taken that second.
pub fn save(
    dir: &Path,
    title: &str,
    when: SystemTime,
    picture: &image::RgbaImage,
) -> image::ImageResult<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = file_name(title, when);
    let mut path = dir.join(&name);
    let stem = name.trim_end_matches(".png");
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).png"));
        n += 1;
    }
    picture.save_with_format(&path, image::ImageFormat::Png)?;
    Ok(path)
}

/// The pictures in a game's screenshot folder, newest first.
pub fn local(dir: &Path) -> Vec<PathBuf> {
    let mut shots: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
                return None;
            }
            Some((e.metadata().ok()?.modified().ok()?, path))
        })
        .collect();
    shots.sort_by(|a, b| b.cmp(a));
    shots.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn when() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_790_762_400)
    }

    #[test]
    fn frames_become_opaque_and_keep_the_shape_they_are_shown_at() {
        let rgba = vec![10u8; 256 * 224 * 4];
        let square = image(&rgba, 256, 224, 256.0 / 224.0).unwrap();
        assert_eq!(square.dimensions(), (256, 224));
        assert!(square.pixels().all(|p| p.0[3] == 255));
        let tv = image(&rgba, 256, 224, 4.0 / 3.0).unwrap();
        assert_eq!(tv.dimensions(), (299, 224));
        assert_eq!(
            image(&rgba, 256, 224, 0.0).unwrap().dimensions(),
            (256, 224)
        );
        assert!(image(&rgba[..10], 256, 224, 0.0).is_none());
    }

    #[test]
    fn files_are_named_after_the_game_and_time() {
        assert_eq!(
            file_name("Super Mario World", when()),
            "Super Mario World 2026-09-30 10-00-00.png"
        );
        assert_eq!(
            file_name("Pokémon: Red/Blue", when()),
            "Pokémon Red Blue 2026-09-30 10-00-00.png"
        );
        assert_eq!(
            file_name("../", when()),
            "Screenshot 2026-09-30 10-00-00.png"
        );
    }

    #[test]
    fn shots_in_the_same_second_get_numbered_and_list_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let picture = image::RgbaImage::new(2, 2);
        let first = save(dir.path(), "Game", when(), &picture).unwrap();
        let second = save(dir.path(), "Game", when(), &picture).unwrap();
        assert_eq!(
            second.file_name().unwrap(),
            "Game 2026-09-30 10-00-00 (2).png"
        );
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        let old = std::fs::File::options().write(true).open(&first).unwrap();
        old.set_modified(when()).unwrap();
        assert_eq!(local(dir.path()), [second, first]);
        assert!(local(&dir.path().join("missing")).is_empty());
    }
}
