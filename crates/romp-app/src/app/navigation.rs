use super::{
    with_controller, Controller, SCREEN_CONNECT, SCREEN_GAME, SCREEN_LIBRARY, SCREEN_PAIRING,
    SCREEN_SETTINGS, SCREEN_WELCOME,
};
use crate::gamepads::Button;
use crate::gettext_noop;
use crate::i18n::translate;
use crate::input::{A, B, DOWN, L, LEFT, R, RIGHT, START, UP, X, Y};
use crate::mapping::BUTTONS;
use crate::navigation::{move_in_grid, Dir};
use crate::pad_labels::{badge, Badge, Family};
use slint::platform::{Key, WindowEvent, WindowEventDispatchResult};
use slint::ComponentHandle;
use slint::{Model, ModelRc, Timer, TimerMode, VecModel};
use std::time::{Duration, Instant};

const SETTINGS_SECTIONS: i32 = 6;
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
            } else {
                navigate_controls(ui.window(), button);
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
            (SCREEN_WELCOME | SCREEN_CONNECT | SCREEN_PAIRING, UP | DOWN) => {
                let delta = if button == UP { 64.0 } else { -64.0 };
                ui.set_setup_scroll_y(ui.get_setup_scroll_y() + delta);
            }
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
            (SCREEN_LIBRARY, Y) => self.surprise(),
            (SCREEN_GAME | SCREEN_SETTINGS, UP | DOWN | LEFT | RIGHT | A) => {
                navigate_controls(ui.window(), button);
            }
            (SCREEN_GAME | SCREEN_SETTINGS, B) => {
                if !send_key(ui.window(), Key::Escape) {
                    self.back_to_library();
                }
            }
            (SCREEN_GAME, L | R) => {
                let delta = if button == L { 160.0 } else { -160.0 };
                ui.set_game_scroll_y(ui.get_game_scroll_y() + delta);
            }
            (SCREEN_GAME, X) => {
                if ui.get_can_edit_collections() {
                    self.toggle_favorite();
                }
            }
            (SCREEN_SETTINGS, L | R) => {
                let step = tab_step(button, crate::i18n::rtl());
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

/// Use native focus order and widget behavior, including popup ownership and scroll-to-focus.
fn navigate_controls(window: &slint::Window, button: u32) {
    let previous = button == UP || button == LEFT;
    let tab = if previous { Key::Backtab } else { Key::Tab };
    match button {
        UP | DOWN => {
            send_key(window, tab);
        }
        LEFT | RIGHT => {
            let arrow = if previous {
                Key::LeftArrow
            } else {
                Key::RightArrow
            };
            let choice = if previous {
                Key::UpArrow
            } else {
                Key::DownArrow
            };
            if !send_key(window, arrow) && !send_key(window, choice) {
                send_key(window, tab);
            }
        }
        A => {
            send_key(window, Key::Return);
        }
        B => {
            send_key(window, Key::Escape);
        }
        _ => {}
    }
}

fn send_key(window: &slint::Window, key: Key) -> bool {
    let text: slint::SharedString = key.into();
    let result = window.dispatch_event_with_result(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
    match result {
        Ok(result) => result == WindowEventDispatchResult::Accepted,
        Err(error) => {
            tracing::warn!(%error, "could not deliver controller navigation to UI");
            true
        }
    }
}

/// LB moves to the tab on the left, which is the next one when tabs run right to left.
fn tab_step(button: u32, rtl: bool) -> i32 {
    if (button == L) != rtl {
        -1
    } else {
        1
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
            (vec![key(Button::West)], gettext_noop!("Surprise me")),
            (vec![key(Button::Start)], gettext_noop!("Settings")),
        ],
        SCREEN_GAME => vec![
            (
                vec![Badge::Text(gettext_noop!("D-pad"))],
                gettext_noop!("Move"),
            ),
            (vec![key(family.confirm())], gettext_noop!("Select")),
            (shoulders, gettext_noop!("Scroll")),
            (vec![key(Button::North)], gettext_noop!("Favorite")),
            (vec![key(family.back())], gettext_noop!("Back")),
        ],
        SCREEN_SETTINGS => vec![
            (
                vec![Badge::Text(gettext_noop!("D-pad"))],
                gettext_noop!("Move or change value"),
            ),
            (vec![key(family.confirm())], gettext_noop!("Select")),
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
    fn pages_support_controller_focus_and_drag_scrolling() {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use slint::{ComponentHandle, PhysicalSize};

        struct Platform(std::rc::Rc<MinimalSoftwareWindow>);
        impl slint::platform::Platform for Platform {
            fn create_window_adapter(
                &self,
            ) -> Result<std::rc::Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError>
            {
                Ok(self.0.clone())
            }
        }

        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
        let ui = crate::AppWindow::new().unwrap();
        ui.set_screen(SCREEN_PAIRING);
        ui.show().unwrap();
        for (height, overflows) in [(400, true), (1000, false)] {
            window.set_size(PhysicalSize::new(640, height));
            window.draw_if_needed(|renderer| {
                let mut pixels = vec![slint::Rgb8Pixel::default(); 640 * height as usize];
                renderer.render(&mut pixels, 640);
            });
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition::new(320.0, 200.0),
                    delta_x: 0.0,
                    delta_y: -64.0,
                });
            assert_eq!(
                ui.get_setup_scroll_y() < 0.0,
                overflows,
                "window size: {:?}, scroll offset: {}",
                ui.window().size(),
                ui.get_setup_scroll_y()
            );
            ui.set_setup_scroll_y(0.0);
        }

        let draw = || {
            window.draw_if_needed(|renderer| {
                let mut pixels = vec![slint::Rgb8Pixel::default(); 800 * 400];
                renderer.render(&mut pixels, 800);
            });
        };
        window.set_size(PhysicalSize::new(800, 400));
        ui.set_game_title("A game with screenshots and saves".into());
        ui.set_game_summary("A long overview. ".repeat(400).into());
        ui.set_game_downloaded(true);
        ui.set_game_playable(true);
        ui.set_show_collections(true);
        ui.set_can_edit_collections(true);
        ui.set_game_shots(ModelRc::new(VecModel::from(vec![
            crate::Shot::default();
            3
        ])));
        let plays = std::rc::Rc::new(std::cell::Cell::new(0));
        let deletes = std::rc::Rc::new(std::cell::Cell::new(0));
        let backs = std::rc::Rc::new(std::cell::Cell::new(0));
        let favorites = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = plays.clone();
        ui.on_play_game(move || count.set(count.get() + 1));
        let count = deletes.clone();
        ui.on_delete_game(move || count.set(count.get() + 1));
        let count = backs.clone();
        ui.on_back_to_library(move || count.set(count.get() + 1));
        let count = favorites.clone();
        ui.on_toggle_favorite(move || count.set(count.get() + 1));
        ui.set_screen(SCREEN_GAME);
        draw();
        navigate_controls(ui.window(), A);
        assert_eq!(plays.get(), 1, "initial confirm retains the primary action");
        for _ in 0..3 {
            navigate_controls(ui.window(), DOWN);
        }
        navigate_controls(ui.window(), A);
        assert_eq!(plays.get(), 2, "confirm activates focused Play");
        navigate_controls(ui.window(), DOWN);
        navigate_controls(ui.window(), A);
        assert_eq!(deletes.get(), 1, "Delete receives confirm instead of Play");
        assert_eq!(plays.get(), 2);
        navigate_controls(ui.window(), DOWN);
        navigate_controls(ui.window(), A);
        assert_eq!(favorites.get(), 1);
        navigate_controls(ui.window(), DOWN);
        navigate_controls(ui.window(), A);
        navigate_controls(ui.window(), B);
        assert_eq!(backs.get(), 0, "Back closes the collections popup first");
        navigate_controls(ui.window(), DOWN);
        draw();
        assert!(
            ui.get_game_scroll_y() < -200.0,
            "offscreen screenshots scroll into view"
        );
        navigate_controls(ui.window(), A);
        navigate_controls(ui.window(), RIGHT);
        navigate_controls(ui.window(), B);
        assert_eq!(backs.get(), 0, "Back closes the screenshot viewer first");

        ui.set_game_scroll_y(0.0);
        draw();
        use slint::platform::PointerEventButton;
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position: slint::LogicalPosition::new(500.0, 350.0),
            button: PointerEventButton::Left,
        });
        for y in [320.0, 280.0, 240.0, 200.0, 150.0] {
            ui.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(500.0, y),
            });
        }
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(500.0, 150.0),
            button: PointerEventButton::Left,
        });
        assert!(
            ui.get_game_scroll_y() < -100.0,
            "mouse-emulated touchscreen swipe scrolls the page"
        );
        assert_eq!(plays.get(), 2, "a swipe must not activate a game action");

        ui.set_screen(SCREEN_SETTINGS);
        ui.set_settings_section(0);
        ui.set_pref_ui_scale(2);
        draw();
        // Two breadcrumb controls, six tabs, then three switches, volume and scale.
        for _ in 0..13 {
            navigate_controls(ui.window(), DOWN);
        }
        navigate_controls(ui.window(), LEFT);
        assert_eq!(
            ui.get_pref_ui_scale(),
            1,
            "controller can recover interface size"
        );
        draw();
        let confirmed = std::rc::Rc::new(std::cell::Cell::new(0));
        let cancelled = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = confirmed.clone();
        ui.on_dialog_accepted(move |_| count.set(count.get() + 1));
        let count = cancelled.clone();
        ui.on_dialog_cancelled(move || count.set(count.get() + 1));
        ui.set_dialog_confirm("Delete".into());
        ui.set_dialog_destructive(true);
        ui.set_dialog_open(true);
        draw();
        navigate_controls(ui.window(), A);
        assert_eq!(cancelled.get(), 1, "confirmation starts on Cancel");
        assert_eq!(confirmed.get(), 0);
        navigate_controls(ui.window(), DOWN);
        navigate_controls(ui.window(), A);
        assert_eq!(
            confirmed.get(),
            1,
            "destructive action requires choosing Confirm"
        );
        ui.set_dialog_open(false);
        draw();
        ui.set_dialog_asks_name(true);
        ui.set_dialog_value("".into());
        ui.set_dialog_open(true);
        draw();
        navigate_controls(ui.window(), UP);
        navigate_controls(ui.window(), A);
        assert_eq!(
            cancelled.get(),
            2,
            "Backtab leaves an empty name field for Cancel"
        );
        assert_eq!(confirmed.get(), 1, "disabled Confirm remains unreachable");
        navigate_controls(ui.window(), DOWN);
        navigate_controls(ui.window(), DOWN);
        navigate_controls(ui.window(), A);
        assert_eq!(
            cancelled.get(),
            3,
            "Tab skips disabled Confirm and wraps inside dialog"
        );
    }

    #[test]
    fn hints_name_the_buttons_of_the_controller_in_use() {
        let library = hints(SCREEN_LIBRARY, Family::Xbox);
        assert_eq!(library[1], (vec![Badge::Text("A")], "Open"));
        assert_eq!(
            library[2],
            (vec![Badge::Text("LB"), Badge::Text("RB")], "Change list")
        );
        assert_eq!(library[3], (vec![Badge::Text("X")], "Surprise me"));
        let game = hints(SCREEN_GAME, Family::Ps5);
        assert_eq!(game[1], (vec![Badge::Shape(Shape::Cross)], "Select"));
        assert_eq!(game[4], (vec![Badge::Shape(Shape::Circle)], "Back"));
        let settings = hints(SCREEN_SETTINGS, Family::Nintendo);
        assert_eq!(settings[3], (vec![Badge::Text("B")], "Back"));
    }

    #[test]
    fn shoulder_buttons_move_through_tabs_the_way_they_run() {
        assert_eq!(tab_step(L, false), -1);
        assert_eq!(tab_step(R, false), 1);
        assert_eq!(tab_step(L, true), 1);
        assert_eq!(tab_step(R, true), -1);
    }
}
