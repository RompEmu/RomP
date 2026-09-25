use crate::input::{Command, Controls};
use crate::paths;
use crate::session::{Session, SessionConfig, SessionEvent};
use crate::GameWindow;
use anyhow::Context;
use cartridge_proto::msg::RunnerMsg;
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
        },
        || {},
    )?;
    slint::run_event_loop()?;
    game.wait_exit(Duration::from_secs(4));
    Ok(())
}

pub fn launch(opts: GameOptions, on_closed: impl Fn() + 'static) -> anyhow::Result<RunningGame> {
    let cfg = SessionConfig {
        runner: paths::runner_exe()?,
        core: opts.core,
        rom: opts.rom,
        system_dir: paths::system_dir(),
        save_dir: opts.save_dir,
        jit: opts.jit,
        load_slot: None,
    };
    let session = Rc::new(RefCell::new(Session::start(&cfg)?));

    let ui = GameWindow::new()?;
    ui.set_game_title(format!("{} — Cartridge", opts.title).into());
    ui.set_status("Starting…".into());

    let finished = Rc::new(Cell::new(false));
    let finish: Rc<dyn Fn()> = Rc::new({
        let session = session.clone();
        let ui = ui.as_weak();
        move || {
            if finished.replace(true) {
                return;
            }
            session.borrow_mut().request_stop();
            if let Some(ui) = ui.upgrade() {
                let _ = ui.hide();
            }
            on_closed();
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
        move || {
            let Some(ui) = ui.upgrade() else { return };
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
                handle_event(&ui, event, &finish);
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

fn apply(session: &RefCell<Session>, command: Command, finish: &Rc<dyn Fn()>) {
    match command {
        Command::Send(msg) => session.borrow_mut().send(&msg),
        Command::Quit => finish(),
    }
}

fn handle_event(ui: &GameWindow, event: SessionEvent, finish: &Rc<dyn Fn()>) {
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
