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

pub const BACKUPS_KEPT: usize = 10;

fn new_backup_dir(dir: &Path) -> io::Result<PathBuf> {
    let root = dir.join("backup");
    let mut stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    while root.join(format!("{stamp:015}")).exists() {
        stamp += 1;
    }
    let target = root.join(format!("{stamp:015}"));
    std::fs::create_dir_all(&target)?;
    Ok(target)
}

fn prune_backups(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir.join("backup")) else {
        return;
    };
    let mut stamped: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
        })
        .collect();
    stamped.sort_by_key(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<u128>().ok())
            .unwrap_or(0)
    });
    let excess = stamped.len().saturating_sub(BACKUPS_KEPT);
    for old in &stamped[..excess] {
        let _ = std::fs::remove_dir_all(old);
    }
}

pub fn backup_bytes(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let target = new_backup_dir(dir)?.join(name);
    std::fs::write(&target, bytes)?;
    prune_backups(dir);
    Ok(target)
}

pub fn backup(dir: &Path, file: &str) -> io::Result<Option<PathBuf>> {
    let source = dir.join(file);
    if !source.exists() {
        return Ok(None);
    }
    let target = new_backup_dir(dir)?.join(file);
    std::fs::copy(&source, &target)?;
    prune_backups(dir);
    Ok(Some(target))
}

fn newest(saves: Vec<crate::romm::types::RemoteSave>) -> Option<crate::romm::types::RemoteSave> {
    saves
        .into_iter()
        .max_by_key(|s| crate::sync::parse_iso(&s.updated_at).unwrap_or(i64::MIN))
}

pub const AUTO_STATE: &str = "auto.state";

pub fn set_aside_auto_state(dir: &Path) -> io::Result<()> {
    let source = dir.join(AUTO_STATE);
    if !source.exists() {
        return Ok(());
    }
    std::fs::rename(&source, new_backup_dir(dir)?.join("auto.state.failed"))?;
    prune_backups(dir);
    Ok(())
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
        .find(|o| o.rom_id == game.rom_id && o.slot.as_deref() == Some(SRAM_SLOT));
    let result = match (op.as_ref().map(|o| o.action.as_str()), local) {
        (Some("upload"), Some(bytes)) => {
            match upload_sram(client, device_id, game, bytes, Some(session), false).await {
                Ok(()) => Ok(SramOutcome::Uploaded),
                Err(Error::Conflict) => {
                    let latest = client
                        .list_saves(game.rom_id, SRAM_SLOT, device_id)
                        .await
                        .ok()
                        .and_then(newest);
                    Ok(SramOutcome::Conflict(SramConflict {
                        save_id: latest.as_ref().map(|s| s.id),
                        server_updated_at: latest.map(|s| s.updated_at),
                        local_updated_at,
                    }))
                }
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
    let (completed, failed) = match &result {
        Ok(SramOutcome::Uploaded | SramOutcome::Downloaded { .. }) => (1, 0),
        Ok(SramOutcome::InSync | SramOutcome::Conflict(_)) => (0, 0),
        Err(_) => (0, 1),
    };
    let _ = client.complete_session(session, completed, failed).await;
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
                None => newest(client.list_saves(game.rom_id, SRAM_SLOT, device_id).await?)
                    .map(|s| s.id)
                    .ok_or_else(|| Error::Decode("no server save to use".into()))?,
            };
            let bytes = client.download_save(save_id, device_id, None).await?;
            install_download(game, &bytes)
        }
    }
}

pub fn games_with_saves(server_saves: &Path) -> Vec<i64> {
    let has_save = |dir: &Path| {
        dir.join(SRAM_FILE).exists()
            || STATE_SLOTS
                .iter()
                .any(|slot| dir.join(format!("{slot}.state")).exists())
    };
    std::fs::read_dir(server_saves)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.parse::<i64>().ok()?;
            has_save(&entry.path()).then_some(id)
        })
        .collect()
}

pub const STATE_SLOTS: [&str; 6] = ["auto", "slot-1", "slot-2", "slot-3", "slot-4", "slot-5"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StateReport {
    pub uploaded: usize,
    pub downloaded: usize,
}

pub async fn sync_states(
    client: &Client,
    store: &std::sync::Mutex<crate::store::Store>,
    game: &GameSaves,
    core_version: &str,
) -> Result<StateReport, Error> {
    let remote = client.states(game.rom_id).await?;
    let io_err = |e: io::Error| Error::Decode(format!("could not write a save state: {e}"));
    let mut report = StateReport::default();
    for slot in STATE_SLOTS {
        let file = format!("{slot}.state");
        let path = game.dir.join(&file);
        let name = state_remote_name(slot, &game.emulator, core_version);
        let server = remote.iter().find(|r| r.file_name == name);
        let local = std::fs::read(&path).ok();
        let record = store.lock().unwrap().state_record(game.rom_id, slot);
        let local_md5 = local.as_deref().map(md5_hex);
        let local_changed =
            local_md5.is_some() && record.as_ref().map(|r| &r.local_md5) != local_md5.as_ref();
        let remote_changed = server.is_some_and(|srv| {
            record.as_ref().and_then(|r| r.remote_updated_at.as_deref())
                != Some(srv.updated_at.as_str())
        });
        let upload = match (local_changed, remote_changed) {
            (true, false) => true,
            (false, true) => false,
            (true, true) => {
                let local_time = file_iso_mtime(&path).and_then(|t| crate::sync::parse_iso(&t));
                let server_time = server.and_then(|s| crate::sync::parse_iso(&s.updated_at));
                local_time.unwrap_or(i64::MIN) > server_time.unwrap_or(i64::MIN)
            }
            (false, false) => continue,
        };
        let recorded = if upload {
            if let Some(srv) = server.filter(|_| remote_changed) {
                let bytes = client.download_state(srv.id).await?;
                backup_bytes(&game.dir, &format!("server-{file}"), &bytes).map_err(io_err)?;
            }
            let bytes = local.expect("local state to upload");
            let md5 = md5_hex(&bytes);
            let saved = client
                .upload_state(game.rom_id, &game.emulator, &name, bytes)
                .await?;
            report.uploaded += 1;
            crate::store::StateRecord {
                local_md5: md5,
                remote_id: Some(saved.id),
                remote_updated_at: Some(saved.updated_at),
            }
        } else {
            let srv = server.expect("remote state to download");
            let bytes = client.download_state(srv.id).await?;
            backup(&game.dir, &file).map_err(io_err)?;
            replace_file(&path, &bytes).map_err(io_err)?;
            report.downloaded += 1;
            crate::store::StateRecord {
                local_md5: md5_hex(&bytes),
                remote_id: Some(srv.id),
                remote_updated_at: Some(srv.updated_at.clone()),
            }
        };
        store
            .lock()
            .unwrap()
            .set_state_record(game.rom_id, slot, &recorded);
    }
    Ok(report)
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
    fn only_the_newest_backups_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(BACKUPS_KEPT + 3) {
            std::fs::write(dir.path().join("game.srm"), format!("v{i}")).unwrap();
            backup(dir.path(), "game.srm").unwrap();
        }
        let kept: Vec<_> = std::fs::read_dir(dir.path().join("backup"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(kept.len(), BACKUPS_KEPT);
        let newest = std::fs::read_dir(dir.path().join("backup"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .max()
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(newest.join("game.srm")).unwrap(),
            format!("v{}", BACKUPS_KEPT + 2)
        );
    }

    #[test]
    fn server_copies_are_backed_up_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let a = backup_bytes(dir.path(), "server-slot-1.state", b"one").unwrap();
        let b = backup_bytes(dir.path(), "server-slot-1.state", b"two").unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(a).unwrap(), b"one");
        assert_eq!(std::fs::read(b).unwrap(), b"two");
    }

    #[test]
    fn a_failed_automatic_save_is_set_aside_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(AUTO_STATE), b"broken").unwrap();
        set_aside_auto_state(dir.path()).unwrap();
        assert!(!dir.path().join(AUTO_STATE).exists());
        let kept: Vec<_> = std::fs::read_dir(dir.path().join("backup"))
            .unwrap()
            .flatten()
            .map(|e| std::fs::read(e.path().join("auto.state.failed")).unwrap())
            .collect();
        assert_eq!(kept, [b"broken".to_vec()]);
        assert!(set_aside_auto_state(dir.path()).is_ok());
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
            state_remote_name("auto", "../x", "v/1"),
            "auto.---x.v-1.state"
        );
    }

    mod sram {
        use super::super::*;
        use crate::romm::client::tests::base_of;
        use serde_json::json;
        use wiremock::matchers::{body_json, method, path, query_param};
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

        async fn completion_counts(server: &MockServer, completed: u32, failed: u32) {
            Mock::given(method("POST"))
                .and(path("/api/sync/sessions/9/complete"))
                .and(body_json(json!({
                    "operations_completed": completed,
                    "operations_failed": failed
                })))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(1)
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
            completion_counts(&server, 0, 0).await;
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
            negotiation(&server, json!([{"action": "upload", "rom_id": 5, "slot": "autosave", "save_id": null, "file_name": "Zelda- Link.srm"}])).await;
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
                json!([{"action": "download", "rom_id": 5, "slot": "autosave", "save_id": 4, "file_name": "x.srm"}]),
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
        async fn operations_for_other_slots_are_ignored() {
            let server = MockServer::start().await;
            negotiation(&server, json!([
                {"action": "download", "rom_id": 5, "save_id": 11, "file_name": "web.srm", "slot": "web"},
                {"action": "no_op", "rom_id": 5, "save_id": 12, "file_name": "x.srm", "slot": "autosave"}
            ])).await;
            completion(&server, 1).await;
            Mock::given(method("GET"))
                .and(path("/api/saves/11/content"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b"wrong".to_vec()))
                .expect(0)
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(SRAM_FILE), b"mine").unwrap();
            let outcome = sync_sram(&client(&server), "dev", &game(dir.path())).await;
            assert_eq!(outcome.unwrap(), SramOutcome::InSync);
            assert_eq!(std::fs::read(dir.path().join(SRAM_FILE)).unwrap(), b"mine");
        }

        #[tokio::test]
        async fn conflict_is_reported_not_resolved() {
            let server = MockServer::start().await;
            negotiation(
                &server,
                json!([{"action": "conflict", "rom_id": 5, "slot": "autosave", "save_id": 4, "file_name": "x.srm",
                "server_updated_at": "2026-09-26T10:00:00+00:00"}]),
            )
            .await;
            completion_counts(&server, 0, 0).await;
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
                json!([{"action": "upload", "rom_id": 5, "slot": "autosave", "save_id": null, "file_name": "x.srm"}]),
            )
            .await;
            Mock::given(method("POST"))
                .and(path("/api/saves"))
                .respond_with(ResponseTemplate::new(409).set_body_json(json!({"detail": "newer"})))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/api/saves"))
                .and(query_param("slot", "autosave"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                    save_json(7, "2026-09-26T10:00:00.5Z"),
                    save_json(6, "2026-09-26T10:00:00+00:00")
                ])))
                .mount(&server)
                .await;
            completion_counts(&server, 0, 0).await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(SRAM_FILE), b"mine").unwrap();
            let outcome = sync_sram(&client(&server), "dev", &game(dir.path()))
                .await
                .unwrap();
            let SramOutcome::Conflict(conflict) = outcome else {
                panic!("expected a conflict");
            };
            assert_eq!(conflict.save_id, Some(7));
            assert_eq!(
                conflict.server_updated_at.as_deref(),
                Some("2026-09-26T10:00:00.5Z")
            );
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

    mod states {
        use super::super::*;
        use crate::romm::client::tests::base_of;
        use crate::store::{StateRecord, Store};
        use serde_json::json;
        use std::sync::Mutex;
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        fn game(dir: &Path) -> GameSaves {
            GameSaves {
                rom_id: 5,
                dir: dir.to_path_buf(),
                title: "Zelda".into(),
                emulator: "snes9x".into(),
            }
        }

        fn state_json(id: i64, name: &str, updated: &str) -> serde_json::Value {
            json!({"id": id, "rom_id": 5, "file_name": name, "updated_at": updated, "emulator": "snes9x"})
        }

        async fn remote(server: &MockServer, states: serde_json::Value) {
            Mock::given(method("GET"))
                .and(path("/api/states"))
                .and(query_param("rom_id", "5"))
                .respond_with(ResponseTemplate::new(200).set_body_json(states))
                .mount(server)
                .await;
        }

        fn client(server: &MockServer) -> Client {
            Client::new(base_of(server, "/")).with_token("t".into())
        }

        fn store() -> Mutex<Store> {
            Mutex::new(Store::open_in_memory().unwrap())
        }

        #[tokio::test]
        async fn uploads_new_local_state_once() {
            let server = MockServer::start().await;
            remote(&server, json!([])).await;
            Mock::given(method("POST"))
                .and(path("/api/states"))
                .and(query_param("emulator", "snes9x"))
                .respond_with(ResponseTemplate::new(200).set_body_json(state_json(
                    3,
                    "slot-1.snes9x.1-63.state",
                    "2026-09-26T10:00:00+00:00",
                )))
                .expect(1)
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("slot-1.state"), b"S1").unwrap();
            let store = store();
            let report = sync_states(&client(&server), &store, &game(dir.path()), "1.63")
                .await
                .unwrap();
            assert_eq!(
                report,
                StateReport {
                    uploaded: 1,
                    downloaded: 0
                }
            );
            let record = store.lock().unwrap().state_record(5, "slot-1").unwrap();
            assert_eq!(record.remote_id, Some(3));
            assert_eq!(record.local_md5, md5_hex(b"S1"));
        }

        #[tokio::test]
        async fn unchanged_states_do_nothing() {
            let server = MockServer::start().await;
            remote(
                &server,
                json!([state_json(3, "slot-1.snes9x.1-63.state", "t1")]),
            )
            .await;
            Mock::given(method("POST"))
                .and(path("/api/states"))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("slot-1.state"), b"S1").unwrap();
            let store = store();
            store.lock().unwrap().set_state_record(
                5,
                "slot-1",
                &StateRecord {
                    local_md5: md5_hex(b"S1"),
                    remote_id: Some(3),
                    remote_updated_at: Some("t1".into()),
                },
            );
            let report = sync_states(&client(&server), &store, &game(dir.path()), "1.63")
                .await
                .unwrap();
            assert_eq!(report, StateReport::default());
        }

        #[tokio::test]
        async fn downloads_newer_remote_state() {
            let server = MockServer::start().await;
            remote(
                &server,
                json!([state_json(4, "slot-2.snes9x.1-63.state", "t2")]),
            )
            .await;
            Mock::given(method("GET"))
                .and(path("/api/states/4/content"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b"REMOTE".to_vec()))
                .mount(&server)
                .await;
            let dir = tempfile::tempdir().unwrap();
            let store = store();
            let report = sync_states(&client(&server), &store, &game(dir.path()), "1.63")
                .await
                .unwrap();
            assert_eq!(report.downloaded, 1);
            assert_eq!(
                std::fs::read(dir.path().join("slot-2.state")).unwrap(),
                b"REMOTE"
            );
        }

        #[tokio::test]
        async fn other_core_versions_are_ignored() {
            let server = MockServer::start().await;
            remote(
                &server,
                json!([
                    state_json(4, "slot-2.snes9x.1-62.state", "t2"),
                    state_json(5, "slot-3.bsnes.1-63.state", "t2")
                ]),
            )
            .await;
            let dir = tempfile::tempdir().unwrap();
            let report = sync_states(&client(&server), &store(), &game(dir.path()), "1.63")
                .await
                .unwrap();
            assert_eq!(report, StateReport::default());
            assert!(!dir.path().join("slot-2.state").exists());
        }
    }

    #[test]
    fn finds_game_folders_with_save_files() {
        let dir = tempfile::tempdir().unwrap();
        for (name, file) in [
            ("7", Some("game.srm")),
            ("9", Some("slot-1.state")),
            ("11", None),
            ("junk", Some("game.srm")),
        ] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
            if let Some(file) = file {
                std::fs::write(dir.path().join(name).join(file), b"x").unwrap();
            }
        }
        let mut ids = games_with_saves(dir.path());
        ids.sort();
        assert_eq!(ids, [7, 9]);
        assert!(games_with_saves(&dir.path().join("missing")).is_empty());
    }
}
