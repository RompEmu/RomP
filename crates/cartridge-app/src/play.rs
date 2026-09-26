use crate::input::{self, Command, Controls};
use crate::paths;
use crate::session::{Session, SessionConfig, SessionEvent};
use crate::GameWindow;
use anyhow::Context;
use cartridge_proto::msg::{PadState, RunnerMsg};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub struct GameOptions {
    pub core: PathBuf,
    pub rom: PathBuf,
    pub save_dir: PathBuf,
    pub title: String,
    pub jit: bool,
    pub options: Vec<(String, String)>,
}

pub struct RunningGame {
    _ui: GameWindow,
    _timer: Timer,
    session: Rc<RefCell<Session>>,
}

impl RunningGame {
    pub fn wait_exit(&self, timeout: Duration) -> bool {
        self.session.borrow().wait_exit(timeout)
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
        },
        |_| {},
    )?;
    slint::run_event_loop()?;
    game.wait_exit(Duration::from_secs(4));
    Ok(())
}

pub type CoreIdentity = (String, String);

pub fn launch(
    opts: GameOptions,
    on_closed: impl Fn(Option<CoreIdentity>) + 'static,
) -> anyhow::Result<RunningGame> {
    let cfg = SessionConfig {
        runner: paths::runner_exe()?,
        core: opts.core,
        rom: opts.rom,
        system_dir: paths::system_dir(),
        save_dir: opts.save_dir,
        jit: opts.jit,
        load_slot: None,
        options: opts.options,
    };
    let session = Rc::new(RefCell::new(Session::start(&cfg)?));

    let ui = GameWindow::new()?;
    ui.set_game_title(format!("{} — Cartridge", opts.title).into());
    ui.set_status("Starting…".into());

    let finished = Rc::new(Cell::new(false));
    let started: Rc<RefCell<Option<CoreIdentity>>> = Rc::default();
    let finish: Rc<dyn Fn()> = Rc::new({
        let session = session.clone();
        let ui = ui.as_weak();
        let started = started.clone();
        move || {
            if finished.replace(true) {
                return;
            }
            session.borrow_mut().request_stop();
            if let Some(ui) = ui.upgrade() {
                let _ = ui.hide();
            }
            on_closed(started.borrow().clone());
        }
    });
    ui.window().on_close_requested({
        let finish = finish.clone();
        move || {
            finish();
            slint::CloseRequestResponse::HideWindow
        }
    });

    let controls = Rc::new(RefCell::new(Controls::default()));
    ui.on_key_event({
        let session = session.clone();
        let controls = controls.clone();
        let finish = finish.clone();
        move |text, pressed, repeat| {
            let result = controls.borrow_mut().key(&text, pressed, repeat);
            match result {
                None => false,
                Some(command) => {
                    if let Some(command) = command {
                        apply(&session, command, &finish);
                    }
                    true
                }
            }
        }
    });
    ui.on_focus_lost({
        let session = session.clone();
        let finish = finish.clone();
        let controls = controls.clone();
        move || {
            let command = controls.borrow_mut().release_all();
            if let Some(command) = command {
                apply(&session, command, &finish);
            }
        }
    });

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(4), {
        let ui = ui.as_weak();
        let session = session.clone();
        let mut buf = Vec::new();
        let mut last_seq = 0;
        let mut gilrs = gilrs::Gilrs::new().ok();
        let finish = finish.clone();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            if let Some(gilrs) = gilrs.as_mut() {
                let command = controls.borrow_mut().set_gamepad(read_gamepad(gilrs));
                if let Some(command) = command {
                    apply(&session, command, &finish);
                }
            }
            let events = {
                let session = session.borrow();
                if let Some(info) = session.frames.read_into(last_seq, &mut buf) {
                    last_seq = info.seq;
                    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                        &buf,
                        info.width,
                        info.height,
                    );
                    ui.set_frame(Image::from_rgba8(pixels));
                    let aspect = if info.aspect > 0.0 {
                        info.aspect
                    } else {
                        info.width as f32 / info.height as f32
                    };
                    ui.set_aspect(aspect);
                }
                session.poll_events()
            };
            for event in events {
                handle_event(&ui, event, &finish, &started);
            }
        }
    });

    ui.show()?;
    Ok(RunningGame {
        _ui: ui,
        _timer: timer,
        session,
    })
}

fn read_gamepad(gilrs: &mut gilrs::Gilrs) -> PadState {
    use gilrs::{Axis, Button};
    while gilrs.next_event().is_some() {}
    let Some((_, pad)) = gilrs.gamepads().find(|(_, g)| g.is_connected()) else {
        return PadState::default();
    };
    let mut state = PadState::default();
    for button in [
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::LeftTrigger,
        Button::RightTrigger,
        Button::LeftTrigger2,
        Button::RightTrigger2,
        Button::Select,
        Button::Start,
        Button::DPadUp,
        Button::DPadDown,
        Button::DPadLeft,
        Button::DPadRight,
        Button::LeftThumb,
        Button::RightThumb,
    ] {
        if pad.is_pressed(button) {
            if let Some(bit) = input::retro_button(button) {
                state.buttons |= 1 << bit;
            }
        }
    }
    let trigger = |b: Button| (pad.button_data(b).map_or(0.0, |d| d.value()) * 32767.0) as i16;
    state.axes = [
        input::stick(pad.value(Axis::LeftStickX), false),
        input::stick(pad.value(Axis::LeftStickY), true),
        input::stick(pad.value(Axis::RightStickX), false),
        input::stick(pad.value(Axis::RightStickY), true),
        trigger(Button::LeftTrigger2),
        trigger(Button::RightTrigger2),
    ];
    state
}

fn apply(session: &RefCell<Session>, command: Command, finish: &Rc<dyn Fn()>) {
    match command {
        Command::Send(msg) => session.borrow_mut().send(&msg),
        Command::Quit => finish(),
    }
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
