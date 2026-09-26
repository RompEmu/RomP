use anyhow::Context;
use cartridge_libretro::{self as lr, HwContextProvider as _};
use cartridge_proto::frame::{self, FrameWriter, SrcFormat};
use cartridge_proto::msg::{AppMsg, RunnerMsg};
use cartridge_runner::frontend::Frontend;
use cartridge_runner::ipc::Link;
use cartridge_runner::state::StateManager;
use cartridge_runner::{archive, audio, hw_gl, perf, sandbox};
use clap::Parser;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

const UNLOAD_GRACE: Duration = Duration::from_secs(2);

#[derive(Parser)]
#[command(name = "cartridge-runner")]
struct Args {
    #[arg(long)]
    core: PathBuf,
    #[arg(long)]
    rom: PathBuf,
    #[arg(long)]
    system_dir: PathBuf,
    #[arg(long)]
    save_dir: PathBuf,
    #[arg(long)]
    socket: PathBuf,
    #[arg(long)]
    frames: String,
    #[arg(long)]
    load_slot: Option<u8>,
    #[arg(long)]
    jit: bool,
    #[arg(long, default_value_t = 100)]
    volume: u8,
    #[arg(long = "option", value_parser = parse_option)]
    options: Vec<(String, String)>,
}

fn parse_option(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .ok_or_else(|| format!("expected KEY=VALUE, got `{s}`"))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();
    let mut frontend = Frontend::new();
    let input = frontend.input.clone();
    lr::set_input_source(Some(std::sync::Arc::new(move |port, device, index, id| {
        input.state(port, device, index, id)
    })));
    let audio = frontend.audio.clone();
    lr::set_audio_sink(Some(std::sync::Arc::new(move |samples: &[i16]| {
        cartridge_runner::audio_pipe::push(&audio, samples)
    })));
    let link = Link::connect(&args.socket, frontend.input.clone()).context("connect to app")?;
    let result = run(&args, &mut frontend, &link);
    let error = result.as_ref().err().map(|e| format!("{e:#}"));
    if let Some(e) = &error {
        tracing::error!("{e}");
    }
    link.send(&RunnerMsg::Exited { error });
    result
}

fn run(args: &Args, frontend: &mut Frontend, link: &Link) -> anyhow::Result<()> {
    let mut frames = FrameWriter::open(&args.frames).context("open frame buffer")?;
    let _ = frame::unlink(&args.frames);
    std::fs::create_dir_all(&args.save_dir)?;
    std::fs::create_dir_all(&args.system_dir)?;
    lr::set_core_dirs(Some(&args.system_dir), Some(&args.save_dir));

    let hw_ctx = hw_gl::HwGlContext::create(hw_gl::FBO_WIDTH, hw_gl::FBO_HEIGHT)
        .map(Box::new)
        .map_err(|e| warn!("HW GL context unavailable: {e}"))
        .ok();
    if let Some(ctx) = &hw_ctx {
        // SAFETY: the boxed context outlives the core and is uninstalled before it drops.
        unsafe {
            lr::install_hw_provider(ctx.as_ref() as *const hw_gl::HwGlContext
                as *mut hw_gl::HwGlContext
                as *mut dyn lr::HwContextProvider)
        };
    }

    archive::remove_stale_scratch(&std::env::temp_dir());
    apply_sandbox(args)?;

    let mut core = unsafe { lr::Core::load(&args.core) }.context("load core")?;
    lr::set_core_option_values(&args.options);
    let sys = core.system_info();
    info!(core = %sys.library_name, version = %sys.library_version, "core loaded");
    let session_tag = std::process::id().to_string();
    let rom = archive::open_rom(
        &args.rom,
        &session_tag,
        sys.need_fullpath,
        &sys.valid_extensions,
    )?;

    core.init(frontend);
    core.load_game(
        lr::GameInfo {
            path: Some(rom.effective_path.as_path()),
            data: rom.bytes.as_deref(),
        },
        frontend,
    )
    .context("the core could not load this game")?;
    if let Some(reset) = lr::take_pending_hw_reset() {
        if let Some(ctx) = &hw_ctx {
            ctx.make_current();
            lr::with_frontend_installed(frontend, || unsafe { reset() });
        }
    }
    for port in 0..4 {
        core.set_controller_port_device(port, lr::RETRO_DEVICE_JOYPAD, frontend);
    }

    let mut saves = StateManager::new(&args.save_dir);
    saves.load_initial_sram(&mut core, frontend);
    if let Some(slot) = args.load_slot {
        // Some cores initialise lazily on the first run.
        core.run(frontend);
        saves.load_state(slot, &mut core, frontend);
        frontend.video_dirty = false;
        frontend.hw_frame_dirty = false;
    }

    let av = core.av_info(frontend);
    let sample_rate = av.timing.sample_rate.round() as u32;
    let buffer_ms: usize = if cfg!(target_os = "macos") { 250 } else { 500 };
    let (_audio, producer) = audio::open(sample_rate, sample_rate as usize * 2 * buffer_ms / 1000)?;
    {
        let mut pipe = frontend.audio.lock();
        pipe.set_producer(producer);
        pipe.set_volume(args.volume);
    }
    link.send(&RunnerMsg::Started {
        core_name: sys.library_name.clone(),
        core_version: sys.library_version.clone(),
        fps: av.timing.fps,
        sample_rate: av.timing.sample_rate,
    });
    let mut rotation = lr::rotation();
    link.send(&RunnerMsg::Rotation(rotation as u8));
    link.send(&RunnerMsg::Controllers {
        ports: lr::controller_info()
            .into_iter()
            .map(|types| types.into_iter().map(|t| (t.name, t.id)).collect())
            .collect(),
    });

    frontend.aspect = av.geometry.aspect_ratio;
    let frame_duration = Duration::from_secs_f64(1.0 / av.timing.fps.max(1.0));
    let mut next_frame_at = Instant::now();
    let mut paused = false;
    let mut stop_requested = false;
    let mut stats = perf::FrameStats::new(Duration::from_secs(10), Instant::now());
    loop {
        let now = Instant::now();
        if now < next_frame_at {
            std::thread::sleep(next_frame_at - now);
        }
        for msg in link.drain() {
            match msg {
                AppMsg::Pause(p) => paused = p,
                AppMsg::Volume(v) => frontend.audio.lock().set_volume(v),
                AppMsg::Key {
                    code,
                    character,
                    modifiers,
                    down,
                } => {
                    frontend.input.set_key(code, down);
                    lr::invoke_keyboard_callback(down, code, character, modifiers);
                }
                AppMsg::PortDevice { port, device } => {
                    core.set_controller_port_device(u32::from(port), device, frontend);
                    frontend.input.set_port_device(u32::from(port), device);
                }
                AppMsg::SaveSlot(slot) => {
                    let ok = saves.save_state(slot, &mut core, frontend);
                    link.send(&RunnerMsg::StateWritten { slot, ok });
                }
                AppMsg::LoadSlot(slot) => {
                    let ok = saves.load_state(slot, &mut core, frontend);
                    link.send(&RunnerMsg::StateLoaded { slot, ok });
                }
                AppMsg::Shutdown => stop_requested = true,
                AppMsg::Pad { .. } | AppMsg::Pointer { .. } | AppMsg::Mouse { .. } => {}
            }
        }
        if stop_requested {
            break;
        }
        if !paused {
            let run_started = Instant::now();
            core.run(frontend);
            let run_time = run_started.elapsed();
            let readback_started = Instant::now();
            if frontend.hw_frame_dirty {
                frontend.hw_frame_dirty = false;
                if let Some(ctx) = &hw_ctx {
                    let (w, h) =
                        hw_gl::hw_frame_size(frontend.hw_frame_width, frontend.hw_frame_height);
                    let pixels = ctx.readback_bgra(w, h);
                    frames.write(
                        &pixels,
                        w,
                        h,
                        w as usize * 4,
                        SrcFormat::Xrgb8888,
                        frontend.aspect,
                    );
                }
            }
            if frontend.video_dirty {
                frontend.video_dirty = false;
                if let Some(v) = &frontend.video {
                    frames.write(
                        &v.data,
                        v.width,
                        v.height,
                        v.pitch,
                        src_format(frontend.video_format),
                        frontend.aspect,
                    );
                }
            }
            let (audio_frames, audio_fill) = frontend.audio.lock().take_stats();
            stats.record(
                run_time,
                readback_started.elapsed(),
                audio_frames,
                audio_fill,
            );
            if let Some(line) = stats.report(Instant::now()) {
                info!("{line}");
            }
            if lr::rotation() != rotation {
                rotation = lr::rotation();
                link.send(&RunnerMsg::Rotation(rotation as u8));
            }
            if saves.tick_sram(&mut core, frontend) {
                link.send(&RunnerMsg::SramWritten);
            }
        }
        if frontend.shutdown {
            break;
        }
        next_frame_at += frame_duration;
        let now = Instant::now();
        if next_frame_at + frame_duration * 4 < now {
            next_frame_at = now;
        }
    }
    if stop_requested {
        saves.save_on_shutdown(&mut core, frontend);
        let scratch = archive::scratch_dir(&session_tag);
        std::thread::spawn(move || {
            std::thread::sleep(UNLOAD_GRACE);
            let _ = std::fs::remove_dir_all(scratch);
            std::process::exit(0);
        });
    } else {
        saves.flush_sram(&mut core, frontend);
    }
    core.unload_game(frontend);
    lr::uninstall_hw_provider();
    drop(core);
    let _ = std::fs::remove_dir_all(archive::scratch_dir(&session_tag));
    Ok(())
}

fn src_format(format: lr::PixelFormat) -> SrcFormat {
    match format {
        lr::PixelFormat::Xrgb8888 => SrcFormat::Xrgb8888,
        lr::PixelFormat::Rgb565 => SrcFormat::Rgb565,
        lr::PixelFormat::Rgb1555 => SrcFormat::Rgb1555,
    }
}

fn apply_sandbox(args: &Args) -> anyhow::Result<()> {
    if std::env::var_os("CARTRIDGE_NO_SANDBOX").is_some() {
        warn!("sandbox disabled by CARTRIDGE_NO_SANDBOX");
        return Ok(());
    }
    let canon = |p: &PathBuf| p.canonicalize().unwrap_or_else(|_| p.clone());
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    sandbox::apply(&sandbox::SandboxParams {
        rom_path: &canon(&args.rom),
        core_path: &canon(&args.core),
        socket_path: &args.socket,
        system_dir: &canon(&args.system_dir),
        save_dir: &canon(&args.save_dir),
        home_dir: &home,
        needs_jit: args.jit,
        permissive_mach: std::env::var_os("CARTRIDGE_SANDBOX_PERMISSIVE_MACH").is_some(),
        permissive_read: std::env::var_os("CARTRIDGE_SANDBOX_PERMISSIVE_READ").is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_parse_as_key_value() {
        assert_eq!(
            parse_option("puae_kickstart=auto"),
            Ok(("puae_kickstart".into(), "auto".into()))
        );
        assert_eq!(parse_option("a=b=c"), Ok(("a".into(), "b=c".into())));
        assert!(parse_option("novalue").is_err());
    }
}
