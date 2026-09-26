use crate::romm::client::{Client, Error, SaveUpload};
use crate::romm::types::ClientSave;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn file_iso_mtime(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(crate::sync::iso_utc(modified))
}

pub fn backup(dir: &Path, file: &str) -> io::Result<Option<PathBuf>> {
    let source = dir.join(file);
    if !source.exists() {
        return Ok(None);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let target_dir = dir.join("backup").join(stamp.to_string());
    std::fs::create_dir_all(&target_dir)?;
    let target = target_dir.join(file);
    std::fs::copy(&source, &target)?;
    Ok(Some(target))
}

pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn clean(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn state_remote_name(slot: &str, core_id: &str, core_version: &str) -> String {
    format!(
        "{}.{}.{}.state",
        clean(slot),
        clean(core_id),
        clean(core_version)
    )
}

pub fn parse_state_name(name: &str) -> Option<(String, String, String)> {
    let stem = name.strip_suffix(".state")?;
    let mut parts = stem.split('.');
    let (slot, core, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    Some((slot.into(), core.into(), version.into()))
}

pub const SRAM_FILE: &str = "game.srm";
pub const SRAM_SLOT: &str = "autosave";

#[derive(Debug, Clone)]
pub struct GameSaves {
    pub rom_id: i64,
    pub dir: PathBuf,
    pub title: String,
    pub emulator: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SramConflict {
    pub save_id: Option<i64>,
    pub server_updated_at: Option<String>,
    pub local_updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SramOutcome {
    InSync,
    Uploaded,
    Downloaded { backup: Option<PathBuf> },
    Conflict(SramConflict),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    Local,
    Server,
}

impl GameSaves {
    fn sram_path(&self) -> PathBuf {
        self.dir.join(SRAM_FILE)
    }

    fn remote_sram_name(&self) -> String {
        let clean: String = self
            .title
            .chars()
            .map(|c| {
                if c.is_control() || "/\\:|*?\"<>+".contains(c) {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        format!("{}.srm", clean.trim())
    }
}

async fn upload_sram(
    client: &Client,
    device_id: &str,
    game: &GameSaves,
    bytes: Vec<u8>,
    session_id: Option<i64>,
    overwrite: bool,
) -> Result<(), Error> {
    let name = game.remote_sram_name();
    client
        .upload_save(SaveUpload {
            rom_id: game.rom_id,
            slot: SRAM_SLOT,
            emulator: &game.emulator,
            device_id,
            session_id,
            overwrite,
            file_name: &name,
            bytes,
        })
        .await
        .map(|_| ())
}

fn install_download(game: &GameSaves, bytes: &[u8]) -> Result<SramOutcome, Error> {
    let io_err = |e: io::Error| Error::Decode(format!("could not write the save: {e}"));
    let backup = backup(&game.dir, SRAM_FILE).map_err(io_err)?;
    replace_file(&game.sram_path(), bytes).map_err(io_err)?;
    Ok(SramOutcome::Downloaded { backup })
}

pub async fn sync_sram(
    client: &Client,
    device_id: &str,
    game: &GameSaves,
) -> Result<SramOutcome, Error> {
    let local = std::fs::read(game.sram_path()).ok();
    let local_updated_at = file_iso_mtime(&game.sram_path());
    let saves: Vec<ClientSave> = local
        .as_ref()
        .map(|bytes| ClientSave {
            rom_id: game.rom_id,
            file_name: game.remote_sram_name(),
            slot: Some(SRAM_SLOT.into()),
            emulator: Some(game.emulator.clone()),
            content_hash: Some(md5_hex(bytes)),
            updated_at: local_updated_at.clone().unwrap_or_default(),
            file_size_bytes: bytes.len() as u64,
        })
        .into_iter()
        .collect();
    let negotiation = client.negotiate(device_id, &saves, &[game.rom_id]).await?;
    let session = negotiation.session_id;
    let op = negotiation
        .operations
        .into_iter()
        .find(|o| o.rom_id == game.rom_id);
    let result = match (op.as_ref().map(|o| o.action.as_str()), local) {
        (Some("upload"), Some(bytes)) => {
            match upload_sram(client, device_id, game, bytes, Some(session), false).await {
                Ok(()) => Ok(SramOutcome::Uploaded),
                Err(Error::Conflict) => Ok(SramOutcome::Conflict(SramConflict {
                    save_id: None,
                    server_updated_at: None,
                    local_updated_at,
                })),
                Err(e) => Err(e),
            }
        }
        (Some("download"), _) => match op.as_ref().and_then(|o| o.save_id) {
            Some(save_id) => {
                let bytes = client
                    .download_save(save_id, device_id, Some(session))
                    .await?;
                install_download(game, &bytes)
            }
            None => Err(Error::Decode("download without a save id".into())),
        },
        (Some("conflict"), _) => {
            let op = op.expect("conflict operation");
            Ok(SramOutcome::Conflict(SramConflict {
                save_id: op.save_id,
                server_updated_at: op.server_updated_at,
                local_updated_at,
            }))
        }
        _ => Ok(SramOutcome::InSync),
    };
    match &result {
        Ok(SramOutcome::Conflict(_)) => {}
        Ok(_) => {
            let _ = client.complete_session(session, 1, 0).await;
        }
        Err(_) => {
            let _ = client.complete_session(session, 0, 1).await;
        }
    }
    result
}

pub async fn resolve_sram(
    client: &Client,
    device_id: &str,
    game: &GameSaves,
    conflict: &SramConflict,
    keep: Keep,
) -> Result<SramOutcome, Error> {
    match keep {
        Keep::Local => {
            let bytes = std::fs::read(game.sram_path())
                .map_err(|e| Error::Decode(format!("could not read the save: {e}")))?;
            upload_sram(client, device_id, game, bytes, None, true).await?;
            Ok(SramOutcome::Uploaded)
        }
        Keep::Server => {
            let save_id = match conflict.save_id {
                Some(id) => id,
                None => client
                    .list_saves(game.rom_id, SRAM_SLOT, device_id)
                    .await?
                    .into_iter()
                    .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
                    .map(|s| s.id)
                    .ok_or_else(|| Error::Decode("no server save to use".into()))?,
            };
            let bytes = client.download_save(save_id, device_id, None).await?;
            install_download(game, &bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_known_vector() {
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn mtime_is_iso_utc() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("game.srm");
        std::fs::write(&file, b"x").unwrap();
        let iso = file_iso_mtime(&file).unwrap();
        assert_eq!(iso.len(), "2026-09-26T10:00:00+00:00".len());
        assert!(iso.ends_with("+00:00"));
        assert!(file_iso_mtime(&dir.path().join("missing")).is_none());
    }

    #[test]
    fn backup_copies_existing_file_only() {
        let dir = tempfile::tempdir().unwrap();
        assert!(backup(dir.path(), "game.srm").unwrap().is_none());
        std::fs::write(dir.path().join("game.srm"), b"old").unwrap();
        let copy = backup(dir.path(), "game.srm").unwrap().unwrap();
        assert!(copy.starts_with(dir.path().join("backup")));
        assert_eq!(std::fs::read(copy).unwrap(), b"old");
        assert_eq!(std::fs::read(dir.path().join("game.srm")).unwrap(), b"old");
    }

    #[test]
    fn replace_file_writes_new_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/game.srm");
        replace_file(&file, b"one").unwrap();
        replace_file(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        assert_eq!(
            std::fs::read_dir(dir.path().join("sub")).unwrap().count(),
            1
        );
    }

    #[test]
    fn state_names_round_trip_and_sanitize() {
        let name = state_remote_name("slot-1", "snes9x", "1.63 185488c");
        assert_eq!(name, "slot-1.snes9x.1-63-185488c.state");
        assert_eq!(
            parse_state_name(&name),
            Some(("slot-1".into(), "snes9x".into(), "1-63-185488c".into()))
        );
        assert_eq!(parse_state_name("Zelda.state"), None);
        assert_eq!(parse_state_name("a.b.c.srm"), None);
        assert_eq!(
            state_remote_name("auto", "../x", "v/1"),
            "auto.---x.v-1.state"
        );
    }

    mod sram {
        use super::super::*;
        use crate::romm::client::tests::base_of;
        use serde_json::json;
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        fn game(dir: &Path) -> GameSaves {
            GameSaves {
                rom_id: 5,
                dir: dir.to_path_buf(),
                title: "Zelda: Link".into(),
                emulator: "snes9x".into(),
            }
        }

        async fn negotiation(server: &MockServer, ops: serde_json::Value) {
            Mock::given(method("POST"))
                .and(path("/api/sync/negotiate"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"session_id": 9, "operations": ops})),
                )
                .mount(server)
                .await;
        }

        async fn completion(server: &MockServer, times: u64) {
            Mock::given(method("POST"))
                .and(path("/api/sync/sessions/9/complete"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(times)
                .mount(server)
                .await;
        }

        fn save_json(id: i64, updated: &str) -> serde_json::Value {
            json!({"id": id, "rom_id": 5, "file_name": "Zelda [t].srm", "slot": "autosave",
                   "updated_at": updated, "content_hash": "h"})
        }

        fn client(server: &MockServer) -> Client {
            Client::new(base_of(server, "/")).with_token("t".into())
        }

        #[tokio::test]
        async fn nothing_to_sync() {
            let server = MockServer::start().await;
            negotiation(&server, json!([])).await;
            completion(&server, 1).await;
            Mock::given(method("POST"))
                .and(path("/api/saves"))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            let outcome = sync_sram(&client(&server), "dev", &game(dir.path())).await;
            assert_eq!(outcome.unwrap(), SramOutcome::InSync);
        }

        #[tokio::test]
        async fn local_only_uploads() {
            let server = MockServer::start().await;
            negotiation(&server, json!([{"action": "upload", "rom_id": 5, "save_id": null, "file_name": "Zelda- Link.srm"}])).await;
            completion(&server, 1).await;
            Mock::given(method("POST"))
                .and(path("/api/saves"))
                .and(query_param("slot", "autosave"))
                .and(query_param("session_id", "9"))
                .and(query_param("overwrite", "false"))
                .respond_with(ResponseTemplate::new(200).set_body_json(save_json(1, "t")))
                .expect(1)
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(SRAM_FILE), b"mine").unwrap();
            let outcome = sync_sram(&client(&server), "dev", &game(dir.path())).await;
            assert_eq!(outcome.unwrap(), SramOutcome::Uploaded);
        }

        #[tokio::test]
        async fn server_newer_downloads_and_backs_up() {
            let server = MockServer::start().await;
            negotiation(
                &server,
                json!([{"action": "download", "rom_id": 5, "save_id": 4, "file_name": "x.srm"}]),
            )
            .await;
            completion(&server, 1).await;
            Mock::given(method("GET"))
                .and(path("/api/saves/4/content"))
                .and(query_param("session_id", "9"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b"new".to_vec()))
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(SRAM_FILE), b"old").unwrap();
            let outcome = sync_sram(&client(&server), "dev", &game(dir.path()))
                .await
                .unwrap();
            let SramOutcome::Downloaded {
                backup: Some(backup),
            } = outcome
            else {
                panic!("{outcome:?}")
            };
            assert_eq!(std::fs::read(backup).unwrap(), b"old");
            assert_eq!(std::fs::read(dir.path().join(SRAM_FILE)).unwrap(), b"new");
        }

        #[tokio::test]
        async fn conflict_is_reported_not_resolved() {
            let server = MockServer::start().await;
            negotiation(
                &server,
                json!([{"action": "conflict", "rom_id": 5, "save_id": 4, "file_name": "x.srm",
                "server_updated_at": "2026-09-26T10:00:00+00:00"}]),
            )
            .await;
            completion(&server, 0).await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(SRAM_FILE), b"mine").unwrap();
            let outcome = sync_sram(&client(&server), "dev", &game(dir.path()))
                .await
                .unwrap();
            let SramOutcome::Conflict(c) = outcome else {
                panic!()
            };
            assert_eq!(c.save_id, Some(4));
            assert_eq!(
                c.server_updated_at.as_deref(),
                Some("2026-09-26T10:00:00+00:00")
            );
            assert!(c.local_updated_at.is_some());
            assert_eq!(std::fs::read(dir.path().join(SRAM_FILE)).unwrap(), b"mine");
        }

        #[tokio::test]
        async fn upload_409_becomes_conflict() {
            let server = MockServer::start().await;
            negotiation(
                &server,
                json!([{"action": "upload", "rom_id": 5, "save_id": null, "file_name": "x.srm"}]),
            )
            .await;
            Mock::given(method("POST"))
                .and(path("/api/saves"))
                .respond_with(ResponseTemplate::new(409).set_body_json(json!({"detail": "newer"})))
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(SRAM_FILE), b"mine").unwrap();
            let outcome = sync_sram(&client(&server), "dev", &game(dir.path()))
                .await
                .unwrap();
            assert!(matches!(
                outcome,
                SramOutcome::Conflict(SramConflict { save_id: None, .. })
            ));
        }

        #[tokio::test]
        async fn resolving_backs_up_the_loser() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/api/saves"))
                .and(query_param("slot", "autosave"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    save_json(6, "2026-09-25T10:00:00+00:00"),
                    save_json(7, "2026-09-26T10:00:00+00:00")
                ])))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/saves/7/content"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b"server".to_vec()))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/api/saves"))
                .and(query_param("overwrite", "true"))
                .respond_with(ResponseTemplate::new(200).set_body_json(save_json(8, "t")))
                .expect(1)
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(SRAM_FILE), b"local").unwrap();
            let conflict = SramConflict {
                save_id: None,
                server_updated_at: None,
                local_updated_at: None,
            };
            let c = client(&server);
            let g = game(dir.path());
            assert_eq!(
                resolve_sram(&c, "dev", &g, &conflict, Keep::Local)
                    .await
                    .unwrap(),
                SramOutcome::Uploaded
            );
            let outcome = resolve_sram(&c, "dev", &g, &conflict, Keep::Server)
                .await
                .unwrap();
            let SramOutcome::Downloaded {
                backup: Some(backup),
            } = outcome
            else {
                panic!()
            };
            assert_eq!(std::fs::read(backup).unwrap(), b"local");
            assert_eq!(
                std::fs::read(dir.path().join(SRAM_FILE)).unwrap(),
                b"server"
            );
        }
    }
}
