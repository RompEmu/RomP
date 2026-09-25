use crate::romm::types::{RomDetail, RomFile};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub file: RomFile,
    pub rel_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    File(PathBuf),
    Playlist { name: String, discs: Vec<PathBuf> },
}

const DISC_ORDER: [&str; 8] = ["m3u", "cue", "gdi", "ccd", "cdi", "chd", "iso", "pbp"];

fn safe(rel: &Path) -> bool {
    !rel.as_os_str().is_empty() && rel.components().all(|c| matches!(c, Component::Normal(_)))
}

pub fn plan(rom: &RomDetail) -> Result<Vec<PlannedFile>, String> {
    let base = if rom.has_multiple_files {
        format!("{}/{}", rom.fs_path, rom.fs_name)
    } else {
        rom.fs_path.clone()
    };
    rom.files
        .iter()
        .map(|file| {
            let dir = file
                .file_path
                .strip_prefix(&base)
                .unwrap_or("")
                .trim_start_matches('/');
            let rel = Path::new(dir).join(&file.file_name);
            if file.file_name.is_empty() || !safe(&rel) {
                return Err(format!(
                    "unsafe file path: {}/{}",
                    file.file_path, file.file_name
                ));
            }
            Ok(PlannedFile {
                file: file.clone(),
                rel_path: rel,
            })
        })
        .collect()
}

fn ext(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn launch_target(rom: &RomDetail, rel_paths: &[PathBuf]) -> Option<Launch> {
    let mut top: Vec<&PathBuf> = rel_paths
        .iter()
        .filter(|p| p.components().count() == 1)
        .collect();
    top.sort();
    for wanted in DISC_ORDER {
        let found: Vec<PathBuf> = top
            .iter()
            .filter(|p| ext(p) == wanted)
            .map(|p| (*p).clone())
            .collect();
        match found.len() {
            0 => continue,
            1 => return Some(Launch::File(found[0].clone())),
            _ if wanted == "m3u" => return Some(Launch::File(found[0].clone())),
            _ => {
                return Some(Launch::Playlist {
                    name: format!("{}.m3u", rom.fs_name),
                    discs: found,
                })
            }
        }
    }
    top.first().map(|p| Launch::File((*p).clone()))
}

pub fn m3u(discs: &[PathBuf]) -> String {
    discs.iter().map(|d| format!("{}\n", d.display())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: i64, path: &str, name: &str) -> RomFile {
        RomFile {
            id,
            file_name: name.into(),
            file_path: path.into(),
            file_size_bytes: 1,
            sha1_hash: None,
        }
    }

    fn rom(multi: bool, files: Vec<RomFile>) -> RomDetail {
        RomDetail {
            id: 1,
            platform_slug: "psx".into(),
            fs_name: "Arc III".into(),
            fs_path: "roms/psx".into(),
            has_multiple_files: multi,
            files,
        }
    }

    fn rels(r: &RomDetail) -> Vec<PathBuf> {
        plan(r).unwrap().into_iter().map(|p| p.rel_path).collect()
    }

    #[test]
    fn single_file_goes_to_root() {
        assert_eq!(
            rels(&rom(false, vec![file(1, "roms/psx", "Game.chd")])),
            [PathBuf::from("Game.chd")]
        );
    }

    #[test]
    fn multi_file_keeps_sub_folders() {
        let r = rom(
            true,
            vec![
                file(1, "roms/psx/Arc III", "Disc 1.chd"),
                file(2, "roms/psx/Arc III/extra", "readme.txt"),
            ],
        );
        assert_eq!(
            rels(&r),
            [
                PathBuf::from("Disc 1.chd"),
                PathBuf::from("extra/readme.txt")
            ]
        );
    }

    #[test]
    fn plan_rejects_escaping_paths() {
        assert!(plan(&rom(false, vec![file(1, "roms/psx", "../evil")])).is_err());
        assert!(plan(&rom(true, vec![file(1, "roms/psx/Arc III/../../..", "x")])).is_err());
        assert!(plan(&rom(false, vec![file(1, "roms/psx", "/etc/passwd")])).is_err());
        assert!(plan(&rom(false, vec![file(1, "roms/psx", "")])).is_err());
    }

    #[test]
    fn two_discs_make_a_playlist() {
        let r = rom(true, vec![]);
        let files = [
            PathBuf::from("Arc (Disc 2).chd"),
            PathBuf::from("Arc (Disc 1).chd"),
        ];
        match launch_target(&r, &files).unwrap() {
            Launch::Playlist { name, discs } => {
                assert_eq!(name, "Arc III.m3u");
                assert_eq!(
                    discs,
                    [
                        PathBuf::from("Arc (Disc 1).chd"),
                        PathBuf::from("Arc (Disc 2).chd")
                    ]
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            m3u(&[PathBuf::from("a.chd"), PathBuf::from("b.chd")]),
            "a.chd\nb.chd\n"
        );
    }

    #[test]
    fn cue_wins_over_bin_and_carts_use_their_file() {
        let r = rom(true, vec![]);
        let files = [
            PathBuf::from("Game (Track 1).bin"),
            PathBuf::from("Game.cue"),
            PathBuf::from("Game (Track 2).bin"),
        ];
        assert!(
            matches!(launch_target(&r, &files), Some(Launch::File(p)) if p == Path::new("Game.cue"))
        );
        assert!(
            matches!(launch_target(&r, &[PathBuf::from("Zelda.sfc")]), Some(Launch::File(p)) if p == Path::new("Zelda.sfc"))
        );
        assert!(launch_target(&r, &[PathBuf::from("sub/x.chd")]).is_none());
    }
}
