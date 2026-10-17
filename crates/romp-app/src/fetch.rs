use crate::download::{already_there, download_file, DownloadError, Foreign};
use crate::layout::{launch_target, m3u, plan, safe_component, Launch};
use crate::romm::client::Client;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const CONTENT_HASHED: [&str; 2] = ["zip", "7z"];

/// Formats only another emulator reads: (extension, that emulator, what RomP plays instead).
const FOREIGN_FORMATS: [(&str, &str, &str); 1] = [("bigpimg", "BigPEmu", ".cue, .cdi or .chd")];

pub fn foreign_format(file_name: &str) -> Option<Foreign> {
    let ext = Path::new(file_name)
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase();
    FOREIGN_FORMATS
        .iter()
        .find(|(e, _, _)| *e == ext)
        .map(|(e, owner, instead)| Foreign {
            ext: format!(".{e}"),
            owner,
            instead,
        })
}

// RomM hashes the ROM inside an archive, not the archive's own bytes.
fn verifiable_sha1<'a>(file_name: &str, sha1: Option<&'a str>) -> Option<&'a str> {
    let ext = Path::new(file_name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    sha1.filter(|_| !CONTENT_HASHED.contains(&ext.as_str()))
}

/// What a download left: the file to start, and whether anything had to be fetched for it.
#[derive(Debug)]
pub struct Downloaded {
    pub launch: PathBuf,
    pub fetched: bool,
}

pub async fn download_files(
    client: &Client,
    rom_id: i64,
    roms_dir: &Path,
    only: Option<&[i64]>,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<Downloaded, DownloadError> {
    let rom = client
        .rom_detail(rom_id)
        .await
        .map_err(DownloadError::Network)?;
    if !safe_component(&rom.platform_slug) {
        return Err(DownloadError::UnsafePath);
    }
    let files = plan(&rom).map_err(|_| DownloadError::UnsafePath)?;
    // Said before a byte is fetched, rather than by the emulator after the whole download.
    if let Some(found) = files.iter().find_map(|f| foreign_format(&f.file.file_name)) {
        return Err(DownloadError::Foreign(found));
    }
    // RomM's "Install on device" names the files to send: the game, its updates and DLC.
    let files: Vec<_> = match only {
        Some(ids) => files
            .into_iter()
            .filter(|f| ids.contains(&f.file.id))
            .collect(),
        None => files,
    };
    if files.is_empty() {
        return Err(DownloadError::Io("nothing to download".into()));
    }
    let dir = roms_dir.join(&rom.platform_slug).join(rom.id.to_string());
    let total: u64 = files
        .iter()
        .map(|f| f.file.file_size_bytes.max(0) as u64)
        .sum();
    let mut done = 0u64;
    let mut fetched = false;
    for planned in &files {
        let base = done;
        let dest = dir.join(&planned.rel_path);
        let sha1 = verifiable_sha1(&planned.file.file_name, planned.file.sha1_hash.as_deref());
        let size = Some(planned.file.file_size_bytes.max(0) as u64);
        fetched |= !already_there(&dest, sha1, size).await;
        download_file(
            client,
            client.rom_file_url(rom.id, &planned.file),
            &dest,
            sha1,
            size,
            &|n| progress(base + n, total),
            cancel,
        )
        .await?;
        done += planned.file.file_size_bytes.max(0) as u64;
    }
    let rels: Vec<(PathBuf, u64)> = files
        .iter()
        .map(|f| (f.rel_path.clone(), f.file.file_size_bytes.max(0) as u64))
        .collect();
    let launch = match launch_target(&rom, &rels) {
        Some(Launch::File(rel)) => dir.join(rel),
        Some(Launch::Playlist { name, discs }) => {
            let path = dir.join(name);
            tokio::fs::write(&path, m3u(&discs))
                .await
                .map_err(|e| DownloadError::Io(e.to_string()))?;
            path
        }
        None => return Err(DownloadError::Io("nothing to launch".into())),
    };
    Ok(Downloaded { launch, fetched })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_emulators_own_format_is_named_before_downloading() {
        let found = foreign_format("Battle Morph.bigpimg").unwrap();
        assert_eq!(found.owner, "BigPEmu");
        assert_eq!(found.instead, ".cue, .cdi or .chd");
        assert!(foreign_format("Battle Morph.BIGPIMG").is_some());
        assert!(foreign_format("Battle Morph.cue").is_none());
        assert!(foreign_format("bigpimg").is_none());
        let text = DownloadError::Foreign(found).to_string();
        assert!(text.contains("BigPEmu") && text.contains(".bigpimg") && text.contains(".chd"));
    }
    use crate::romm::client::tests::base_of;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn files_already_here_are_not_fetched_again_and_say_so() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 7, "platform_id": 32, "platform_slug": "psx", "fs_name": "Arc", "fs_path": "roms/psx",
                "has_multiple_files": false,
                "files": [
                    {"id": 1, "file_name": "Arc.chd", "file_path": "roms/psx", "file_size_bytes": 4, "sha1_hash": null}
                ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("file_ids", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("disc"))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/")).with_token("t".into());
        let first = download_files(
            &client,
            7,
            dir.path(),
            Some(&[1]),
            &|_, _| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert!(first.fetched);
        let again = download_files(
            &client,
            7,
            dir.path(),
            Some(&[1]),
            &|_, _| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert!(!again.fetched);
        assert_eq!(again.launch, first.launch);
    }

    #[tokio::test]
    async fn only_the_files_romm_chose_come_down() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 7, "platform_id": 32, "platform_slug": "psx", "fs_name": "Arc", "fs_path": "roms/psx",
                "has_multiple_files": true,
                "files": [
                    {"id": 1, "file_name": "Arc (Disc 1).chd", "file_path": "roms/psx/Arc", "file_size_bytes": 3, "sha1_hash": null},
                    {"id": 2, "file_name": "Arc (Disc 2).chd", "file_path": "roms/psx/Arc", "file_size_bytes": 4, "sha1_hash": null}
                ]})))
            .mount(&server)
            .await;
        for (id, body, times) in [("1", "one", 0), ("2", "disc", 1)] {
            Mock::given(method("GET"))
                .and(query_param("file_ids", id))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .expect(times)
                .mount(&server)
                .await;
        }
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/")).with_token("t".into());
        let launch = download_files(
            &client,
            7,
            dir.path(),
            Some(&[2]),
            &|_, _| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap()
        .launch;
        let game_dir = dir.path().join("psx/7");
        assert!(launch.starts_with(&game_dir));
        assert!(game_dir.join("Arc (Disc 2).chd").exists());
        assert!(!game_dir.join("Arc (Disc 1).chd").exists());
    }

    #[tokio::test]
    async fn downloads_discs_and_writes_playlist() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 7, "platform_id": 32, "platform_slug": "psx", "fs_name": "Arc", "fs_path": "roms/psx",
                "has_multiple_files": true,
                "files": [
                    {"id": 1, "file_name": "Arc (Disc 1).chd", "file_path": "roms/psx/Arc", "file_size_bytes": 3, "sha1_hash": null},
                    {"id": 2, "file_name": "Arc (Disc 2).chd", "file_path": "roms/psx/Arc", "file_size_bytes": 4, "sha1_hash": null}
                ]})))
            .mount(&server)
            .await;
        for (id, body) in [("1", "one"), ("2", "disc")] {
            Mock::given(method("GET"))
                .and(query_param("file_ids", id))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(&server)
                .await;
        }
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/")).with_token("t".into());
        let last = std::sync::Mutex::new((0, 0));
        let launch = download_files(
            &client,
            7,
            dir.path(),
            None,
            &|d, t| *last.lock().unwrap() = (d, t),
            &AtomicBool::new(false),
        )
        .await
        .unwrap()
        .launch;
        let game_dir = dir.path().join("psx/7");
        assert_eq!(launch, game_dir.join("Arc.m3u"));
        assert_eq!(
            std::fs::read_to_string(&launch).unwrap(),
            "Arc (Disc 1).chd\nArc (Disc 2).chd\n"
        );
        assert_eq!(
            std::fs::read_to_string(game_dir.join("Arc (Disc 2).chd")).unwrap(),
            "disc"
        );
        assert_eq!(*last.lock().unwrap(), (7, 7));
    }

    #[tokio::test]
    async fn unsafe_paths_are_refused_before_downloading() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 7, "platform_id": 1, "platform_slug": "snes", "fs_name": "x", "fs_path": "roms/snes",
                "has_multiple_files": false,
                "files": [{"id": 1, "file_name": "../../evil", "file_path": "roms/snes", "file_size_bytes": 1, "sha1_hash": null}]})))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/"));
        let err = download_files(
            &client,
            7,
            dir.path(),
            None,
            &|_, _| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafePath));
    }

    #[tokio::test]
    async fn unsafe_platform_slug_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 7, "platform_slug": "..", "fs_name": "x", "fs_path": "roms/x",
                "has_multiple_files": false,
                "files": [{"id": 1, "file_name": "x.sfc", "file_path": "roms/x", "file_size_bytes": 1, "sha1_hash": null}]})))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/"));
        let err = download_files(
            &client,
            7,
            dir.path(),
            None,
            &|_, _| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafePath));
    }

    #[tokio::test]
    async fn archives_are_checked_by_size_because_romm_hashes_their_contents() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/8"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 8, "platform_slug": "genesis", "fs_name": "Pool.zip", "fs_path": "roms/genesis",
                "has_multiple_files": false,
                "files": [{"id": 1, "file_name": "Pool.zip", "file_path": "roms/genesis", "file_size_bytes": 7,
                           "sha1_hash": "443b96c518120078fb33f3ff9586a2b7ebc141c7"}]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("file_ids", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("zipdata"))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/"));
        let launch = download_files(
            &client,
            8,
            dir.path(),
            None,
            &|_, _| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap()
        .launch;
        assert_eq!(std::fs::read(launch).unwrap(), b"zipdata");
    }
}
