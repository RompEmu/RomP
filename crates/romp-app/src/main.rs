#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod achievements;
mod app;
mod avatar;
mod awake;
mod bios;
mod collections;
mod console_settings;
mod cores;
mod covers;
mod credentials;
mod details;
mod dock;
mod download;
mod external;
mod fetch;
mod gamepads;
mod grid;
mod i18n;
mod identity;
mod input;
mod installs;
mod keyboard;
mod layout;
mod licenses;
mod logs;
mod looks;
mod mapping;
mod mouse;
mod navigation;
mod notifications;
mod pad_labels;
mod paths;
mod play;
mod play_stats;
mod players;
mod ports;
mod prefs;
mod qr;
mod restore;
mod romm;
mod rotation;
mod rpcs3;
mod rumble;
mod saves;
mod scale;
mod screenshot;
mod session;
mod shading;
mod slots;
mod smoke;
mod storage;
mod store;
mod sync;
mod updates;
mod vulkan;
mod xemu;

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

#[derive(Parser)]
#[command(name = "romp")]
struct Args {
    #[arg(long, requires = "rom")]
    core: Option<PathBuf>,
    #[arg(long, requires = "core")]
    rom: Option<PathBuf>,
    #[arg(long)]
    jit: bool,
    #[arg(long)]
    vulkan: bool,
    /// Plays the game briefly as a test, writing a frame to this folder, and exits with an error
    /// if it doesn't play.
    #[arg(long, requires = "core")]
    smoke: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    // The old data folder only moves into a data folder that isn't there yet, so the log
    // folder inside it comes after.
    let adopted = paths::adopt_legacy_data(&paths::data_dir());
    let (log, log_error) = match logs::open(&paths::logs_dir()) {
        Ok(file) => (Some(file), None),
        Err(e) => (None, Some(e)),
    };
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(log.map(|file| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
        }))
        .init();
    tracing::info!(
        revision = env!("ROMP_BUILD_REVISION"),
        "RomP {} on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Some(e) = log_error {
        tracing::warn!("opening the log file: {e}");
    }
    let args = Args::parse();
    if let Err(e) = adopted {
        tracing::warn!("moving the old data folder: {e}");
    }
    tracing::info!(
        desktop = ?std::env::var("XDG_CURRENT_DESKTOP").ok(),
        session_desktop = ?std::env::var("XDG_SESSION_DESKTOP").ok(),
        session_type = ?std::env::var("XDG_SESSION_TYPE").ok(),
        gamescope_display = ?std::env::var("GAMESCOPE_WAYLAND_DISPLAY").ok(),
        x11_display = std::env::var_os("DISPLAY").is_some(),
        wayland_display = std::env::var_os("WAYLAND_DISPLAY").is_some(),
        backend_request = ?std::env::var("SLINT_BACKEND").ok(),
        slint_scale = ?std::env::var("SLINT_SCALE_FACTOR").ok(),
        x11_scale = ?std::env::var("WINIT_X11_SCALE_FACTOR").ok(),
        "UI session"
    );
    slint::BackendSelector::new()
        .with_winit_custom_application_handler(mouse::RawMouse)
        .select()?;
    match (args.core, args.rom) {
        (Some(core), Some(rom)) => play::run(core, rom, args.jit, args.vulkan, args.smoke),
        _ => app::run(),
    }
}
