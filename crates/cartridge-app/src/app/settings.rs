use super::{on_ui, with_controller, Controller, SCREEN_LIBRARY};
use crate::details::human_size;
use crate::players::KEYBOARD;
use crate::{paths, storage, DeviceRow, KeyHint, StorageRow};
use slint::{Model, ModelRc, Timer, TimerMode, VecModel};
use std::time::Duration;

const SCREEN_SETTINGS: i32 = 4;
const SECTION_PLAYERS: i32 = 1;
const SECTION_STORAGE: i32 = 2;

const KEY_HINTS: [(&str, &str); 9] = [
    ("Arrow keys", "D-pad"),
    ("X  Z  S  A", "A, B, X, Y"),
    ("Q  W", "L, R"),
    ("D  F", "L2, R2"),
    ("Enter", "Start"),
    ("Backspace", "Select"),
    ("F5", "Save state"),
    ("F7", "Load state"),
    ("Esc", "Quit the game"),
];

impl Controller {
    pub(super) fn save_players(&self) {
        let json = self.players.borrow().to_json();
        self.shared.store.lock().unwrap().set("players", &json);
    }

    pub(super) fn open_settings(&self) {
        let Some(ui) = self.ui() else { return };
        ui.set_settings_server(self.server().trim_end_matches('/').into());
        ui.set_settings_keys(ModelRc::new(VecModel::from(
            KEY_HINTS
                .iter()
                .map(|(keys, action)| KeyHint {
                    keys: (*keys).into(),
                    action: (*action).into(),
                })
                .collect::<Vec<_>>(),
        )));
        ui.set_screen(SCREEN_SETTINGS);
        self.settings_section_changed(ui.get_settings_section());
    }

    pub(super) fn settings_section_changed(&self, section: i32) {
        let watching = section == SECTION_PLAYERS;
        if watching {
            self.refresh_devices();
            let timer = Timer::default();
            timer.start(TimerMode::Repeated, Duration::from_millis(50), || {
                with_controller(|c| c.refresh_devices())
            });
            *self.settings_timer.borrow_mut() = Some(timer);
        } else {
            self.settings_timer.borrow_mut().take();
        }
        if section == SECTION_STORAGE {
            self.measure_storage();
        }
    }

    pub(super) fn close_settings(&self) {
        self.settings_timer.borrow_mut().take();
        if let Some(ui) = self.ui() {
            ui.set_screen(SCREEN_LIBRARY);
        }
    }

    fn device_rows(&self) -> Vec<DeviceRow> {
        let (pads, active) = {
            let mut gamepads = self.gamepads.borrow_mut();
            gamepads.poll();
            (gamepads.connected(), gamepads.recently_active())
        };
        let keys: Vec<String> = pads.iter().map(|p| p.key.clone()).collect();
        let mut players = self.players.borrow_mut();
        let before = players.clone();
        let mut rows = vec![DeviceRow {
            key: KEYBOARD.into(),
            name: "Keyboard".into(),
            detail: "Built in".into(),
            player: i32::from(players.player(KEYBOARD).unwrap_or(0)),
            active: false,
        }];
        for pad in pads {
            let player = players.connect(&pad.key, &keys);
            rows.push(DeviceRow {
                active: active.contains(&pad.key),
                key: pad.key.into(),
                name: pad.name.into(),
                detail: "Controller".into(),
                player: i32::from(player.unwrap_or(0)),
            });
        }
        let changed = *players != before;
        drop(players);
        if changed {
            self.save_players();
        }
        rows
    }

    fn refresh_devices(&self) {
        let Some(ui) = self.ui() else { return };
        let rows = self.device_rows();
        let current = ui.get_settings_devices();
        let same_devices = current.row_count() == rows.len()
            && current
                .iter()
                .zip(&rows)
                .all(|(a, b)| a.key == b.key && a.player == b.player);
        if same_devices {
            for (i, row) in rows.into_iter().enumerate() {
                if current.row_data(i).is_some_and(|r| r.active != row.active) {
                    current.set_row_data(i, row);
                }
            }
        } else {
            ui.set_settings_devices(ModelRc::new(VecModel::from(rows)));
        }
    }

    pub(super) fn assign_player(&self, key: String, player: i32) {
        let player = u8::try_from(player).ok().filter(|p| *p > 0);
        self.players.borrow_mut().set(&key, player);
        self.save_players();
        self.refresh_devices();
    }

    fn measure_storage(&self) {
        let Some(ui) = self.ui() else { return };
        ui.set_settings_storage_busy(true);
        self.shared.rt.spawn_blocking(|| {
            let sizes: Vec<(&'static str, u64)> = storage::categories()
                .into_iter()
                .map(|(label, path)| (label, storage::dir_size(&path)))
                .collect();
            on_ui(move |c| c.show_storage(sizes));
        });
    }

    fn show_storage(&self, sizes: Vec<(&'static str, u64)>) {
        let Some(ui) = self.ui() else { return };
        let total: u64 = sizes.iter().map(|(_, s)| s).sum();
        ui.set_settings_storage(ModelRc::new(VecModel::from(
            sizes
                .into_iter()
                .map(|(label, size)| StorageRow {
                    label: label.into(),
                    size: human_size(size as i64).into(),
                })
                .collect::<Vec<_>>(),
        )));
        ui.set_settings_storage_total(human_size(total as i64).into());
        ui.set_settings_storage_busy(false);
    }

    pub(super) fn clear_images(&self) {
        storage::clear_dir(&paths::covers_dir());
        self.icon_requests.borrow_mut().clear();
        self.measure_storage();
    }

    pub(super) fn show_folder(&self) {
        let _ = open::that_detached(paths::data_dir());
    }
}
