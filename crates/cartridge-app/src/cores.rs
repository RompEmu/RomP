use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;

pub const BUILDBOT: &str = "https://buildbot.libretro.com/nightly";

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
    (&["ps2"], core("play", "Play!", "play_libretro", true)),
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

pub fn is_computer(core_id: &str) -> bool {
    matches!(core_id, "puae" | "dosbox_pure" | "vice_x64sc" | "fuse")
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
    } else {
        "so"
    };
    format!("{}.{ext}", core.lib)
}

pub fn buildbot_url(base: &str, core: &CoreInfo) -> String {
    let target = if cfg!(target_os = "macos") {
        "apple/osx/arm64"
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
        "desmume" => &[
            ("desmume_pointer_type", "touch"),
            ("desmume_screens_layout", "top/bottom"),
        ],
        // GLideN64 renders black with the macOS OpenGL driver.
        "mupen64plus_next" if cfg!(target_os = "macos") => {
            &[("mupen64plus-rdp-plugin", "angrylion")]
        }
        _ => &[],
    };
    options
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
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

pub struct Cores {
    dir: PathBuf,
}

impl Cores {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
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
        let resp = http
            .get(buildbot_url(base, core))
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
        let version = resp
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("nightly")
            .to_string();
        let zip_bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        let file = lib_file(core);
        let lib = tokio::task::spawn_blocking({
            let file = file.clone();
            move || -> Result<Vec<u8>, String> {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
                    .map_err(|e| e.to_string())?;
                let mut entry = archive
                    .by_name(&file)
                    .map_err(|_| format!("{file} is missing from the download"))?;
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
    fn buildbot_url_targets_this_host() {
        let url = buildbot_url(BUILDBOT, core_for_platform("snes").unwrap());
        if cfg!(target_os = "macos") {
            assert_eq!(url, "https://buildbot.libretro.com/nightly/apple/osx/arm64/latest/snes9x_libretro.dylib.zip");
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
