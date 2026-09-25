mod input;
mod paths;
mod session;

use cartridge_proto::msg::{AppMsg, RunnerMsg};
use clap::Parser;
use input::{KeyAction, Pad};
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
    let name = args
        .rom
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cfg = SessionConfig {
        runner: paths::runner_exe()?,
        core: args.core,
        rom: args.rom,
        system_dir: data.join("system"),
        save_dir: data.join("saves").join(format!("local-{name}")),
        jit: args.jit,
        load_slot: None,
    };
    let session = Rc::new(RefCell::new(Session::start(&cfg)?));

    let ui = GameWindow::new()?;
    ui.set_game_title(format!("{name} — Cartridge").into());

    let pad = RefCell::new(Pad::default());
    ui.on_key_event({
        let session = session.clone();
        move |text, pressed| {
            let Some(action) = input::map_key(&text) else {
                return false;
            };
            let mut session = session.borrow_mut();
            match action {
                KeyAction::Button(button) => {
                    if let Some(state) = pad.borrow_mut().set(button, pressed) {
                        session.send(&AppMsg::Pad { port: 0, state });
                    }
                }
                KeyAction::SaveSlot(slot) if pressed => session.send(&AppMsg::SaveSlot(slot)),
                KeyAction::LoadSlot(slot) if pressed => session.send(&AppMsg::LoadSlot(slot)),
                KeyAction::Quit if pressed => {
                    let _ = slint::quit_event_loop();
                }
                _ => {}
            }
            true
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
