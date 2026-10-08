use super::{on_ui, Controller};
use crate::saves;
use crate::slots;
use crate::store::GameDetail;
use crate::SlotCard;
use slint::{ModelRc, VecModel};

impl Controller {
    /// The game page's saves: the in-game save, where you left off, then every filled slot.
    pub(super) fn show_save_slots(&self, detail: &GameDetail) {
        self.render_save_slots(detail, false);
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
            let server = saves::for_emulator(server, &game.save_emulator);
            if let Some(record) = local.and_then(|md5| saves::matching_server_save(&md5, &server)) {
                store.lock().unwrap().set_in_game_save_record(
                    game.rom_id,
                    &record.md5,
                    record.synced_at,
                );
            }
            let on_server = !server.is_empty();
            on_ui(move |c| {
                if c.current_game_id() == Some(detail.id) {
                    c.render_save_slots(&detail, on_server);
                }
            });
        });
    }

    fn render_save_slots(&self, detail: &GameDetail, on_server: bool) {
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
                on_server,
            )
        });
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

    pub(super) fn play_slot(&self, slot: i32) {
        let Ok(slot) = u8::try_from(slot) else {
            return;
        };
        self.play_game_from(Some(slot));
    }
}
