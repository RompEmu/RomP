use super::{on_ui, Controller};
use crate::romm::client::Error;
use crate::saves::{self, InGameSaveStatus, ServerSaves};
use crate::slots;
use crate::store::GameDetail;
use crate::SlotCard;
use slint::{ModelRc, VecModel};
use tr::tr;

impl Controller {
    /// The game page's saves: the in-game save, where you left off, then every filled slot.
    pub(super) fn show_save_slots(&self, detail: &GameDetail) {
        self.render_save_slots(detail, &ServerSaves::default());
        // The server is asked only when there's no save here, or no record of syncing it.
        let game = self.game_saves(detail);
        let local = game.as_ref().and_then(saves::local_md5);
        let recorded = self
            .shared
            .store
            .lock()
            .unwrap()
            .in_game_save_record(detail.id)
            .is_some();
        if (local.is_some() && recorded) || self.offline.get() {
            return;
        }
        let (Some(client), Some(device), Some(game)) =
            (self.client.borrow().clone(), self.sync_device(), game)
        else {
            return;
        };
        let store = self.shared.store.clone();
        let detail = detail.clone();
        self.shared.rt.spawn(async move {
            let Ok(server) = client
                .list_saves(game.rom_id, saves::SRAM_SLOT, &device)
                .await
            else {
                return;
            };
            let summary = saves::server_saves(&server, &detail.platform_slug, &game.save_emulator);
            let ours = saves::for_emulator(server, &game.save_emulator);
            if let Some(record) = local.and_then(|md5| saves::matching_server_save(&md5, &ours)) {
                store.lock().unwrap().set_in_game_save_record(
                    game.rom_id,
                    &record.md5,
                    record.synced_at,
                );
            }
            on_ui(move |c| {
                if c.current_game_id() == Some(detail.id) {
                    c.render_save_slots(&detail, &summary);
                }
            });
        });
    }

    fn render_save_slots(&self, detail: &GameDetail, server: &ServerSaves) {
        let Some(ui) = self.ui() else { return };
        let dir = crate::paths::game_save_dir(&crate::paths::data_dir(), &self.server(), detail.id);
        let now = std::time::SystemTime::now();
        let in_game = self.game_saves(detail).and_then(|game| {
            let store = self.shared.store.lock().unwrap();
            saves::in_game_save_status(
                saves::local_md5(&game).as_deref(),
                store.in_game_save_record(detail.id).as_ref(),
                self.sync_device().is_some(),
                store.pending().contains(&detail.id),
                server,
            )
        });
        *self.other_save.borrow_mut() = match &in_game {
            Some(InGameSaveStatus::FromOtherEmulator(other)) => Some((detail.id, other.clone())),
            _ => None,
        };
        let cards: Vec<SlotCard> = in_game
            .map(|status| slots::in_game_card(&status, now))
            .into_iter()
            .chain(
                slots::read(&dir, slots::AUTO..=crate::input::SLOTS)
                    .iter()
                    .filter(|info| !info.is_empty())
                    .map(|info| slots::card(info, now, None)),
            )
            .collect();
        ui.set_game_save_slots(ModelRc::new(VecModel::from(cards)));
        ui.set_game_slots_hardcore(self.ra_account().is_some() && self.ra_hardcore());
    }

    /// Copies another emulator's save from RomM as this game's, which then syncs as RomP's.
    pub(super) fn use_other_save(&self) {
        let Some(detail) = self.current_game() else {
            return;
        };
        let offered = self.other_save.borrow().clone();
        let Some((_, other)) = offered.filter(|(id, _)| *id == detail.id) else {
            return;
        };
        let (Some(client), Some(device), Some(game)) = (
            self.client.borrow().clone(),
            self.sync_device(),
            self.game_saves(&detail),
        ) else {
            return;
        };
        let emulator = other.emulator.clone();
        self.game_status(detail.id, tr!("Copying the save from {}…", emulator));
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            let result = async {
                let bytes = client.download_save(other.id, &device, None).await?;
                let Some(bytes) = saves::adapt_other_save(&detail.platform_slug, bytes) else {
                    return Ok(false);
                };
                saves::install_download(&game, &bytes)?;
                let outcome = saves::sync_sram(&client, &device, &game).await?;
                saves::remember_outcome(&store, &game, &outcome);
                Ok::<_, Error>(true)
            }
            .await;
            on_ui(move |c| {
                let status = match result {
                    Ok(true) => tr!(
                        "Using the save from {}. It syncs as RomP's from now on.",
                        emulator
                    ),
                    Ok(false) => tr!(
                        "The save from {} isn't one this emulator can read.",
                        emulator
                    ),
                    Err(e) => {
                        tracing::warn!("using {emulator}'s save for {}: {e}", detail.title);
                        tr!("Couldn't use the save: {e}.", e)
                    }
                };
                c.game_status(detail.id, status);
                if c.current_game_id() == Some(detail.id) {
                    c.show_save_slots(&detail);
                }
            });
        });
    }

    pub(super) fn play_slot(&self, slot: i32) {
        let Ok(slot) = u8::try_from(slot) else {
            return;
        };
        self.play_game_from(Some(slot));
    }
}
