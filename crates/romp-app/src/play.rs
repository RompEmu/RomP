use crate::gamepads::Gamepads;
use crate::input::{self, map_key, Command, Controls, KeyAction};
use crate::mapping::{Hotkey, Mappings};
use crate::paths;
use crate::players::{Assignments, KEYBOARD};
use crate::ports;
use crate::prefs::Preferences;
use crate::restore::{Observed, Placement};
use crate::session::{Session, SessionConfig, SessionEvent};
use crate::GameWindow;
use crate::{LookRow, PortRow};
use anyhow::Context;
use romp_proto::msg::{AppMsg, RunnerMsg};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
use slint::{ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

pub type SavePorts = Box<dyn Fn(Vec<(u8, u32)>)>;
pub type VolumeChanged = Box<dyn Fn(u8)>;
pub type PopupsChanged = Box<dyn Fn(u8, u8)>;
pub type SavePlacement = Box<dyn Fn(usize, &Observed)>;
pub type ScreenshotTaken = Box<dyn Fn(PathBuf)>;

/// How a console is drawn, and where the player's changes to it are kept.
pub struct LookOptions {
    pub platform: String,
    pub choice: crate::looks::Choice,
    pub shaders: PathBuf,
    pub changed: Box<dyn Fn(crate::looks::Choice)>,
}
type Saved = Result<PathBuf, String>;

pub struct GameOptions {
    pub core: PathBuf,
    pub rom: PathBuf,
    pub save_dir: PathBuf,
    pub title: String,
    pub jit: bool,
    pub vulkan: bool,
    pub options: Vec<(String, String)>,
    pub split_screens: bool,
    pub gamepads: Rc<RefCell<Gamepads>>,
    pub players: Rc<RefCell<Assignments>>,
    pub prefs: Preferences,
    pub load_slot: Option<u8>,
    pub auto_state: bool,
    pub mappings: Rc<RefCell<Mappings>>,
    pub nintendo: bool,
    pub mouse: bool,
    pub computer: bool,
    pub port_devices: Vec<(u8, u32)>,
    pub save_ports: SavePorts,
    pub volume_changed: VolumeChanged,
    pub popups_changed: PopupsChanged,
    pub placements: Vec<Option<Placement>>,
    pub save_placement: SavePlacement,
    pub screenshot_taken: ScreenshotTaken,
    pub achievements: Option<crate::achievements::popups::Launch>,
    pub look: Option<LookOptions>,
}

pub struct CoreGame {
    game: Rc<Game>,
    _timer: Timer,
}

pub enum RunningGame {
    Core(CoreGame),
    External(crate::external::Running),
}

impl RunningGame {
    pub fn wait_exit(&self, timeout: Duration) -> bool {
        match self {
            Self::Core(core) => core.game.session.borrow().wait_exit(timeout),
            Self::External(running) => running.wait_exit(timeout),
        }
    }

    pub fn apply_prefs(&self, prefs: &Preferences) {
        if let Self::Core(core) = self {
            core.game.apply_prefs(prefs);
        }
    }

    pub fn save_placements(&self) {
        if let Self::Core(core) = self {
            core.game.save_placements();
        }
    }
}

pub fn run(core: PathBuf, rom: PathBuf, jit: bool, vulkan: bool) -> anyhow::Result<()> {
    let rom = rom
        .canonicalize()
        .with_context(|| format!("ROM not found: {}", rom.display()))?;
    let title = rom
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let save_dir = paths::data_dir()
        .join("saves")
        .join(paths::local_save_dir_name(&rom));
    let game = launch(
        GameOptions {
            screenshot_taken: Box::new(|_| {}),
            achievements: None,
            look: std::env::var("ROMP_PLATFORM")
                .ok()
                .map(|platform| LookOptions {
                    choice: crate::looks::default_choice(&platform),
                    platform,
                    shaders: paths::shaders_dir(),
                    changed: Box::new(|_| {}),
                }),
            core,
            rom,
            save_dir,
            title,
            jit,
            vulkan,
            options: Vec::new(),
            split_screens: false,
            gamepads: Rc::new(RefCell::new(Gamepads::new())),
            players: Rc::default(),
            prefs: Preferences::default(),
            load_slot: None,
            auto_state: true,
            mappings: Rc::default(),
            nintendo: false,
            mouse: false,
            computer: false,
            port_devices: Vec::new(),
            save_ports: Box::new(|_| {}),
            volume_changed: Box::new(|_| {}),
            popups_changed: Box::new(|_, _| {}),
            placements: Vec::new(),
            save_placement: Box::new(|_, _| {}),
        },
        |_| {},
        || {},
    )?;
    slint::run_event_loop()?;
    game.wait_exit(Duration::from_secs(4));
    Ok(())
}

pub type CoreIdentity = (String, String);

const RESUME_GRACE: Duration = Duration::from_secs(20);

struct Game {
    session: RefCell<Session>,
    windows: Vec<GameWindow>,
    shading: Vec<Rc<RefCell<crate::shading::Shading>>>,
    look: RefCell<Option<LookOptions>>,
    look_open: Cell<bool>,
    look_focus: Cell<i32>,
    controls: RefCell<Controls>,
    paused: Cell<bool>,
    menu_open: Cell<bool>,
    focus_paused: Cell<bool>,
    pause_unfocused: Cell<bool>,
    finish: Rc<dyn Fn()>,
    open_controllers: Box<dyn Fn()>,
    mappings: Rc<RefCell<Mappings>>,
    menu_focus: Cell<i32>,
    pad_buttons: Cell<u16>,
    menu_combo: Cell<bool>,
    mouse_captured: Cell<bool>,
    mouse_buttons: Cell<u8>,
    always_mouse: bool,
    port_options: RefCell<Vec<ports::Options>>,
    port_devices: RefCell<Vec<u32>>,
    saved_ports: Vec<(u8, u32)>,
    save_ports: SavePorts,
    volume: Cell<u8>,
    volume_changed: VolumeChanged,
    popups: Cell<u8>,
    corner: Cell<u8>,
    popups_changed: PopupsChanged,
    achievements_focus: Cell<i32>,
    save_placement: SavePlacement,
    computer: bool,
    held_keys: RefCell<HashSet<u32>>,
    modifiers: Cell<u16>,
    rotation: Cell<u8>,
    aim: Cell<(i16, i16)>,
    title: String,
    screenshot_dir: PathBuf,
    screenshot_wanted: Cell<bool>,
    screenshots: (
        std::sync::mpsc::Sender<Saved>,
        std::sync::mpsc::Receiver<Saved>,
    ),
    screenshot_taken: ScreenshotTaken,
    achievements: RefCell<Option<crate::achievements::popups::Link>>,
    achievements_open: Cell<bool>,
    achievement_list: RefCell<Vec<romp_proto::msg::AchievementInfo>>,
    hardcore: Cell<bool>,
    save_dir: PathBuf,
    slots_open: Cell<bool>,
    slots_focus: Cell<u8>,
    thumbnail_wanted: Cell<Option<u8>>,
    thumbnail_frame: Cell<u32>,
    recent_thumbnail: RefCell<Option<image::RgbaImage>>,
    thumbnails: (std::sync::mpsc::Sender<u8>, std::sync::mpsc::Receiver<u8>),
}

const MENU_RESUME: i32 = 0;
const MENU_PAUSE: i32 = 1;
const MENU_RESTART: i32 = 2;
const MENU_SAVE_STATES: i32 = 3;
const MENU_LOOK: i32 = 4;
const MENU_VOLUME: i32 = 5;
const MENU_FULLSCREEN: i32 = 6;
const MENU_SCREENSHOT: i32 = 7;
const MENU_ACHIEVEMENTS: i32 = 8;
const MENU_CONTROLLERS: i32 = 9;
const MENU_PORTS_START: i32 = 10;
const ACHIEVEMENT_ROW: f32 = 52.0;
const VOLUME_STEP: i32 = 10;
/// How often, in frames, the small picture kept for the automatic save is refreshed.
const THUMBNAIL_EVERY: u32 = 120;

const MENU_ROW: [i32; 3] = [MENU_RESUME, MENU_PAUSE, MENU_RESTART];

fn menu_move(focus: i32, button: u32, items: i32) -> Option<i32> {
    let directional = matches!(button, input::UP | input::DOWN | input::LEFT | input::RIGHT);
    if focus < 0 {
        return directional.then_some(MENU_RESUME);
    }
    if let Some(c) = MENU_ROW.iter().position(|&i| i == focus) {
        return match button {
            input::LEFT => Some(MENU_ROW[c.saturating_sub(1)]),
            input::RIGHT => Some(MENU_ROW[(c + 1).min(2)]),
            input::UP => Some(focus),
            input::DOWN => Some(MENU_SAVE_STATES),
            _ => None,
        };
    }
    match button {
        input::UP if focus == MENU_SAVE_STATES => Some(MENU_RESUME),
        input::UP => Some((focus - 1).max(0)),
        input::DOWN => Some((focus + 1).min(items - 1)),
        _ => None,
    }
}

/// Moves around the 2×2 grid of save slots, stopping at its edges.
fn slot_grid_move(slot: u8, button: u32) -> u8 {
    let (row, col) = ((slot.max(1) - 1) / 2, (slot.max(1) - 1) % 2);
    let (row, col) = match button {
        input::LEFT => (row, col.saturating_sub(1)),
        input::RIGHT => (row, (col + 1).min(1)),
        input::UP => (row.saturating_sub(1), col),
        input::DOWN => ((row + 1).min(1), col),
        _ => (row, col),
    };
    row * 2 + col + 1
}

/// Command-W on a Mac, which Slint reports as Control there, and Control-W elsewhere.
fn closes_window(text: &str, mods: crate::mapping::Mods) -> bool {
    mods.ctrl && !mods.alt && !mods.meta && matches!(text, "w" | "W" | "\u{17}")
}

fn stepped_volume(volume: u8, delta: i32) -> u8 {
    let volume = i32::from(volume);
    let stepped = if delta > 0 {
        (volume / VOLUME_STEP + 1) * VOLUME_STEP
    } else if volume % VOLUME_STEP != 0 {
        volume / VOLUME_STEP * VOLUME_STEP
    } else {
        volume - VOLUME_STEP
    };
    stepped.clamp(0, 100) as u8
}

impl Game {
    fn primary(&self) -> &GameWindow {
        &self.windows[0]
    }

    fn save_placements(&self) {
        for (i, window) in self.windows.iter().enumerate() {
            if window.window().is_visible() {
                (self.save_placement)(i, &Observed::of(window.window()));
            }
        }
    }

    fn send(&self, msg: &AppMsg) {
        self.session.borrow_mut().send(msg);
    }

    fn run_commands(&self, commands: Vec<Command>) {
        for command in commands {
            match command {
                Command::Send(AppMsg::LoadSlot(slot)) => self.load_slot(slot),
                Command::Send(AppMsg::SaveSlot(slot)) => self.save_slot(slot),
                Command::Send(msg) => self.send(&msg),
                Command::Menu => self.toggle_menu(),
                Command::TogglePause => self.set_paused(!self.paused.get()),
                Command::ToggleFullscreen => self.toggle_fullscreen(),
                Command::Screenshot => self.screenshot_wanted.set(true),
                Command::SlotChanged(slot) => self.show_slot(slot),
            }
        }
    }

    fn capture_mouse(&self, on: bool) {
        if self.mouse_captured.replace(on) == on {
            return;
        }
        crate::mouse::capture(self.primary().window(), on);
        self.primary().set_mouse_captured(on);
        self.mouse_buttons.set(0);
        self.send(&AppMsg::Mouse {
            dx: 0,
            dy: 0,
            buttons: 0,
        });
        if on {
            let key = if self.computer { "F12" } else { "Esc" };
            flash(
                self.primary(),
                format!("Mouse captured. Press {key} to release it."),
            );
        }
    }

    fn pointer(&self, u: f32, v: f32, pressed: bool, bottom_half: bool) {
        let (u, v) = crate::rotation::unrotate_point(u, v, self.rotation.get());
        let (x, y) = input::pointer_coords(u, v, bottom_half);
        self.aim.set((x, y));
        self.send(&AppMsg::Pointer { x, y, pressed });
    }

    fn mouse_button(&self, bit: i32, pressed: bool) {
        let Ok(bit) = u8::try_from(bit) else { return };
        let buttons = if pressed {
            self.mouse_buttons.get() | bit
        } else {
            self.mouse_buttons.get() & !bit
        };
        self.mouse_buttons.set(buttons);
        if self.primary().get_lightgun_mode() {
            let (x, y) = self.aim.get();
            self.send(&AppMsg::Pointer {
                x,
                y,
                pressed: buttons & 1 != 0,
            });
        }
        self.send(&AppMsg::Mouse {
            dx: 0,
            dy: 0,
            buttons,
        });
    }

    fn send_mouse_motion(&self) {
        if !self.mouse_captured.get() {
            return;
        }
        let (dx, dy) = crate::mouse::take_motion();
        let (dx, dy) = crate::rotation::unrotate_delta(dx, dy, self.rotation.get());
        if (dx, dy) != (0, 0) {
            self.send(&AppMsg::Mouse {
                dx,
                dy,
                buttons: self.mouse_buttons.get(),
            });
        }
    }

    fn set_paused(&self, paused: bool) {
        if paused {
            self.capture_mouse(false);
            self.release_keys();
            let released = self.controls.borrow_mut().release_all();
            self.run_commands(released);
        }
        self.paused.set(paused);
        self.send(&AppMsg::Pause(paused));
        for window in &self.windows {
            window.set_paused(paused);
        }
    }

    /// The menu key closes an open list first, then the menu.
    fn toggle_menu(&self) {
        if self.achievements_open.get() {
            self.close_achievements();
        } else if self.slots_open.get() {
            self.close_slots();
        } else if self.look_open.get() {
            self.close_look();
        } else {
            self.set_menu(!self.menu_open.get());
        }
    }

    fn set_menu(&self, open: bool) {
        self.menu_open.set(open);
        self.set_menu_focus(-1);
        self.primary().set_menu_open(open);
        self.focus_paused.set(false);
        self.set_paused(open);
    }

    fn toggle_fullscreen(&self) {
        let window = self.primary().window();
        let fullscreen = !window.is_fullscreen();
        window.set_fullscreen(fullscreen);
        self.primary().set_fullscreen(fullscreen);
    }

    /// Saves the frame on screen off the UI thread; `screenshot_saved` picks up the result.
    fn capture(&self, rgba: &[u8], shown: Option<(u32, u32, f32)>) {
        let Some((width, height, aspect)) = shown else {
            return flash(self.primary(), "The game hasn't drawn anything yet".into());
        };
        let (pixels, width, height) =
            crate::rotation::rotate(rgba, width, height, self.rotation.get());
        let dir = self.screenshot_dir.clone();
        let title = self.title.clone();
        let done = self.screenshots.0.clone();
        std::thread::spawn(move || {
            let saved = crate::screenshot::image(&pixels, width, height, aspect)
                .ok_or_else(|| "the picture was incomplete".to_string())
                .and_then(|picture| {
                    crate::screenshot::save(&dir, &title, std::time::SystemTime::now(), &picture)
                        .map_err(|e| e.to_string())
                });
            let _ = done.send(saved);
        });
    }

    fn screenshot_saved(&self) {
        while let Ok(saved) = self.screenshots.1.try_recv() {
            match saved {
                Ok(path) => {
                    flash(self.primary(), "Screenshot saved".into());
                    (self.screenshot_taken)(path);
                }
                Err(e) => flash(self.primary(), format!("Couldn't save the screenshot: {e}")),
            }
        }
    }

    fn show_achievements(&self, now: std::time::Instant) {
        let polled = self.achievements.borrow_mut().as_mut().map(|l| l.poll(now));
        let Some(polled) = polled else {
            return;
        };
        for reply in &polled.replies {
            self.send(reply);
        }
        if polled.new_badges && self.achievements_open.get() {
            self.show_achievement_rows();
        }
        let chip = polled.chip.clone().unwrap_or_default();
        if self.primary().get_chip_text() != chip.as_str() {
            self.primary().set_chip_text(chip.into());
        }
        let overlay = self
            .achievements
            .borrow_mut()
            .as_mut()
            .and_then(|l| l.overlay(now));
        if let Some(overlay) = overlay {
            let ui = self.primary();
            ui.set_trackers(ModelRc::new(VecModel::from(
                overlay
                    .trackers
                    .into_iter()
                    .map(slint::SharedString::from)
                    .collect::<Vec<_>>(),
            )));
            ui.set_challenge_badges(ModelRc::new(VecModel::from(overlay.challenges)));
            match overlay.progress {
                Some(progress) => {
                    ui.set_progress_title(progress.title.into());
                    ui.set_progress_detail(progress.detail.into());
                    ui.set_has_progress_badge(progress.badge.is_some());
                    ui.set_progress_badge(progress.badge.unwrap_or_default());
                }
                None => ui.set_progress_title("".into()),
            }
        }
        let ui = self.primary();
        match polled.view {
            Some(view) => {
                let changed = ui.get_toast_title() != view.title.as_str()
                    || ui.get_toast_detail() != view.detail.as_str();
                if changed || ui.get_toast_has_badge() != view.badge.is_some() {
                    ui.set_toast_title(view.title.into());
                    ui.set_toast_detail(view.detail.into());
                    ui.set_toast_has_badge(view.badge.is_some());
                    ui.set_toast_badge(view.badge.unwrap_or_default());
                }
            }
            None if !ui.get_toast_title().is_empty() => ui.set_toast_title("".into()),
            None => {}
        }
    }

    /// Draws every window with the console's look, and shows it in the menu.
    fn apply_look(&self) {
        let look = self.look.borrow();
        let Some(options) = look.as_ref() else {
            for (window, shading) in self.windows.iter().zip(&self.shading) {
                shading.borrow_mut().set_stages(Vec::new());
                show_plain(window, &shading.borrow());
            }
            return;
        };
        let choice = options.choice;
        let stages = crate::looks::stages(&choice, &options.platform, &options.shaders);
        let rows: Vec<LookRow> = crate::looks::rows(&choice, &options.platform)
            .into_iter()
            .map(|row| LookRow {
                label: crate::looks::row_label(row).into(),
                value: crate::looks::row_value(&choice, row).into(),
            })
            .collect();
        let rows = ModelRc::new(VecModel::from(rows));
        for (window, shading) in self.windows.iter().zip(&self.shading) {
            shading.borrow_mut().set_stages(stages.clone());
            if !shading.borrow().active() {
                show_plain(window, &shading.borrow());
            }
            match choice.look {
                crate::looks::Look::Sharp => window.set_sharp(true),
                crate::looks::Look::Smooth => window.set_sharp(false),
                _ => {}
            }
            window.set_look_label(crate::looks::label(choice.look).into());
            window.set_look_rows(rows.clone());
            window.window().request_redraw();
        }
    }

    fn look_rows(&self) -> Vec<crate::looks::Row> {
        self.look
            .borrow()
            .as_ref()
            .map(|l| crate::looks::rows(&l.choice, &l.platform))
            .unwrap_or_default()
    }

    fn open_look(&self) {
        if self.look.borrow().is_none() {
            return;
        }
        self.look_open.set(true);
        self.focus_look(0);
        self.primary().set_look_open(true);
        self.primary().window().request_redraw();
    }

    fn close_look(&self) {
        self.look_open.set(false);
        self.primary().set_look_open(false);
    }

    fn focus_look(&self, row: i32) {
        self.look_focus.set(row);
        self.primary().set_look_focus(row);
    }

    /// Steps one of the look panel's rows left or right.
    fn step_look(&self, row: i32, delta: i32) {
        let Some(row) = usize::try_from(row)
            .ok()
            .and_then(|i| self.look_rows().get(i).copied())
        else {
            return;
        };
        {
            let mut look = self.look.borrow_mut();
            let Some(options) = look.as_mut() else {
                return;
            };
            crate::looks::step(&mut options.choice, &options.platform, row, delta);
            (options.changed)(options.choice);
        }
        self.apply_look();
        if self.look_focus.get() >= self.look_rows().len() as i32 {
            self.focus_look(0);
        }
    }

    fn open_slots(&self) {
        self.slots_open.set(true);
        self.slots_focus.set(self.controls.borrow().slot());
        self.show_slots();
        self.primary().set_slots_open(true);
    }

    fn close_slots(&self) {
        self.slots_open.set(false);
        self.primary().set_slots_open(false);
    }

    fn focus_slot(&self, slot: u8) {
        self.slots_focus.set(slot.clamp(1, input::SLOTS));
        self.primary()
            .set_slots_focus(i32::from(self.slots_focus.get()));
    }

    fn show_slots(&self) {
        let now = std::time::SystemTime::now();
        let current = self.controls.borrow().slot();
        let cards: Vec<crate::SlotCard> = crate::slots::read(&self.save_dir, 1..=input::SLOTS)
            .iter()
            .map(|info| crate::slots::card(info, now, Some(current)))
            .collect();
        let ui = self.primary();
        ui.set_slot_cards(ModelRc::new(VecModel::from(cards)));
        ui.set_slots_focus(i32::from(self.slots_focus.get()));
    }

    /// Saves to `slot`, which becomes the current one, with a picture of the game as it is now.
    fn save_slot(&self, slot: u8) {
        self.controls.borrow_mut().set_slot(slot);
        self.thumbnail_wanted.set(Some(slot));
        self.send(&AppMsg::SaveSlot(slot));
    }

    fn load_from(&self, slot: u8) {
        let saved = crate::slots::read(&self.save_dir, [slot]);
        if saved.first().is_none_or(crate::slots::SlotInfo::is_empty) {
            return;
        }
        self.controls.borrow_mut().set_slot(slot);
        self.load_slot(slot);
        if !self.hardcore.get() {
            self.close_slots();
            self.set_menu(false);
        }
    }

    /// Writes a slot's picture off the UI thread; `thumbnail_written` hears when it's there.
    fn write_thumbnail(&self, slot: u8, picture: image::RgbaImage) {
        let path = crate::slots::thumbnail_path(&self.save_dir, slot);
        let done = self.thumbnails.0.clone();
        std::thread::spawn(move || {
            if let Err(e) = picture.save_with_format(&path, image::ImageFormat::Png) {
                tracing::warn!("saving the picture for slot {slot}: {e}");
            }
            let _ = done.send(slot);
        });
    }

    fn take_thumbnails(&self, rgba: &[u8], shown: Option<(u32, u32, f32)>, frame: u32) {
        let wanted = self.thumbnail_wanted.take();
        let due = frame != self.thumbnail_frame.get() && frame.is_multiple_of(THUMBNAIL_EVERY);
        if wanted.is_none() && !due {
            return;
        }
        self.thumbnail_frame.set(frame);
        let Some((width, height, aspect)) = shown else {
            return;
        };
        let turns = self.rotation.get();
        let (pixels, width, height) = if turns.is_multiple_of(4) {
            (std::borrow::Cow::Borrowed(rgba), width, height)
        } else {
            let (rotated, w, h) = crate::rotation::rotate(rgba, width, height, turns);
            (std::borrow::Cow::Owned(rotated), w, h)
        };
        let Some(picture) = crate::slots::thumbnail(&pixels, width, height, aspect) else {
            return;
        };
        if let Some(slot) = wanted {
            self.write_thumbnail(slot, picture.clone());
        }
        *self.recent_thumbnail.borrow_mut() = Some(picture);
    }

    fn thumbnail_written(&self) {
        let mut any = false;
        while self.thumbnails.1.try_recv().is_ok() {
            any = true;
        }
        if any && self.slots_open.get() {
            self.show_slots();
        }
    }

    /// The automatic save made on quitting gets the most recent picture of the game.
    fn write_auto_thumbnail(&self) {
        if let Some(picture) = self.recent_thumbnail.borrow_mut().take() {
            self.write_thumbnail(crate::slots::AUTO, picture);
        }
    }

    /// Hardcore mode never loads a state; the runner refuses too, this explains why nothing happened.
    fn load_slot(&self, slot: u8) {
        if self.hardcore.get() {
            return flash(
                self.primary(),
                "Loading states is off in hardcore mode".into(),
            );
        }
        self.send(&AppMsg::LoadSlot(slot));
    }

    fn open_achievements(&self) {
        if self.achievements.borrow().is_none() {
            return;
        }
        self.achievements_open.set(true);
        self.focus_achievements(0);
        self.send(&AppMsg::ListAchievements);
        let ui = self.primary();
        ui.set_achievements_scroll(0.0);
        ui.set_achievements_open(true);
        self.show_achievement_rows();
    }

    /// Shows how much RetroAchievements may show over the game, and where.
    fn apply_popups(&self) {
        use crate::achievements::popups::{Level, CORNERS, LEVELS};
        if let Some(link) = self.achievements.borrow_mut().as_mut() {
            link.set_level(Level::from_index(self.popups.get()));
        }
        let label = |list: &[&str], i: u8| list.get(usize::from(i)).copied().unwrap_or("").into();
        for window in &self.windows {
            window.set_popups_label(label(&LEVELS, self.popups.get()));
            window.set_corner_label(label(&CORNERS, self.corner.get()));
            window.set_achievement_corner(i32::from(self.corner.get()));
        }
    }

    fn step_popups(&self, delta: i32) {
        let levels = crate::achievements::popups::LEVELS.len() as i32;
        let level = (i32::from(self.popups.get()) + delta).clamp(0, levels - 1);
        self.popups.set(level as u8);
        self.popups_saved();
    }

    fn step_corner(&self, delta: i32) {
        let corners = crate::achievements::popups::CORNERS.len() as i32;
        let corner = (i32::from(self.corner.get()) + delta).rem_euclid(corners);
        self.corner.set(corner as u8);
        self.popups_saved();
    }

    fn popups_saved(&self) {
        self.apply_popups();
        (self.popups_changed)(self.popups.get(), self.corner.get());
    }

    /// Moves between the panel's popup settings (0 and 1) and its list (2).
    fn focus_achievements(&self, focus: i32) {
        self.achievements_focus.set(focus);
        self.primary().set_achievements_focus(focus);
    }

    fn close_achievements(&self) {
        self.achievements_open.set(false);
        self.primary().set_achievements_open(false);
    }

    fn scroll_achievements(&self, rows: f32) {
        let ui = self.primary();
        let max = -(self.achievement_list.borrow().len() as f32 * ACHIEVEMENT_ROW);
        let scroll =
            (ui.get_achievements_scroll() - rows * ACHIEVEMENT_ROW).clamp(max.min(0.0), 0.0);
        ui.set_achievements_scroll(scroll);
    }

    fn achievement_list_arrived(&self, list: Vec<romp_proto::msg::AchievementInfo>) {
        let list = crate::achievements::without_notices(list);
        let label = crate::achievements::summary(&list);
        if let Some(link) = self.achievements.borrow_mut().as_mut() {
            for a in &list {
                link.want_badge(crate::achievements::badge_url(a));
            }
        }
        *self.achievement_list.borrow_mut() = list;
        for window in &self.windows {
            window.set_achievements_label(label.clone().into());
        }
        if self.achievements_open.get() {
            self.show_achievement_rows();
        }
    }

    fn show_achievement_rows(&self) {
        let link = self.achievements.borrow();
        let mut rows: Vec<crate::AchievementRow> = self
            .achievement_list
            .borrow()
            .iter()
            .map(|a| {
                let badge = link
                    .as_ref()
                    .and_then(|l| l.badge(crate::achievements::badge_url(a)));
                let note = link.as_ref().and_then(|l| l.session_note(&a.title));
                crate::AchievementRow {
                    title: a.title.clone().into(),
                    detail: crate::achievements::row_detail(a).into(),
                    has_badge: badge.is_some(),
                    badge: badge.unwrap_or_default(),
                    unlocked: a.unlocked,
                    note: note.unwrap_or_default().into(),
                }
            })
            .collect();
        // What happened while playing comes first.
        rows.sort_by_key(|row| row.note.is_empty());
        self.primary()
            .set_achievement_rows(ModelRc::new(VecModel::from(rows)));
    }

    fn show_slot(&self, slot: u8) {
        flash(self.primary(), format!("Save slot {slot}"));
    }

    fn set_menu_focus(&self, focus: i32) {
        self.menu_focus.set(focus);
        self.primary().set_menu_focus(focus);
    }

    fn menu_items(&self) -> i32 {
        MENU_PORTS_START + self.pickable_ports().len() as i32 + 1
    }

    fn pickable_ports(&self) -> Vec<usize> {
        ports::pickable(&self.port_options.borrow())
    }

    fn port_row(&self, item: i32) -> Option<usize> {
        let row = usize::try_from(item - MENU_PORTS_START).ok()?;
        self.pickable_ports().get(row).copied()
    }

    fn set_ports(&self, offered: Vec<ports::Options>) {
        tracing::debug!(?offered, "controller ports");
        let devices: Vec<u32> = offered
            .iter()
            .enumerate()
            .map(|(port, options)| {
                let saved = self
                    .saved_ports
                    .iter()
                    .find(|(p, _)| usize::from(*p) == port)
                    .map(|(_, d)| *d);
                ports::choose(options, saved)
            })
            .collect();
        for (port, device) in devices.iter().enumerate() {
            if *device != ports::JOYPAD {
                self.send(&AppMsg::PortDevice {
                    port: port as u8,
                    device: *device,
                });
            }
        }
        *self.port_options.borrow_mut() = offered;
        *self.port_devices.borrow_mut() = devices;
        self.refresh_ports();
    }

    fn cycle_port(&self, port: usize, step: i32) {
        let device = {
            let options = self.port_options.borrow();
            let Some(options) = options.get(port) else {
                return;
            };
            let current = self
                .port_devices
                .borrow()
                .get(port)
                .copied()
                .unwrap_or(ports::JOYPAD);
            ports::cycle(options, current, step)
        };
        if let Some(slot) = self.port_devices.borrow_mut().get_mut(port) {
            *slot = device;
        }
        self.send(&AppMsg::PortDevice {
            port: port as u8,
            device,
        });
        let chosen = self
            .port_devices
            .borrow()
            .iter()
            .enumerate()
            .map(|(p, d)| (p as u8, *d))
            .collect();
        (self.save_ports)(chosen);
        self.refresh_ports();
    }

    fn refresh_ports(&self) {
        let devices = self.port_devices.borrow().clone();
        let mouse = self.always_mouse || ports::uses_mouse(&devices);
        let gun = !mouse && ports::uses_lightgun(&devices);
        if !mouse {
            self.capture_mouse(false);
        }
        let rows: Vec<PortRow> = {
            let options = self.port_options.borrow();
            self.pickable_ports()
                .into_iter()
                .map(|port| PortRow {
                    label: format!("Port {}", port + 1).into(),
                    device: ports::name_of(&options[port], devices[port]).into(),
                })
                .collect()
        };
        for window in &self.windows {
            window.set_mouse_mode(mouse);
            window.set_lightgun_mode(gun);
        }
        self.primary().set_ports(ModelRc::new(VecModel::from(rows)));
    }

    fn activate(&self, item: i32) {
        if let Some(port) = self.port_row(item) {
            return self.cycle_port(port, 1);
        }
        if item == self.menu_items() - 1 {
            return (self.finish)();
        }
        match item {
            MENU_RESUME => self.set_menu(false),
            MENU_PAUSE => self.set_paused(!self.paused.get()),
            MENU_RESTART => self.restart(),
            MENU_SAVE_STATES => self.open_slots(),
            MENU_LOOK => self.open_look(),
            MENU_VOLUME => self.step_volume(1),
            MENU_FULLSCREEN => self.toggle_fullscreen(),
            MENU_SCREENSHOT => self.screenshot_wanted.set(true),
            MENU_ACHIEVEMENTS => self.open_achievements(),
            MENU_CONTROLLERS => (self.open_controllers)(),
            _ => {}
        }
    }

    fn restart(&self) {
        self.send(&AppMsg::Reset);
        self.set_menu(false);
    }

    fn step_volume(&self, delta: i32) {
        let volume = stepped_volume(self.volume.get(), delta);
        self.volume.set(volume);
        self.send(&AppMsg::Volume(volume));
        for window in &self.windows {
            window.set_volume(i32::from(volume));
        }
        (self.volume_changed)(volume);
    }

    fn menu_button(&self, button: u32) {
        if self.look_open.get() {
            let rows = self.look_rows().len() as i32;
            let focus = self.look_focus.get();
            match button {
                input::UP => self.focus_look((focus - 1).max(0)),
                input::DOWN => self.focus_look((focus + 1).min(rows - 1)),
                input::LEFT => self.step_look(focus, -1),
                input::RIGHT => self.step_look(focus, 1),
                input::A | input::B | input::START => self.close_look(),
                _ => {}
            }
            return;
        }
        if self.slots_open.get() {
            let focus = self.slots_focus.get();
            match button {
                input::LEFT | input::RIGHT | input::UP | input::DOWN => {
                    self.focus_slot(slot_grid_move(focus, button));
                }
                input::A | input::START => self.load_from(focus),
                input::X => self.save_slot(focus),
                input::B => self.close_slots(),
                _ => {}
            }
            return;
        }
        if self.achievements_open.get() {
            let focus = self.achievements_focus.get();
            let step = if button == input::LEFT { -1 } else { 1 };
            match button {
                input::UP if focus == 2 && self.primary().get_achievements_scroll() >= 0.0 => {
                    self.focus_achievements(1);
                }
                input::UP if focus == 2 => self.scroll_achievements(-1.0),
                input::UP => self.focus_achievements((focus - 1).max(0)),
                input::DOWN if focus == 2 => self.scroll_achievements(1.0),
                input::DOWN => self.focus_achievements(focus + 1),
                input::LEFT | input::RIGHT if focus == 0 => self.step_popups(step),
                input::LEFT | input::RIGHT if focus == 1 => self.step_corner(step),
                input::B | input::A | input::START => self.close_achievements(),
                _ => {}
            }
            return;
        }
        let focus = self.menu_focus.get();
        if let Some(next) = menu_move(focus, button, self.menu_items()) {
            return self.set_menu_focus(next);
        }
        let step = if button == input::LEFT { -1 } else { 1 };
        match button {
            input::LEFT | input::RIGHT => {
                if let Some(port) = self.port_row(focus) {
                    self.cycle_port(port, step);
                } else if focus == MENU_VOLUME {
                    self.step_volume(step);
                }
            }
            input::A | input::START => self.activate(focus.max(0)),
            input::B => self.set_menu(false),
            _ => {}
        }
    }

    fn pads(&self, inputs: &[crate::gamepads::PadInput]) {
        let combo = inputs.iter().any(|p| {
            let held = |b: u32| p.state.buttons & 1 << b != 0;
            p.guide || (held(input::SELECT) && held(input::START))
        });
        if combo && !self.menu_combo.get() {
            self.toggle_menu();
        }
        self.menu_combo.set(combo);
        let buttons = inputs.iter().fold(0u16, |b, p| b | p.state.buttons);
        let pressed = buttons & !self.pad_buttons.replace(buttons);
        if self.menu_open.get() {
            for (button, _) in crate::mapping::BUTTONS {
                if pressed & 1 << button != 0 {
                    self.menu_button(button);
                }
            }
        }
    }

    fn computer_key(&self, text: &str, pressed: bool, repeat: bool) -> bool {
        let Some(key) = crate::keyboard::retro_key(text) else {
            return false;
        };
        if repeat {
            return true;
        }
        let bit = crate::keyboard::modifier_bit(key.code);
        let modifiers = if pressed {
            self.modifiers.get() | bit
        } else {
            self.modifiers.get() & !bit
        };
        self.modifiers.set(modifiers);
        {
            let mut held = self.held_keys.borrow_mut();
            if pressed {
                held.insert(key.code);
            } else if !held.remove(&key.code) {
                return true;
            }
        }
        self.send(&AppMsg::Key {
            code: key.code,
            character: if pressed { key.character } else { 0 },
            modifiers,
            down: pressed,
        });
        true
    }

    fn release_keys(&self) {
        let held: Vec<u32> = self.held_keys.borrow_mut().drain().collect();
        self.modifiers.set(0);
        for code in held {
            self.send(&AppMsg::Key {
                code,
                character: 0,
                modifiers: 0,
                down: false,
            });
        }
    }

    fn key(&self, text: &str, mods: crate::mapping::Mods, pressed: bool, repeat: bool) -> bool {
        if closes_window(text, mods) {
            if pressed && !repeat {
                (self.finish)();
            }
            return true;
        }
        if self.computer {
            let menu_key = text.chars().eq([char::from(slint::platform::Key::F12)]);
            if menu_key {
                if pressed && !repeat {
                    self.toggle_menu();
                }
                return true;
            }
            if !self.menu_open.get() {
                return self.paused.get() || self.computer_key(text, pressed, repeat);
            }
        }
        let mappings = self.mappings.borrow();
        let action = map_key(text, mods, &mappings);
        if self.menu_open.get() {
            if let Some(KeyAction::Button(button)) = action {
                if pressed {
                    self.menu_button(button);
                }
                return true;
            }
        }
        let result = self
            .controls
            .borrow_mut()
            .key(text, mods, pressed, repeat, &mappings);
        match result {
            None => false,
            Some(commands) => {
                self.run_commands(commands);
                true
            }
        }
    }

    fn window_active(&self, active: bool) {
        if !active {
            self.capture_mouse(false);
        }
        if active {
            if self.focus_paused.replace(false) && !self.menu_open.get() {
                self.set_paused(false);
            }
        } else if self.pause_unfocused.get() && !self.paused.get() {
            self.focus_paused.set(true);
            self.set_paused(true);
        }
    }

    fn apply_prefs(&self, prefs: &Preferences) {
        self.pause_unfocused.set(prefs.pause_unfocused);
        self.popups.set(prefs.achievement_popups);
        self.corner.set(prefs.achievement_corner);
        self.apply_popups();
        self.volume.set(prefs.volume);
        for window in &self.windows {
            window.set_volume(i32::from(prefs.volume));
        }
        self.send(&AppMsg::Volume(prefs.volume));
    }
}

pub fn launch(
    opts: GameOptions,
    on_closed: impl Fn(Option<CoreIdentity>) + 'static,
    open_controllers: impl Fn() + 'static,
) -> anyhow::Result<RunningGame> {
    let hardcore = opts.achievements.as_ref().is_some_and(|a| a.hardcore);
    let tracks_achievements = opts.achievements.is_some();
    let cfg = SessionConfig {
        runner: paths::runner_exe()?,
        core: opts.core,
        rom: opts.rom,
        system_dir: paths::system_dir(),
        save_dir: opts.save_dir.clone(),
        jit: opts.jit,
        vulkan: opts.vulkan,
        load_slot: opts.load_slot,
        options: opts.options,
        volume: opts.prefs.volume,
        auto_state: opts.auto_state,
    };
    let session = Session::start(&cfg)?;

    let ui = GameWindow::new()?;
    ui.set_game_title(format!("{} — Romp", opts.title).into());
    ui.set_game_name(opts.title.clone().into());
    ui.set_mouse_mode(opts.mouse);
    ui.set_menu_key(if opts.computer { "F12" } else { "Esc" }.into());
    ui.set_has_achievements(tracks_achievements);
    ui.set_hardcore(hardcore);
    ui.set_achievements_label("Not started yet".into());
    {
        let mappings = opts.mappings.borrow();
        let label = |hotkey| crate::mapping::combo_label(&mappings.hotkey_key(hotkey)).into();
        ui.set_pause_key(label(Hotkey::Pause));
        ui.set_save_key(label(Hotkey::SaveState));
        ui.set_load_key(label(Hotkey::LoadState));
        ui.set_slot_key(label(Hotkey::NextSlot));
        ui.set_fullscreen_key(label(Hotkey::Fullscreen));
        ui.set_screenshot_key(label(Hotkey::Screenshot));
    }
    ui.set_has_menu(true);
    ui.set_status("Starting…".into());
    let mut windows = vec![ui];
    if opts.split_screens {
        let window = GameWindow::new()?;
        window.set_game_title(format!("{} — Touch screen", opts.title).into());
        windows.push(window);
    }
    for window in &windows {
        window.set_volume(i32::from(opts.prefs.volume));
    }

    let finished = Rc::new(Cell::new(false));
    let started: Rc<RefCell<Option<CoreIdentity>>> = Rc::default();
    let game = Rc::new_cyclic(|weak: &Weak<Game>| {
        let finish: Rc<dyn Fn()> = Rc::new({
            let weak = weak.clone();
            let started = started.clone();
            move || {
                if finished.replace(true) {
                    return;
                }
                if let Some(game) = weak.upgrade() {
                    game.write_auto_thumbnail();
                    game.save_placements();
                    game.capture_mouse(false);
                    game.session.borrow_mut().request_stop();
                    for window in &game.windows {
                        let _ = window.hide();
                    }
                }
                on_closed(started.borrow().clone());
            }
        });
        Game {
            session: RefCell::new(session),
            shading: windows.iter().map(shade).collect(),
            look: RefCell::new(opts.look),
            look_open: Cell::new(false),
            look_focus: Cell::new(0),
            windows,
            controls: RefCell::new(Controls::default()),
            paused: Cell::new(false),
            menu_open: Cell::new(false),
            focus_paused: Cell::new(false),
            pause_unfocused: Cell::new(opts.prefs.pause_unfocused),
            finish,
            open_controllers: Box::new(open_controllers),
            mappings: opts.mappings.clone(),
            menu_focus: Cell::new(-1),
            pad_buttons: Cell::new(0),
            menu_combo: Cell::new(false),
            mouse_captured: Cell::new(false),
            mouse_buttons: Cell::new(0),
            always_mouse: opts.mouse,
            port_options: RefCell::default(),
            port_devices: RefCell::default(),
            saved_ports: opts.port_devices.clone(),
            save_ports: opts.save_ports,
            volume: Cell::new(opts.prefs.volume),
            volume_changed: opts.volume_changed,
            popups: Cell::new(opts.prefs.achievement_popups),
            corner: Cell::new(opts.prefs.achievement_corner),
            popups_changed: opts.popups_changed,
            achievements_focus: Cell::new(0),
            save_placement: opts.save_placement,
            computer: opts.computer,
            held_keys: RefCell::default(),
            modifiers: Cell::new(0),
            rotation: Cell::new(0),
            aim: Cell::new((0, 0)),
            title: opts.title.clone(),
            screenshot_dir: opts.save_dir.join(crate::screenshot::FOLDER),
            screenshot_wanted: Cell::new(false),
            screenshots: std::sync::mpsc::channel(),
            screenshot_taken: opts.screenshot_taken,
            achievements: RefCell::new(
                opts.achievements
                    .map(crate::achievements::popups::Link::new),
            ),
            achievements_open: Cell::new(false),
            achievement_list: RefCell::default(),
            hardcore: Cell::new(hardcore),
            save_dir: opts.save_dir.clone(),
            slots_open: Cell::new(false),
            slots_focus: Cell::new(1),
            thumbnail_wanted: Cell::new(None),
            thumbnail_frame: Cell::new(0),
            recent_thumbnail: RefCell::new(None),
            thumbnails: std::sync::mpsc::channel(),
        }
    });
    let _ = game
        .controls
        .borrow_mut()
        .set_keyboard_player(opts.players.borrow().player(KEYBOARD));
    for (i, window) in game.windows.iter().enumerate() {
        wire(window, &game, i == 1);
    }
    game.apply_look();
    game.apply_popups();

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(4), {
        let weak = Rc::downgrade(&game);
        let mut buf = Vec::new();
        let mut last_seq = 0;
        let mut shown = None;
        let mut frame_count: u32 = 0;
        let gamepads = opts.gamepads.clone();
        let players = opts.players.clone();
        let mappings = opts.mappings.clone();
        let nintendo = opts.nintendo;
        let mut known: HashSet<String> = HashSet::new();
        let mut first_poll = true;
        let resumed = opts.load_slot == Some(0);
        let launched = std::time::Instant::now();
        let save_dir = opts.save_dir.clone();
        move || {
            let Some(game) = weak.upgrade() else { return };
            let ui = game.primary();
            let (inputs, states): (Vec<_>, Vec<_>) = {
                let mut pads = gamepads.borrow_mut();
                pads.poll();
                let connected = pads.connected();
                let keys: Vec<String> = connected.iter().map(|p| p.key.clone()).collect();
                let mut players = players.borrow_mut();
                for pad in &connected {
                    if !known.insert(pad.key.clone()) {
                        continue;
                    }
                    let player = players.connect(&pad.key, &keys);
                    if !first_poll {
                        flash(
                            ui,
                            match player {
                                Some(p) => format!("{} → Player {p}", pad.name),
                                None => format!(
                                    "{} connected. Choose its player in Settings.",
                                    pad.name
                                ),
                            },
                        );
                    }
                }
                known.retain(|k| keys.contains(k));
                first_poll = false;
                let inputs = pads.states(&mappings.borrow(), nintendo);
                let states = inputs
                    .iter()
                    .map(|p| (players.player(&p.key), p.state))
                    .collect();
                (inputs, states)
            };
            game.pads(&inputs);
            game.send_mouse_motion();
            if !game.paused.get() && !game.menu_open.get() {
                let keyboard = players.borrow().player(KEYBOARD);
                let mut commands = game.controls.borrow_mut().set_keyboard_player(keyboard);
                commands.extend(game.controls.borrow_mut().set_gamepads(states));
                game.run_commands(commands);
            }
            let events = {
                let session = game.session.borrow();
                if let Some(info) = session.frames.read_into(last_seq, &mut buf) {
                    last_seq = info.seq;
                    frame_count = frame_count.wrapping_add(1);
                    shown = Some((info.width, info.height, info.aspect));
                    match (
                        game.windows.get(1),
                        split_frame(&buf, info.width, info.height),
                    ) {
                        (Some(bottom), Some((top_half, bottom_half, half))) => {
                            show_frame(ui, &game.shading[0], top_half, info.width, half, 0.0);
                            show_frame(bottom, &game.shading[1], bottom_half, info.width, half, 0.0);
                        }
                        _ if game.rotation.get() % 4 != 0 => {
                            let turns = game.rotation.get();
                            let (rotated, w, h) =
                                crate::rotation::rotate(&buf, info.width, info.height, turns);
                            show_frame(ui, &game.shading[0], &rotated, w, h, info.aspect);
                        }
                        _ => show_frame(ui, &game.shading[0], &buf, info.width, info.height, info.aspect),
                    }
                }
                session.poll_events()
            };
            if game.screenshot_wanted.take() {
                game.capture(&buf, shown);
            }
            game.take_thumbnails(&buf, shown, frame_count);
            game.thumbnail_written();
            game.screenshot_saved();
            game.show_achievements(std::time::Instant::now());
            for event in events {
                if let SessionEvent::Runner(RunnerMsg::AchievementsRequest {
                    id,
                    url,
                    post,
                    content_type,
                    agent,
                }) = event
                {
                    if let Some(link) = game.achievements.borrow().as_ref() {
                        link.request(id, url, post, content_type, agent);
                    }
                    continue;
                }
                if let SessionEvent::Runner(RunnerMsg::Achievement(event)) = event {
                    if let Some(link) = game.achievements.borrow_mut().as_mut() {
                        link.event(&event);
                    }
                    if matches!(event, romp_proto::msg::AchievementEvent::HardcoreOff(_)) {
                        game.hardcore.set(false);
                        for window in &game.windows {
                            window.set_hardcore(false);
                        }
                    }
                    if matches!(
                        event,
                        romp_proto::msg::AchievementEvent::GameLoaded { .. }
                            | romp_proto::msg::AchievementEvent::Unlocked { .. }
                    ) {
                        game.send(&AppMsg::ListAchievements);
                    }
                    continue;
                }
                if let SessionEvent::Runner(RunnerMsg::StateWritten { ok: true, .. }) = &event {
                    if game.slots_open.get() {
                        game.show_slots();
                    }
                }
                if let SessionEvent::Runner(RunnerMsg::AchievementList(list)) = event {
                    game.achievement_list_arrived(list);
                    continue;
                }
                if let SessionEvent::Runner(RunnerMsg::Controllers { ports }) = event {
                    game.set_ports(ports);
                    continue;
                }
                if let SessionEvent::Runner(RunnerMsg::Rotation(turns)) = event {
                    tracing::debug!(turns, "rotation");
                    game.rotation.set(turns);
                    let tall = turns % 2 == 1;
                    let window = ui.window();
                    let size = window.size();
                    let window_tall = size.height > size.width;
                    if tall != window_tall && !window.is_fullscreen() {
                        let (w, h) = if tall { (600.0, 800.0) } else { (960.0, 720.0) };
                        window.set_size(slint::LogicalSize::new(w, h));
                    }
                    continue;
                }
                let started_now = matches!(event, SessionEvent::Runner(RunnerMsg::Started { .. }));
                let failed_resume = resumed
                    && launched.elapsed() < RESUME_GRACE
                    && matches!(event, SessionEvent::Ended { code, .. } if code != Some(0));
                handle_event(ui, event, &game.finish, &started);
                if failed_resume {
                    if let Err(e) = crate::saves::set_aside_auto_state(&save_dir) {
                        tracing::warn!("setting aside the automatic save: {e}");
                    }
                    ui.set_status(
                        "The game couldn't continue from where you left off, so that automatic save was set aside. Start the game again to play from the beginning or from a save slot.".into(),
                    );
                }
                if started_now {
                    let start = game
                        .achievements
                        .borrow()
                        .as_ref()
                        .map(crate::achievements::popups::Link::start_message);
                    if let Some(message) = start {
                        game.send(&message);
                    }
                }
                if started_now && game.computer {
                    flash(
                        ui,
                        "Your keyboard goes to the game. Press F12 for the menu.".into(),
                    );
                }
            }
        }
    });

    let placements = opts.placements;
    let saved = |i: usize| placements.get(i).copied().flatten();
    for (i, window) in game.windows.iter().enumerate() {
        if let Some(placement) = saved(i) {
            placement.apply(window.window());
        }
    }
    let ui = game.primary();
    ui.show()?;
    crate::scale::track(ui);
    if opts.prefs.fullscreen {
        ui.window().set_fullscreen(true);
        ui.set_fullscreen(true);
    }
    if let Some(window) = game.windows.get(1) {
        window.show()?;
        crate::scale::track(window);
        if saved(1).is_none() {
            let position = ui.window().position();
            let width = ui.window().size().width as i32;
            window.window().set_position(slint::PhysicalPosition::new(
                position.x + width + 16,
                position.y,
            ));
        }
    }
    Ok(RunningGame::Core(CoreGame {
        game,
        _timer: timer,
    }))
}

/// Draws `window`'s game through the shader with the window's own OpenGL, as the picture it shows.
fn shade(window: &GameWindow) -> Rc<RefCell<crate::shading::Shading>> {
    let shading = Rc::new(RefCell::new(crate::shading::Shading::default()));
    let state = shading.clone();
    let weak = window.as_weak();
    let installed = window
        .window()
        .set_rendering_notifier(move |rendering, api| {
            let mut state = state.borrow_mut();
            match rendering {
                slint::RenderingState::RenderingSetup => {
                    if let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = api {
                        state.setup(get_proc_address);
                    } else {
                        tracing::warn!("shaders need OpenGL, which this window doesn't draw with");
                        state.set_unavailable();
                        if let Some(window) = weak.upgrade() {
                            show_plain(&window, &state);
                        }
                    }
                }
                slint::RenderingState::BeforeRendering => {
                    let Some(window) = weak.upgrade() else { return };
                    if !state.active() {
                        return;
                    }
                    let size = window.window().size();
                    if let Some(drawn) = state.draw((size.width, size.height)) {
                        let texture = unsafe {
                            slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                                drawn.texture,
                                (drawn.width, drawn.height).into(),
                            )
                        }
                        .origin(slint::BorrowedOpenGLTextureOrigin::TopLeft)
                        .build();
                        window.set_frame(texture);
                    } else if !state.active() {
                        show_plain(&window, &state);
                    }
                    if state.settling() {
                        window.window().request_redraw();
                    }
                }
                slint::RenderingState::RenderingTeardown => state.teardown(),
                _ => {}
            }
        });
    if let Err(e) = installed {
        tracing::warn!("shaders are unavailable in this window: {e}");
        shading.borrow_mut().set_unavailable();
    }
    shading
}

/// Shows the game's last frame as it is, for when the shader stops drawing it.
fn show_plain(window: &GameWindow, shading: &crate::shading::Shading) {
    if let Some((rgba, width, height)) = shading.frame() {
        let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba, width, height);
        window.set_frame(Image::from_rgba8(pixels));
    }
}

fn show_frame(
    window: &GameWindow,
    shading: &RefCell<crate::shading::Shading>,
    rgba: &[u8],
    width: u32,
    height: u32,
    aspect: f32,
) {
    let aspect = if aspect > 0.0 {
        aspect
    } else {
        width as f32 / height.max(1) as f32
    };
    window.set_aspect(aspect);
    shading.borrow_mut().set_frame(rgba, width, height, aspect);
    if shading.borrow().active() {
        window.window().request_redraw();
        return;
    }
    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba, width, height);
    window.set_frame(Image::from_rgba8(pixels));
}

fn wire(window: &GameWindow, game: &Rc<Game>, bottom_half: bool) {
    let weak = Rc::downgrade(game);
    let with = move |f: &dyn Fn(&Game)| {
        if let Some(game) = weak.upgrade() {
            f(&game);
        }
    };
    let with = Rc::new(with);
    window.on_pointer({
        let with = with.clone();
        move |u, v, pressed| with(&|g| g.pointer(u, v, pressed, bottom_half))
    });
    window.window().on_close_requested({
        let with = with.clone();
        move || {
            with(&|g| (g.finish)());
            slint::CloseRequestResponse::HideWindow
        }
    });
    window.on_key_event({
        let with = with.clone();
        move |text, pressed, repeat, modifiers| {
            let mods = crate::mapping::Mods {
                ctrl: modifiers.control,
                alt: modifiers.alt,
                shift: modifiers.shift,
                meta: modifiers.meta,
            };
            let handled = Cell::new(false);
            with(&|g| handled.set(g.key(&text, mods, pressed, repeat)));
            handled.get()
        }
    });
    window.on_focus_lost({
        let with = with.clone();
        move || {
            with(&|g| {
                g.release_keys();
                let released = g.controls.borrow_mut().release_all();
                g.run_commands(released);
            });
        }
    });
    window.on_window_active({
        let with = with.clone();
        move |active| with(&|g| g.window_active(active))
    });
    window.on_capture_mouse({
        let with = with.clone();
        move || with(&|g| g.capture_mouse(true))
    });
    window.on_mouse_button({
        let with = with.clone();
        move |bit, pressed| with(&|g| g.mouse_button(bit, pressed))
    });
    window.on_cycle_port({
        let with = with.clone();
        move |row, step| {
            with(&|g| {
                let port = usize::try_from(row)
                    .ok()
                    .and_then(|r| g.pickable_ports().get(r).copied());
                if let Some(port) = port {
                    g.cycle_port(port, step);
                }
            });
        }
    });
    window.on_resume({
        let with = with.clone();
        move || with(&|g| g.set_menu(false))
    });
    window.on_toggle_pause({
        let with = with.clone();
        move || with(&|g| g.set_paused(!g.paused.get()))
    });
    window.on_restart({
        let with = with.clone();
        move || with(&|g| g.restart())
    });
    window.on_step_volume({
        let with = with.clone();
        move |delta| with(&|g| g.step_volume(delta))
    });
    window.on_toggle_fullscreen({
        let with = with.clone();
        move || with(&|g| g.toggle_fullscreen())
    });
    window.on_take_screenshot({
        let with = with.clone();
        move || with(&|g| g.screenshot_wanted.set(true))
    });
    window.on_open_look({
        let with = with.clone();
        move || with(&|g| g.open_look())
    });
    window.on_close_look({
        let with = with.clone();
        move || with(&|g| g.close_look())
    });
    window.on_step_look({
        let with = with.clone();
        move |row, delta| with(&|g| g.step_look(row, delta))
    });
    window.on_open_slots({
        let with = with.clone();
        move || with(&|g| g.open_slots())
    });
    window.on_close_slots({
        let with = with.clone();
        move || with(&|g| g.close_slots())
    });
    window.on_save_slot({
        let with = with.clone();
        move |slot| with(&|g| g.save_slot(u8::try_from(slot).unwrap_or(1)))
    });
    window.on_load_slot({
        let with = with.clone();
        move |slot| with(&|g| g.load_from(u8::try_from(slot).unwrap_or(1)))
    });
    window.on_open_achievements({
        let with = with.clone();
        move || with(&|g| g.open_achievements())
    });
    window.on_close_achievements({
        let with = with.clone();
        move || with(&|g| g.close_achievements())
    });
    window.on_step_popups({
        let with = with.clone();
        move |delta| with(&|g| g.step_popups(delta))
    });
    window.on_step_corner({
        let with = with.clone();
        move |delta| with(&|g| g.step_corner(delta))
    });
    window.on_open_controllers({
        let with = with.clone();
        move || with(&|g| (g.open_controllers)())
    });
    window.on_quit({
        let with = with.clone();
        move || with(&|g| (g.finish)())
    });
}

fn handle_event(
    ui: &GameWindow,
    event: SessionEvent,
    finish: &Rc<dyn Fn()>,
    started: &RefCell<Option<CoreIdentity>>,
) {
    if let SessionEvent::Ended { code, .. } = &event {
        tracing::info!(?code, "emulator exited");
    }
    match event {
        SessionEvent::Runner(RunnerMsg::Started {
            core_name,
            core_version,
            ..
        }) => {
            tracing::info!("running {core_name} {core_version}");
            ui.set_status("".into());
            *started.borrow_mut() = Some((core_name, core_version));
        }
        SessionEvent::Runner(RunnerMsg::StateWritten { slot, ok }) => flash(
            ui,
            if ok {
                format!("Saved to slot {slot}")
            } else {
                format!("Could not save slot {slot}")
            },
        ),
        SessionEvent::Runner(RunnerMsg::StateLoaded { slot, ok }) => flash(
            ui,
            if ok {
                format!("Loaded slot {slot}")
            } else {
                format!("Slot {slot} is empty or unreadable")
            },
        ),
        SessionEvent::Runner(RunnerMsg::Exited { error: Some(error) }) => {
            ui.set_status(error.into());
        }
        SessionEvent::Runner(_) => {}
        SessionEvent::Ended { code: Some(0), .. } => finish(),
        SessionEvent::Ended { code, log_tail } => {
            let code = code.map_or("a signal".to_string(), |c| format!("code {c}"));
            let log = log_tail
                .iter()
                .rev()
                .take(8)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            ui.set_status(format!("The emulator stopped ({code}).\n{log}").into());
        }
    }
}

fn flash(ui: &GameWindow, text: String) {
    ui.set_status(text.into());
    let ui = ui.as_weak();
    Timer::single_shot(Duration::from_secs(2), move || {
        if let Some(ui) = ui.upgrade() {
            ui.set_status("".into());
        }
    });
}

pub fn split_frame(rgba: &[u8], width: u32, height: u32) -> Option<(&[u8], &[u8], u32)> {
    let half = height / 2;
    let split = (width * half * 4) as usize;
    if half == 0 || rgba.len() < split * 2 {
        return None;
    }
    let (top, bottom) = rgba.split_at(split);
    Some((top, &bottom[..split], half))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_or_control_w_closes_the_game() {
        let mods = |ctrl, alt, shift, meta| crate::mapping::Mods {
            ctrl,
            alt,
            shift,
            meta,
        };
        assert!(closes_window("w", mods(true, false, false, false)));
        assert!(closes_window("W", mods(true, false, true, false)));
        assert!(closes_window("\u{17}", mods(true, false, false, false)));
        assert!(
            !closes_window("w", mods(false, false, false, false)),
            "plain W is a game key"
        );
        assert!(
            !closes_window("w", mods(false, false, false, true)),
            "Control-W on a Mac"
        );
        assert!(!closes_window("w", mods(true, true, false, false)));
        assert!(!closes_window("s", mods(true, false, false, false)));
    }

    #[test]
    fn menu_focus_moves_across_the_button_row_and_down_the_list() {
        let items = MENU_PORTS_START + 1;
        assert_eq!(menu_move(-1, input::DOWN, items), Some(MENU_RESUME));
        assert_eq!(
            menu_move(MENU_RESUME, input::RIGHT, items),
            Some(MENU_PAUSE)
        );
        assert_eq!(
            menu_move(MENU_PAUSE, input::RIGHT, items),
            Some(MENU_RESTART)
        );
        assert_eq!(
            menu_move(MENU_RESTART, input::RIGHT, items),
            Some(MENU_RESTART)
        );
        assert_eq!(
            menu_move(MENU_RESTART, input::LEFT, items),
            Some(MENU_PAUSE)
        );
        assert_eq!(
            menu_move(MENU_RESUME, input::LEFT, items),
            Some(MENU_RESUME)
        );
        assert_eq!(menu_move(MENU_RESUME, input::UP, items), Some(MENU_RESUME));
        assert_eq!(
            menu_move(MENU_RESTART, input::DOWN, items),
            Some(MENU_SAVE_STATES)
        );
        assert_eq!(
            menu_move(MENU_SAVE_STATES, input::UP, items),
            Some(MENU_RESUME)
        );
        assert_eq!(
            menu_move(MENU_SAVE_STATES, input::DOWN, items),
            Some(MENU_LOOK)
        );
        assert_eq!(menu_move(MENU_VOLUME, input::UP, items), Some(MENU_LOOK));
        assert_eq!(
            menu_move(MENU_LOOK, input::UP, items),
            Some(MENU_SAVE_STATES)
        );
        assert_eq!(menu_move(items - 1, input::DOWN, items), Some(items - 1));
        assert_eq!(menu_move(MENU_VOLUME, input::LEFT, items), None);
    }

    #[test]
    fn slot_focus_moves_around_the_grid_and_stops_at_its_edges() {
        assert_eq!(slot_grid_move(1, input::RIGHT), 2);
        assert_eq!(slot_grid_move(2, input::RIGHT), 2);
        assert_eq!(slot_grid_move(2, input::DOWN), 4);
        assert_eq!(slot_grid_move(4, input::LEFT), 3);
        assert_eq!(slot_grid_move(3, input::LEFT), 3);
        assert_eq!(slot_grid_move(3, input::UP), 1);
        assert_eq!(slot_grid_move(2, input::UP), 2);
        assert_eq!(slot_grid_move(4, input::DOWN), 4);
    }

    #[test]
    fn volume_steps_in_tens_and_stays_in_range() {
        assert_eq!(stepped_volume(70, 1), 80);
        assert_eq!(stepped_volume(70, -1), 60);
        assert_eq!(stepped_volume(95, 1), 100);
        assert_eq!(stepped_volume(100, 1), 100);
        assert_eq!(stepped_volume(5, -1), 0);
        assert_eq!(stepped_volume(73, 1), 80, "lands on the next step");
        assert_eq!(stepped_volume(73, -1), 70);
    }

    #[test]
    fn stacked_screens_split_at_half_height() {
        let (w, h) = (2u32, 4u32);
        let rgba: Vec<u8> = (0..(w * h * 4) as u8).collect();
        let (top, bottom, half) = split_frame(&rgba, w, h).unwrap();
        assert_eq!(half, 2);
        assert_eq!(top, &rgba[..16]);
        assert_eq!(bottom, &rgba[16..]);
        assert!(split_frame(&rgba[..4], 1, 1).is_none());
    }
}
