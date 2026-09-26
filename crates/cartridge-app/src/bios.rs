use crate::download::{download_file, DownloadError};
use crate::romm::client::Client;
use crate::romm::types::Firmware;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

const DC_FILES: [&str; 4] = ["dc_boot.bin", "dc_flash.bin", "naomi.zip", "awbios.zip"];
pub const ARCADE: [&str; 6] = ["arcade", "neogeoaes", "neogeomvs", "cps1", "cps2", "cps3"];

static REQUIRED: &[(&str, &[&[&str]])] = &[
    ("psx", &[&["scph5500.bin", "scph5501.bin", "scph5502.bin"]]),
    (
        "segacd",
        &[&["bios_CD_E.bin", "bios_CD_U.bin", "bios_CD_J.bin"]],
    ),
    ("saturn", &[&["sega_101.bin", "mpr-17933.bin"]]),
    (
        "turbografx-cd",
        &[&[
            "syscard3.pce",
            "syscard2.pce",
            "syscard1.pce",
            "gexpress.pce",
        ]],
    ),
    ("intellivision", &[&["grom.bin"], &["exec.bin"]]),
    ("odyssey-2", &[&["o2rom.bin"]]),
    ("3do", &[&["panafz1.bin", "panafz10.bin", "goldstar.bin"]]),
    ("lynx", &[&["lynxboot.img"]]),
    ("fds", &[&["disksys.rom"]]),
    ("neogeoaes", &[&["neogeo.zip"]]),
    ("neogeomvs", &[&["neogeo.zip"]]),
];

pub fn placement(platform_slug: &str, file_name: &str) -> PathBuf {
    if ARCADE.contains(&platform_slug) {
        Path::new("fbneo").join(file_name)
    } else if DC_FILES.iter().any(|f| f.eq_ignore_ascii_case(file_name)) {
        Path::new("dc").join(file_name)
    } else {
        PathBuf::from(file_name)
    }
}

fn present(system_dir: &Path, platform_slug: &str, file_name: &str) -> bool {
    let rel = placement(platform_slug, file_name);
    let dir = system_dir.join(rel.parent().unwrap_or(Path::new("")));
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(file_name)
        })
    })
}

fn join_names(names: &[&str]) -> String {
    match names {
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
        [] => String::new(),
    }
}

fn unmet(platform_slug: &str, system_dir: &Path) -> Vec<&'static [&'static str]> {
    let Some((_, groups)) = REQUIRED.iter().find(|(slug, _)| *slug == platform_slug) else {
        return Vec::new();
    };
    groups
        .iter()
        .copied()
        .filter(|group| !group.iter().any(|f| present(system_dir, platform_slug, f)))
        .collect()
}

pub fn missing(platform_slug: &str, system_dir: &Path) -> Vec<String> {
    unmet(platform_slug, system_dir)
        .into_iter()
        .map(join_names)
        .collect()
}

pub fn missing_message(missing: &[String]) -> String {
    format!(
        "This game needs BIOS files your server doesn't have: {}. Add them to this platform's firmware in RomM.",
        missing.join("; ")
    )
}

pub async fn ensure(
    client: &Client,
    platform_id: i64,
    platform_slug: &str,
    system_dir: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<String>, DownloadError> {
    fetch_firmware(client, platform_id, platform_slug, system_dir, cancel).await?;
    let wanted: Vec<&str> = unmet(platform_slug, system_dir).concat();
    if !wanted.is_empty() {
        let elsewhere: Vec<Firmware> = client
            .firmware(None)
            .await
            .map_err(DownloadError::Network)?
            .into_iter()
            .filter(|fw| wanted.iter().any(|w| w.eq_ignore_ascii_case(&fw.file_name)))
            .collect();
        place(client, elsewhere, platform_slug, system_dir, cancel).await?;
    }
    Ok(missing(platform_slug, system_dir))
}

pub async fn fetch_firmware(
    client: &Client,
    platform_id: i64,
    platform_slug: &str,
    system_dir: &Path,
    cancel: &AtomicBool,
) -> Result<usize, DownloadError> {
    let list = client
        .firmware(Some(platform_id))
        .await
        .map_err(DownloadError::Network)?;
    place(client, list, platform_slug, system_dir, cancel).await
}

async fn place(
    client: &Client,
    list: Vec<Firmware>,
    platform_slug: &str,
    system_dir: &Path,
    cancel: &AtomicBool,
) -> Result<usize, DownloadError> {
    let mut fetched = 0;
    for fw in list {
        let rel = placement(platform_slug, &fw.file_name);
        if fw.file_name.is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(DownloadError::UnsafePath);
        }
        let dest = system_dir.join(rel);
        let existed = dest.exists();
        download_file(
            client,
            client.firmware_url(&fw),
            &dest,
            fw.sha1_hash.as_deref(),
            None,
            &|_| {},
            cancel,
        )
        .await?;
        if !existed {
            fetched += 1;
        }
    }
    Ok(fetched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::client::tests::base_of;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn dreamcast_files_go_under_dc() {
        assert_eq!(
            placement("dc", "dc_boot.bin"),
            PathBuf::from("dc/dc_boot.bin")
        );
        assert_eq!(
            placement("psx", "scph5501.bin"),
            PathBuf::from("scph5501.bin")
        );
    }

    #[test]
    fn arcade_bios_goes_where_finalburn_neo_looks() {
        for slug in ["arcade", "neogeoaes", "neogeomvs", "cps2"] {
            assert_eq!(
                placement(slug, "neogeo.zip"),
                PathBuf::from("fbneo/neogeo.zip")
            );
        }
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(missing("neogeomvs", dir.path()), ["neogeo.zip"]);
        std::fs::create_dir(dir.path().join("fbneo")).unwrap();
        std::fs::write(dir.path().join("fbneo/neogeo.zip"), b"x").unwrap();
        assert!(missing("neogeomvs", dir.path()).is_empty());
    }

    #[tokio::test]
    async fn missing_bios_is_found_under_other_platforms() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/firmware"))
            .and(query_param("platform_id", "4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/firmware"))
            .and(query_param_is_missing("platform_id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": 1, "file_name": "BIOS_CD_U.BIN", "sha1_hash": null},
                {"id": 2, "file_name": "scph5501.bin", "sha1_hash": null}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/firmware/1/content/BIOS_CD_U.BIN"))
            .respond_with(ResponseTemplate::new(200).set_body_string("cd"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/firmware/2/content/scph5501.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ps"))
            .expect(0)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/"));
        let still_missing = ensure(&client, 4, "segacd", dir.path(), &AtomicBool::new(false))
            .await
            .unwrap();
        assert!(still_missing.is_empty());
        assert_eq!(
            std::fs::read(dir.path().join("BIOS_CD_U.BIN")).unwrap(),
            b"cd"
        );
    }

    #[tokio::test]
    async fn bios_the_server_lacks_is_reported() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/firmware"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/"));
        assert_eq!(
            ensure(&client, 9, "lynx", dir.path(), &AtomicBool::new(false))
                .await
                .unwrap(),
            ["lynxboot.img"]
        );
        assert!(
            ensure(&client, 9, "snes", dir.path(), &AtomicBool::new(false))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missing_reports_unmet_groups_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            missing("psx", dir.path()),
            ["scph5500.bin, scph5501.bin or scph5502.bin"]
        );
        std::fs::write(dir.path().join("SCPH5501.BIN"), b"x").unwrap();
        assert!(missing("psx", dir.path()).is_empty());
        std::fs::write(dir.path().join("grom.bin"), b"x").unwrap();
        assert_eq!(missing("intellivision", dir.path()), ["exec.bin"]);
        assert!(missing("snes", dir.path()).is_empty());
    }

    #[tokio::test]
    async fn fetch_downloads_platform_firmware_into_place() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/firmware"))
            .and(query_param("platform_id", "8"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": 5, "file_name": "dc_boot.bin", "file_path": "bios/dc", "file_size_bytes": 3, "sha1_hash": null}])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/firmware/5/content/dc_boot.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_string("bio"))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/"));
        let n = fetch_firmware(&client, 8, "dc", dir.path(), &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            std::fs::read(dir.path().join("dc/dc_boot.bin")).unwrap(),
            b"bio"
        );
    }
}
