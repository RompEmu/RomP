use crate::download::{download_file, DownloadError};
use crate::layout::{launch_target, m3u, plan, safe_component, Launch};
use crate::romm::client::Client;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub async fn download_game(
    client: &Client,
    rom_id: i64,
    roms_dir: &Path,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<PathBuf, DownloadError> {
    let rom = client
        .rom_detail(rom_id)
        .await
        .map_err(DownloadError::Network)?;
    if !safe_component(&rom.platform_slug) {
        return Err(DownloadError::UnsafePath);
    }
    let files = plan(&rom).map_err(|_| DownloadError::UnsafePath)?;
    let dir = roms_dir.join(&rom.platform_slug).join(rom.id.to_string());
    let total: u64 = files
        .iter()
        .map(|f| f.file.file_size_bytes.max(0) as u64)
        .sum();
    let mut done = 0u64;
    for planned in &files {
        let base = done;
        download_file(
            client,
            client.rom_file_url(rom.id, &planned.file),
            &dir.join(&planned.rel_path),
            planned.file.sha1_hash.as_deref(),
            Some(planned.file.file_size_bytes.max(0) as u64),
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
    match launch_target(&rom, &rels) {
        Some(Launch::File(rel)) => Ok(dir.join(rel)),
        Some(Launch::Playlist { name, discs }) => {
            let path = dir.join(name);
            tokio::fs::write(&path, m3u(&discs))
                .await
                .map_err(|e| DownloadError::Io(e.to_string()))?;
            Ok(path)
        }
        None => Err(DownloadError::Io("nothing to launch".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::client::tests::base_of;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
        for (id, body) in [("1", "one"), ("2", "twoo")] {
            Mock::given(method("GET"))
                .and(query_param("file_ids", id))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(&server)
                .await;
        }
        let dir = tempfile::tempdir().unwrap();
        let client = Client::new(base_of(&server, "/")).with_token("t".into());
        let last = std::sync::Mutex::new((0, 0));
        let launch = download_game(
            &client,
            7,
            dir.path(),
            &|d, t| *last.lock().unwrap() = (d, t),
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        let game_dir = dir.path().join("psx/7");
        assert_eq!(launch, game_dir.join("Arc.m3u"));
        assert_eq!(
            std::fs::read_to_string(&launch).unwrap(),
            "Arc (Disc 1).chd\nArc (Disc 2).chd\n"
        );
        assert_eq!(
            std::fs::read_to_string(game_dir.join("Arc (Disc 2).chd")).unwrap(),
            "twoo"
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
        let err = download_game(&client, 7, dir.path(), &|_, _| {}, &AtomicBool::new(false))
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
        let err = download_game(&client, 7, dir.path(), &|_, _| {}, &AtomicBool::new(false))
            .await
            .unwrap_err();
        assert!(matches!(err, DownloadError::UnsafePath));
    }
}
