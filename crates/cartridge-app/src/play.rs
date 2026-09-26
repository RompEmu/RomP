use crate::gamepads::Gamepads;
use crate::input::{self, map_key, Command, Controls, KeyAction};
use crate::paths;
use crate::players::{Assignments, KEYBOARD};
use crate::prefs::Preferences;
use crate::session::{Session, SessionConfig, SessionEvent};
use crate::GameWindow;
use anyhow::Context;
use cartridge_proto::msg::{AppMsg, RunnerMsg};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

pub struct GameOptions {
    pub core: PathBuf,
    pub rom: PathBuf,
    pub save_dir: PathBuf,
    pub title: String,
    pub jit: bool,
    pub options: Vec<(String, String)>,
    pub split_screens: bool,
    pub gamepads: Rc<RefCell<Gamepads>>,
    pub players: Rc<RefCell<Assignments>>,
    pub prefs: Preferences,
    pub load_slot: Option<u8>,
}

pub struct RunningGame {
    game: Rc<Game>,
    _timer: Timer,
}

impl RunningGame {
    pub fn wait_exit(&self, timeout: Duration) -> bool {
        self.game.session.borrow().wait_exit(timeout)
    }

    pub fn apply_prefs(&self, prefs: &Preferences) {
        self.game.apply_prefs(prefs);
    }
}

pub fn run(core: PathBuf, rom: PathBuf, jit: bool) -> anyhow::Result<()> {
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
            core,
            rom,
            save_dir,
            title,
            jit,
            options: Vec::new(),
            split_screens: false,
            gamepads: Rc::new(RefCell::new(Gamepads::new())),
            players: Rc::default(),
            prefs: Preferences::default(),
            load_slot: None,
        },
        |_| {},
        || {},
    )?;
    slint::run_event_loop()?;
    game.wait_exit(Duration::from_secs(4));
    Ok(())
}

pub type CoreIdentity = (String, String);

struct Game {
    session: RefCell<Session>,
    windows: Vec<GameWindow>,
    controls: RefCell<Controls>,
    paused: Cell<bool>,
    menu_open: Cell<bool>,
    focus_paused: Cell<bool>,
    pause_unfocused: Cell<bool>,
    finish: Rc<dyn Fn()>,
    open_controllers: Box<dyn Fn()>,
}

impl Game {
    fn primary(&self) -> &GameWindow {
        &self.windows[0]
    }

    fn send(&self, msg: &AppMsg) {
        self.session.borrow_mut().send(msg);
    }

    fn run_commands(&self, commands: Vec<Command>) {
        for command in commands {
            match command {
                Command::Send(msg) => self.send(&msg),
                Command::Menu => self.set_menu(!self.menu_open.get()),
                Command::TogglePause => self.set_paused(!self.paused.get()),
                Command::ToggleFullscreen => self.toggle_fullscreen(),
                Command::SlotChanged(slot) => self.show_slot(slot),
            }
        }
    }

    fn set_paused(&self, paused: bool) {
        if paused {
            let released = self.controls.borrow_mut().release_all();
            self.run_commands(released);
        }
        self.paused.set(paused);
        self.send(&AppMsg::Pause(paused));
        for window in &self.windows {
            window.set_paused(paused);
        }
    }

    fn set_menu(&self, open: bool) {
        self.menu_open.set(open);
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

    fn show_slot(&self, slot: u8) {
        self.primary().set_slot(i32::from(slot));
        flash(self.primary(), format!("Save slot {slot}"));
    }

    fn step_slot(&self, delta: i32) {
        let slots = i32::from(input::SLOTS);
        let current = i32::from(self.controls.borrow().slot());
        let slot = (current - 1 + delta).rem_euclid(slots) + 1;
        self.controls.borrow_mut().set_slot(slot as u8);
        self.primary().set_slot(slot);
    }

    fn key(&self, text: &str, pressed: bool, repeat: bool) -> bool {
        let action = map_key(text);
        if self.menu_open.get() && matches!(action, Some(KeyAction::Button(_))) {
            return true;
        }
        let result = self.controls.borrow_mut().key(text, pressed, repeat);
        match result {
            None => false,
            Some(commands) => {
                self.run_commands(commands);
                true
            }
        }
    }

    fn window_active(&self, active: bool) {
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
        for window in &self.windows {
            window.set_sharp(prefs.sharp_pixels);
        }
        self.send(&AppMsg::Volume(prefs.volume));
    }
}

pub fn launch(
    opts: GameOptions,
    on_closed: impl Fn(Option<CoreIdentity>) + 'static,
    open_controllers: impl Fn() + 'static,
) -> anyhow::Result<RunningGame> {
    let cfg = SessionConfig {
        runner: paths::runner_exe()?,
        core: opts.core,
        rom: opts.rom,
        system_dir: paths::system_dir(),
        save_dir: opts.save_dir,
        jit: opts.jit,
        load_slot: opts.load_slot,
        options: opts.options,
        volume: opts.prefs.volume,
    };
    let session = Session::start(&cfg)?;

    let ui = GameWindow::new()?;
    ui.set_game_title(format!("{} — Cartridge", opts.title).into());
    ui.set_game_name(opts.title.clone().into());
    ui.set_has_menu(true);
    ui.set_status("Starting…".into());
    let mut windows = vec![ui];
    if opts.split_screens {
        let window = GameWindow::new()?;
        window.set_game_title(format!("{} — Touch screen", opts.title).into());
        windows.push(window);
    }
    for window in &windows {
        window.set_sharp(opts.prefs.sharp_pixels);
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
            windows,
            controls: RefCell::new(Controls::default()),
            paused: Cell::new(false),
            menu_open: Cell::new(false),
            focus_paused: Cell::new(false),
            pause_unfocused: Cell::new(opts.prefs.pause_unfocused),
            finish,
            open_controllers: Box::new(open_controllers),
        }
    });
    let _ = game
        .controls
        .borrow_mut()
        .set_keyboard_player(opts.players.borrow().player(KEYBOARD));
    for (i, window) in game.windows.iter().enumerate() {
        wire(window, &game, i == 1);
    }

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(4), {
        let weak = Rc::downgrade(&game);
        let mut buf = Vec::new();
        let mut last_seq = 0;
        let gamepads = opts.gamepads.clone();
        let players = opts.players.clone();
        let mut known: HashSet<String> = HashSet::new();
        let mut first_poll = true;
        move || {
            let Some(game) = weak.upgrade() else { return };
            let ui = game.primary();
            let states: Vec<_> = {
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
                pads.states()
                    .into_iter()
                    .map(|(key, state)| (players.player(&key), state))
                    .collect()
            };
            if !game.paused.get() {
                let keyboard = players.borrow().player(KEYBOARD);
                let mut commands = game.controls.borrow_mut().set_keyboard_player(keyboard);
                commands.extend(game.controls.borrow_mut().set_gamepads(states));
                game.run_commands(commands);
            }
            let events = {
                let session = game.session.borrow();
                if let Some(info) = session.frames.read_into(last_seq, &mut buf) {
                    last_seq = info.seq;
                    match (
                        game.windows.get(1),
                        split_frame(&buf, info.width, info.height),
                    ) {
                        (Some(bottom), Some((top_half, bottom_half, half))) => {
                            show_frame(ui, top_half, info.width, half, 0.0);
                            show_frame(bottom, bottom_half, info.width, half, 0.0);
                        }
                        _ => show_frame(ui, &buf, info.width, info.height, info.aspect),
                    }
                }
                session.poll_events()
            };
            for event in events {
                handle_event(ui, event, &game.finish, &started);
            }
        }
    });

    let ui = game.primary();
    ui.show()?;
    if opts.prefs.fullscreen {
        ui.window().set_fullscreen(true);
        ui.set_fullscreen(true);
    }
    if let Some(window) = game.windows.get(1) {
        window.show()?;
        let position = ui.window().position();
        let width = ui.window().size().width as i32;
        window.window().set_position(slint::PhysicalPosition::new(
            position.x + width + 16,
            position.y,
        ));
    }
    Ok(RunningGame {
        game,
        _timer: timer,
    })
}

fn show_frame(window: &GameWindow, rgba: &[u8], width: u32, height: u32, aspect: f32) {
    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba, width, height);
    window.set_frame(Image::from_rgba8(pixels));
    let aspect = if aspect > 0.0 {
        aspect
    } else {
        width as f32 / height.max(1) as f32
    };
    window.set_aspect(aspect);
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
        move |u, v, pressed| {
            let (x, y) = input::pointer_coords(u, v, bottom_half);
            with(&|g| g.send(&AppMsg::Pointer { x, y, pressed }));
        }
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
        move |text, pressed, repeat| {
            let handled = Cell::new(false);
            with(&|g| handled.set(g.key(&text, pressed, repeat)));
            handled.get()
        }
    });
    window.on_focus_lost({
        let with = with.clone();
        move || {
            with(&|g| {
                let released = g.controls.borrow_mut().release_all();
                g.run_commands(released);
            })
        }
    });
    window.on_window_active({
        let with = with.clone();
        move |active| with(&|g| g.window_active(active))
    });
    window.on_resume({
        let with = with.clone();
        move || with(&|g| g.set_menu(false))
    });
    window.on_toggle_pause({
        let with = with.clone();
        move || with(&|g| g.set_paused(!g.paused.get()))
    });
    window.on_save_state({
        let with = with.clone();
        move || with(&|g| g.send(&AppMsg::SaveSlot(g.controls.borrow().slot())))
    });
    window.on_load_state({
        let with = with.clone();
        move || with(&|g| g.send(&AppMsg::LoadSlot(g.controls.borrow().slot())))
    });
    window.on_step_slot({
        let with = with.clone();
        move |delta| with(&|g| g.step_slot(delta))
    });
    window.on_toggle_fullscreen({
        let with = with.clone();
        move || with(&|g| g.toggle_fullscreen())
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
            ui.set_status(error.into())
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
