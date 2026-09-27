use crate::input::{A, B, DOWN, L, L2, L3, LEFT, R, R2, R3, RIGHT, SELECT, START, UP, X, Y};
use crate::mapping::Mappings;
use serde::{Deserialize, Serialize};
use slint::platform::Key;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "xbox";

#[derive(Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

fn asset_suffix() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("-macos-universal.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("-x86_64.AppImage")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("-aarch64.AppImage")
    } else {
        None
    }
}

pub fn release_asset(assets: &[Asset]) -> Option<&Asset> {
    let suffix = asset_suffix()?;
    assets.iter().find(|a| {
        a.name.starts_with("xemu-") && a.name.ends_with(suffix) && !a.name.contains("-dbg-")
    })
}

const SPECIAL_KEYS: [(Key, u32); 29] = [
    (Key::Return, 40),
    (Key::Escape, 41),
    (Key::Backspace, 42),
    (Key::Tab, 43),
    (Key::Space, 44),
    (Key::F1, 58),
    (Key::F2, 59),
    (Key::F3, 60),
    (Key::F4, 61),
    (Key::F5, 62),
    (Key::F6, 63),
    (Key::F7, 64),
    (Key::F8, 65),
    (Key::F9, 66),
    (Key::F10, 67),
    (Key::F11, 68),
    (Key::F12, 69),
    (Key::Insert, 73),
    (Key::Home, 74),
    (Key::PageUp, 75),
    (Key::Delete, 76),
    (Key::End, 77),
    (Key::PageDown, 78),
    (Key::RightArrow, 79),
    (Key::LeftArrow, 80),
    (Key::DownArrow, 81),
    (Key::UpArrow, 82),
    (Key::Shift, 225),
    (Key::Control, 224),
];

const MORE_KEYS: [(Key, u32); 5] = [
    (Key::Alt, 226),
    (Key::Meta, 227),
    (Key::ControlR, 228),
    (Key::ShiftR, 229),
    (Key::AltGr, 230),
];

pub fn scancode(key: &str) -> Option<u32> {
    let mut chars = key.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if let Some((_, code)) = SPECIAL_KEYS
        .iter()
        .chain(MORE_KEYS.iter())
        .find(|(k, _)| char::from(*k) == c)
    {
        return Some(*code);
    }
    let c = c.to_ascii_lowercase();
    match c {
        'a'..='z' => Some(4 + (c as u32 - 'a' as u32)),
        '1'..='9' => Some(30 + (c as u32 - '1' as u32)),
        '0' => Some(39),
        '-' => Some(45),
        '=' => Some(46),
        '[' => Some(47),
        ']' => Some(48),
        '\\' => Some(49),
        ';' => Some(51),
        '\'' => Some(52),
        '`' => Some(53),
        ',' => Some(54),
        '.' => Some(55),
        '/' => Some(56),
        _ => None,
    }
}

const PAD_BUTTONS: [(&str, u32); 14] = [
    ("a", B),
    ("b", A),
    ("x", Y),
    ("y", X),
    ("white", L),
    ("black", R),
    ("ltrigger", L2),
    ("rtrigger", R2),
    ("lstick_btn", L3),
    ("rstick_btn", R3),
    ("start", START),
    ("back", SELECT),
    ("dpad_up", UP),
    ("dpad_down", DOWN),
];

const DPAD_SIDES: [(&str, u32); 2] = [("dpad_left", LEFT), ("dpad_right", RIGHT)];

const STICKS: [(&str, u32); 8] = [
    ("lstick_up", 12),
    ("lstick_left", 13),
    ("lstick_down", 14),
    ("lstick_right", 15),
    ("rstick_up", 96),
    ("rstick_left", 92),
    ("rstick_down", 90),
    ("rstick_right", 94),
];

const GUIDE: u32 = 41;

pub fn keyboard_map(mappings: &Mappings) -> BTreeMap<&'static str, u32> {
    let mut map: BTreeMap<&'static str, u32> = PAD_BUTTONS
        .iter()
        .chain(DPAD_SIDES.iter())
        .map(|(field, button)| (*field, scancode(&mappings.key_for(*button)).unwrap_or(0)))
        .collect();
    map.insert("guide", GUIDE);
    let used: Vec<u32> = map.values().copied().collect();
    for (field, code) in STICKS {
        map.insert(field, if used.contains(&code) { 0 } else { code });
    }
    map
}

pub fn keyboard_port(keyboard_player: Option<u8>, pads: usize) -> Option<u8> {
    let player = keyboard_player?;
    let port = if pads == 0 { player } else { pads as u8 + 1 };
    (port <= 4).then_some(port)
}

#[derive(Debug, Default, PartialEq)]
pub struct Bios {
    pub bootrom: Option<PathBuf>,
    pub flash: Option<PathBuf>,
}

const BOOTROM_SIZE: u64 = 512;
const FLASH_SIZES: [u64; 2] = [256 * 1024, 1024 * 1024];

pub fn find_bios(dir: &Path) -> Bios {
    let mut files: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| (e.path(), meta.len()))
        })
        .collect();
    files.sort();
    Bios {
        bootrom: files
            .iter()
            .find(|(_, len)| *len == BOOTROM_SIZE)
            .map(|(p, _)| p.clone()),
        flash: files
            .iter()
            .find(|(_, len)| FLASH_SIZES.contains(len))
            .map(|(p, _)| p.clone()),
    }
}

pub struct LaunchConfig {
    pub bootrom: PathBuf,
    pub flash: PathBuf,
    pub eeprom: PathBuf,
    pub hdd: PathBuf,
    pub dvd: PathBuf,
    pub screenshots: PathBuf,
    pub fullscreen: bool,
    pub sharp: bool,
    pub volume: u8,
    pub ui_scale: f32,
    pub keyboard: BTreeMap<&'static str, u32>,
    pub keyboard_port: Option<u8>,
}

#[derive(Serialize)]
struct Config<'a> {
    general: General,
    input: Input<'a>,
    display: Display,
    audio: Audio,
    sys: Sys,
}

#[derive(Serialize)]
struct General {
    show_welcome: bool,
    skip_boot_anim: bool,
    screenshot_dir: String,
    updates: Updates,
}

#[derive(Serialize)]
struct Updates {
    check: bool,
}

#[derive(Serialize)]
struct Input<'a> {
    auto_bind: bool,
    bindings: BTreeMap<String, String>,
    keyboard_controller_scancode_map: &'a BTreeMap<&'static str, u32>,
}

#[derive(Serialize)]
struct Display {
    renderer: &'static str,
    filtering: &'static str,
    window: Window,
    ui: Ui,
}

#[derive(Serialize)]
struct Window {
    fullscreen_on_startup: bool,
}

#[derive(Serialize)]
struct Ui {
    show_menubar: bool,
    scale: f64,
    auto_scale: bool,
}

#[derive(Serialize)]
struct Audio {
    volume_limit: f64,
}

#[derive(Serialize)]
struct Sys {
    files: Files,
}

#[derive(Serialize)]
struct Files {
    bootrom_path: String,
    flashrom_path: String,
    eeprom_path: String,
    hdd_path: String,
    dvd_path: String,
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn config_toml(cfg: &LaunchConfig) -> String {
    let bindings = (1..=4)
        .map(|port| {
            let bound = if cfg.keyboard_port == Some(port) {
                "keyboard"
            } else {
                ""
            };
            (format!("port{port}"), bound.to_string())
        })
        .collect();
    let config = Config {
        general: General {
            show_welcome: false,
            skip_boot_anim: true,
            screenshot_dir: text(&cfg.screenshots),
            updates: Updates { check: false },
        },
        input: Input {
            auto_bind: true,
            bindings,
            keyboard_controller_scancode_map: &cfg.keyboard,
        },
        display: Display {
            renderer: if cfg!(target_os = "macos") {
                "OPENGL"
            } else {
                "VULKAN"
            },
            filtering: if cfg.sharp { "nearest" } else { "linear" },
            window: Window {
                fullscreen_on_startup: cfg.fullscreen,
            },
            ui: Ui {
                show_menubar: false,
                scale: f64::from(cfg.ui_scale),
                auto_scale: false,
            },
        },
        audio: Audio {
            volume_limit: f64::from(cfg.volume.min(100)) / 100.0,
        },
        sys: Sys {
            files: Files {
                bootrom_path: text(&cfg.bootrom),
                flashrom_path: text(&cfg.flash),
                eeprom_path: text(&cfg.eeprom),
                hdd_path: text(&cfg.hdd),
                dvd_path: text(&cfg.dvd),
            },
        },
    };
    toml::to_string(&config).expect("xemu config serializes")
}

pub const RELEASES: &str = "https://api.github.com/repos/xemu-project/xemu/releases/latest";
pub const HDD_IMAGE: &str =
    "https://github.com/xemu-project/xemu-hdd-image/releases/download/1.0/xbox_hdd.qcow2.zip";
const HDD_FILE: &str = "xbox_hdd.qcow2";
const LOG_TAIL: usize = 40;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: String,
    exe: String,
}

fn exe_in(dir: &Path, asset: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        dir.join("xemu.app/Contents/MacOS/xemu")
    } else {
        dir.join(asset)
    }
}

pub struct Xemu {
    dir: PathBuf,
}

impl Xemu {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn installed(&self) -> Option<PathBuf> {
        let manifest: Manifest =
            serde_json::from_slice(&std::fs::read(self.dir.join("manifest.json")).ok()?).ok()?;
        let exe = self.dir.join(manifest.exe);
        exe.is_file().then_some(exe)
    }

    pub async fn install(&self, http: &reqwest::Client, releases: &str) -> Result<PathBuf, String> {
        let unreachable = |_| "Could not reach GitHub to download xemu".to_string();
        let release: Release = http
            .get(releases)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(unreachable)?
            .error_for_status()
            .map_err(|e| format!("xemu is not available right now ({e})"))?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let asset =
            release_asset(&release.assets).ok_or("xemu has no download for this computer")?;
        let bytes = http
            .get(&asset.browser_download_url)
            .send()
            .await
            .map_err(unreachable)?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let version_dir = self.dir.join(&release.tag_name);
        let name = asset.name.clone();
        let dir = version_dir.clone();
        tokio::task::spawn_blocking(move || unpack(&bytes, &dir, &name))
            .await
            .map_err(|e| e.to_string())??;
        let exe = exe_in(&version_dir, &asset.name);
        if !exe.is_file() {
            return Err("The xemu download did not contain the emulator".into());
        }
        let manifest = Manifest {
            version: release.tag_name.clone(),
            exe: exe
                .strip_prefix(&self.dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned(),
        };
        std::fs::write(
            self.dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest json"),
        )
        .map_err(|e| e.to_string())?;
        Ok(exe)
    }

    pub fn hdd_template(&self) -> PathBuf {
        self.dir.join(HDD_FILE)
    }

    pub async fn ensure_hdd_template(
        &self,
        http: &reqwest::Client,
        url: &str,
    ) -> Result<PathBuf, String> {
        let path = self.hdd_template();
        if path.is_file() {
            return Ok(path);
        }
        let bytes = http
            .get(url)
            .send()
            .await
            .map_err(|_| "Could not reach GitHub to download the Xbox hard disk".to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let target = path.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut archive =
                zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
            let mut entry = archive
                .by_name(HDD_FILE)
                .map_err(|_| format!("{HDD_FILE} is missing from the download"))?;
            std::fs::create_dir_all(target.parent().expect("hdd has a parent"))
                .map_err(|e| e.to_string())?;
            let tmp = target.with_extension("tmp");
            let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(path)
    }
}

fn unpack(bytes: &[u8], dir: &Path, name: &str) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if name.ends_with(".zip") {
        let zip = dir.join(name);
        std::fs::write(&zip, bytes).map_err(|e| e.to_string())?;
        let status = std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&zip)
            .arg(dir)
            .status()
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&zip);
        if !status.success() {
            return Err("Could not unpack xemu".into());
        }
    } else {
        use std::os::unix::fs::PermissionsExt;
        let exe = dir.join(name);
        std::fs::write(&exe, bytes).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn prepare_hdd(template: &Path, save_dir: &Path) -> std::io::Result<PathBuf> {
    let hdd = save_dir.join(HDD_FILE);
    if !hdd.exists() {
        std::fs::create_dir_all(save_dir)?;
        std::fs::copy(template, &hdd)?;
    }
    Ok(hdd)
}

pub struct Running {
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
}

impl Running {
    pub fn wait_exit(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if !matches!(self.child.lock().unwrap().try_wait(), Ok(None)) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }
}

pub fn launch(
    exe: &Path,
    config: &Path,
    dvd: &Path,
    fullscreen: bool,
    on_exit: impl FnOnce(Option<i32>, Vec<String>) + Send + 'static,
) -> std::io::Result<Running> {
    use std::io::BufRead;
    let mut command = std::process::Command::new(exe);
    command
        .arg("-config_path")
        .arg(config)
        .arg("-dvd_path")
        .arg(dvd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    if fullscreen {
        command.arg("-full-screen");
    }
    let mut child = command.spawn()?;
    let stderr = child.stderr.take().expect("stderr is piped");
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    let waiter = child.clone();
    std::thread::spawn(move || {
        let mut tail = std::collections::VecDeque::new();
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            tracing::debug!("[xemu] {line}");
            if tail.len() == LOG_TAIL {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        let code = loop {
            match waiter.lock().unwrap().try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) => {}
                Err(_) => break None,
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        on_exit(code, tail.into());
    });
    Ok(Running { child })
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::platform::Key;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
        }
    }

    #[test]
    fn picks_the_signed_release_build_for_this_computer() {
        let assets = [
            asset("xemu-0.8.136-dbg-macos-universal-unsigned.zip"),
            asset("xemu-0.8.136-macos-universal-unsigned.zip"),
            asset("xemu-0.8.136-macos-universal.zip"),
            asset("xemu-0.8.136-dbg-x86_64.AppImage"),
            asset("xemu-0.8.136-x86_64.AppImage"),
            asset("xemu-0.8.136-aarch64.AppImage"),
            asset("xemu-0.8.136-windows-x86_64.zip"),
        ];
        let picked = release_asset(&assets).map(|a| a.name.as_str());
        if cfg!(target_os = "macos") {
            assert_eq!(picked, Some("xemu-0.8.136-macos-universal.zip"));
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(picked, Some("xemu-0.8.136-x86_64.AppImage"));
        }
        assert!(release_asset(&[asset("LICENSE.txt")]).is_none());
    }

    #[test]
    fn keys_translate_to_sdl_scancodes() {
        assert_eq!(scancode("a"), Some(4));
        assert_eq!(scancode("Z"), Some(29));
        assert_eq!(scancode("1"), Some(30));
        assert_eq!(scancode("0"), Some(39));
        assert_eq!(scancode(&char::from(Key::Return).to_string()), Some(40));
        assert_eq!(scancode(&char::from(Key::Escape).to_string()), Some(41));
        assert_eq!(scancode(&char::from(Key::UpArrow).to_string()), Some(82));
        assert_eq!(scancode(&char::from(Key::F1).to_string()), Some(58));
        assert_eq!(scancode(" "), Some(44));
        assert_eq!(scancode("/"), Some(56));
        assert_eq!(scancode(""), None);
    }

    #[test]
    fn keyboard_layout_follows_the_app_mapping() {
        let mut mappings = Mappings::default();
        let map = keyboard_map(&mappings);
        assert_eq!(map["a"], 29, "bottom face button is Z by default");
        assert_eq!(map["b"], 27);
        assert_eq!(map["x"], 4);
        assert_eq!(map["y"], 22);
        assert_eq!(map["white"], 20);
        assert_eq!(map["black"], 26);
        assert_eq!(map["ltrigger"], 7);
        assert_eq!(map["rtrigger"], 9);
        assert_eq!(map["start"], 40);
        assert_eq!(map["back"], 42);
        assert_eq!(map["dpad_up"], 82);
        assert_eq!(map["guide"], 41);
        assert_eq!(map["lstick_up"], 12);
        mappings.set_key(crate::input::B, "i");
        let map = keyboard_map(&mappings);
        assert_eq!(map["a"], 12);
        assert_eq!(map["lstick_up"], 0, "a key used by a button is not reused");
        assert_eq!(map.len(), 25);
    }

    #[test]
    fn keyboard_steps_aside_for_connected_controllers() {
        assert_eq!(keyboard_port(Some(1), 0), Some(1));
        assert_eq!(keyboard_port(Some(3), 0), Some(3));
        assert_eq!(keyboard_port(Some(1), 1), Some(2));
        assert_eq!(keyboard_port(Some(1), 4), None);
        assert_eq!(keyboard_port(None, 0), None);
    }

    #[test]
    fn each_game_gets_its_own_copy_of_the_hard_disk() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template.qcow2");
        std::fs::write(&template, b"blank").unwrap();
        let save = dir.path().join("saves/7");
        let hdd = prepare_hdd(&template, &save).unwrap();
        assert_eq!(std::fs::read(&hdd).unwrap(), b"blank");
        std::fs::write(&hdd, b"played").unwrap();
        prepare_hdd(&template, &save).unwrap();
        assert_eq!(
            std::fs::read(&hdd).unwrap(),
            b"played",
            "an existing disk is kept"
        );
    }

    #[tokio::test]
    async fn install_fetches_the_latest_release_for_this_computer() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        if asset_suffix() != Some("-macos-universal.zip") {
            return;
        }
        let server = MockServer::start().await;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "xemu.app/Contents/MacOS/xemu",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, b"binary").unwrap();
        let zip = zip.finish().unwrap().into_inner();
        let name = "xemu-0.9.0-macos-universal.zip";
        Mock::given(method("GET"))
            .and(path("/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.9.0",
                "assets": [{ "name": name, "browser_download_url": format!("{}/{name}", server.uri()) }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{name}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let xemu = Xemu::new(dir.path().to_path_buf());
        assert!(xemu.installed().is_none());
        let exe = xemu
            .install(&reqwest::Client::new(), &format!("{}/latest", server.uri()))
            .await
            .unwrap();
        assert_eq!(exe, dir.path().join("v0.9.0/xemu.app/Contents/MacOS/xemu"));
        assert_eq!(xemu.installed(), Some(exe));
    }

    #[test]
    fn bios_files_are_recognised_by_size() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_bios(dir.path()), Bios::default());
        std::fs::write(dir.path().join("mcpx_1.0.bin"), vec![0u8; 512]).unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("Complex_4627.bin"), vec![0u8; 1024 * 1024]).unwrap();
        let bios = find_bios(dir.path());
        assert_eq!(bios.bootrom, Some(dir.path().join("mcpx_1.0.bin")));
        assert_eq!(bios.flash, Some(dir.path().join("Complex_4627.bin")));
    }

    #[test]
    fn config_carries_the_app_preferences() {
        let cfg = LaunchConfig {
            bootrom: "/sys/mcpx.bin".into(),
            flash: "/sys/bios \"x\".bin".into(),
            eeprom: "/save/eeprom.bin".into(),
            hdd: "/save/xbox_hdd.qcow2".into(),
            dvd: "/roms/Halo.iso".into(),
            screenshots: "/save/screenshots".into(),
            fullscreen: true,
            sharp: true,
            volume: 40,
            ui_scale: 1.5,
            keyboard: keyboard_map(&Mappings::default()),
            keyboard_port: Some(2),
        };
        let text = config_toml(&cfg);
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        let get = |path: &str| {
            path.split('.')
                .try_fold(toml::Value::Table(parsed.clone()), |v, k| v.get(k).cloned())
                .unwrap_or_else(|| panic!("{path} missing in\n{text}"))
        };
        assert_eq!(get("general.show_welcome").as_bool(), Some(false));
        assert_eq!(get("general.updates.check").as_bool(), Some(false));
        assert_eq!(get("general.skip_boot_anim").as_bool(), Some(true));
        assert_eq!(
            get("display.window.fullscreen_on_startup").as_bool(),
            Some(true)
        );
        assert_eq!(get("display.filtering").as_str(), Some("nearest"));
        let renderer = if cfg!(target_os = "macos") {
            "OPENGL"
        } else {
            "VULKAN"
        };
        assert_eq!(get("display.renderer").as_str(), Some(renderer));
        assert_eq!(get("display.ui.show_menubar").as_bool(), Some(false));
        assert_eq!(get("display.ui.scale").as_float(), Some(1.5));
        assert_eq!(get("display.ui.auto_scale").as_bool(), Some(false));
        assert_eq!(get("audio.volume_limit").as_float(), Some(0.4));
        assert_eq!(
            get("sys.files.flashrom_path").as_str(),
            Some("/sys/bios \"x\".bin")
        );
        assert_eq!(get("sys.files.dvd_path").as_str(), Some("/roms/Halo.iso"));
        assert_eq!(
            get("sys.files.hdd_path").as_str(),
            Some("/save/xbox_hdd.qcow2")
        );
        assert_eq!(get("input.bindings.port2").as_str(), Some("keyboard"));
        assert_eq!(get("input.bindings.port1").as_str(), Some(""));
        assert_eq!(get("input.auto_bind").as_bool(), Some(true));
        assert_eq!(
            get("input.keyboard_controller_scancode_map.guide").as_integer(),
            Some(41)
        );
    }
}
