use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;

pub const BUILDBOT: &str = "https://buildbot.libretro.com/nightly";
pub const ARMSX2_RELEASE: &str = "https://github.com/RompEmu/ARMSX2/releases/latest/download";
const ARMSX2_ASSET: &str = "armsx2_libretro-macos-arm64.zip";

#[derive(Debug, PartialEq, Eq)]
pub struct CoreInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub lib: &'static str,
    pub jit: bool,
}

const fn core(id: &'static str, name: &'static str, lib: &'static str, jit: bool) -> CoreInfo {
    CoreInfo { id, name, lib, jit }
}

const ARMSX2: CoreInfo = core("armsx2", "ARMSX2", "armsx2_libretro", true);

#[cfg(target_os = "macos")]
const PS2: CoreInfo = ARMSX2;
#[cfg(all(target_arch = "x86_64", not(target_os = "macos")))]
const PS2: CoreInfo = core("pcsx2", "PCSX2", "pcsx2_libretro", true);
#[cfg(not(any(target_os = "macos", target_arch = "x86_64")))]
const PS2: CoreInfo = core("play", "Play!", "play_libretro", true);

static CORES: &[(&[&str], CoreInfo)] = &[
    (
        &["snes", "sfam"],
        core("snes9x", "Snes9x", "snes9x_libretro", false),
    ),
    (
        &["nes", "famicom", "fds"],
        core("nestopia", "Nestopia UE", "nestopia_libretro", false),
    ),
    (
        &["genesis", "sms", "gamegear", "segacd", "sg1000"],
        core(
            "genesis_plus_gx",
            "Genesis Plus GX",
            "genesis_plus_gx_libretro",
            false,
        ),
    ),
    (
        &["gb", "gbc", "gba"],
        core("mgba", "mGBA", "mgba_libretro", false),
    ),
    (
        &["dc"],
        core("flycast", "Flycast", "flycast_libretro", true),
    ),
    (
        &["n64"],
        core(
            "mupen64plus_next",
            "Mupen64Plus-Next",
            "mupen64plus_next_libretro",
            true,
        ),
    ),
    (
        &["tg16"],
        core(
            "mednafen_pce_fast",
            "Beetle PCE Fast",
            "mednafen_pce_fast_libretro",
            false,
        ),
    ),
    (
        &["turbografx-cd"],
        core("mednafen_pce", "Beetle PCE", "mednafen_pce_libretro", false),
    ),
    (
        &["supergrafx"],
        core(
            "mednafen_supergrafx",
            "Beetle SuperGrafx",
            "mednafen_supergrafx_libretro",
            false,
        ),
    ),
    (
        &["psx"],
        core(
            "mednafen_psx_hw",
            "Beetle PSX HW",
            "mednafen_psx_hw_libretro",
            false,
        ),
    ),
    (
        &["saturn"],
        core(
            "mednafen_saturn",
            "Beetle Saturn",
            "mednafen_saturn_libretro",
            false,
        ),
    ),
    (
        &["atari2600"],
        core("stella", "Stella", "stella_libretro", false),
    ),
    (
        &["neo-geo-pocket", "neo-geo-pocket-color"],
        core(
            "mednafen_ngp",
            "Beetle NeoPop",
            "mednafen_ngp_libretro",
            false,
        ),
    ),
    (
        &["sega32"],
        core("picodrive", "PicoDrive", "picodrive_libretro", false),
    ),
    (
        &["jaguar"],
        core(
            "virtualjaguar",
            "Virtual Jaguar",
            "virtualjaguar_libretro",
            false,
        ),
    ),
    (
        &["wonderswan", "wonderswan-color"],
        core(
            "mednafen_wswan",
            "Beetle WonderSwan",
            "mednafen_wswan_libretro",
            false,
        ),
    ),
    (
        &["virtualboy"],
        core("mednafen_vb", "Beetle VB", "mednafen_vb_libretro", false),
    ),
    (&["vectrex"], core("vecx", "Vecx", "vecx_libretro", false)),
    (
        &["c64"],
        core("vice_x64sc", "VICE x64sc", "vice_x64sc_libretro", false),
    ),
    (&["zxs"], core("fuse", "Fuse", "fuse_libretro", false)),
    (&["amiga"], core("puae", "PUAE", "puae_libretro", false)),
    (
        &["lynx"],
        core(
            "mednafen_lynx",
            "Beetle Lynx",
            "mednafen_lynx_libretro",
            false,
        ),
    ),
    (
        &["intellivision"],
        core("freeintv", "FreeIntv", "freeintv_libretro", false),
    ),
    (&["odyssey-2"], core("o2em", "O2EM", "o2em_libretro", false)),
    (&["psp"], core("ppsspp", "PPSSPP", "ppsspp_libretro", true)),
    (&["3do"], core("opera", "Opera", "opera_libretro", false)),
    (
        &["dos"],
        core("dosbox_pure", "DOSBox Pure", "dosbox_pure_libretro", true),
    ),
    (
        &["nds"],
        core("desmume", "DeSmuME", "desmume_libretro", true),
    ),
    (&["ps2"], PS2),
    (
        &["ngc", "wii"],
        core("dolphin", "Dolphin", "dolphin_libretro", true),
    ),
    (
        &["msx", "msx2", "msx2plus"],
        core("bluemsx", "blueMSX", "bluemsx_libretro", false),
    ),
    (
        &["colecovision"],
        core("gearcoleco", "Gearcoleco", "gearcoleco_libretro", false),
    ),
    (
        &["philips-cd-i"],
        core("same_cdi", "SAME CDi", "same_cdi_libretro", false),
    ),
    (
        &crate::bios::ARCADE,
        core("fbneo", "FinalBurn Neo", "fbneo_libretro", false),
    ),
];

pub fn all_cores() -> impl Iterator<Item = &'static CoreInfo> {
    CORES.iter().map(|(_, core)| core)
}

pub fn core_for_platform(slug: &str) -> Option<&'static CoreInfo> {
    CORES
        .iter()
        .find(|(slugs, _)| slugs.contains(&slug))
        .map(|(_, c)| c)
}

const NINTENDO: [&str; 11] = [
    "nes",
    "famicom",
    "fds",
    "snes",
    "sfam",
    "n64",
    "gb",
    "gbc",
    "gba",
    "nds",
    "virtualboy",
];

pub fn resumes_reliably(core_id: &str) -> bool {
    core_id != "play"
}

pub fn is_computer(core_id: &str) -> bool {
    matches!(core_id, "puae" | "dosbox_pure" | "vice_x64sc" | "fuse")
}

// These draw through Vulkan where this computer has a working Vulkan device, which on
// macOS is the bundled MoltenVK.
const VULKAN_CORES: [(&str, &[(&str, &str)]); 6] = [
    ("dolphin", &[]),
    ("flycast", &[]),
    (
        "mednafen_psx_hw",
        &[("beetle_psx_hw_renderer", "hardware_vk")],
    ),
    (
        "mupen64plus_next",
        &[("mupen64plus-rdp-plugin", "parallel")],
    ),
    ("ppsspp", &[("ppsspp_backend", "vulkan")]),
    ("pcsx2", &[("pcsx2_renderer", "Vulkan")]),
];

fn vulkan_options(core_id: &str) -> Option<&'static [(&'static str, &'static str)]> {
    if core_id == ARMSX2.id {
        return Some(&[("armsx2_renderer", "Vulkan")]);
    }
    if !crate::vulkan::available() {
        return None;
    }
    VULKAN_CORES
        .iter()
        .find(|(id, _)| *id == core_id)
        .map(|(_, options)| *options)
}

pub fn uses_vulkan(core_id: &str) -> bool {
    vulkan_options(core_id).is_some()
}

pub fn download_base(core: &CoreInfo) -> &'static str {
    if core.id == ARMSX2.id {
        ARMSX2_RELEASE
    } else {
        BUILDBOT
    }
}

fn core_url(base: &str, core: &CoreInfo) -> String {
    if core.id == ARMSX2.id {
        format!("{base}/{ARMSX2_ASSET}")
    } else {
        buildbot_url(base, core)
    }
}

pub fn uses_mouse(core_id: &str) -> bool {
    matches!(core_id, "puae" | "dosbox_pure")
}

pub fn is_nintendo(platform_slug: &str) -> bool {
    NINTENDO.contains(&platform_slug)
}

pub fn lib_file(core: &CoreInfo) -> String {
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(windows) {
        "dll"
    } else {
        "so"
    };
    format!("{}.{ext}", core.lib)
}

pub fn buildbot_url(base: &str, core: &CoreInfo) -> String {
    let target = if cfg!(target_os = "macos") {
        "apple/osx/arm64"
    } else if cfg!(windows) {
        "windows/x86_64"
    } else {
        "linux/x86_64"
    };
    format!("{base}/{target}/latest/{}.zip", lib_file(core))
}

pub fn default_options(core_id: &str) -> Vec<(String, String)> {
    let options: &[(&str, &str)] = match core_id {
        "puae" => &[
            ("puae_model", "A600"),
            ("puae_kickstart", "auto"),
            ("puae_chipmem_size", "4"),
            ("puae_fastmem_size", "8"),
            ("puae_floppy_speed", "800"),
            ("puae_immediate_blits", "true"),
        ],
        "mednafen_psx_hw" => &[("beetle_psx_analog_toggle", "enabled")],
        "vice_x64sc" => &[("vice_drive_true_emulation", "disabled")],
        "nestopia" => &[("nestopia_zapper_device", "lightgun")],
        "pcsx2" => &[
            ("pcsx2_renderer", "OpenGL"),
            ("pcsx2_shared_memory_cards", "disabled"),
        ],
        "desmume" => &[
            ("desmume_pointer_type", "touch"),
            ("desmume_screens_layout", "top/bottom"),
        ],
        _ => &[],
    };
    let mut options: Vec<(String, String)> = options
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    for (key, value) in vulkan_options(core_id).unwrap_or_default() {
        match options.iter_mut().find(|(k, _)| k == key) {
            Some(existing) => existing.1 = value.to_string(),
            None => options.push((key.to_string(), value.to_string())),
        }
    }
    options
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Manifest {
    id: String,
    file: String,
    sha256: String,
    version: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub const SYSTEM_FILES: &str = "https://buildbot.libretro.com/assets/system";

const SYSTEM_FILE_SETS: [(&str, &str, &str); 3] = [
    ("dolphin", "Dolphin.zip", "dolphin-emu/Sys/codehandler.bin"),
    ("bluemsx", "blueMSX.zip", "Databases/msxromdb.xml"),
    ("ppsspp", "PPSSPP.zip", "PPSSPP/ppge_atlas.zim"),
];

fn system_file_set(core: &CoreInfo) -> Option<(&'static str, &'static str)> {
    SYSTEM_FILE_SETS
        .iter()
        .find(|(id, _, _)| *id == core.id)
        .map(|(_, zip, marker)| (*zip, *marker))
}

pub fn system_files_present(core: &CoreInfo, system_dir: &std::path::Path) -> bool {
    system_file_set(core).is_none_or(|(_, marker)| system_dir.join(marker).exists())
}

pub async fn install_system_files(
    http: &reqwest::Client,
    base: &str,
    core: &CoreInfo,
    system_dir: &std::path::Path,
) -> Result<(), String> {
    let Some((zip_name, marker)) = system_file_set(core) else {
        return Ok(());
    };
    if system_dir.join(marker).exists() {
        return Ok(());
    }
    let resp = http
        .get(format!("{base}/{zip_name}"))
        .send()
        .await
        .map_err(|_| "Could not reach the download server".to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "{}'s system files are not available ({})",
            core.name,
            resp.status()
        ));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let system_dir = system_dir.to_path_buf();
    let marker = PathBuf::from(marker);
    tokio::task::spawn_blocking(move || extract_into(&bytes, &system_dir, &marker))
        .await
        .map_err(|e| e.to_string())?
}

fn extract_into(
    bytes: &[u8],
    dir: &std::path::Path,
    marker: &std::path::Path,
) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry
            .enclosed_name()
            .ok_or_else(|| format!("unsafe path in download: {}", entry.name()))?;
        entries.push((i, name, entry.is_dir()));
    }
    entries.sort_by_key(|(_, name, _)| name == marker);
    for (i, name, is_dir) in entries {
        let target = dir.join(&name);
        if is_dir {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let mut out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn published_checksum(http: &reqwest::Client, url: &str) -> Result<String, String> {
    let published = http
        .get(format!("{url}.sha256"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "Could not check the core download".to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    Ok(published
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase())
}

pub struct Cores {
    dir: PathBuf,
}

impl Cores {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Whether a core published as a release has a newer build than the one installed.
    pub async fn update_available(
        &self,
        http: &reqwest::Client,
        base: &str,
        core: &CoreInfo,
    ) -> bool {
        if core.id != ARMSX2.id {
            return false;
        }
        let Some(manifest) = std::fs::read(self.dir.join(core.id).join("manifest.json"))
            .ok()
            .and_then(|m| serde_json::from_slice::<Manifest>(&m).ok())
        else {
            return false;
        };
        published_checksum(http, &core_url(base, core))
            .await
            .is_ok_and(|latest| latest != manifest.version)
    }

    pub fn installed(&self, core: &CoreInfo) -> Option<PathBuf> {
        let dir = self.dir.join(core.id);
        let manifest: Manifest =
            serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()?;
        let path = dir.join(&manifest.file);
        (sha256_hex(&std::fs::read(&path).ok()?) == manifest.sha256).then_some(path)
    }

    pub async fn install(
        &self,
        http: &reqwest::Client,
        base: &str,
        core: &CoreInfo,
    ) -> Result<PathBuf, String> {
        let url = core_url(base, core);
        let resp = http
            .get(&url)
            .send()
            .await
            .map_err(|_| "Could not reach the core download server".to_string())?;
        if !resp.status().is_success() {
            return Err(format!(
                "{} is not available for this computer ({})",
                core.name,
                resp.status()
            ));
        }
        let mut version = resp
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("nightly")
            .to_string();
        let zip_bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        if core.id == ARMSX2.id {
            version = published_checksum(http, &url).await?;
            if !version.eq_ignore_ascii_case(&sha256_hex(&zip_bytes)) {
                return Err("The core download did not match its checksum".into());
            }
        }
        let file = lib_file(core);
        let lib = tokio::task::spawn_blocking({
            let file = file.clone();
            move || -> Result<Vec<u8>, String> {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
                    .map_err(|e| e.to_string())?;
                let index = (0..archive.len())
                    .find(|&i| {
                        archive.name_for_index(i).is_some_and(|name| {
                            std::path::Path::new(name).file_name()
                                == Some(std::ffi::OsStr::new(&file))
                        })
                    })
                    .ok_or_else(|| format!("{file} is missing from the download"))?;
                let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                entry.read_to_end(&mut out).map_err(|e| e.to_string())?;
                Ok(out)
            }
        })
        .await
        .map_err(|e| e.to_string())??;
        let dir = self.dir.join(core.id);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| e.to_string())?;
        let path = dir.join(&file);
        let tmp = dir.join(format!("{file}.tmp"));
        tokio::fs::write(&tmp, &lib)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| e.to_string())?;
        let manifest = Manifest {
            id: core.id.into(),
            file,
            sha256: sha256_hex(&lib),
            version,
        };
        tokio::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest json"),
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn zip_with(name: &str, body: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body).unwrap();
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn platforms_map_to_cores() {
        assert_eq!(core_for_platform("snes").unwrap().id, "snes9x");
        assert_eq!(
            core_for_platform("psx").unwrap().lib,
            "mednafen_psx_hw_libretro"
        );
        assert!(core_for_platform("dc").unwrap().jit);
        assert!(core_for_platform("xbox").is_none());
        for slug in ["ngc", "wii"] {
            let core = core_for_platform(slug).unwrap();
            assert_eq!((core.id, core.jit), ("dolphin", true));
        }
        for slug in ["msx", "msx2", "msx2plus"] {
            assert_eq!(core_for_platform(slug).unwrap().id, "bluemsx");
        }
        assert_eq!(core_for_platform("colecovision").unwrap().id, "gearcoleco");
        assert_eq!(core_for_platform("philips-cd-i").unwrap().id, "same_cdi");
        for slug in ["arcade", "neogeoaes", "cps2"] {
            assert_eq!(core_for_platform(slug).unwrap().id, "fbneo");
        }
    }

    #[test]
    fn cores_draw_with_vulkan_where_it_works() {
        let vulkan = crate::vulkan::available();
        assert!(uses_vulkan("armsx2"));
        assert!(!uses_vulkan("snes9x"));
        for core in [
            "dolphin",
            "flycast",
            "mednafen_psx_hw",
            "mupen64plus_next",
            "ppsspp",
            "pcsx2",
        ] {
            assert_eq!(uses_vulkan(core), vulkan, "{core}");
        }
        let n64 = default_options("mupen64plus_next");
        let rdp = n64.iter().find(|(k, _)| k == "mupen64plus-rdp-plugin");
        assert_eq!(rdp.map(|(_, v)| v.as_str()), vulkan.then_some("parallel"));
        let ps2 = default_options("pcsx2");
        let renderer = ps2.iter().filter(|(k, _)| k == "pcsx2_renderer").count();
        assert_eq!(renderer, 1, "one renderer choice, Vulkan replacing OpenGL");
        assert!(default_options("mednafen_psx_hw")
            .contains(&("beetle_psx_analog_toggle".into(), "enabled".into())));
        assert_eq!(download_base(&ARMSX2), ARMSX2_RELEASE);
        assert_eq!(download_base(core_for_platform("snes").unwrap()), BUILDBOT);
        if cfg!(target_os = "macos") {
            assert_eq!(core_for_platform("ps2").unwrap().id, "armsx2");
        }
    }

    async fn armsx2_release(zip: Vec<u8>, checksum: String) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/{ARMSX2_ASSET}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{ARMSX2_ASSET}.sha256")))
            .respond_with(ResponseTemplate::new(200).set_body_string(checksum))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn release_cores_are_checked_and_found_inside_folders() {
        let zip = zip_with(
            &format!("armsx2-macos-arm64-libretro/{}", lib_file(&ARMSX2)),
            b"ps2",
        );
        let checksum = format!("{}  {ARMSX2_ASSET}\n", sha256_hex(&zip));
        let server = armsx2_release(zip, checksum).await;
        let dir = tempfile::tempdir().unwrap();
        let cores = Cores::new(dir.path().to_path_buf());
        let installed = cores
            .install(&reqwest::Client::new(), &server.uri(), &ARMSX2)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&installed).unwrap(), b"ps2");
        assert!(installed.ends_with(format!("armsx2/{}", lib_file(&ARMSX2))));
    }

    #[tokio::test]
    async fn a_newer_release_core_is_noticed() {
        let zip = zip_with(&lib_file(&ARMSX2), b"ps2");
        let checksum = sha256_hex(&zip);
        let server = armsx2_release(zip, format!("{checksum}  {ARMSX2_ASSET}\n")).await;
        let dir = tempfile::tempdir().unwrap();
        let cores = Cores::new(dir.path().to_path_buf());
        let http = reqwest::Client::new();
        assert!(!cores.update_available(&http, &server.uri(), &ARMSX2).await);
        cores.install(&http, &server.uri(), &ARMSX2).await.unwrap();
        assert!(!cores.update_available(&http, &server.uri(), &ARMSX2).await);

        let newer = armsx2_release(Vec::new(), format!("{}  x\n", "1".repeat(64))).await;
        assert!(cores.update_available(&http, &newer.uri(), &ARMSX2).await);
        let offline = "http://127.0.0.1:9";
        assert!(!cores.update_available(&http, offline, &ARMSX2).await);
    }

    #[tokio::test]
    async fn release_cores_that_do_not_match_their_checksum_are_refused() {
        let zip = zip_with(&lib_file(&ARMSX2), b"ps2");
        let server = armsx2_release(zip, format!("{}  x\n", "0".repeat(64))).await;
        let dir = tempfile::tempdir().unwrap();
        let cores = Cores::new(dir.path().to_path_buf());
        let result = cores
            .install(&reqwest::Client::new(), &server.uri(), &ARMSX2)
            .await;
        assert!(result.is_err());
        assert!(cores.installed(&ARMSX2).is_none());
    }

    #[test]
    fn play_does_not_resume_from_automatic_saves() {
        assert!(!resumes_reliably("play"));
        assert!(resumes_reliably("snes9x"));
    }

    #[test]
    fn buildbot_url_targets_this_host() {
        let url = buildbot_url(BUILDBOT, core_for_platform("snes").unwrap());
        if cfg!(target_os = "macos") {
            assert_eq!(url, "https://buildbot.libretro.com/nightly/apple/osx/arm64/latest/snes9x_libretro.dylib.zip");
        } else if cfg!(windows) {
            assert_eq!(
                url,
                "https://buildbot.libretro.com/nightly/windows/x86_64/latest/snes9x_libretro.dll.zip"
            );
        } else {
            assert_eq!(
                url,
                "https://buildbot.libretro.com/nightly/linux/x86_64/latest/snes9x_libretro.so.zip"
            );
        }
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    #[tokio::test]
    async fn system_files_are_installed_once_into_the_system_folder() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/Dolphin.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_of(&[
                ("dolphin-emu/Sys/codehandler.bin", b"code"),
                ("dolphin-emu/Sys/GC/font.bin", b"font"),
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let core = core_for_platform("ngc").unwrap();
        assert!(!system_files_present(core, dir.path()));
        let http = reqwest::Client::new();
        install_system_files(&http, &server.uri(), core, dir.path())
            .await
            .unwrap();
        assert!(system_files_present(core, dir.path()));
        assert_eq!(
            std::fs::read(dir.path().join("dolphin-emu/Sys/GC/font.bin")).unwrap(),
            b"font"
        );
        install_system_files(&http, &server.uri(), core, dir.path())
            .await
            .unwrap();
        let snes = core_for_platform("snes").unwrap();
        assert!(system_files_present(snes, dir.path()));
    }

    #[tokio::test]
    async fn system_files_that_escape_the_folder_are_refused() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/blueMSX.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_of(&[
                ("Databases/msxromdb.xml", b"db"),
                ("../evil.txt", b"x"),
            ])))
            .mount(&server)
            .await;
        let root = tempfile::tempdir().unwrap();
        let system = root.path().join("system");
        let core = core_for_platform("msx").unwrap();
        let result =
            install_system_files(&reqwest::Client::new(), &server.uri(), core, &system).await;
        assert!(result.is_err());
        assert!(!root.path().join("evil.txt").exists());
        assert!(!system_files_present(core, &system));
    }

    #[tokio::test]
    async fn install_extracts_and_verifies() {
        let core = core_for_platform("snes").unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(buildbot_url("", core).as_str()))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(zip_with(&lib_file(core), b"core-bytes")),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cores = Cores::new(dir.path().to_path_buf());
        assert!(cores.installed(core).is_none());
        let installed = cores
            .install(&reqwest::Client::new(), &server.uri(), core)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&installed).unwrap(), b"core-bytes");
        assert_eq!(cores.installed(core), Some(installed));
    }

    #[tokio::test]
    async fn tampered_core_is_not_installed() {
        let core = core_for_platform("snes").unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(zip_with(&lib_file(core), b"core-bytes")),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cores = Cores::new(dir.path().to_path_buf());
        let installed = cores
            .install(&reqwest::Client::new(), &server.uri(), core)
            .await
            .unwrap();
        std::fs::write(&installed, b"evil").unwrap();
        assert!(cores.installed(core).is_none());
    }

    #[tokio::test]
    async fn archive_without_the_library_fails() {
        let core = core_for_platform("snes").unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_with("other.txt", b"x")))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        assert!(Cores::new(dir.path().to_path_buf())
            .install(&reqwest::Client::new(), &server.uri(), core)
            .await
            .is_err());
    }

    #[test]
    fn amiga_defaults_boot_a_real_kickstart() {
        let options = default_options("puae");
        assert!(options.contains(&("puae_kickstart".into(), "auto".into())));
        assert!(options.contains(&("puae_model".into(), "A600".into())));
        assert!(default_options("mednafen_psx_hw")
            .contains(&("beetle_psx_analog_toggle".into(), "enabled".into())));
        assert!(default_options("snes9x").is_empty());
    }
}
