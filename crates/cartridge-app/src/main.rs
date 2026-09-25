#![allow(dead_code)]
mod covers;
mod credentials;
mod grid;
mod identity;
mod input;
mod paths;
mod qr;
mod romm;
mod session;
mod store;
mod sync;

use anyhow::Context;
use cartridge_proto::msg::RunnerMsg;
use clap::Parser;
use input::{Command, Controls};
use session::{Session, SessionConfig, SessionEvent};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

#[derive(Parser)]
#[command(name = "cartridge")]
struct Args {
    #[arg(long)]
    core: PathBuf,
    #[arg(long)]
    rom: PathBuf,
    #[arg(long)]
    jit: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    let data = paths::data_dir();
    let rom = args
        .rom
        .canonicalize()
        .with_context(|| format!("ROM not found: {}", args.rom.display()))?;
    let name = rom
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cfg = SessionConfig {
        runner: paths::runner_exe()?,
        core: args.core,
        save_dir: data.join("saves").join(paths::local_save_dir_name(&rom)),
        rom,
        system_dir: data.join("system"),
        jit: args.jit,
        load_slot: None,
    };
    let session = Rc::new(RefCell::new(Session::start(&cfg)?));

    let ui = GameWindow::new()?;
    ui.set_game_title(format!("{name} — Cartridge").into());

    ui.set_status("Starting…".into());

    let controls = Rc::new(RefCell::new(Controls::default()));
    ui.on_key_event({
        let session = session.clone();
        let controls = controls.clone();
        move |text, pressed, repeat| match controls.borrow_mut().key(&text, pressed, repeat) {
            None => false,
            Some(command) => {
                if let Some(command) = command {
                    run(&mut session.borrow_mut(), command);
                }
                true
            }
        }
    });
    ui.on_focus_lost({
        let session = session.clone();
        move || {
            if let Some(command) = controls.borrow_mut().release_all() {
                run(&mut session.borrow_mut(), command);
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
            for event in session.poll_events() {
                handle_event(&ui, event);
            }
        }
    });

    ui.run()?;
    drop(timer);
    let mut session = session.borrow_mut();
    session.request_stop();
    session.wait_exit(Duration::from_secs(4));
    Ok(())
}

fn run(session: &mut Session, command: Command) {
    match command {
        Command::Send(msg) => session.send(&msg),
        Command::Quit => {
            let _ = slint::quit_event_loop();
        }
    }
}

fn handle_event(ui: &GameWindow, event: SessionEvent) {
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
        SessionEvent::Ended { code: Some(0), .. } => {
            let _ = slint::quit_event_loop();
        }
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
