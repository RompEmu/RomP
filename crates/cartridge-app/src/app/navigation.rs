use super::{with_controller, Controller, SCREEN_GAME, SCREEN_LIBRARY, SCREEN_SETTINGS};
use crate::input::{A, B, DOWN, L, LEFT, R, RIGHT, START, UP, X};
use crate::mapping::{stick_to_dpad, BUTTONS};
use crate::navigation::{move_in_grid, Dir};
use slint::ComponentHandle;
use slint::{Model, Timer, TimerMode};
use std::time::{Duration, Instant};

const SETTINGS_SECTIONS: i32 = 4;
const DPAD: u16 = 1 << UP | 1 << DOWN | 1 << LEFT | 1 << RIGHT;

impl Controller {
    pub(super) fn start_navigation(&self) {
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(16), || {
            with_controller(|c| c.nav_tick())
        });
        *self.nav_timer.borrow_mut() = Some(timer);
    }

    fn nav_tick(&self) {
        if self.running.borrow().is_some() {
            self.nav_repeat.borrow_mut().update(0, DPAD, Instant::now());
            return;
        }
        let (inputs, connected) = {
            let mut gamepads = self.gamepads.borrow_mut();
            gamepads.poll();
            let inputs = gamepads.states(&self.mappings.borrow(), false);
            let connected = !inputs.is_empty();
            (inputs, connected)
        };
        self.update_pad_hints(connected);
        let buttons = inputs
            .iter()
            .fold(0u16, |b, p| b | stick_to_dpad(p.state).buttons);
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

    fn update_pad_hints(&self, connected: bool) {
        let Some(ui) = self.ui() else { return };
        let hints = if !connected || ui.get_dialog_open() || ui.get_remap_open() {
            ""
        } else {
            match ui.get_screen() {
                SCREEN_LIBRARY => {
                    "D-pad  Move    A  Open    LB / RB  Change list    Start  Settings"
                }
                SCREEN_GAME => "A  Play or download    X  Favorite    B  Back",
                SCREEN_SETTINGS => "LB / RB  Change section    B  Back",
                _ => "",
            }
        };
        if ui.get_pad_hints() != hints {
            ui.set_pad_hints(hints.into());
        }
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
            let columns = self.library.borrow().columns.max(1);
            ui.set_grid_focus_row((i / columns) as i32);
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
