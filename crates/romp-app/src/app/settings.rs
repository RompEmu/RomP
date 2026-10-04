use super::{on_ui, with_controller, Controller, SCREEN_LIBRARY, SCREEN_SETTINGS};
use crate::achievements::popups;
use crate::console_settings::{self, Chosen};
use crate::cores::core_for_platform;
use crate::details::human_size;
use crate::i18n::translate;
use crate::mapping::{self, Assigned, BUTTONS, HOTKEYS};
use crate::pad_labels::{Family, CHOICES};
use crate::players::KEYBOARD;
use crate::prefs::{Preferences, UI_SCALES};
use crate::RemapRow;
use crate::{
    paths, storage, ConsoleGroup, ConsoleOption, DeviceRow, KeyHint, LookConsole, LookSetting,
    StorageRow,
};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::time::Duration;
use tr::tr;

const SECTION_PLAYERS: i32 = 2;
const SECTION_STORAGE: i32 = 3;
const SECTION_CONSOLES: i32 = 4;
const SECTION_LOOKS: i32 = 5;

pub(super) const SHORTCUTS: &str = "shortcuts";

const KEYBOARD_HINT: &str =
    crate::gettext_noop!("Choose a button, then press the key you want for it.");
const SHORTCUTS_HINT: &str = crate::gettext_noop!(
    "Choose a shortcut, then press the key you want for it. Esc always opens the game menu."
);
const PAD_HINT: &str =
    crate::gettext_noop!("Choose a button, then press the controller button you want for it.");

impl Controller {
    pub(super) fn save_players(&self) {
        let json = self.players.borrow().to_json();
        self.shared.store.lock().unwrap().set("players", &json);
    }

    pub(super) fn open_settings(&self) {
        let Some(ui) = self.ui() else { return };
        ui.set_settings_server(self.server().trim_end_matches('/').into());
        self.show_key_hints();
        self.show_ra_account();
        let prefs = self.prefs.get();
        ui.set_pref_pause_unfocused(prefs.pause_unfocused);
        ui.set_pref_resume(prefs.resume);
        ui.set_pref_fullscreen(prefs.fullscreen);
        ui.set_pref_volume(f32::from(prefs.volume));
        let names = |list: &[&str]| {
            ModelRc::new(VecModel::from(
                list.iter()
                    .map(|s| slint::SharedString::from(translate(s)))
                    .collect::<Vec<_>>(),
            ))
        };
        ui.set_pref_popup_levels(names(&popups::LEVELS));
        ui.set_pref_popup_corners(names(&popups::CORNERS));
        ui.set_pref_achievement_popups(i32::from(prefs.achievement_popups));
        ui.set_pref_achievement_corner(i32::from(prefs.achievement_corner));
        let languages: Vec<slint::SharedString> = [tr!("System"), "English".to_string()]
            .into_iter()
            .chain(crate::i18n::LANGUAGES.iter().map(|l| l.name.to_string()))
            .map(slint::SharedString::from)
            .collect();
        ui.set_pref_languages(ModelRc::new(VecModel::from(languages)));
        ui.set_pref_language(i32::from(prefs.language));
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
            volume: ui.get_pref_volume().round().clamp(0.0, 100.0) as u8,
            ui_scale: UI_SCALES
                .get(ui.get_pref_ui_scale() as usize)
                .copied()
                .unwrap_or(100),
            achievement_popups: index_within(ui.get_pref_achievement_popups(), &popups::LEVELS),
            achievement_corner: index_within(ui.get_pref_achievement_corner(), &popups::CORNERS),
            language: u8::try_from(ui.get_pref_language()).unwrap_or(0),
        };
        let before = self.prefs.get();
        if prefs == before {
            return;
        }
        if prefs.ui_scale != before.ui_scale {
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
        if prefs.language != before.language {
            crate::i18n::apply(crate::i18n::choose(
                prefs.language,
                crate::i18n::system().as_deref(),
            ));
            self.open_settings();
        }
    }

    pub(super) fn set_game_popups(&self, level: u8, corner: u8) {
        let prefs = Preferences {
            achievement_popups: level,
            achievement_corner: corner,
            ..self.prefs.get()
        };
        self.prefs.set(prefs);
        self.shared
            .store
            .lock()
            .unwrap()
            .set("prefs", &prefs.to_json());
        if let Some(ui) = self.ui() {
            ui.set_pref_achievement_popups(i32::from(level));
            ui.set_pref_achievement_corner(i32::from(corner));
        }
    }

    pub(super) fn set_game_volume(&self, volume: u8) {
        let prefs = Preferences {
            volume,
            ..self.prefs.get()
        };
        self.prefs.set(prefs);
        self.shared
            .store
            .lock()
            .unwrap()
            .set("prefs", &prefs.to_json());
        if let Some(ui) = self.ui() {
            ui.set_pref_volume(f32::from(volume));
        }
    }

    pub(super) fn settings_section_changed(&self, section: i32) {
        let watching = section == SECTION_PLAYERS;
        if watching {
            self.refresh_devices();
            let timer = Timer::default();
            timer.start(TimerMode::Repeated, Duration::from_millis(50), || {
                with_controller(|c| c.refresh_devices());
            });
            *self.settings_timer.borrow_mut() = Some(timer);
        } else {
            self.settings_timer.borrow_mut().take();
        }
        if section == SECTION_STORAGE {
            self.measure_storage();
        }
        if section == SECTION_CONSOLES {
            self.show_consoles();
        }
        if section == SECTION_LOOKS {
            self.show_looks();
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
            name: tr!("Keyboard").into(),
            detail: tr!("Built in").into(),
            player: i32::from(players.player(KEYBOARD).unwrap_or(0)),
            active: false,
        }];
        for pad in pads {
            let player = players.connect(&pad.key, &keys);
            rows.push(DeviceRow {
                active: active.contains(&pad.key),
                key: pad.key.into(),
                name: pad.name.into(),
                detail: tr!("Controller").into(),
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

    fn show_key_hints(&self) {
        let Some(ui) = self.ui() else { return };
        let mappings = self.mappings.borrow();
        let hints: Vec<KeyHint> = std::iter::once(KeyHint {
            keys: "Esc".into(),
            action: tr!("Game menu").into(),
        })
        .chain(HOTKEYS.iter().map(|(hotkey, _, label)| KeyHint {
            keys: mapping::combo_label(&mappings.hotkey_key(*hotkey)).into(),
            action: translate(label).into(),
        }))
        .collect();
        ui.set_settings_keys(ModelRc::new(VecModel::from(hints)));
    }

    pub(super) fn customize(&self, device: String) {
        let Some(ui) = self.ui() else { return };
        let title = if device == SHORTCUTS {
            tr!("Shortcuts")
        } else if device == KEYBOARD {
            tr!("Keyboard")
        } else {
            let pads = self.gamepads.borrow().connected();
            pads.into_iter()
                .find(|p| p.key == device)
                .map_or_else(|| tr!("Controller"), |p| p.name)
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
        if device == SHORTCUTS {
            let rows: Vec<RemapRow> = HOTKEYS
                .iter()
                .enumerate()
                .map(|(i, (hotkey, _, label))| RemapRow {
                    name: translate(label).into(),
                    binding: mapping::combo_label(&mappings.hotkey_key(*hotkey)).into(),
                    waiting: waiting == Some(i as u32),
                })
                .collect();
            ui.set_remap_rows(ModelRc::new(VecModel::from(rows)));
            ui.set_remap_waiting(waiting.is_some());
            ui.set_remap_hint(
                notice
                    .map_or_else(|| translate(SHORTCUTS_HINT), str::to_string)
                    .into(),
            );
            ui.set_remap_labels_shown(false);
            return;
        }
        let model = mapping::model_of(&device);
        let detected = self
            .gamepads
            .borrow()
            .connected()
            .into_iter()
            .find(|pad| pad.key == device)
            .map_or_else(Default::default, |pad| pad.family);
        let family = mappings.family(model, detected);
        let automatic = match detected.choice() {
            Some(i) => tr!("Automatic ({})", translate(CHOICES[i].2)),
            None => tr!("Automatic"),
        };
        let choices: Vec<slint::SharedString> = std::iter::once(automatic)
            .chain(CHOICES.iter().map(|(_, _, label)| (*label).to_string()))
            .map(slint::SharedString::from)
            .collect();
        ui.set_remap_label_choices(ModelRc::new(VecModel::from(choices)));
        ui.set_remap_labels(
            mappings
                .labels(model)
                .and_then(Family::choice)
                .map_or(0, |i| i as i32 + 1),
        );
        ui.set_remap_labels_shown(!keyboard);
        let rows: Vec<RemapRow> = BUTTONS
            .iter()
            .map(|(button, name)| RemapRow {
                name: translate(name).into(),
                binding: if keyboard {
                    mapping::key_label(&mappings.key_for(*button))
                } else {
                    translate(crate::pad_labels::name(
                        family,
                        mappings.pad_button(model, *button),
                    ))
                }
                .into(),
                waiting: waiting == Some(*button),
            })
            .collect();
        ui.set_remap_rows(ModelRc::new(VecModel::from(rows)));
        ui.set_remap_waiting(waiting.is_some());
        let hint = notice.map_or_else(
            || translate(if keyboard { KEYBOARD_HINT } else { PAD_HINT }),
            str::to_string,
        );
        ui.set_remap_hint(hint.into());
    }

    pub(super) fn remap_pick(&self, index: i32) {
        let shortcuts = self.remap_device.borrow().as_deref() == Some(SHORTCUTS);
        let button = usize::try_from(index).ok().and_then(|i| {
            if shortcuts {
                (i < HOTKEYS.len()).then_some(i as u32)
            } else {
                BUTTONS.get(i).map(|(b, _)| *b)
            }
        });
        self.remap_waiting.set(button);
        self.gamepads.borrow_mut().take_presses();
        self.show_remap(None);
    }

    pub(super) fn remap_key(&self, text: String, mods: mapping::Mods) {
        let (Some(button), Some(device)) =
            (self.remap_waiting.get(), self.remap_device.borrow().clone())
        else {
            return;
        };
        if text == char::from(slint::platform::Key::Escape).to_string() {
            self.remap_waiting.set(None);
            return self.show_remap(None);
        }
        if device != KEYBOARD && device != SHORTCUTS {
            return;
        }
        if device == SHORTCUTS && mapping::is_modifier(&text) {
            return;
        }
        let outcome = {
            let mut mappings = self.mappings.borrow_mut();
            if device == SHORTCUTS {
                let hotkey = HOTKEYS[button as usize].0;
                let previous = mappings.hotkey_key(hotkey);
                (
                    mappings.set_hotkey(hotkey, &mapping::combo(mods, &text)),
                    previous,
                )
            } else {
                let previous = mappings.key_for(button);
                (mappings.set_key(button, &text), previous)
            }
        };
        let notice = match outcome {
            (Assigned::Reserved, _) => {
                return self.show_remap(Some(&tr!(
                    "Esc always opens the game menu. Choose a different key."
                )));
            }
            (Assigned::Swapped(other), previous) => Some(tr!(
                "{0} now uses {1}.",
                translate(&other),
                mapping::combo_label(&previous)
            )),
            (Assigned::Set, _) => None,
        };
        self.remap_waiting.set(None);
        self.save_mappings();
        self.show_key_hints();
        self.show_remap(notice.as_deref());
    }

    fn capture_pad_press(&self) {
        let (Some(button), Some(device)) =
            (self.remap_waiting.get(), self.remap_device.borrow().clone())
        else {
            return;
        };
        if device == KEYBOARD || device == SHORTCUTS {
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
        if device == SHORTCUTS {
            self.mappings.borrow_mut().reset_hotkeys();
        } else if device == KEYBOARD {
            self.mappings.borrow_mut().reset_keyboard();
        } else {
            self.mappings
                .borrow_mut()
                .reset_pad(mapping::model_of(&device));
        }
        self.remap_waiting.set(None);
        self.save_mappings();
        self.show_key_hints();
        self.show_remap(None);
    }

    pub(super) fn remap_labels_chosen(&self, index: i32) {
        let Some(device) = self.remap_device.borrow().clone() else {
            return;
        };
        let family = usize::try_from(index - 1)
            .ok()
            .and_then(|i| CHOICES.get(i))
            .map(|(family, _, _)| *family);
        self.mappings
            .borrow_mut()
            .set_labels(mapping::model_of(&device), family);
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

    pub(super) fn console_choices(&self) -> Chosen {
        console_settings::chosen_from_json(
            self.shared
                .store
                .lock()
                .unwrap()
                .get(console_settings::STORE_KEY)
                .as_deref(),
        )
    }

    fn show_consoles(&self) {
        let Some(ui) = self.ui() else { return };
        let chosen = self.console_choices();
        let groups: Vec<ConsoleGroup> = console_settings::consoles()
            .map(|console| ConsoleGroup {
                name: console.name.into(),
                options: ModelRc::new(VecModel::from(
                    console
                        .settings
                        .iter()
                        .map(|setting| ConsoleOption {
                            key: setting.key.into(),
                            label: translate(setting.label).into(),
                            detail: translate(setting.detail).into(),
                            choices: ModelRc::new(VecModel::from(
                                setting
                                    .choices
                                    .iter()
                                    .map(|c| translate(c.label).into())
                                    .collect::<Vec<slint::SharedString>>(),
                            )),
                            current: console_settings::selected(setting, &chosen) as i32,
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect();
        ui.set_settings_consoles(ModelRc::new(VecModel::from(groups)));
    }

    /// The consoles in the library that Romp draws itself, with how each one's games look.
    fn show_looks(&self) {
        let Some(ui) = self.ui() else { return };
        let platforms = self.shared.store.lock().unwrap().platforms(false);
        let consoles: Vec<LookConsole> = platforms
            .into_iter()
            .filter(|p| core_for_platform(&p.slug).is_some() || p.slug == crate::xemu::PLATFORM)
            .map(|p| {
                let choice = self.look_choice(&p.slug);
                let settings: Vec<LookSetting> = crate::looks::rows(&choice, &p.slug)
                    .into_iter()
                    .map(|row| LookSetting {
                        label: translate(crate::looks::row_label(row)).into(),
                        choices: ModelRc::new(VecModel::from(
                            crate::looks::row_choices(row, &p.slug)
                                .into_iter()
                                .map(|c| slint::SharedString::from(translate(c)))
                                .collect::<Vec<_>>(),
                        )),
                        current: crate::looks::row_selected(&choice, &p.slug, row) as i32,
                    })
                    .collect();
                LookConsole {
                    slug: p.slug.into(),
                    name: p.name.into(),
                    settings: ModelRc::new(VecModel::from(settings)),
                }
            })
            .collect();
        ui.set_settings_looks(ModelRc::new(VecModel::from(consoles)));
    }

    pub(super) fn look_setting_changed(&self, platform: String, row: i32, index: i32) {
        let mut choice = self.look_choice(&platform);
        let rows = crate::looks::rows(&choice, &platform);
        let (Some(row), Ok(index)) = (
            usize::try_from(row).ok().and_then(|i| rows.get(i).copied()),
            usize::try_from(index),
        ) else {
            return;
        };
        crate::looks::select(&mut choice, &platform, row, index);
        self.save_look_choice(&platform, &choice);
        self.show_looks();
    }

    pub(super) fn console_option_changed(&self, key: String, index: i32) {
        let mut chosen = self.console_choices();
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if console_settings::choose(&mut chosen, &key, index) {
            let json = serde_json::to_string(&chosen).expect("console settings serialize");
            self.shared
                .store
                .lock()
                .unwrap()
                .set(console_settings::STORE_KEY, &json);
        }
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

/// A picked index, kept within `list`.
fn index_within(index: i32, list: &[&str]) -> u8 {
    index.clamp(0, list.len() as i32 - 1) as u8
}
