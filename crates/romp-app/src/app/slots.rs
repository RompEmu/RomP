use super::Controller;
use crate::slots::{self, SlotInfo};
use crate::store::GameDetail;
use crate::SlotCard;
use slint::{Image, ModelRc, VecModel};

impl Controller {
    /// The game page's save states: where you left off, then every filled slot.
    pub(super) fn show_save_slots(&self, detail: &GameDetail) {
        let Some(ui) = self.ui() else { return };
        let dir = crate::paths::game_save_dir(&crate::paths::data_dir(), &self.server(), detail.id);
        let now = std::time::SystemTime::now();
        let cards: Vec<SlotCard> = slots::read(
            &dir,
            (slots::AUTO..=crate::input::SLOTS).collect::<Vec<_>>(),
        )
        .into_iter()
        .filter(|info| !info.is_empty())
        .map(|info: SlotInfo| {
            let thumbnail = info
                .thumbnail
                .as_ref()
                .and_then(|p| Image::load_from_path(p).ok());
            SlotCard {
                slot: i32::from(info.slot),
                label: info.label().into(),
                detail: info
                    .saved_at
                    .map(|t| slots::when(t, now))
                    .unwrap_or_default()
                    .into(),
                has_thumbnail: thumbnail.is_some(),
                thumbnail: thumbnail.unwrap_or_default(),
                empty: false,
                current: false,
            }
        })
        .collect();
        ui.set_game_save_slots(ModelRc::new(VecModel::from(cards)));
    }

    pub(super) fn play_slot(&self, slot: i32) {
        let Ok(slot) = u8::try_from(slot) else {
            return;
        };
        self.play_game_from(Some(slot));
    }
}
