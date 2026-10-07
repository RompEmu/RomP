use crate::external::{Asset, Emulator, Running};
use crate::input::{A, B, DOWN, L, L2, L3, LEFT, R, R2, R3, RIGHT, SELECT, START, UP, X, Y};
use crate::mapping::Mappings;
use slint::platform::Key;
use std::ffi::OsString;
use std::fmt::Write;
use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "ps3";
pub const FIRMWARE: &str = "the PS3 system software (PS3UPDAT.PUP)";
const MAX_PLAYERS: u8 = 7;
const INPUT_PROFILE: &str = "romp";

fn build() -> Option<(&'static str, &'static str)> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(("rpcs3-binaries-mac-arm64", "_macos_aarch64.7z"))
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some(("rpcs3-binaries-mac", "_macos.7z"))
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some(("rpcs3-binaries-win", "_win64_msvc.7z"))
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(("rpcs3-binaries-linux", "_linux64.AppImage"))
    } else {
        None
    }
}

pub fn available() -> bool {
    build().is_some()
}

pub fn releases() -> Option<String> {
    let (repo, _) = build()?;
    Some(format!(
        "https://api.github.com/repos/RPCS3/{repo}/releases/latest"
    ))
}

pub fn release_asset(assets: &[Asset]) -> Option<&Asset> {
    let (_, suffix) = build()?;
    assets
        .iter()
        .find(|a| a.name.starts_with("rpcs3-") && a.name.ends_with(suffix))
}

fn exe_in(dir: &Path, asset: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        dir.join("RPCS3.app/Contents/MacOS/rpcs3")
    } else if cfg!(windows) {
        dir.join("rpcs3.exe")
    } else {
        dir.join(asset)
    }
}

pub fn emulator(dir: PathBuf) -> Emulator {
    Emulator {
        name: "RPCS3",
        dir,
        pick: release_asset,
        exe_in,
    }
}

/// RPCS3's settings, system software and saves, kept apart from any RPCS3 installed outside Romp.
pub struct Home(pub PathBuf);

impl Home {
    pub fn env(&self) -> Vec<(&'static str, OsString)> {
        if cfg!(target_os = "macos") {
            vec![("HOME", self.0.clone().into_os_string())]
        } else if cfg!(windows) {
            let mut dir = self.0.clone().into_os_string();
            dir.push("/");
            vec![("RPCS3_CONFIG_DIR", dir)]
        } else {
            vec![
                ("XDG_CONFIG_HOME", self.0.join("config").into_os_string()),
                ("XDG_CACHE_HOME", self.0.join("cache").into_os_string()),
            ]
        }
    }

    pub fn config_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.0.join("Library/Application Support/rpcs3")
        } else if cfg!(windows) {
            self.0.clone()
        } else {
            self.0.join("config/rpcs3")
        }
    }

    fn input_dir(&self) -> PathBuf {
        let config = self.config_dir();
        let config = if cfg!(windows) {
            config.join("config")
        } else {
            config
        };
        config.join("input_configs/global")
    }

    pub fn firmware_installed(&self) -> bool {
        self.config_dir()
            .join("dev_flash/vsh/etc/version.txt")
            .is_file()
    }

    fn command(&self, exe: &Path) -> std::process::Command {
        let mut command = std::process::Command::new(exe);
        command.envs(self.env());
        command
    }

    /// RPCS3 can crash while closing after a successful install, so success is judged by the files it leaves.
    pub fn install_firmware(&self, exe: &Path, pup: &Path) -> Result<(), String> {
        let output = self
            .command(exe)
            .arg("--headless")
            .arg("--installfw")
            .arg(pup)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| tr::tr!("Could not start {name}: {e}", name = "RPCS3", e))?;
        if self.firmware_installed() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("RPCS3 firmware install failed: {stderr}");
        Err(tr::tr!("RPCS3 could not install the PS3 system software."))
    }

    pub fn write_input(&self, text: &str) -> std::io::Result<()> {
        let dir = self.input_dir();
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{INPUT_PROFILE}.yml")), text)
    }
}

pub fn find_firmware(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pup")))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// What RPCS3 boots: a disc image, or the EBOOT.BIN inside a game folder.
pub fn boot_path(rom: &Path) -> Result<&Path, String> {
    let name = rom
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name == "eboot.bin" || name.ends_with(".iso") {
        return Ok(rom);
    }
    let kind = Path::new(&name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_else(|| name.clone());
    Err(tr::tr!(
        "RomP plays PS3 games stored as a game folder, an ISO, or a zip or 7z of one, and this one is a {kind} file.",
        kind
    ))
}

pub fn is_archive(rom: &Path) -> bool {
    rom.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("7z") || e.eq_ignore_ascii_case("zip"))
}

/// The game folder's EBOOT.BIN, or else a disc image, nearest the top of `dir`.
pub fn find_boot(dir: &Path) -> Option<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(next) = dirs.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                files.push(path);
            }
        }
    }
    let nearest = |wanted: fn(&str) -> bool| {
        files
            .iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| wanted(&n.to_string_lossy().to_ascii_lowercase()))
            })
            .min_by_key(|p| (p.components().count(), p.to_path_buf()))
            .cloned()
    };
    nearest(|n| n == "eboot.bin").or_else(|| nearest(|n| n.ends_with(".iso")))
}

/// Unpacks a downloaded archive into a folder beside it and removes the archive, returning what to boot.
pub fn unpack_game(archive: &Path) -> Result<PathBuf, String> {
    let (Some(parent), Some(stem), Some(name)) =
        (archive.parent(), archive.file_stem(), archive.file_name())
    else {
        return Err(tr::tr!("The download has no name"));
    };
    let dir = parent.join(stem);
    let mut unpacking = stem.to_os_string();
    unpacking.push(".unpacking");
    let unpacking = parent.join(unpacking);
    let _ = std::fs::remove_dir_all(&unpacking);
    std::fs::create_dir_all(&unpacking).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let unpacked = crate::external::extract(
        std::io::BufReader::new(file),
        &name.to_string_lossy(),
        &unpacking,
    )
    .map_err(|e| tr::tr!("Could not unpack the game: {e}", e))
    .and_then(|()| {
        find_boot(&unpacking)
            .ok_or_else(|| tr::tr!("The archive has no PS3 game folder or ISO in it"))
    });
    let boot = match unpacked {
        Ok(boot) => boot,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&unpacking);
            return Err(e);
        }
    };
    let rel = boot
        .strip_prefix(&unpacking)
        .map_err(|e| e.to_string())?
        .to_path_buf();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&unpacking, &dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(archive);
    Ok(dir.join(rel))
}

pub struct LaunchConfig {
    pub fullscreen: bool,
    pub volume: u8,
}

pub fn config_yml(cfg: &LaunchConfig) -> String {
    format!(
        "Miscellaneous:\n  \
           Automatically start games after boot: true\n  \
           Exit RPCS3 when process finishes: true\n  \
           Start games in fullscreen mode: {}\n\
         Audio:\n  \
           Master Volume: {}\n",
        cfg.fullscreen,
        cfg.volume.min(100),
    )
}

/// RPCS3 tells identical controllers apart by numbering them in the order they connected.
pub fn pad_devices(names: &[String]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let same = names[..=i].iter().filter(|n| *n == name).count();
            format!("{name} {same}")
        })
        .collect()
}

pub fn keyboard_player(keyboard_player: Option<u8>, pads: usize) -> Option<u8> {
    let player = keyboard_player?;
    let port = if pads == 0 { player } else { pads as u8 + 1 };
    (port <= MAX_PLAYERS).then_some(port)
}

const PAD_BUTTONS: [(&str, u32); 16] = [
    ("Cross", B),
    ("Circle", A),
    ("Square", Y),
    ("Triangle", X),
    ("L1", L),
    ("R1", R),
    ("L2", L2),
    ("R2", R2),
    ("L3", L3),
    ("R3", R3),
    ("Start", START),
    ("Select", SELECT),
    ("Up", UP),
    ("Down", DOWN),
    ("Left", LEFT),
    ("Right", RIGHT),
];

const LEFT_STICK: [(&str, char); 4] = [
    ("Left Stick Up", 'I'),
    ("Left Stick Left", 'J'),
    ("Left Stick Down", 'K'),
    ("Left Stick Right", 'L'),
];

/// Qt's names for special keys, which RPCS3 uses; macOS gets symbols.
const SPECIAL_KEYS: [(Key, &str, &str); 17] = [
    (Key::Return, "↵", "Return"),
    (Key::Backspace, "⌫", "Backspace"),
    (Key::Tab, "⇥", "Tab"),
    (Key::Delete, "⌦", "Del"),
    (Key::Insert, "Ins", "Ins"),
    (Key::Home, "↖", "Home"),
    (Key::End, "↘", "End"),
    (Key::PageUp, "⇞", "PgUp"),
    (Key::PageDown, "⇟", "PgDown"),
    (Key::UpArrow, "↑", "Up"),
    (Key::DownArrow, "↓", "Down"),
    (Key::LeftArrow, "←", "Left"),
    (Key::RightArrow, "→", "Right"),
    (Key::Shift, "Shift", "Shift"),
    (Key::Control, "Ctrl", "Ctrl"),
    (Key::Alt, "Alt", "Alt"),
    (Key::Meta, "Meta", "Meta"),
];

fn key_name(key: &str) -> Option<String> {
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if c == ' ' {
        return Some("Space".into());
    }
    if c.is_ascii_graphic() {
        return Some(c.to_ascii_uppercase().to_string());
    }
    SPECIAL_KEYS
        .iter()
        .find(|(k, _, _)| char::from(*k) == c)
        .map(|(_, mac, other)| {
            if cfg!(target_os = "macos") {
                mac.to_string()
            } else {
                other.to_string()
            }
        })
}

/// Every button is written, so RPCS3's own layout never adds a second meaning to a key.
pub fn keyboard_bindings(mappings: &Mappings) -> Vec<(&'static str, String)> {
    let mut bindings: Vec<(&'static str, String)> = PAD_BUTTONS
        .iter()
        .map(|(field, button)| {
            let key = key_name(&mappings.key_for(*button)).unwrap_or_default();
            (*field, key)
        })
        .collect();
    let used: Vec<String> = bindings.iter().map(|(_, k)| k.clone()).collect();
    for (field, key) in LEFT_STICK {
        let key = key.to_string();
        let bound = if used.contains(&key) {
            String::new()
        } else {
            key
        };
        bindings.push((field, bound));
    }
    bindings
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn input_yml(pads: &[String], keyboard: Option<(u8, &[(&'static str, String)])>) -> String {
    let mut out = String::new();
    for (i, device) in pads.iter().take(MAX_PLAYERS.into()).enumerate() {
        let _ = write!(
            out,
            "Player {} Input:\n  Handler: SDL\n  Device: {}\n",
            i + 1,
            quoted(device)
        );
    }
    if let Some((player, bindings)) = keyboard {
        let _ = write!(
            out,
            "Player {player} Input:\n  Handler: Keyboard\n  Device: Keyboard\n  Config:\n"
        );
        for (field, key) in bindings {
            let _ = writeln!(out, "    {field}: {}", quoted(key));
        }
    }
    out
}

pub fn launch(
    exe: &Path,
    home: &Home,
    config: &Path,
    boot: &Path,
    fullscreen: bool,
    on_exit: impl FnOnce(Option<i32>, Vec<String>) + Send + 'static,
) -> std::io::Result<Running> {
    let mut command = home.command(exe);
    command
        .arg("--no-gui")
        .arg("--config")
        .arg(config)
        .arg("--input-config")
        .arg(INPUT_PROFILE);
    if fullscreen {
        command.arg("--fullscreen");
    }
    command.arg(boot);
    crate::external::launch(command, "rpcs3", on_exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
            digest: None,
        }
    }

    #[test]
    fn picks_the_build_for_this_computer() {
        let assets = [
            asset("rpcs3-v0.0.42-20083-3e9d881a_macos_aarch64.7z"),
            asset("rpcs3-v0.0.42-20083-3e9d881a_win64_msvc.7z"),
            asset("rpcs3-v0.0.42-20083-3e9d881a_win64_msvc.7z.sha256"),
            asset("rpcs3-v0.0.42-20083-3e9d881a_linux64.AppImage"),
        ];
        let picked = release_asset(&assets).map(|a| a.name.as_str());
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                picked,
                Some("rpcs3-v0.0.42-20083-3e9d881a_macos_aarch64.7z")
            );
        } else if cfg!(all(windows, target_arch = "x86_64")) {
            assert_eq!(picked, Some("rpcs3-v0.0.42-20083-3e9d881a_win64_msvc.7z"));
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(
                picked,
                Some("rpcs3-v0.0.42-20083-3e9d881a_linux64.AppImage")
            );
        }
        assert!(release_asset(&[asset("LICENSE")]).is_none());
        assert_eq!(releases().is_some(), available());
    }

    #[test]
    fn firmware_is_found_whatever_its_case() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_firmware(dir.path()), None);
        std::fs::write(dir.path().join("notes.txt"), b"hi").unwrap();
        std::fs::write(dir.path().join("ps3updat.pup"), b"fw").unwrap();
        assert_eq!(
            find_firmware(dir.path()),
            Some(dir.path().join("ps3updat.pup"))
        );
    }

    #[test]
    fn firmware_counts_as_installed_once_rpcs3_wrote_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home(dir.path().to_path_buf());
        assert!(!home.firmware_installed());
        let etc = home.config_dir().join("dev_flash/vsh/etc");
        std::fs::create_dir_all(&etc).unwrap();
        std::fs::write(etc.join("version.txt"), b"release:04.9300:").unwrap();
        assert!(home.firmware_installed());
    }

    #[test]
    fn rpcs3_keeps_its_files_in_romps_folder() {
        let home = Home(PathBuf::from("/data/rpcs3"));
        let env = home.env();
        assert!(!env.is_empty());
        if cfg!(windows) {
            assert_eq!(
                env[0].1, "/data/rpcs3/",
                "RPCS3 drops the last part without a slash"
            );
        } else {
            assert!(env
                .iter()
                .all(|(_, v)| Path::new(v).starts_with("/data/rpcs3")));
        }
        assert!(home.config_dir().starts_with("/data/rpcs3"));
        assert!(home.input_dir().ends_with("input_configs/global"));
    }

    #[test]
    fn folders_and_disc_images_boot_and_other_files_explain_themselves() {
        let eboot = Path::new("/roms/ps3/7/PS3_GAME/USRDIR/EBOOT.BIN");
        assert_eq!(boot_path(eboot), Ok(eboot));
        let iso = Path::new("/roms/ps3/8/Game.ISO");
        assert_eq!(boot_path(iso), Ok(iso));
        let pkg = boot_path(Path::new("/roms/ps3/9/Game.pkg")).unwrap_err();
        assert!(pkg.contains(".pkg"), "{pkg}");
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn archived_games_are_unpacked_beside_the_download() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Bejeweled 3 (USA).zip");
        std::fs::write(
            &archive,
            zip_of(&[
                ("Bejeweled 3 (USA).iso", b"disc"),
                ("Bejeweled 3 (USA).dkey", b"key"),
            ]),
        )
        .unwrap();
        assert!(is_archive(&archive));
        let boot = unpack_game(&archive).unwrap();
        let game = dir.path().join("Bejeweled 3 (USA)");
        assert_eq!(boot, game.join("Bejeweled 3 (USA).iso"));
        assert_eq!(std::fs::read(&boot).unwrap(), b"disc");
        assert!(game.join("Bejeweled 3 (USA).dkey").is_file());
        assert!(!archive.exists(), "the archive is removed");
        assert_eq!(boot_path(&boot), Ok(boot.as_path()));
    }

    #[test]
    fn archived_game_folders_boot_their_top_eboot() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Game.zip");
        std::fs::write(
            &archive,
            zip_of(&[
                ("Game/PS3_DISC.SFB", b"sfb"),
                ("Game/PS3_GAME/USRDIR/EBOOT.BIN", b"elf"),
                ("Game/PS3_GAME/USRDIR/bin/EBOOT.BIN", b"other"),
            ]),
        )
        .unwrap();
        let boot = unpack_game(&archive).unwrap();
        assert_eq!(boot, dir.path().join("Game/Game/PS3_GAME/USRDIR/EBOOT.BIN"));
    }

    #[test]
    fn an_archive_without_a_game_is_kept_and_explained() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Manual.zip");
        std::fs::write(&archive, zip_of(&[("manual.pdf", b"pdf")])).unwrap();
        let err = unpack_game(&archive).unwrap_err();
        assert!(err.contains("no PS3 game"), "{err}");
        assert!(archive.exists());
        assert!(!dir.path().join("Manual").exists());
        assert!(!dir.path().join("Manual.unpacking").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seven_zip_games_unpack() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Game.iso"), b"disc").unwrap();
        let archive = dir.path().join("Game.7z");
        let made = std::process::Command::new("/usr/bin/tar")
            .args(["--format", "7zip", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg("Game.iso")
            .status()
            .unwrap();
        assert!(made.success());
        let boot = unpack_game(&archive).unwrap();
        assert_eq!(std::fs::read(boot).unwrap(), b"disc");
    }

    #[test]
    fn config_carries_the_app_preferences() {
        let text = config_yml(&LaunchConfig {
            fullscreen: false,
            volume: 140,
        });
        assert!(text.contains("\n  Start games in fullscreen mode: false\n"));
        assert!(text.contains("\n  Exit RPCS3 when process finishes: true\n"));
        assert!(text.ends_with("Audio:\n  Master Volume: 100\n"));
    }

    #[test]
    fn identical_controllers_are_numbered() {
        let names = ["PS5 Controller", "Xbox Controller", "PS5 Controller"].map(String::from);
        assert_eq!(
            pad_devices(&names),
            ["PS5 Controller 1", "Xbox Controller 1", "PS5 Controller 2"]
        );
    }

    #[test]
    fn keyboard_follows_the_app_mapping() {
        let mut mappings = Mappings::default();
        mappings.set_key(B, "i");
        let bindings = keyboard_bindings(&mappings);
        let get = |field: &str| {
            bindings
                .iter()
                .find(|(f, _)| *f == field)
                .map(|(_, k)| k.as_str())
        };
        assert_eq!(get("Cross"), Some("I"));
        assert_eq!(
            get("Left Stick Up"),
            Some(""),
            "a key used by a button is not reused"
        );
        assert_eq!(get("Left Stick Left"), Some("J"));
        let enter = if cfg!(target_os = "macos") {
            "↵"
        } else {
            "Return"
        };
        assert_eq!(get("Start"), Some(enter));
        assert_eq!(key_name(" ").as_deref(), Some("Space"));
        assert_eq!(key_name(";").as_deref(), Some(";"));
        assert_eq!(key_name(&char::from(Key::F5).to_string()), None);
        assert_eq!(bindings.len(), 20);
    }

    #[test]
    fn input_config_puts_pads_first_and_the_keyboard_after() {
        let pads = pad_devices(&["DualSense \"Edge\"".to_string()]);
        let bindings = [("Cross", "Z".to_string())];
        let text = input_yml(&pads, Some((2, &bindings)));
        assert_eq!(
            text,
            "Player 1 Input:\n  Handler: SDL\n  Device: \"DualSense \\\"Edge\\\" 1\"\n\
             Player 2 Input:\n  Handler: Keyboard\n  Device: Keyboard\n  Config:\n    Cross: \"Z\"\n"
        );
        assert_eq!(keyboard_player(Some(1), 0), Some(1));
        assert_eq!(keyboard_player(Some(1), 2), Some(3));
        assert_eq!(keyboard_player(Some(1), 7), None);
        assert_eq!(keyboard_player(None, 0), None);
    }
}
