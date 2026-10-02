use super::Controller;
use crate::slots;
use crate::store::GameDetail;
use crate::SlotCard;
use slint::{ModelRc, VecModel};

impl Controller {
    /// The game page's save states: where you left off, then every filled slot.
    pub(super) fn show_save_slots(&self, detail: &GameDetail) {
        let Some(ui) = self.ui() else { return };
        let dir = crate::paths::game_save_dir(&crate::paths::data_dir(), &self.server(), detail.id);
        let now = std::time::SystemTime::now();
        let cards: Vec<SlotCard> = slots::read(&dir, slots::AUTO..=crate::input::SLOTS)
            .iter()
            .filter(|info| !info.is_empty())
            .map(|info| slots::card(info, now, None))
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
