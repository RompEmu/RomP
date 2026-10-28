use super::collections::{BACKLOG_KEY, NOW_PLAYING_KEY};
use super::{on_ui, Controller};
use crate::romm::types::{PlayStatus, Progress};
use crate::StatusOption;
use slint::{ModelRc, VecModel};
use tr::tr;

/// The two flags come first in the list, then how far you got.
const FLAGS: usize = 2;

fn progress_label(progress: Progress) -> String {
    match progress {
        Progress::Incomplete => tr!("Incomplete"),
        Progress::Finished => tr!("Finished"),
        Progress::Completed100 => tr!("Completed 100%"),
        Progress::Retired => tr!("Retired"),
        Progress::NeverPlaying => tr!("Never playing"),
    }
}

fn options(status: PlayStatus) -> Vec<(String, bool)> {
    let mut options = vec![
        (tr!("Now playing"), status.now_playing),
        (tr!("Backlogged"), status.backlogged),
    ];
    options.extend(
        Progress::ALL
            .into_iter()
            .map(|p| (progress_label(p), status.progress == Some(p))),
    );
    options
}

fn toggled(status: PlayStatus, index: usize, on: bool) -> PlayStatus {
    let mut status = status;
    match index {
        0 => status.now_playing = on,
        1 => status.backlogged = on,
        _ => match Progress::ALL.get(index - FLAGS) {
            Some(&p) if on => status.progress = Some(p),
            Some(&p) if status.progress == Some(p) => status.progress = None,
            _ => {}
        },
    }
    status
}

fn label(status: PlayStatus) -> String {
    let names: Vec<String> = options(status)
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(name, _)| name)
        .collect();
    if names.is_empty() {
        tr!("Status…")
    } else {
        names.join(" · ")
    }
}

impl Controller {
    fn can_edit_status(&self) -> bool {
        self.has_scope("roms.user.write") && !self.offline.get() && self.client.borrow().is_some()
    }

    pub(super) fn refresh_status_controls(&self, rom_id: i64) {
        let Some(ui) = self.ui() else { return };
        let status = self.shared.store.lock().unwrap().play_status(rom_id);
        ui.set_show_status(self.has_scope("roms.user.read"));
        ui.set_can_edit_status(self.can_edit_status());
        ui.set_game_status_label(label(status).into());
        let options: Vec<StatusOption> = options(status)
            .into_iter()
            .map(|(label, checked)| StatusOption {
                label: label.into(),
                checked,
            })
            .collect();
        ui.set_game_status_options(ModelRc::new(VecModel::from(options)));
    }

    pub(super) fn toggle_status(&self, index: usize, on: bool) {
        let Some(rom_id) = self.current_game_id() else {
            return;
        };
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        if !self.can_edit_status() {
            return;
        }
        let before = self.shared.store.lock().unwrap().play_status(rom_id);
        let after = toggled(before, index, on);
        if after == before {
            return;
        }
        self.status_changed(rom_id, after);
        self.shared.rt.spawn(async move {
            let result = client.set_play_status(rom_id, after).await;
            on_ui(move |c| {
                if let Err(e) = result {
                    c.status_changed(rom_id, before);
                    c.game_status(rom_id, tr!("Couldn't update the game's status: {e}.", e));
                }
            });
        });
    }

    fn status_changed(&self, rom_id: i64, status: PlayStatus) {
        self.shared
            .store
            .lock()
            .unwrap()
            .set_play_status(rom_id, status);
        self.reload_sidebar();
        if matches!(
            self.selected.borrow().as_str(),
            NOW_PLAYING_KEY | BACKLOG_KEY
        ) {
            self.reload_games();
        }
        if self.current_game_id() == Some(rom_id) {
            self.refresh_status_controls(rom_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_PLAYING: usize = 0;
    const BACKLOGGED: usize = 1;
    const FINISHED: usize = 3;
    const RETIRED: usize = 5;

    #[test]
    fn options_list_the_flags_then_how_far_you_got() {
        let status = PlayStatus {
            backlogged: true,
            progress: Some(Progress::Finished),
            ..PlayStatus::default()
        };
        let options = options(status);
        let labels: Vec<&str> = options.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Now playing",
                "Backlogged",
                "Incomplete",
                "Finished",
                "Completed 100%",
                "Retired",
                "Never playing"
            ]
        );
        let checked: Vec<usize> = (0..options.len()).filter(|&i| options[i].1).collect();
        assert_eq!(checked, [BACKLOGGED, FINISHED]);
    }

    #[test]
    fn ticking_a_progress_replaces_the_one_before_and_unticking_clears_it() {
        let finished = toggled(PlayStatus::default(), FINISHED, true);
        assert_eq!(finished.progress, Some(Progress::Finished));
        let retired = toggled(finished, RETIRED, true);
        assert_eq!(retired.progress, Some(Progress::Retired));
        assert_eq!(toggled(retired, RETIRED, false).progress, None);
        assert_eq!(
            toggled(retired, FINISHED, false).progress,
            Some(Progress::Retired),
            "unticking another leaves the one that's set"
        );
    }

    #[test]
    fn the_flags_toggle_on_their_own() {
        let playing = toggled(PlayStatus::default(), NOW_PLAYING, true);
        let both = toggled(playing, BACKLOGGED, true);
        assert!(both.now_playing && both.backlogged);
        assert!(!toggled(both, NOW_PLAYING, false).now_playing);
        assert_eq!(toggled(both, 99, true), both);
    }

    #[test]
    fn the_button_names_what_is_set() {
        assert_eq!(label(PlayStatus::default()), "Status…");
        assert_eq!(
            label(PlayStatus {
                now_playing: true,
                progress: Some(Progress::Completed100),
                ..PlayStatus::default()
            }),
            "Now playing · Completed 100%"
        );
    }
}
