use crate::gamepads::Gamepads;
use crate::input::{self, map_key, Command, Controls, KeyAction};
use crate::mapping::Mappings;
use crate::paths;
use crate::players::{Assignments, KEYBOARD};
use crate::ports;
use crate::prefs::Preferences;
use crate::restore::{Observed, Placement};
use crate::session::{Session, SessionConfig, SessionEvent};
use crate::GameWindow;
use crate::PortRow;
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
pub type SavePlacement = Box<dyn Fn(usize, &Observed)>;

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
    pub auto_state: bool,
    pub mappings: Rc<RefCell<Mappings>>,
    pub nintendo: bool,
    pub mouse: bool,
    pub computer: bool,
    pub port_devices: Vec<(u8, u32)>,
    pub save_ports: SavePorts,
    pub volume_changed: VolumeChanged,
    pub placements: Vec<Option<Placement>>,
    pub save_placement: SavePlacement,
}

pub struct CoreGame {
    game: Rc<Game>,
    _timer: Timer,
}

pub enum RunningGame {
    Core(CoreGame),
    External(crate::xemu::Running),
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
            auto_state: true,
            mappings: Rc::default(),
            nintendo: false,
            mouse: false,
            computer: false,
            port_devices: Vec::new(),
            save_ports: Box::new(|_| {}),
            volume_changed: Box::new(|_| {}),
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
    save_placement: SavePlacement,
    computer: bool,
    held_keys: RefCell<HashSet<u32>>,
    modifiers: Cell<u16>,
    rotation: Cell<u8>,
    aim: Cell<(i16, i16)>,
}

const MENU_RESUME: i32 = 0;
const MENU_PAUSE: i32 = 1;
const MENU_RESTART: i32 = 2;
const MENU_SLOT: i32 = 3;
const MENU_SAVE: i32 = 4;
const MENU_LOAD: i32 = 5;
const MENU_VOLUME: i32 = 6;
const MENU_FULLSCREEN: i32 = 7;
const MENU_CONTROLLERS: i32 = 8;
const MENU_PORTS_START: i32 = 9;
const VOLUME_STEP: i32 = 10;

const MENU_ROWS: [[i32; 3]; 2] = [
    [MENU_RESUME, MENU_PAUSE, MENU_RESTART],
    [MENU_SLOT, MENU_SAVE, MENU_LOAD],
];

fn menu_move(focus: i32, button: u32, items: i32) -> Option<i32> {
    let directional = matches!(button, input::UP | input::DOWN | input::LEFT | input::RIGHT);
    if focus < 0 {
        return directional.then_some(MENU_RESUME);
    }
    let cell = MENU_ROWS
        .iter()
        .enumerate()
        .find_map(|(r, row)| row.iter().position(|&i| i == focus).map(|c| (r, c)));
    match (button, cell) {
        (input::LEFT, Some((r, c))) => Some(MENU_ROWS[r][c.saturating_sub(1)]),
        (input::RIGHT, Some((r, c))) => Some(MENU_ROWS[r][(c + 1).min(2)]),
        (input::UP, Some((0, _))) => Some(focus),
        (input::UP, Some((r, c))) => Some(MENU_ROWS[r - 1][c]),
        (input::DOWN, Some((0, c))) => Some(MENU_ROWS[1][c]),
        (input::DOWN, Some(_)) => Some(MENU_VOLUME),
        (input::UP, None) if focus == MENU_VOLUME => Some(MENU_SLOT),
        (input::UP, None) => Some((focus - 1).max(0)),
        (input::DOWN, None) => Some((focus + 1).min(items - 1)),
        _ => None,
    }
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
                Command::Send(msg) => self.send(&msg),
                Command::Menu => self.set_menu(!self.menu_open.get()),
                Command::TogglePause => self.set_paused(!self.paused.get()),
                Command::ToggleFullscreen => self.toggle_fullscreen(),
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
            MENU_SLOT => self.step_slot(1),
            MENU_SAVE => self.send(&AppMsg::SaveSlot(self.controls.borrow().slot())),
            MENU_LOAD => self.send(&AppMsg::LoadSlot(self.controls.borrow().slot())),
            MENU_VOLUME => self.step_volume(1),
            MENU_FULLSCREEN => self.toggle_fullscreen(),
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
            self.set_menu(!self.menu_open.get());
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

    fn key(&self, text: &str, pressed: bool, repeat: bool) -> bool {
        if self.computer {
            let menu_key = text.chars().eq([char::from(slint::platform::Key::F12)]);
            if menu_key {
                if pressed && !repeat {
                    self.set_menu(!self.menu_open.get());
                }
                return true;
            }
            if !self.menu_open.get() {
                return self.paused.get() || self.computer_key(text, pressed, repeat);
            }
        }
        let mappings = self.mappings.borrow();
        let action = map_key(text, &mappings);
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
            .key(text, pressed, repeat, &mappings);
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
        self.volume.set(prefs.volume);
        for window in &self.windows {
            window.set_sharp(prefs.sharp_pixels);
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
    let cfg = SessionConfig {
        runner: paths::runner_exe()?,
        core: opts.core,
        rom: opts.rom,
        system_dir: paths::system_dir(),
        save_dir: opts.save_dir.clone(),
        jit: opts.jit,
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
            save_placement: opts.save_placement,
            computer: opts.computer,
            held_keys: RefCell::default(),
            modifiers: Cell::new(0),
            rotation: Cell::new(0),
            aim: Cell::new((0, 0)),
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
                    match (
                        game.windows.get(1),
                        split_frame(&buf, info.width, info.height),
                    ) {
                        (Some(bottom), Some((top_half, bottom_half, half))) => {
                            show_frame(ui, top_half, info.width, half, 0.0);
                            show_frame(bottom, bottom_half, info.width, half, 0.0);
                        }
                        _ if game.rotation.get() % 4 != 0 => {
                            let turns = game.rotation.get();
                            let (rotated, w, h) =
                                crate::rotation::rotate(&buf, info.width, info.height, turns);
                            show_frame(ui, &rotated, w, h, info.aspect);
                        }
                        _ => show_frame(ui, &buf, info.width, info.height, info.aspect),
                    }
                }
                session.poll_events()
            };
            for event in events {
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
                g.release_keys();
                let released = g.controls.borrow_mut().release_all();
                g.run_commands(released);
            })
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
            })
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
    fn menu_focus_moves_across_the_two_button_rows_and_down_the_list() {
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
        assert_eq!(menu_move(MENU_RESUME, input::DOWN, items), Some(MENU_SLOT));
        assert_eq!(menu_move(MENU_RESTART, input::DOWN, items), Some(MENU_LOAD));
        assert_eq!(menu_move(MENU_SLOT, input::RIGHT, items), Some(MENU_SAVE));
        assert_eq!(menu_move(MENU_SAVE, input::RIGHT, items), Some(MENU_LOAD));
        assert_eq!(menu_move(MENU_SAVE, input::LEFT, items), Some(MENU_SLOT));
        assert_eq!(menu_move(MENU_SAVE, input::UP, items), Some(MENU_PAUSE));
        assert_eq!(menu_move(MENU_LOAD, input::DOWN, items), Some(MENU_VOLUME));
        assert_eq!(menu_move(MENU_VOLUME, input::UP, items), Some(MENU_SLOT));
        assert_eq!(menu_move(MENU_RESUME, input::UP, items), Some(MENU_RESUME));
        assert_eq!(menu_move(items - 1, input::DOWN, items), Some(items - 1));
        assert_eq!(menu_move(MENU_VOLUME, input::LEFT, items), None);
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
