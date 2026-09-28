#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod avatar;
mod bios;
mod collections;
mod cores;
mod covers;
mod credentials;
mod details;
mod dock;
mod download;
mod fetch;
mod gamepads;
mod grid;
mod identity;
mod input;
mod keyboard;
mod layout;
mod mapping;
mod mouse;
mod navigation;
mod paths;
mod play;
mod players;
mod ports;
mod prefs;
mod qr;
mod restore;
mod romm;
mod rotation;
mod saves;
mod scale;
mod session;
mod storage;
mod store;
mod sync;
mod xemu;

use clap::Parser;
use std::path::PathBuf;
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
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    if let Err(e) = paths::adopt_legacy_data(&paths::data_dir()) {
        tracing::warn!("moving the old data folder: {e}");
    }
    slint::BackendSelector::new()
        .with_winit_custom_application_handler(mouse::RawMouse)
        .select()?;
    match (args.core, args.rom) {
        (Some(core), Some(rom)) => play::run(core, rom, args.jit),
        _ => app::run(),
    }
}
