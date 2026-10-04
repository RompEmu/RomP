use super::{with_controller, Controller, SCREEN_GAME, SCREEN_LIBRARY, SCREEN_SETTINGS};
use crate::gamepads::Button;
use crate::gettext_noop;
use crate::i18n::translate;
use crate::input::{A, B, DOWN, L, LEFT, R, RIGHT, START, UP, X};
use crate::mapping::BUTTONS;
use crate::navigation::{move_in_grid, Dir};
use crate::pad_labels::{badge, Badge, Family};
use slint::ComponentHandle;
use slint::{Model, ModelRc, Timer, TimerMode, VecModel};
use std::time::{Duration, Instant};

const SETTINGS_SECTIONS: i32 = 4;
const DPAD: u16 = 1 << UP | 1 << DOWN | 1 << LEFT | 1 << RIGHT;

impl Controller {
    pub(super) fn start_navigation(&self) {
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(16), || {
            with_controller(|c| c.nav_tick());
        });
        *self.nav_timer.borrow_mut() = Some(timer);
    }

    fn nav_tick(&self) {
        if self.running.borrow().is_some() {
            self.nav_repeat.borrow_mut().update(0, DPAD, Instant::now());
            return;
        }
        let (inputs, family) = {
            let mut gamepads = self.gamepads.borrow_mut();
            gamepads.poll();
            let mappings = self.mappings.borrow();
            let inputs = gamepads.states(&mappings, false);
            let family =
                (!inputs.is_empty()).then(|| gamepads.family_in_use(&mappings).unwrap_or_default());
            (inputs, family)
        };
        self.update_pad_hints(family);
        let buttons = inputs.iter().fold(0u16, |b, p| b | p.menu.buttons);
        let pressed = self
            .nav_repeat
            .borrow_mut()
            .update(buttons, DPAD, Instant::now());
        for (button, _) in BUTTONS {
            if pressed & 1 << button != 0 {
                self.nav_button(button);
            }
        }
    }

    fn update_pad_hints(&self, family: Option<Family>) {
        let Some(ui) = self.ui() else { return };
        let hints = match family {
            Some(family) if !ui.get_dialog_open() && !ui.get_remap_open() => {
                hints(ui.get_screen(), family)
            }
            _ => Vec::new(),
        };
        if *self.pad_hints.borrow() == hints {
            return;
        }
        let rows: Vec<crate::PadHint> = hints
            .iter()
            .map(|(keys, action)| crate::PadHint {
                keys: ModelRc::new(VecModel::from(
                    keys.iter().map(|k| pad_key(*k)).collect::<Vec<_>>(),
                )),
                action: translate(action).into(),
            })
            .collect();
        ui.set_pad_hints(ModelRc::new(VecModel::from(rows)));
        *self.pad_hints.borrow_mut() = hints;
    }

    pub(super) fn mouse_back(&self, window: slint::winit_030::winit::window::WindowId) {
        let Some(ui) = self.ui() else { return };
        if crate::mouse::window_id(ui.window()) != Some(window)
            || ui.get_dialog_open()
            || ui.get_remap_open()
        {
            return;
        }
        if matches!(ui.get_screen(), SCREEN_GAME | SCREEN_SETTINGS) {
            self.back_to_library();
        }
    }

    fn nav_button(&self, button: u32) {
        let button = crate::mapping::as_seen(button, crate::i18n::rtl());
        let Some(ui) = self.ui() else { return };
        if ui.get_dialog_open() {
            if button == B {
                self.close_dialog();
            }
            return;
        }
        if ui.get_remap_open() {
            if button == B && self.remap_waiting.get().is_none() {
                self.remap_close();
            }
            return;
        }
        match (ui.get_screen(), button) {
            (SCREEN_LIBRARY, UP) => self.move_card(Dir::Up),
            (SCREEN_LIBRARY, DOWN) => self.move_card(Dir::Down),
            (SCREEN_LIBRARY, LEFT) => self.move_card(Dir::Left),
            (SCREEN_LIBRARY, RIGHT) => self.move_card(Dir::Right),
            (SCREEN_LIBRARY, A) => {
                let id = self
                    .nav_card
                    .get()
                    .and_then(|i| self.library.borrow().games.get(i).map(|g| g.id));
                if let Some(id) = id {
                    self.open_game(id);
                }
            }
            (SCREEN_LIBRARY, L) => self.step_sidebar(-1),
            (SCREEN_LIBRARY, R) => self.step_sidebar(1),
            (SCREEN_LIBRARY, START) => self.open_settings(),
            (SCREEN_GAME, A) => {
                if ui.get_game_downloaded() && !ui.get_game_busy() {
                    self.play_game();
                } else if ui.get_game_playable() && ui.get_game_can_download() {
                    self.download_game();
                }
            }
            (SCREEN_GAME, X) => {
                if ui.get_can_edit_collections() {
                    self.toggle_favorite();
                }
            }
            (SCREEN_GAME | SCREEN_SETTINGS, B) => self.back_to_library(),
            (SCREEN_SETTINGS, L | R) => {
                let step = if button == L { -1 } else { 1 };
                let section = (ui.get_settings_section() + step).rem_euclid(SETTINGS_SECTIONS);
                ui.set_settings_section(section);
                self.settings_section_changed(section);
            }
            _ => {}
        }
    }

    fn move_card(&self, dir: Dir) {
        let (total, columns) = {
            let lib = self.library.borrow();
            (lib.games.len(), lib.columns)
        };
        if total == 0 {
            return;
        }
        let next = match self.nav_card.get() {
            Some(current) => move_in_grid(current, total, columns, dir),
            None => 0,
        };
        self.focus_card(Some(next));
    }

    pub(super) fn focus_card(&self, index: Option<usize>) {
        let previous = self.nav_card.replace(index);
        for (i, focused) in [(previous, false), (index, true)] {
            let Some(i) = i else { continue };
            let id = self.library.borrow().games.get(i).map(|g| g.id);
            if let Some(id) = id {
                self.set_card_focused(id, focused);
            }
        }
        if let (Some(ui), Some(i)) = (self.ui(), index) {
            let lib = self.library.borrow();
            let row = i / lib.columns.max(1);
            if let Some((top, bottom)) = crate::grid::row_span(&lib.shelves, row) {
                ui.set_grid_focus_top(top);
                ui.set_grid_focus_bottom(bottom);
            }
            ui.set_grid_focus_row(row as i32);
        }
    }

    fn step_sidebar(&self, step: i32) {
        let Some(ui) = self.ui() else { return };
        let keys: Vec<String> = ui
            .get_sidebar()
            .iter()
            .filter(|e| !e.header)
            .map(|e| e.key.to_string())
            .collect();
        if keys.is_empty() {
            return;
        }
        let current = keys
            .iter()
            .position(|k| *k == *self.selected.borrow())
            .unwrap_or(0) as i32;
        let next = (current + step).rem_euclid(keys.len() as i32) as usize;
        self.nav_card.set(None);
        self.select(keys[next].clone());
        self.focus_card(Some(0));
    }
}

/// The buttons in a hint and what pressing them does.
pub(super) type Hint = (Vec<Badge>, &'static str);

fn hints(screen: i32, family: Family) -> Vec<Hint> {
    let key = |button: Button| badge(family, button);
    let shoulders = vec![key(Button::LeftTrigger), key(Button::RightTrigger)];
    match screen {
        SCREEN_LIBRARY => vec![
            (
                vec![Badge::Text(gettext_noop!("D-pad"))],
                gettext_noop!("Move"),
            ),
            (vec![key(family.confirm())], gettext_noop!("Open")),
            (shoulders, gettext_noop!("Change list")),
            (vec![key(Button::Start)], gettext_noop!("Settings")),
        ],
        SCREEN_GAME => vec![
            (
                vec![key(family.confirm())],
                gettext_noop!("Play or download"),
            ),
            (vec![key(Button::North)], gettext_noop!("Favorite")),
            (vec![key(family.back())], gettext_noop!("Back")),
        ],
        SCREEN_SETTINGS => vec![
            (shoulders, gettext_noop!("Change section")),
            (vec![key(family.back())], gettext_noop!("Back")),
        ],
        _ => Vec::new(),
    }
}

fn pad_key(badge: Badge) -> crate::PadKey {
    match badge {
        Badge::Text(text) => crate::PadKey {
            text: translate(text).into(),
            shape: "".into(),
        },
        Badge::Shape(shape) => crate::PadKey {
            text: "".into(),
            shape: shape.key().into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad_labels::Shape;

    #[test]
    fn hints_name_the_buttons_of_the_controller_in_use() {
        let library = hints(SCREEN_LIBRARY, Family::Xbox);
        assert_eq!(library[1], (vec![Badge::Text("A")], "Open"));
        assert_eq!(
            library[2],
            (vec![Badge::Text("LB"), Badge::Text("RB")], "Change list")
        );
        let game = hints(SCREEN_GAME, Family::Ps5);
        assert_eq!(
            game[0],
            (vec![Badge::Shape(Shape::Cross)], "Play or download")
        );
        assert_eq!(game[2], (vec![Badge::Shape(Shape::Circle)], "Back"));
        let settings = hints(SCREEN_SETTINGS, Family::Nintendo);
        assert_eq!(settings[1], (vec![Badge::Text("B")], "Back"));
    }
}
