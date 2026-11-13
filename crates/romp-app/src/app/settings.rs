use super::{on_ui, with_controller, Controller, SCREEN_LIBRARY, SCREEN_SETTINGS};
use crate::achievements::popups;
use crate::console_settings::{self, Chosen};
use crate::cores::core_for_platform;
use crate::details::human_size;
use crate::gamepads::Button;
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

/// Which buttons are held while a menu combination is chosen. It waits for the controller to
/// be let go first, so the press that opened the row isn't taken as part of the combination.
#[derive(Default)]
pub(super) struct MenuComboCapture {
    armed: bool,
    pad: Option<String>,
    buttons: Vec<Button>,
}

impl MenuComboCapture {
    fn restart(&mut self) {
        *self = Self::default();
    }

    /// Takes what each controller holds now, and gives the buttons held together once they're let go.
    fn update(&mut self, held: &[(String, Vec<Button>)]) -> Option<Vec<Button>> {
        let down = held.iter().find(|(_, buttons)| !buttons.is_empty());
        if !self.armed {
            self.armed = down.is_none();
            return None;
        }
        match down {
            Some((pad, buttons)) => {
                if self.pad.as_ref() != Some(pad) {
                    self.pad = Some(pad.clone());
                    self.buttons.clear();
                }
                for button in buttons {
                    if !self.buttons.contains(button) {
                        self.buttons.push(*button);
                    }
                }
                None
            }
            None if self.pad.is_some() => {
                let buttons = std::mem::take(&mut self.buttons);
                self.pad = None;
                Some(buttons)
            }
            None => None,
        }
    }
}

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
        ui.set_pref_check_updates(prefs.check_updates);
        ui.set_pref_accept_installs(prefs.accept_installs);
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
        ui.set_pref_appearance(i32::from(prefs.appearance));
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
            check_updates: ui.get_pref_check_updates(),
            accept_installs: ui.get_pref_accept_installs(),
            appearance: u8::try_from(ui.get_pref_appearance()).unwrap_or(0),
            whats_new: self.prefs.get().whats_new,
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
        if prefs.check_updates != before.check_updates {
            self.check_for_updates();
        }
        if prefs.accept_installs != before.accept_installs {
            self.refresh_installs();
        }
        if prefs.appearance != before.appearance {
            ui.global::<crate::Palette>()
                .set_color_scheme(prefs.color_scheme());
        }
        if prefs.language != before.language {
            crate::i18n::apply(crate::i18n::pick(prefs.language));
            ui.global::<crate::Locale>().set_rtl(crate::i18n::rtl());
            self.rebuild_rows();
            self.reload_sidebar();
            self.update_heading();
            if self.offline.get() && !ui.get_syncing() {
                ui.set_sync_status(tr!("Offline: showing downloaded games").into());
            }
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
                if current.row_data(i).as_ref() != Some(&row) {
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
        let mappings = self.mappings.borrow();
        self.shared
            .store
            .lock()
            .unwrap()
            .set("mappings", &mappings.to_json());
        if let Some((slug, game)) = self.game_mappings.borrow().as_ref() {
            *game.borrow_mut() = mappings.for_console(slug);
        }
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
        let consoles: Vec<(String, String)> = self
            .shared
            .store
            .lock()
            .unwrap()
            .platforms(false)
            .into_iter()
            .filter(|p| {
                super::game::playable(&p.slug) && remappable_on(&p.slug, device == KEYBOARD)
            })
            .map(|p| (p.slug, p.name))
            .collect();
        // A change made for all consoles would be hidden by the running
        // game's own layer, so its console is the one people mean to change.
        *self.remap_console.borrow_mut() = self
            .game_mappings
            .borrow()
            .as_ref()
            .map(|(slug, _)| slug.clone())
            .filter(|slug| device != SHORTCUTS && consoles.iter().any(|(s, _)| s == slug));
        *self.remap_consoles.borrow_mut() = consoles;
        self.remap_console_query.borrow_mut().clear();
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
            let mut rows: Vec<RemapRow> = HOTKEYS
                .iter()
                .enumerate()
                .map(|(i, (hotkey, _, label))| RemapRow {
                    name: translate(label).into(),
                    binding: mapping::combo_label(&mappings.hotkey_key(*hotkey)).into(),
                    waiting: waiting == Some(i as u32),
                })
                .collect();
            let combo = mappings.menu_combo();
            let menu_waiting = waiting == Some(HOTKEYS.len() as u32);
            rows.push(RemapRow {
                name: tr!("Game menu (controller)").into(),
                binding: if combo.is_empty() {
                    tr!("Guide, or Select + Start")
                } else {
                    let family = self.gamepads.borrow().family_in_use(&mappings);
                    crate::pad_labels::combo_name(family.unwrap_or_default(), &combo)
                }
                .into(),
                waiting: menu_waiting,
            });
            ui.set_remap_rows(ModelRc::new(VecModel::from(rows)));
            ui.set_remap_waiting(waiting.is_some());
            let hint = match notice {
                Some(notice) => notice.to_string(),
                None if menu_waiting => {
                    tr!("Hold two or three controller buttons together, then let go. Esc cancels.")
                }
                None => translate(SHORTCUTS_HINT),
            };
            ui.set_remap_hint(hint.into());
            ui.set_remap_labels_shown(false);
            ui.set_remap_console_shown(false);
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
        ui.set_remap_console_shown(true);
        let console = self.remap_console.borrow().clone();
        let consoles = self.remap_consoles.borrow();
        let shown = matching_consoles(&consoles, &self.remap_console_query.borrow());
        let names: Vec<slint::SharedString> = std::iter::once(tr!("All consoles"))
            .chain(shown.iter().map(|i| consoles[*i].1.clone()))
            .map(slint::SharedString::from)
            .collect();
        ui.set_remap_consoles(ModelRc::new(VecModel::from(names)));
        // -1 when the chosen console is filtered out of the list, so no row looks chosen.
        ui.set_remap_console(console.as_ref().map_or(0, |slug| {
            shown
                .iter()
                .position(|i| &consoles[*i].0 == slug)
                .map_or(-1, |p| p as i32 + 1)
        }));
        ui.set_remap_console_name(
            console
                .as_ref()
                .and_then(|slug| consoles.iter().find(|(s, _)| s == slug))
                .map_or_else(|| tr!("All consoles"), |(_, name)| name.clone())
                .into(),
        );
        let view = match &console {
            Some(slug) => mappings.for_console(slug),
            None => mappings.clone(),
        };
        let rows: Vec<RemapRow> = self
            .remap_buttons()
            .iter()
            .map(|(button, name)| RemapRow {
                name: translate(name).into(),
                binding: if keyboard {
                    mapping::key_label(&view.key_for(*button))
                } else {
                    translate(crate::pad_labels::name(
                        family,
                        view.pad_button(model, *button),
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

    /// The buttons the pages list, so a tapped row index and the row shown agree.
    fn remap_buttons(&self) -> Vec<(u32, &'static str)> {
        match self.remap_console.borrow().as_deref() {
            Some(slug) => crate::controllers::buttons(slug),
            None => BUTTONS.to_vec(),
        }
    }

    /// `index` is a row of the picker's list, which the search may have shortened.
    pub(super) fn remap_console_chosen(&self, index: i32) {
        let slug = {
            let consoles = self.remap_consoles.borrow();
            let shown = matching_consoles(&consoles, &self.remap_console_query.borrow());
            usize::try_from(index - 1)
                .ok()
                .and_then(|row| shown.get(row))
                .map(|i| consoles[*i].0.clone())
        };
        *self.remap_console.borrow_mut() = slug;
        self.remap_console_query.borrow_mut().clear();
        self.remap_waiting.set(None);
        self.show_remap(None);
    }

    pub(super) fn remap_console_search(&self, query: String) {
        *self.remap_console_query.borrow_mut() = query;
        self.show_remap(None);
    }

    pub(super) fn remap_pick(&self, index: i32) {
        let shortcuts = self.remap_device.borrow().as_deref() == Some(SHORTCUTS);
        let button = usize::try_from(index).ok().and_then(|i| {
            if shortcuts {
                // The row after the keyboard shortcuts is the controller's game menu combination.
                (i <= HOTKEYS.len()).then_some(i as u32)
            } else {
                self.remap_buttons().get(i).map(|(b, _)| *b)
            }
        });
        self.remap_waiting.set(button);
        self.menu_combo_capture.borrow_mut().restart();
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
        if device == SHORTCUTS && (mapping::is_modifier(&text) || button as usize >= HOTKEYS.len())
        {
            return;
        }
        let console = self.remap_console.borrow().clone();
        let outcome = {
            let mut mappings = self.mappings.borrow_mut();
            if device == SHORTCUTS {
                let hotkey = HOTKEYS[button as usize].0;
                let previous = mappings.hotkey_key(hotkey);
                (
                    mappings.set_hotkey(hotkey, &mapping::combo(mods, &text)),
                    previous,
                )
            } else if let Some(slug) = console.as_deref() {
                let previous = mappings.for_console(slug).key_for(button);
                (mappings.set_console_key(slug, button, &text), previous)
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
            (Assigned::Taken(label), _) => {
                return self.show_remap(Some(&tr!(
                    "{0} is a shortcut key. Choose a different key.",
                    translate(&label)
                )));
            }
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
        if device == SHORTCUTS {
            if button as usize == HOTKEYS.len() {
                self.capture_menu_combo();
            }
            return;
        }
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
        let console = self.remap_console.borrow().clone();
        let mut mappings = self.mappings.borrow_mut();
        match console.as_deref() {
            Some(slug) => mappings.set_console_pad_button(slug, &model, button, physical),
            None => mappings.set_pad_button(&model, button, physical),
        }
        drop(mappings);
        self.remap_waiting.set(None);
        self.save_mappings();
        self.show_remap(None);
    }

    fn capture_menu_combo(&self) {
        let held = {
            let mut gamepads = self.gamepads.borrow_mut();
            gamepads.poll();
            gamepads.held()
        };
        let Some(buttons) = self.menu_combo_capture.borrow_mut().update(&held) else {
            return;
        };
        if self.mappings.borrow_mut().set_menu_combo(&buttons) {
            self.remap_waiting.set(None);
            self.save_mappings();
            self.show_remap(None);
        } else {
            self.show_remap(Some(&tr!(
                "That takes two or three buttons held together. Try again."
            )));
        }
    }

    pub(super) fn remap_reset(&self) {
        let Some(device) = self.remap_device.borrow().clone() else {
            return;
        };
        let console = self.remap_console.borrow().clone();
        let mut mappings = self.mappings.borrow_mut();
        if device == SHORTCUTS {
            mappings.reset_hotkeys();
        } else if device == KEYBOARD {
            match console.as_deref() {
                Some(slug) => mappings.reset_console_keyboard(slug),
                None => mappings.reset_keyboard(),
            }
        } else {
            let model = mapping::model_of(&device);
            match console.as_deref() {
                Some(slug) => mappings.reset_console_pad(slug, model),
                None => mappings.reset_pad(model),
            }
        }
        drop(mappings);
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
        self.remap_console.borrow_mut().take();
        self.remap_consoles.borrow_mut().clear();
        self.remap_console_query.borrow_mut().clear();
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
        let groups: Vec<ConsoleGroup> = console_settings::console_groups(&chosen)
            .filter_map(|console| {
                let mut options: Vec<ConsoleOption> = Vec::new();
                let cores = console_settings::cores_for(console.platform);
                if cores.len() > 1 {
                    options.push(ConsoleOption {
                        key: console_settings::core_key(console.platform).into(),
                        label: translate(console_settings::EMULATOR_LABEL).into(),
                        detail: translate(console_settings::EMULATOR_DETAIL).into(),
                        choices: ModelRc::new(VecModel::from(
                            cores
                                .iter()
                                .map(|c| slint::SharedString::from(c.name))
                                .collect::<Vec<_>>(),
                        )),
                        current: cores.iter().position(|c| c.id == console.core).unwrap_or(0)
                            as i32,
                    });
                }
                // Resolution settings need the Vulkan renderer, which isn't everywhere.
                if !console.vulkan || crate::cores::uses_vulkan(console.core) {
                    options.extend(console.settings.iter().map(|setting| {
                        ConsoleOption {
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
                        }
                    }));
                }
                (!options.is_empty()).then(|| ConsoleGroup {
                    name: console.name.into(),
                    options: ModelRc::new(VecModel::from(options)),
                })
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
            if key.starts_with(console_settings::CORE_KEY_PREFIX) {
                // The console now lists the other emulator's settings.
                self.show_consoles();
            }
        }
    }

    fn show_storage(&self, sizes: Vec<(&'static str, u64)>) {
        let Some(ui) = self.ui() else { return };
        let total: u64 = sizes.iter().map(|(_, s)| s).sum();
        ui.set_settings_storage(ModelRc::new(VecModel::from(
            sizes
                .into_iter()
                .map(|(label, size)| StorageRow {
                    label: translate(label).into(),
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

    pub(super) fn show_log(&self) {
        let _ = open::that_detached(paths::logs_dir());
    }

    pub(super) fn copy_log(&self) {
        let Some(ui) = self.ui() else { return };
        let note = match crate::logs::last_game_in(&paths::logs_dir()) {
            None => tr!("No game was played in RomP’s last five launches."),
            Some(log) => match arboard::Clipboard::new().and_then(|mut c| c.set_text(log)) {
                Ok(()) => tr!("Copied. Paste it into your bug report."),
                Err(e) => {
                    tracing::warn!("copying the log: {e}");
                    tr!("The log couldn’t be copied. Use Show log and attach romp.log instead.")
                }
            },
        };
        ui.set_about_log_note(note.into());
    }
}

/// A picked index, kept within `list`.
fn index_within(index: i32, list: &[&str]) -> u8 {
    index.clamp(0, list.len() as i32 - 1) as u8
}

/// xemu and RPCS3 read the keyboard layout Romp writes into their settings, but bind controllers
/// themselves, so a controller remap saved for them would never reach the game.
fn remappable_on(slug: &str, keyboard: bool) -> bool {
    keyboard || core_for_platform(slug).is_some()
}

/// The consoles whose name or platform slug holds `query`, ignoring case, as positions in `consoles`.
fn matching_consoles(consoles: &[(String, String)], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    consoles
        .iter()
        .enumerate()
        .filter(|(_, (slug, name))| {
            query.is_empty()
                || name.to_lowercase().contains(&query)
                || slug.to_lowercase().contains(&query)
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{matching_consoles, remappable_on, Button, MenuComboCapture};

    #[test]
    fn standalone_emulators_only_take_a_keyboard_layout() {
        for slug in ["xbox", "ps3"] {
            assert!(remappable_on(slug, true), "{slug} keyboard");
            assert!(!remappable_on(slug, false), "{slug} controller");
        }
        assert!(remappable_on("snes", false));
        assert!(remappable_on("snes", true));
    }

    fn consoles() -> Vec<(String, String)> {
        [
            ("snes", "Super Nintendo"),
            ("psx", "PlayStation"),
            ("ps3", "PlayStation 3"),
            ("genesis", "Sega Genesis"),
        ]
        .iter()
        .map(|(slug, name)| (slug.to_string(), name.to_string()))
        .collect()
    }

    #[test]
    fn no_search_lists_every_console() {
        assert_eq!(matching_consoles(&consoles(), ""), [0, 1, 2, 3]);
        assert_eq!(matching_consoles(&consoles(), "   "), [0, 1, 2, 3]);
    }

    #[test]
    fn a_search_matches_names_ignoring_case() {
        assert_eq!(matching_consoles(&consoles(), "playst"), [1, 2]);
        assert_eq!(matching_consoles(&consoles(), "NINTENDO"), [0]);
    }

    #[test]
    fn a_search_also_matches_the_short_name_people_know_a_console_by() {
        assert_eq!(matching_consoles(&consoles(), "psx"), [1]);
    }

    #[test]
    fn a_search_that_matches_nothing_lists_nothing() {
        assert!(matching_consoles(&consoles(), "dreamcast").is_empty());
    }

    fn held(pad: &str, buttons: &[Button]) -> Vec<(String, Vec<Button>)> {
        vec![(pad.to_string(), buttons.to_vec())]
    }

    #[test]
    fn a_menu_combination_is_what_was_held_together_once_it_is_let_go() {
        let mut capture = MenuComboCapture::default();
        assert_eq!(capture.update(&held("a", &[])), None);
        assert_eq!(capture.update(&held("a", &[Button::LeftThumb])), None);
        let both = [Button::LeftThumb, Button::RightThumb];
        assert_eq!(capture.update(&held("a", &both)), None);
        assert_eq!(capture.update(&held("a", &[Button::RightThumb])), None);
        assert_eq!(capture.update(&held("a", &[])), Some(both.to_vec()));
        assert_eq!(capture.update(&held("a", &[])), None);
    }

    #[test]
    fn the_press_that_chose_the_row_is_not_part_of_the_combination() {
        let mut capture = MenuComboCapture::default();
        assert_eq!(capture.update(&held("a", &[Button::South])), None);
        assert_eq!(capture.update(&held("a", &[])), None);
        assert_eq!(capture.update(&held("a", &[Button::Select])), None);
        assert_eq!(capture.update(&held("a", &[])), Some(vec![Button::Select]));
    }

    #[test]
    fn buttons_of_two_controllers_are_not_mixed() {
        let mut capture = MenuComboCapture::default();
        capture.update(&held("a", &[]));
        capture.update(&held("a", &[Button::LeftThumb]));
        capture.update(&held("b", &[Button::Select, Button::Start]));
        assert_eq!(
            capture.update(&held("b", &[])),
            Some(vec![Button::Select, Button::Start])
        );
    }
}
