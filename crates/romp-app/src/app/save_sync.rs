use super::{on_ui, Controller};
use crate::cores::{is_default_core, CoreInfo};
use crate::paths;
use crate::play::CoreIdentity;
use crate::romm::client::{Client, Error};
use crate::saves::{self, GameSaves, Keep, SramConflict, SramOutcome};
use crate::store::{GameDetail, Store};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tr::tr;

pub(super) struct PendingLaunch {
    pub detail: GameDetail,
    pub rom: PathBuf,
    pub core: &'static CoreInfo,
    pub core_path: PathBuf,
    pub conflict: SramConflict,
}

pub(super) fn conflict_text(conflict: &SramConflict) -> String {
    let when = |t: &Option<String>| {
        t.as_deref()
            .and_then(crate::sync::parse_iso)
            .map(|ms| {
                let time =
                    std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms.max(0) as u64);
                crate::sync::iso_utc(time)
                    .replace('T', " ")
                    .replace("+00:00", " UTC")
            })
            .unwrap_or_else(|| tr!("unknown"))
    };
    tr!(
        "This computer: {0}. Server: {1}. The one you don't choose is kept as a backup.",
        when(&conflict.local_updated_at),
        when(&conflict.server_updated_at)
    )
}

/// The default emulator's key is the one RomP used before a platform had a second emulator.
pub(super) fn version_key(game: &GameSaves) -> String {
    match &game.states_core {
        Some(core) => format!("core_version:{}:{core}", game.rom_id),
        None => format!("core_version:{}", game.rom_id),
    }
}

async fn sync_game(
    client: &Client,
    store: &Mutex<Store>,
    device_id: &str,
    game: &GameSaves,
    core_version: Option<&str>,
) -> Result<SramOutcome, Error> {
    let last = store
        .lock()
        .unwrap()
        .in_game_save_record(game.rom_id, game.record_variant());
    let sram = saves::sync_sram(client, device_id, game, last.as_ref()).await?;
    saves::remember_outcome(store, game, &sram);
    if let Some(version) = core_version {
        saves::sync_states(client, store, game, version).await?;
    }
    Ok(sram)
}

fn saves_for(
    detail: &GameDetail,
    core: &'static CoreInfo,
    base: PathBuf,
    variant: Option<String>,
) -> GameSaves {
    let dir = match &variant {
        Some(variant) => base.join(crate::saves::VARIANTS).join(variant),
        None => base,
    };
    if core.id == "melondsds" {
        if let Err(e) = crate::saves::adopt_desmume_saves(&dir) {
            tracing::warn!("moving {}'s DeSmuME save to melonDS: {e}", detail.title);
        }
    }
    let rom = detail.local_path.as_deref().map(std::path::Path::new);
    let (save_file, save_emulator) = crate::saves::in_game_save(core.id, &dir, rom);
    GameSaves {
        rom_id: detail.id,
        dir,
        title: detail.title.clone(),
        emulator: core.id.to_string(),
        save_file,
        save_emulator,
        variant,
        states_core: (!is_default_core(&detail.platform_slug, core.id))
            .then(|| core.id.to_string()),
    }
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
        let prompt = if self.sync_device().is_none() {
            Some(tr!("Pair again to turn on save sync"))
        } else if !self.has_scope("collections.write") {
            Some(tr!("Pair again to use favorites and collections"))
        } else if !self.has_scope("roms.user.write") {
            Some(tr!("Pair again to share when you last played"))
        } else if !self.has_scope("roms.user.read") {
            Some(tr!("Pair again to see how long you've played"))
        } else if self.ra_account().is_some() && !self.has_scope("me.write") {
            Some(tr!(
                "Pair again to keep RetroAchievements progress current in RomM"
            ))
        } else {
            None
        };
        if let Some(ui) = self.ui() {
            ui.set_needs_pairing(signed_in && prompt.is_some());
            ui.set_pair_prompt(prompt.unwrap_or_default().into());
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
        let core =
            crate::console_settings::chosen_core(&self.console_choices(), &detail.platform_slug)?;
        Some(self.game_saves_for(detail, core))
    }

    /// A game's saves as `core` keeps them, whichever emulator is chosen now.
    pub(super) fn game_saves_for(&self, detail: &GameDetail, core: &'static CoreInfo) -> GameSaves {
        let variant = self
            .shared
            .store
            .lock()
            .unwrap()
            .chosen_file(detail.id)
            .map(|file| crate::saves::variant_of(&file));
        let base = paths::game_save_dir(&paths::data_dir(), &self.server(), detail.id);
        saves_for(detail, core, base, variant)
    }

    pub(super) fn clear_conflict(&self) {
        if self.pending_launch.borrow_mut().take().is_some() {
            self.preparing.set(false);
        }
        if let Some(ui) = self.ui() {
            ui.set_save_conflict(false);
        }
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
        let game = self.game_saves_for(&pending.detail, pending.core);
        let (Some(client), Some(device)) = (self.client.borrow().clone(), self.sync_device())
        else {
            self.preparing.set(false);
            if let Some(ui) = self.ui() {
                ui.set_save_conflict(false);
            }
            return;
        };
        if let Some(ui) = self.ui() {
            ui.set_save_conflict(false);
            ui.set_game_status(tr!("Syncing your save…").into());
        }
        let conflict = pending.conflict.clone();
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            let result = saves::resolve_sram(&client, &device, &game, &conflict, keep).await;
            if let Ok(outcome) = &result {
                saves::remember_outcome(&store, &game, outcome);
            }
            on_ui(move |c| {
                if let Err(e) = result {
                    tracing::warn!("resolving save conflict: {e}");
                }
                c.start_game(pending.detail, pending.rom, pending.core, pending.core_path);
            });
        });
    }

    pub(super) fn after_play(
        &self,
        detail: GameDetail,
        core: Option<&'static CoreInfo>,
        identity: Option<CoreIdentity>,
    ) {
        let Some(game) = core.map(|core| self.game_saves_for(&detail, core)) else {
            return;
        };
        if let Some((_, version)) = &identity {
            self.shared
                .store
                .lock()
                .unwrap()
                .set(&version_key(&game), version);
        }
        let Some(device) = self.sync_device() else {
            self.shared.store.lock().unwrap().add_pending(detail.id);
            return self.game_status(
                detail.id,
                tr!(
                    "Save sync is off. Choose \"{}\" in the library.",
                    tr!("Pair again to turn on save sync")
                ),
            );
        };
        let client = self.client.borrow().clone();
        let Some(client) = client.filter(|_| !self.offline.get()) else {
            self.shared.store.lock().unwrap().add_pending(detail.id);
            return self.game_status(
                detail.id,
                tr!("Saves will sync when the server is reachable."),
            );
        };
        let store = self.shared.store.clone();
        let version = identity.map(|(_, v)| v);
        let id = detail.id;
        self.syncing_game.set(Some(id));
        self.shared.rt.spawn(async move {
            let result = sync_game(&client, &store, &device, &game, version.as_deref()).await;
            on_ui(move |c| c.after_play_synced(id, result));
        });
    }

    fn after_play_synced(&self, id: i64, result: Result<SramOutcome, Error>) {
        self.syncing_game.set(None);
        let status = match result {
            Ok(SramOutcome::Conflict(_)) => {
                tr!("The save here and the one on RomM differ. You'll be asked which to keep next time you play.")
            }
            Ok(_) => tr!("In-game save and save states synced."),
            Err(Error::Unreachable) => {
                self.shared.store.lock().unwrap().add_pending(id);
                tr!("Saves will sync when the server is reachable.")
            }
            Err(e) => {
                tracing::warn!("syncing saves for {id}: {e}");
                tr!("Couldn't sync saves: {e}.", e)
            }
        };
        self.game_status(id, status);
        if let Some(detail) = self.current_game().filter(|d| d.id == id) {
            self.show_save_slots(&detail);
        }
    }

    pub(super) fn sync_pending(&self) {
        let busy = self.running.borrow().is_some()
            || self.preparing.get()
            || self.syncing_game.get().is_some();
        let (Some(client), Some(device)) = (self.client.borrow().clone(), self.sync_device())
        else {
            return;
        };
        if busy {
            return;
        }
        let details: Vec<GameDetail> = {
            let store = self.shared.store.lock().unwrap();
            store
                .pending()
                .into_iter()
                .filter_map(|id| store.game(id))
                .collect()
        };
        let jobs: Vec<(GameSaves, Option<String>)> = details
            .iter()
            .filter_map(|detail| self.game_saves(detail))
            .map(|game| {
                let version = self.shared.store.lock().unwrap().get(&version_key(&game));
                (game, version)
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        let store: Arc<Mutex<Store>> = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            for (game, version) in jobs {
                match sync_game(&client, &store, &device, &game, version.as_deref()).await {
                    Ok(_) => store.lock().unwrap().remove_pending(game.rom_id),
                    Err(Error::Unreachable) => {}
                    Err(e) => {
                        tracing::warn!("pending save sync for {}: {e}", game.rom_id);
                        store.lock().unwrap().remove_pending(game.rom_id);
                    }
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
        let text = conflict_text(&SramConflict {
            save_id: Some(1),
            server_updated_at: Some("2026-09-26T12:00:00.5+02:00".into()),
            local_updated_at: None,
        });
        assert!(text.contains("Server: 2026-09-26 10:00:00 UTC"));
    }

    fn psx_game(states_core: Option<&str>) -> GameSaves {
        GameSaves {
            rom_id: 7,
            dir: "games/7".into(),
            title: "Crash".into(),
            emulator: states_core.unwrap_or("mednafen_psx_hw").into(),
            save_file: saves::SRAM_FILE.into(),
            save_emulator: states_core.unwrap_or("mednafen_psx_hw").into(),
            variant: None,
            states_core: states_core.map(Into::into),
        }
    }

    #[test]
    fn each_emulator_remembers_its_own_version() {
        assert_eq!(version_key(&psx_game(None)), "core_version:7");
        assert_eq!(
            version_key(&psx_game(Some("swanstation"))),
            "core_version:7:swanstation"
        );
    }

    fn psx_detail() -> GameDetail {
        GameDetail {
            id: 7,
            title: "Crash".into(),
            platform_id: 1,
            platform_slug: "psx".into(),
            platform: "PlayStation".into(),
            platform_category: None,
            summary: None,
            size_bytes: 0,
            cover_small: None,
            cover_large: None,
            local_path: None,
            meta: Default::default(),
            screenshots: Vec::new(),
            version_file: None,
        }
    }

    #[test]
    fn saves_follow_the_emulator_given() {
        let base = std::path::Path::new("games/7");
        let core = |id| {
            crate::cores::choices_for_platform("psx")
                .find(|c| c.id == id)
                .unwrap()
        };
        let beetle = saves_for(&psx_detail(), core("mednafen_psx_hw"), base.into(), None);
        assert_eq!(beetle.emulator, "mednafen_psx_hw");
        assert_eq!(beetle.states_dir(), base);
        assert_eq!(version_key(&beetle), "core_version:7");
        let swan = saves_for(&psx_detail(), core("swanstation"), base.into(), None);
        assert_eq!(swan.emulator, "swanstation");
        assert_eq!(swan.states_dir(), base.join("swanstation"));
        assert_eq!(version_key(&swan), "core_version:7:swanstation");
        let disc = saves_for(
            &psx_detail(),
            core("swanstation"),
            base.into(),
            Some("Disc 2".into()),
        );
        assert_eq!(
            disc.states_dir(),
            base.join(saves::VARIANTS)
                .join("Disc 2")
                .join("swanstation")
        );
    }
}
