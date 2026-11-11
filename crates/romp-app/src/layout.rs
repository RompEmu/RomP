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
    let shallowest_of = |wanted: &str| {
        let matching: Vec<&PathBuf> = files
            .iter()
            .map(|(p, _)| p)
            .filter(|p| ext(p) == wanted)
            .collect();
        let shallowest = matching.iter().map(|p| depth(p)).min()?;
        let mut found: Vec<PathBuf> = matching
            .into_iter()
            .filter(|p| depth(p) == shallowest)
            .cloned()
            .collect();
        found.sort_by_cached_key(|p| (disc_number(p).unwrap_or(u32::MAX), p.clone()));
        Some(found)
    };
    // A disc beside the game's folder wins over one in a sub-folder, such as a translation.
    let disc_depth = files
        .iter()
        .map(|(p, _)| p)
        .filter(|p| DISC_ORDER.contains(&ext(p).as_str()))
        .map(&depth)
        .min();
    for wanted in DISC_ORDER {
        let Some(mut found) = shallowest_of(wanted) else {
            continue;
        };
        if found.first().map(depth) != disc_depth {
            continue;
        }
        if found.len() == 1 || wanted == "m3u" {
            return Some(Launch::File(found.swap_remove(0)));
        }
        return Some(playlist(&rom.fs_name, found));
    }
    if let Some(mut zips) = shallowest_of("zip").filter(|z| z.len() > 1) {
        let disks = crate::cores::core_for_platform(&rom.platform_slug)
            .is_some_and(|c| crate::cores::reads_zipped_disks(c.id));
        if disks {
            // PUAE picks the Amiga model from the name it's given, so the list carries the disks' tags,
            // such as "(AGA)", which the game's own name may not.
            let first = zips[0]
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned());
            return Some(playlist(first.as_deref().unwrap_or(&rom.fs_name), zips));
        }
        // Several zips are also an arcade game and its BIOS, which the fallback below handles.
        if zips.iter().any(|z| disc_number(z).is_some()) {
            return Some(Launch::File(zips.swap_remove(0)));
        }
    }
    files
        .iter()
        .filter(|(p, _)| !is_extra(p))
        .min_by(|(a, sa), (b, sb)| depth(a).cmp(&depth(b)).then(sb.cmp(sa)).then(a.cmp(b)))
        .map(|(p, _)| Launch::File(p.clone()))
}

/// Files RomM keeps beside a game that never start it.
const NOT_PLAYABLE: [&str; 6] = [
    "manual",
    "walkthrough",
    "screenshot",
    "soundtrack",
    "patch",
    "cheat",
];

/// Disc sheets, which start a disc made of the track files beside them.
const SHEETS: [&str; 4] = ["cue", "gdi", "ccd", "mds"];
const TRACKS: [&str; 4] = ["bin", "img", "raw", "sub"];

fn folder(p: &Path) -> &Path {
    p.parent().unwrap_or(Path::new(""))
}

/// Discs and their tracks, which a playlist beside them starts.
const DISC_MEDIA: [&str; 12] = [
    "cue", "gdi", "ccd", "cdi", "chd", "iso", "pbp", "mds", "bin", "img", "raw", "sub",
];

pub fn is_playlist(p: &Path) -> bool {
    ext(p) == "m3u"
}

/// Inside a folder or named with a leading dot, where sets keep the discs their playlist starts.
fn is_hidden(p: &Path) -> bool {
    p.components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
}

/// Whether `p` is a disc that a playlist in its folder starts, so it isn't a set of its own.
fn in_playlist_folder(p: &Path, files: &[PlannedFile]) -> bool {
    DISC_MEDIA.contains(&ext(p).as_str())
        && files
            .iter()
            .any(|f| is_playlist(&f.rel_path) && folder(&f.rel_path) == folder(p))
}

/// Whether `p` is a track of a disc whose sheet is among `files`, so it doesn't start alone.
fn is_track(p: &Path, files: &[PlannedFile]) -> bool {
    TRACKS.contains(&ext(p).as_str())
        && files.iter().any(|f| {
            folder(&f.rel_path) == folder(p) && SHEETS.contains(&ext(&f.rel_path).as_str())
        })
}

/// The files a player can choose to start a multi-file game with, such as a game with its
/// translations or a folder of hacks of one game, the one RomP starts by itself first; or
/// nothing when RomP starts the whole set itself: several discs, a PS3 game. A playlist stands
/// for its discs, which share its saves.
pub fn file_choices(rom: &RomDetail, files: &[PlannedFile]) -> Vec<PathBuf> {
    // RomM counts only the files at the top of a game's folder as multiple, so a game with
    // translations or hacks in sub-folders is still worth looking through.
    if files.len() < 2 || rom.platform_slug == crate::rpcs3::PLATFORM {
        return Vec::new();
    }
    let sized: Vec<(PathBuf, u64)> = files
        .iter()
        .map(|f| (f.rel_path.clone(), f.file.file_size_bytes.max(0) as u64))
        .collect();
    let Some(Launch::File(automatic)) = launch_target(rom, &sized) else {
        return Vec::new();
    };
    let mut choices: Vec<PathBuf> = files
        .iter()
        .filter(|f| {
            f.file
                .category
                .as_deref()
                .is_none_or(|c| !NOT_PLAYABLE.contains(&c))
        })
        .map(|f| f.rel_path.clone())
        .filter(|p| {
            !is_extra(p) && !is_hidden(p) && !is_track(p, files) && !in_playlist_folder(p, files)
        })
        .collect();
    if choices.len() < 2 {
        return Vec::new();
    }
    choices.sort_by_cached_key(|p| (*p != automatic, p.to_string_lossy().to_lowercase()));
    choices
}

/// The files that starting with `pick` needs: it, and the tracks beside it when it's a sheet.
pub fn files_for(pick: &Path, files: &[PlannedFile]) -> Vec<PathBuf> {
    let sheet = SHEETS.contains(&ext(pick).as_str());
    files
        .iter()
        .map(|f| &f.rel_path)
        .filter(|p| {
            *p == pick || (sheet && folder(p) == folder(pick) && TRACKS.contains(&ext(p).as_str()))
        })
        .cloned()
        .collect()
}

/// The files a playlist at `m3u` names, as paths in the game's folder: one a line, except
/// comments, written with either slash, and with RetroArch's "|label" after the file. A line
/// that would leave the game's folder names nothing.
pub fn playlist_entries(m3u: &Path, text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(|line| line.trim_start_matches('\u{feff}').trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let name = line.split('|').next()?.trim().replace('\\', "/");
            let mut rel = folder(m3u).to_path_buf();
            for part in Path::new(&name).components() {
                match part {
                    Component::Normal(part) => rel.push(part),
                    Component::CurDir => {}
                    Component::ParentDir if rel.pop() => {}
                    _ => return None,
                }
            }
            safe(&rel).then_some(rel)
        })
        .collect()
}

/// A path's parts in lower case, to compare paths whatever their slashes, as on Windows RomM's
/// folders keep theirs and the file name joins with the other kind.
fn lowercase_parts(p: &Path) -> Vec<String> {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

/// The files that starting the playlist `m3u` needs: it, and the files it names with their
/// tracks. When it names none that RomM has, the discs a set keeps beside its playlist, in a
/// `.Name` folder or in a `Name` folder.
pub fn playlist_files(m3u: &Path, text: &str, files: &[PlannedFile]) -> Vec<PathBuf> {
    let named: Vec<&PathBuf> = playlist_entries(m3u, text)
        .iter()
        .filter_map(|entry| {
            let entry = lowercase_parts(entry);
            files
                .iter()
                .map(|f| &f.rel_path)
                .find(|p| lowercase_parts(p) == entry)
        })
        .collect();
    let discs: Vec<&PathBuf> = if named.is_empty() {
        let stem = m3u.file_stem().unwrap_or_default().to_string_lossy();
        let home = folder(m3u);
        let homes = [
            home.to_path_buf(),
            home.join(format!(".{stem}")),
            home.join(&*stem),
        ];
        files
            .iter()
            .map(|f| &f.rel_path)
            .filter(|p| {
                DISC_MEDIA.contains(&ext(p).as_str()) && homes.iter().any(|h| h == folder(p))
            })
            .collect()
    } else {
        named
    };
    let mut needed = vec![m3u.to_path_buf()];
    for disc in discs {
        for p in files_for(disc, files) {
            if !needed.contains(&p) {
                needed.push(p);
            }
        }
    }
    needed
}

/// Whether `file` of a game downloaded to `dir` can start: a playlist needs what it names, as
/// a download stopped part way leaves it without some of its discs.
pub fn ready(dir: &Path, file: &Path) -> bool {
    let path = dir.join(file);
    if !is_playlist(file) {
        return path.exists();
    }
    std::fs::read(&path).is_ok_and(|bytes| {
        playlist_entries(file, &String::from_utf8_lossy(&bytes))
            .iter()
            .all(|entry| dir.join(entry).exists())
    })
}

fn playlist(stem: &str, discs: Vec<PathBuf>) -> Launch {
    let name = format!("{stem}.m3u");
    let name = if safe_component(&name) {
        name
    } else {
        "game.m3u".to_string()
    };
    Launch::Playlist { name, discs }
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
            category: None,
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

    fn choices(files: Vec<RomFile>) -> Vec<PathBuf> {
        let mut r = rom(true, files);
        r.fs_path = "roms/snes".into();
        r.fs_name = "Super Metroid".into();
        r.platform_slug = "snes".into();
        let planned = plan(&r).unwrap();
        file_choices(&r, &planned)
    }

    #[test]
    fn a_folder_of_hacks_offers_each_one() {
        let dir = "roms/snes/Super Metroid";
        let mut manual = file(4, dir, "Manual.pdf");
        manual.category = Some("manual".into());
        let mut patch = file(5, &format!("{dir}/patch"), "Fix.ips");
        patch.category = Some("patch".into());
        assert_eq!(
            choices(vec![
                file(1, dir, "Super Metroid (Japan, USA).zip"),
                file(2, &format!("{dir}/hack"), "Super Metroid - Advent.smc"),
                file(3, dir, "readme.txt"),
                manual,
                patch,
                file(6, dir, "Hyper Metroid.smc"),
            ]),
            [
                PathBuf::from("Hyper Metroid.smc"),
                PathBuf::from("hack/Super Metroid - Advent.smc"),
                PathBuf::from("Super Metroid (Japan, USA).zip"),
            ]
        );
    }

    #[test]
    fn a_disc_offers_its_translations() {
        let dir = "roms/snes/Super Metroid";
        let translations = format!("{dir}/translation");
        assert_eq!(
            choices(vec![
                file(1, dir, "Tales (Japan).chd"),
                file(2, &translations, "Tales (T-En).chd"),
                file(3, &translations, "Tales (T-En 2).cue"),
                file(4, &translations, "Tales (T-En 2) (Track 1).bin"),
            ]),
            [
                PathBuf::from("Tales (Japan).chd"),
                PathBuf::from("translation/Tales (T-En 2).cue"),
                PathBuf::from("translation/Tales (T-En).chd"),
            ]
        );
    }

    #[test]
    fn a_game_with_one_file_on_top_offers_those_in_its_sub_folders() {
        let dir = "roms/psx/Tales";
        let r = rom(
            false,
            vec![
                file(1, dir, "Tales (Japan).chd"),
                file(2, &format!("{dir}/translation"), "Tales (T-En).chd"),
            ],
        );
        let names: Vec<_> = file_choices(&r, &plan(&r).unwrap())
            .iter()
            .map(|p| p.file_name().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["Tales (Japan).chd", "Tales (T-En).chd"]);
    }

    #[test]
    fn a_sheet_comes_down_with_its_tracks() {
        let dir = "roms/snes/Super Metroid";
        let translations = format!("{dir}/translation");
        let mut r = rom(
            true,
            vec![
                file(1, dir, "Tales (Japan).chd"),
                file(2, &translations, "Tales (T-En).cue"),
                file(3, &translations, "Tales (T-En) (Track 1).bin"),
                file(4, &translations, "Tales (T-En) (Track 2).bin"),
            ],
        );
        r.fs_path = "roms/snes".into();
        r.fs_name = "Super Metroid".into();
        let planned = plan(&r).unwrap();
        assert_eq!(
            files_for(Path::new("translation/Tales (T-En).cue"), &planned).len(),
            3
        );
        assert_eq!(
            files_for(Path::new("Tales (Japan).chd"), &planned),
            [PathBuf::from("Tales (Japan).chd")]
        );
    }

    #[test]
    fn each_playlist_is_one_choice_without_its_discs() {
        let dir = "roms/snes/Super Metroid";
        let t = format!("{dir}/translation");
        assert_eq!(
            choices(vec![
                file(1, dir, "Game.m3u"),
                file(2, &format!("{dir}/.Game"), "Game (Disc 1).chd"),
                file(3, &format!("{dir}/.Game"), "Game (Disc 2).chd"),
                file(4, &t, "Game (T-En).m3u"),
                file(5, &t, "Game (T-En) (Disc 1).cue"),
                file(6, &t, "Game (T-En) (Disc 1).bin"),
                file(7, &t, "Game (T-En) (Disc 2).chd"),
                file(8, dir, "._Game.m3u"),
            ]),
            [
                PathBuf::from("Game.m3u"),
                PathBuf::from("translation/Game (T-En).m3u"),
            ]
        );
    }

    fn planned(dir: &str, names: &[&str]) -> Vec<PlannedFile> {
        let mut r = rom(true, Vec::new());
        r.fs_name = "Game".into();
        r.files = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let (sub, name) = n.rsplit_once('/').map_or(("", *n), |(d, n)| (d, n));
                let path = if sub.is_empty() {
                    dir.to_string()
                } else {
                    format!("{dir}/{sub}")
                };
                file(i as i64, &path, name)
            })
            .collect();
        plan(&r).unwrap()
    }

    #[test]
    fn a_playlist_names_its_files_the_ways_sets_write_them() {
        let text = "\u{feff}#EXTM3U\r\n.T\\T (Disc 1).cue\r\n./T (Disc 2).chd|Disc 2\r\n\r\n../../x.chd\n/etc/passwd\n";
        assert_eq!(
            playlist_entries(Path::new("translation/T.m3u"), text),
            [
                PathBuf::from("translation/.T/T (Disc 1).cue"),
                PathBuf::from("translation/T (Disc 2).chd"),
            ]
        );
    }

    #[test]
    fn a_playlist_comes_down_with_what_it_names_and_nothing_else() {
        let files = planned(
            "roms/psx/Game",
            &[
                "Game.m3u",
                ".Game/Game (Disc 1).chd",
                "translation/T.m3u",
                "translation/.T/T (Disc 1).cue",
                "translation/.T/T (Disc 1).bin",
                "translation/.T/T (Disc 2).CHD",
            ],
        );
        let m3u = Path::new("translation/T.m3u");
        assert_eq!(
            playlist_files(m3u, ".T/T (Disc 1).cue\n.T/T (Disc 2).chd\n", &files),
            [
                PathBuf::from("translation/T.m3u"),
                PathBuf::from("translation/.T/T (Disc 1).cue"),
                PathBuf::from("translation/.T/T (Disc 1).bin"),
                PathBuf::from("translation/.T/T (Disc 2).CHD"),
            ]
        );
        assert_eq!(
            playlist_files(m3u, "elsewhere.chd\n", &files),
            playlist_files(m3u, ".T/T (Disc 1).cue\n.T/T (Disc 2).chd\n", &files),
        );
    }

    #[test]
    fn a_playlist_is_ready_once_its_discs_are() {
        let dir = tempfile::tempdir().unwrap();
        let m3u = Path::new("translation/T.m3u");
        std::fs::create_dir_all(dir.path().join("translation/.T")).unwrap();
        assert!(!ready(dir.path(), m3u));
        std::fs::write(dir.path().join(m3u), ".T/T (Disc 1).chd\n").unwrap();
        assert!(!ready(dir.path(), m3u));
        std::fs::write(dir.path().join("translation/.T/T (Disc 1).chd"), "").unwrap();
        assert!(ready(dir.path(), m3u));
    }

    #[test]
    fn a_lone_playlist_offers_no_choice() {
        let dir = "roms/snes/Super Metroid";
        assert!(choices(vec![
            file(1, dir, "Game.m3u"),
            file(2, dir, "Game (Disc 1).chd"),
            file(3, dir, "Game (Disc 2).chd"),
        ])
        .is_empty());
    }

    #[test]
    fn discs_and_lone_games_offer_no_choice() {
        let dir = "roms/snes/Super Metroid";
        assert!(choices(vec![
            file(1, dir, "Game (Disc 1).chd"),
            file(2, dir, "Game (Disc 2).chd"),
        ])
        .is_empty());
        assert!(choices(vec![file(1, dir, "Game.cue"), file(2, dir, "Game.bin")]).is_empty());
        assert!(choices(vec![file(1, dir, "Game.smc"), file(2, dir, "readme.txt")]).is_empty());
        let r = rom(false, vec![file(1, "roms/snes", "Game.smc")]);
        assert!(file_choices(&r, &plan(&r).unwrap()).is_empty());
    }

    #[test]
    fn a_disc_beside_the_game_starts_before_one_in_a_sub_folder() {
        let r = rom(true, Vec::new());
        let files = sized(&[
            PathBuf::from("Tales (Japan).chd"),
            PathBuf::from("translation/Tales (T-En).cue"),
            PathBuf::from("translation/Tales (T-En) (Track 1).bin"),
        ]);
        assert_eq!(
            launch_target(&r, &files),
            Some(Launch::File(PathBuf::from("Tales (Japan).chd")))
        );
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
    fn zipped_disks_make_a_playlist_for_an_emulator_that_opens_zips() {
        let mut r = rom(true, vec![]);
        r.platform_slug = "amiga".into();
        let files = vec![
            (PathBuf::from("Soccer Kid_Disk1.zip"), 767_602),
            (PathBuf::from("Soccer Kid_Disk2.zip"), 794_874),
            (PathBuf::from("Soccer Kid_Disk0.zip"), 703_522),
        ];
        match launch_target(&r, &files).unwrap() {
            Launch::Playlist { name, discs } => {
                assert_eq!(name, "Soccer Kid_Disk0.m3u");
                assert_eq!(
                    discs,
                    [
                        "Soccer Kid_Disk0.zip",
                        "Soccer Kid_Disk1.zip",
                        "Soccer Kid_Disk2.zip"
                    ]
                    .map(PathBuf::from)
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn zipped_discs_start_from_the_first_for_other_emulators() {
        let r = rom(true, vec![]);
        let files = vec![
            (PathBuf::from("Game (Disc 2).zip"), 900),
            (PathBuf::from("Game (Disc 1).zip"), 800),
        ];
        assert_eq!(
            launch_target(&r, &files),
            Some(Launch::File(PathBuf::from("Game (Disc 1).zip")))
        );
    }

    #[test]
    fn an_arcade_game_beside_its_bios_still_starts_the_game() {
        let mut r = rom(true, vec![]);
        r.platform_slug = "arcade".into();
        let files = vec![
            (PathBuf::from("neogeo.zip"), 1_000),
            (PathBuf::from("mslug.zip"), 9_000),
        ];
        assert_eq!(
            launch_target(&r, &files),
            Some(Launch::File(PathBuf::from("mslug.zip")))
        );
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
