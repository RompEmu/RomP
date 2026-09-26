mod app;
mod bios;
mod collections;
mod cores;
mod covers;
mod credentials;
mod details;
mod download;
mod fetch;
mod gamepads;
mod grid;
mod identity;
mod input;
mod layout;
mod mapping;
mod paths;
mod play;
mod players;
mod prefs;
mod qr;
mod romm;
mod saves;
mod session;
mod storage;
mod store;
mod sync;

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

#[derive(Parser)]
#[command(name = "cartridge")]
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
    match (args.core, args.rom) {
        (Some(core), Some(rom)) => play::run(core, rom, args.jit),
        _ => app::run(),
    }
}
