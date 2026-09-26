use super::{on_ui, Controller};
use crate::cores::core_for_platform;
use crate::paths;
use crate::play::CoreIdentity;
use crate::romm::client::{Client, Error};
use crate::saves::{self, GameSaves, Keep, SramConflict, SramOutcome};
use crate::store::{GameDetail, Store};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub(super) struct PendingLaunch {
    pub detail: GameDetail,
    pub rom: PathBuf,
    pub jit: bool,
    pub core: PathBuf,
    pub conflict: SramConflict,
}

pub(super) fn conflict_text(conflict: &SramConflict) -> String {
    let when = |t: &Option<String>| {
        t.as_deref()
            .map(|t| t.replace('T', " ").replace("+00:00", " UTC"))
            .unwrap_or_else(|| "unknown".into())
    };
    format!(
        "This computer: {}. Server: {}. The one you don't choose is kept as a backup.",
        when(&conflict.local_updated_at),
        when(&conflict.server_updated_at)
    )
}

fn version_key(rom_id: i64) -> String {
    format!("core_version:{rom_id}")
}

async fn sync_game(
    client: &Client,
    store: &Mutex<Store>,
    device_id: &str,
    game: &GameSaves,
    core_version: Option<&str>,
) -> Result<SramOutcome, Error> {
    let sram = saves::sync_sram(client, device_id, game).await?;
    if let Some(version) = core_version {
        saves::sync_states(client, store, game, version).await?;
    }
    Ok(sram)
}

impl Controller {
    pub(super) fn sync_device(&self) -> Option<String> {
        let store = self.shared.store.lock().unwrap();
        let scopes = store.get("scopes")?;
        scopes
            .split(' ')
            .any(|s| s == "devices.write")
            .then(|| store.get("device_uuid"))
            .flatten()
    }

    pub(super) fn update_pairing_prompt(&self) {
        let signed_in = self.client.borrow().is_some();
        if let Some(ui) = self.ui() {
            ui.set_needs_pairing(signed_in && self.sync_device().is_none());
        }
    }

    pub(super) fn pair_again(&self) {
        let Some(server) = self.client.borrow().as_ref().map(|c| c.base().to_string()) else {
            return;
        };
        if let Some(ui) = self.ui() {
            ui.set_server_url(server.trim_end_matches('/').into());
        }
        self.connect();
    }

    pub(super) fn game_saves(&self, detail: &GameDetail) -> Option<GameSaves> {
        let core = core_for_platform(&detail.platform_slug)?;
        Some(GameSaves {
            rom_id: detail.id,
            dir: paths::game_save_dir(&paths::data_dir(), &self.server(), detail.id),
            title: detail.title.clone(),
            emulator: core.id.to_string(),
        })
    }

    pub(super) fn show_conflict(&self, pending: PendingLaunch) {
        if let Some(ui) = self.ui() {
            ui.set_conflict_text(conflict_text(&pending.conflict).into());
            ui.set_save_conflict(true);
            ui.set_game_status("".into());
        }
        *self.pending_launch.borrow_mut() = Some(pending);
    }

    pub(super) fn keep_save(&self, keep: Keep) {
        let Some(pending) = self.pending_launch.borrow_mut().take() else {
            return;
        };
        let (Some(client), Some(device), Some(game)) = (
            self.client.borrow().clone(),
            self.sync_device(),
            self.game_saves(&pending.detail),
        ) else {
            return;
        };
        if let Some(ui) = self.ui() {
            ui.set_save_conflict(false);
            ui.set_game_status("Syncing your save…".into());
        }
        let conflict = pending.conflict.clone();
        self.shared.rt.spawn(async move {
            let result = saves::resolve_sram(&client, &device, &game, &conflict, keep).await;
            on_ui(move |c| {
                if let Err(e) = result {
                    tracing::warn!("resolving save conflict: {e}");
                }
                c.start_game(pending.detail, pending.rom, pending.jit, pending.core);
            });
        });
    }

    pub(super) fn after_play(&self, detail: GameDetail, identity: Option<CoreIdentity>) {
        if let Some((_, version)) = &identity {
            self.shared
                .store
                .lock()
                .unwrap()
                .set(&version_key(detail.id), version);
        }
        let (Some(device), Some(game)) = (self.sync_device(), self.game_saves(&detail)) else {
            return;
        };
        let client = self.client.borrow().clone();
        let Some(client) = client.filter(|_| !self.offline.get()) else {
            self.shared.store.lock().unwrap().add_pending(detail.id);
            return self.game_status(
                detail.id,
                "Saves will sync when the server is reachable.".into(),
            );
        };
        let store = self.shared.store.clone();
        let version = identity.map(|(_, v)| v);
        let id = detail.id;
        self.shared.rt.spawn(async move {
            let result = sync_game(&client, &store, &device, &game, version.as_deref()).await;
            on_ui(move |c| c.after_play_synced(id, result));
        });
    }

    fn after_play_synced(&self, id: i64, result: Result<SramOutcome, Error>) {
        let status = match result {
            Ok(SramOutcome::Conflict(_)) => {
                "A newer save is on the server. You'll be asked which to keep next time you play."
            }
            Ok(_) => "Saves synced.",
            Err(e) => {
                tracing::warn!("syncing saves for {id}: {e}");
                self.shared.store.lock().unwrap().add_pending(id);
                "Saves will sync when the server is reachable."
            }
        };
        self.game_status(id, status.into());
    }

    pub(super) fn sync_pending(&self) {
        let (Some(client), Some(device)) = (self.client.borrow().clone(), self.sync_device())
        else {
            return;
        };
        let jobs: Vec<(GameSaves, Option<String>)> = {
            let store = self.shared.store.lock().unwrap();
            store
                .pending()
                .into_iter()
                .filter_map(|id| {
                    let detail = store.game(id)?;
                    Some((detail, store.get(&version_key(id))))
                })
                .collect::<Vec<_>>()
        }
        .into_iter()
        .filter_map(|(detail, version)| Some((self.game_saves(&detail)?, version)))
        .collect();
        if jobs.is_empty() {
            return;
        }
        let store: Arc<Mutex<Store>> = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            for (game, version) in jobs {
                match sync_game(&client, &store, &device, &game, version.as_deref()).await {
                    Ok(_) => store.lock().unwrap().remove_pending(game.rom_id),
                    Err(e) => tracing::warn!("pending save sync for {}: {e}", game.rom_id),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_text_names_both_times() {
        let text = conflict_text(&SramConflict {
            save_id: Some(1),
            server_updated_at: Some("2026-09-26T10:00:00+00:00".into()),
            local_updated_at: None,
        });
        assert!(text.contains("Server: 2026-09-26 10:00:00 UTC"));
        assert!(text.contains("This computer: unknown"));
    }
}
