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

pub fn safe_component(name: &str) -> bool {
    let path = Path::new(name);
    path.components().count() == 1 && safe(path)
}

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

const EXTRAS: [&str; 22] = [
    "txt", "nfo", "diz", "pdf", "doc", "docx", "md", "html", "htm", "url", "jpg", "jpeg", "png",
    "gif", "bmp", "xml", "sfv", "md5", "sha1", "srm", "sav", "log",
];

/// Readmes, covers and the like. A PICO-8 cart saved as a picture (`.p8.png`) is the game.
fn is_extra(p: &Path) -> bool {
    let pico8_cart = p.file_name().is_some_and(|n| {
        n.to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".p8.png")
    });
    EXTRAS.contains(&ext(p).as_str()) && !pico8_cart
}

pub fn launch_target(rom: &RomDetail, files: &[(PathBuf, u64)]) -> Option<Launch> {
    let depth = |p: &PathBuf| p.components().count();
    if rom.platform_slug == crate::rpcs3::PLATFORM {
        let eboot = files
            .iter()
            .map(|(p, _)| p)
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("EBOOT.BIN"))
            })
            .min_by_key(|p| depth(p));
        if let Some(eboot) = eboot {
            return Some(Launch::File(eboot.clone()));
        }
    }
    for wanted in DISC_ORDER {
        let matching: Vec<&PathBuf> = files
            .iter()
            .map(|(p, _)| p)
            .filter(|p| ext(p) == wanted)
            .collect();
        let Some(shallowest) = matching.iter().map(|p| depth(p)).min() else {
            continue;
        };
        let mut found: Vec<PathBuf> = matching
            .into_iter()
            .filter(|p| depth(p) == shallowest)
            .cloned()
            .collect();
        found.sort_by_cached_key(|p| (disc_number(p).unwrap_or(u32::MAX), p.clone()));
        if found.len() == 1 || wanted == "m3u" {
            return Some(Launch::File(found.swap_remove(0)));
        }
        let name = format!("{}.m3u", rom.fs_name);
        let name = if safe_component(&name) {
            name
        } else {
            "game.m3u".to_string()
        };
        return Some(Launch::Playlist { name, discs: found });
    }
    files
        .iter()
        .filter(|(p, _)| !is_extra(p))
        .min_by(|(a, sa), (b, sb)| depth(a).cmp(&depth(b)).then(sb.cmp(sa)).then(a.cmp(b)))
        .map(|(p, _)| Launch::File(p.clone()))
}

/// The disc a file's name says it is, as RomM reads it: "(Disc 2)", "(CD 2)", "(Disk 1 of 2)",
/// "(Disc 2A)" or "(Disc B)", where B is 2.
fn disc_number(p: &Path) -> Option<u32> {
    let name = p.file_name()?.to_string_lossy().to_ascii_lowercase();
    name.split('(')
        .filter_map(|group| group.split_once(')'))
        .find_map(|(tag, _)| {
            let rest = ["disque", "disc", "disk", "cd"]
                .iter()
                .find_map(|word| tag.strip_prefix(word))?;
            let mut words: Vec<&str> = rest.split_whitespace().collect();
            if let [.., of, total] = words[..] {
                if of == "of" && total.chars().all(|c| c.is_ascii_digit()) {
                    words.truncate(words.len() - 2);
                }
            }
            let [word] = words[..] else { return None };
            let digits = word.trim_end_matches(|c: char| c.is_ascii_lowercase());
            if !digits.is_empty() && word.len() - digits.len() <= 1 {
                return digits.parse().ok();
            }
            let spaced = rest.starts_with(char::is_whitespace);
            match word.as_bytes() {
                [letter @ b'a'..=b'z'] if spaced => Some(u32::from(letter - b'a') + 1),
                _ => None,
            }
        })
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
            ra_hash: None,
            ra_id: None,
            merged_ra_metadata: None,
        }
    }

    fn sized(paths: &[PathBuf]) -> Vec<(PathBuf, u64)> {
        paths.iter().map(|p| (p.clone(), 1)).collect()
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
    fn ps3_game_folders_launch_their_eboot() {
        let mut r = rom(true, vec![]);
        r.platform_slug = "ps3".into();
        let files = [
            (PathBuf::from("PS3_DISC.SFB"), 1),
            (PathBuf::from("PS3_GAME/USRDIR/data/level.iso"), 900),
            (PathBuf::from("PS3_GAME/USRDIR/data/EBOOT.BIN"), 5),
            (PathBuf::from("PS3_GAME/USRDIR/EBOOT.BIN"), 10),
        ];
        assert_eq!(
            launch_target(&r, &files),
            Some(Launch::File(PathBuf::from("PS3_GAME/USRDIR/EBOOT.BIN")))
        );
        let iso = [
            (PathBuf::from("Game.iso"), 1),
            (PathBuf::from("Game.dkey"), 1),
        ];
        assert_eq!(
            launch_target(&r, &iso),
            Some(Launch::File(PathBuf::from("Game.iso")))
        );
    }

    #[test]
    fn two_discs_make_a_playlist() {
        let r = rom(true, vec![]);
        let files = [
            PathBuf::from("Arc (Disc 2).chd"),
            PathBuf::from("Arc (Disc 1).chd"),
        ];
        match launch_target(&r, &sized(&files)).unwrap() {
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
    fn discs_play_in_the_order_their_names_number_them() {
        let r = rom(true, vec![]);
        let files = sized(&[
            PathBuf::from("Game (Disc 10).chd"),
            PathBuf::from("Game (Bonus).chd"),
            PathBuf::from("Game (Disc 2).chd"),
            PathBuf::from("Game (Disc 1).chd"),
        ]);
        match launch_target(&r, &files).unwrap() {
            Launch::Playlist { discs, .. } => assert_eq!(
                discs,
                [
                    "Game (Disc 1).chd",
                    "Game (Disc 2).chd",
                    "Game (Disc 10).chd",
                    "Game (Bonus).chd"
                ]
                .map(PathBuf::from)
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn disc_numbers_are_read_the_ways_rom_sets_write_them() {
        let number = |name: &str| disc_number(Path::new(name));
        assert_eq!(number("Game (Disc 2).cue"), Some(2));
        assert_eq!(number("Game (CD 3).chd"), Some(3));
        assert_eq!(number("Game (Disk 1 of 2).adf"), Some(1));
        assert_eq!(number("Game (disque 2).cue"), Some(2));
        assert_eq!(number("Game (Disc 2A).cue"), Some(2));
        assert_eq!(number("Game (Disc B).cue"), Some(2));
        assert_eq!(number("Game (CDi).chd"), None);
        assert_eq!(number("Game (Discovery).chd"), None);
        assert_eq!(number("Game.chd"), None);
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
            matches!(launch_target(&r, &sized(&files)), Some(Launch::File(p)) if p == Path::new("Game.cue"))
        );
        assert!(
            matches!(launch_target(&r, &sized(&[PathBuf::from("Zelda.sfc")])), Some(Launch::File(p)) if p == Path::new("Zelda.sfc"))
        );
        assert!(launch_target(&r, &[]).is_none());
    }

    #[test]
    fn playlist_name_never_leaves_the_game_folder() {
        let mut r = rom(true, vec![]);
        r.fs_name = "../../evil".into();
        let files = [PathBuf::from("a.chd"), PathBuf::from("b.chd")];
        match launch_target(&r, &sized(&files)).unwrap() {
            Launch::Playlist { name, .. } => assert_eq!(name, "game.m3u"),
            other => panic!("{other:?}"),
        }
        assert!(safe_component("psx"));
        assert!(!safe_component(".."));
        assert!(!safe_component("a/b"));
        assert!(!safe_component(""));
    }

    #[test]
    fn discs_in_a_sub_folder_are_found() {
        let r = rom(true, vec![]);
        let files = sized(&[
            PathBuf::from("readme.txt"),
            PathBuf::from("CD/Game.cue"),
            PathBuf::from("CD/Game.bin"),
        ]);
        assert_eq!(
            launch_target(&r, &files),
            Some(Launch::File(PathBuf::from("CD/Game.cue")))
        );
    }

    #[test]
    fn fallback_skips_extras_and_prefers_the_largest_file() {
        let r = rom(true, vec![]);
        let files = vec![
            (PathBuf::from("a-manual.pdf"), 900),
            (PathBuf::from("b-readme.txt"), 5),
            (PathBuf::from("game.bin"), 100),
            (PathBuf::from("patch.ips"), 10),
        ];
        assert_eq!(
            launch_target(&r, &files),
            Some(Launch::File(PathBuf::from("game.bin")))
        );
        assert_eq!(launch_target(&r, &[(PathBuf::from("notes.txt"), 1)]), None);
    }

    #[test]
    fn a_pico8_cart_saved_as_a_picture_is_the_game() {
        let r = rom(false, vec![]);
        let cart = PathBuf::from("Across The River.p8.png");
        assert_eq!(
            launch_target(&r, &[(cart.clone(), 32_224)]),
            Some(Launch::File(cart.clone()))
        );
        let with_cover = [(PathBuf::from("cover.png"), 90_000), (cart.clone(), 32_224)];
        assert_eq!(launch_target(&r, &with_cover), Some(Launch::File(cart)));
    }
}
