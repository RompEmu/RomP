use super::{on_ui, with_controller, Controller, SCREEN_LIBRARY, SCREEN_SETTINGS};
use crate::details::human_size;
use crate::mapping::{self, BUTTONS};
use crate::players::KEYBOARD;
use crate::prefs::{Preferences, UI_SCALES};
use crate::RemapRow;
use crate::{paths, storage, DeviceRow, KeyHint, StorageRow};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::time::Duration;

const SECTION_PLAYERS: i32 = 2;
const SECTION_STORAGE: i32 = 3;

const KEY_HINTS: [(&str, &str); 6] = [
    ("Esc", "Game menu"),
    ("P", "Pause"),
    ("F5", "Save state"),
    ("F7", "Load state"),
    ("F6", "Next save slot"),
    ("F11", "Full screen"),
];

const KEYBOARD_HINT: &str = "Choose a button, then press the key you want for it.";
const PAD_HINT: &str = "Choose a button, then press the controller button you want for it.";

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
        let prefs = self.prefs.get();
        ui.set_pref_pause_unfocused(prefs.pause_unfocused);
        ui.set_pref_resume(prefs.resume);
        ui.set_pref_fullscreen(prefs.fullscreen);
        ui.set_pref_sharp(prefs.sharp_pixels);
        ui.set_pref_volume(f32::from(prefs.volume));
        ui.set_pref_ui_scale(
            UI_SCALES
                .iter()
                .position(|s| *s == prefs.ui_scale)
                .unwrap_or(0) as i32,
        );
        let mappings = self.mappings.borrow();
        ui.set_pref_nintendo_labels(mappings.nintendo_labels);
        ui.set_pref_stick_dpad(mappings.stick_dpad);
        drop(mappings);
        ui.set_screen(SCREEN_SETTINGS);
        self.settings_section_changed(ui.get_settings_section());
    }

    pub(super) fn open_players(&self) {
        let Some(ui) = self.ui() else { return };
        ui.set_settings_section(SECTION_PLAYERS);
        let _ = ui.show();
        self.open_settings();
    }

    pub(super) fn prefs_changed(&self) {
        let Some(ui) = self.ui() else { return };
        let prefs = Preferences {
            pause_unfocused: ui.get_pref_pause_unfocused(),
            resume: ui.get_pref_resume(),
            fullscreen: ui.get_pref_fullscreen(),
            sharp_pixels: ui.get_pref_sharp(),
            volume: ui.get_pref_volume().round().clamp(0.0, 100.0) as u8,
            ui_scale: UI_SCALES
                .get(ui.get_pref_ui_scale() as usize)
                .copied()
                .unwrap_or(100),
        };
        if prefs == self.prefs.get() {
            return;
        }
        if prefs.ui_scale != self.prefs.get().ui_scale {
            crate::scale::set_factor(prefs.scale_factor());
        }
        self.prefs.set(prefs);
        self.shared
            .store
            .lock()
            .unwrap()
            .set("prefs", &prefs.to_json());
        if let Some(running) = self.running.borrow().as_ref() {
            running.apply_prefs(&prefs);
        }
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
        self.remap_close();
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
        self.capture_pad_press();
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

    fn save_mappings(&self) {
        let json = self.mappings.borrow().to_json();
        self.shared.store.lock().unwrap().set("mappings", &json);
    }

    pub(super) fn controller_options_changed(&self) {
        let Some(ui) = self.ui() else { return };
        {
            let mut mappings = self.mappings.borrow_mut();
            mappings.nintendo_labels = ui.get_pref_nintendo_labels();
            mappings.stick_dpad = ui.get_pref_stick_dpad();
        }
        self.save_mappings();
    }

    pub(super) fn customize(&self, device: String) {
        let Some(ui) = self.ui() else { return };
        let title = if device == KEYBOARD {
            "Keyboard".to_string()
        } else {
            let pads = self.gamepads.borrow().connected();
            pads.into_iter()
                .find(|p| p.key == device)
                .map_or("Controller".into(), |p| p.name)
        };
        ui.set_remap_title(title.into());
        *self.remap_device.borrow_mut() = Some(device);
        self.remap_waiting.set(None);
        self.show_remap(None);
        ui.set_remap_open(true);
    }

    fn show_remap(&self, notice: Option<&str>) {
        let Some(ui) = self.ui() else { return };
        let Some(device) = self.remap_device.borrow().clone() else {
            return;
        };
        let keyboard = device == KEYBOARD;
        let waiting = self.remap_waiting.get();
        let mappings = self.mappings.borrow();
        let rows: Vec<RemapRow> = BUTTONS
            .iter()
            .map(|(button, name)| RemapRow {
                name: (*name).into(),
                binding: if keyboard {
                    mapping::key_label(&mappings.key_for(*button))
                } else {
                    mapping::physical_label(
                        mappings.pad_button(mapping::model_of(&device), *button),
                    )
                    .to_string()
                }
                .into(),
                waiting: waiting == Some(*button),
            })
            .collect();
        ui.set_remap_rows(ModelRc::new(VecModel::from(rows)));
        ui.set_remap_waiting(waiting.is_some());
        let hint = notice.unwrap_or(if keyboard { KEYBOARD_HINT } else { PAD_HINT });
        ui.set_remap_hint(hint.into());
    }

    pub(super) fn remap_pick(&self, index: i32) {
        let button = usize::try_from(index)
            .ok()
            .and_then(|i| BUTTONS.get(i))
            .map(|(b, _)| *b);
        self.remap_waiting.set(button);
        self.gamepads.borrow_mut().take_presses();
        self.show_remap(None);
    }

    pub(super) fn remap_key(&self, text: String) {
        let (Some(button), Some(device)) =
            (self.remap_waiting.get(), self.remap_device.borrow().clone())
        else {
            return;
        };
        if device != KEYBOARD {
            return;
        }
        if !self.mappings.borrow_mut().set_key(button, &text) {
            return self.show_remap(Some(
                "That key is a shortcut. Choose a different key for this button.",
            ));
        }
        self.remap_waiting.set(None);
        self.save_mappings();
        self.show_remap(None);
    }

    fn capture_pad_press(&self) {
        let (Some(button), Some(device)) =
            (self.remap_waiting.get(), self.remap_device.borrow().clone())
        else {
            return;
        };
        if device == KEYBOARD {
            return;
        }
        let model = mapping::model_of(&device).to_string();
        let presses = self.gamepads.borrow_mut().take_presses();
        let Some((_, physical)) = presses
            .into_iter()
            .find(|(key, _)| mapping::model_of(key) == model)
        else {
            return;
        };
        self.mappings
            .borrow_mut()
            .set_pad_button(&model, button, physical);
        self.remap_waiting.set(None);
        self.save_mappings();
        self.show_remap(None);
    }

    pub(super) fn remap_reset(&self) {
        let Some(device) = self.remap_device.borrow().clone() else {
            return;
        };
        if device == KEYBOARD {
            self.mappings.borrow_mut().reset_keyboard();
        } else {
            self.mappings
                .borrow_mut()
                .reset_pad(mapping::model_of(&device));
        }
        self.remap_waiting.set(None);
        self.save_mappings();
        self.show_remap(None);
    }

    pub(super) fn remap_close(&self) {
        self.remap_device.borrow_mut().take();
        self.remap_waiting.set(None);
        if let Some(ui) = self.ui() {
            ui.set_remap_open(false);
        }
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
