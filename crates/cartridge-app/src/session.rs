use anyhow::{anyhow, Context, Result};
use cartridge_proto::frame::FrameReader;
use cartridge_proto::msg::{AppMsg, RunnerMsg};
use cartridge_proto::wire;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOG_TAIL: usize = 40;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_GRACE: Duration = Duration::from_secs(3);
const WRITE_TIMEOUT: Duration = Duration::from_millis(50);

pub struct SessionConfig {
    pub runner: PathBuf,
    pub core: PathBuf,
    pub rom: PathBuf,
    pub system_dir: PathBuf,
    pub save_dir: PathBuf,
    pub jit: bool,
    pub load_slot: Option<u8>,
}

#[derive(Debug)]
pub enum SessionEvent {
    Runner(RunnerMsg),
    Ended {
        code: Option<i32>,
        log_tail: Vec<String>,
    },
}

pub struct Session {
    stream: UnixStream,
    pub frames: FrameReader,
    events: Receiver<SessionEvent>,
    child: Arc<Mutex<Child>>,
}

pub fn runner_args(cfg: &SessionConfig, socket: &Path, frames: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--core".into(),
        cfg.core.clone().into_os_string(),
        "--rom".into(),
        cfg.rom.clone().into_os_string(),
        "--system-dir".into(),
        cfg.system_dir.clone().into_os_string(),
        "--save-dir".into(),
        cfg.save_dir.clone().into_os_string(),
        "--socket".into(),
        socket.as_os_str().to_owned(),
        "--frames".into(),
        frames.into(),
    ];
    if let Some(slot) = cfg.load_slot {
        args.push("--load-slot".into());
        args.push(slot.to_string().into());
    }
    if cfg.jit {
        args.push("--jit".into());
    }
    args
}

impl Session {
    pub fn start(cfg: &SessionConfig) -> Result<Self> {
        let id = unique_id();
        let frames_name = format!("/cart-{id}");
        let frames = FrameReader::create(&frames_name).context("create frame buffer")?;
        let socket = std::env::temp_dir().join(format!("cartridge-{id}.sock"));
        let listener = UnixListener::bind(&socket).context("bind runner socket")?;
        listener.set_nonblocking(true)?;
        let spawned = Command::new(&cfg.runner)
            .args(runner_args(cfg, &socket, &frames_name))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&socket);
                return Err(e).with_context(|| format!("start {}", cfg.runner.display()));
            }
        };
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        let log_thread =
            spawn_log_reader(child.stderr.take().expect("stderr is piped"), tail.clone());
        let accepted = accept(&listener, &mut child);
        let _ = std::fs::remove_file(&socket);
        let stream = match accepted {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = log_thread.join();
                let log = tail
                    .lock()
                    .unwrap()
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(e.context(log));
            }
        };

        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let child = Arc::new(Mutex::new(child));
        let (tx, events) = mpsc::channel();
        let mut reader = stream.try_clone()?;
        let wait_child = child.clone();
        std::thread::spawn(move || {
            while let Ok(msg) = wire::read_msg::<_, RunnerMsg>(&mut reader) {
                if tx.send(SessionEvent::Runner(msg)).is_err() {
                    break;
                }
            }
            let code = loop {
                match wait_child.lock().unwrap().try_wait() {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) => {}
                    Err(_) => break None,
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let _ = log_thread.join();
            let log_tail = tail.lock().unwrap().iter().cloned().collect();
            let _ = tx.send(SessionEvent::Ended { code, log_tail });
        });
        Ok(Self {
            stream,
            frames,
            events,
            child,
        })
    }

    pub fn send(&mut self, msg: &AppMsg) {
        if let Err(e) = wire::write_msg(&mut self.stream, msg) {
            tracing::warn!("send to runner: {e}");
        }
    }

    pub fn poll_events(&self) -> Vec<SessionEvent> {
        self.events.try_iter().collect()
    }

    pub fn request_stop(&mut self) {
        self.send(&AppMsg::Shutdown);
        let child = self.child.clone();
        std::thread::spawn(move || {
            std::thread::sleep(STOP_GRACE);
            let mut child = child.lock().unwrap();
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
        });
    }

    pub fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !matches!(self.child.lock().unwrap().try_wait(), Ok(None)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

fn unique_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "{:x}{:x}{:x}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn spawn_log_reader(stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut raw = Vec::new();
        while matches!(reader.read_until(b'\n', &mut raw), Ok(n) if n > 0) {
            let line = String::from_utf8_lossy(&raw).trim_end().to_string();
            raw.clear();
            eprintln!("[runner] {line}");
            let mut tail = tail.lock().unwrap();
            if tail.len() == LOG_TAIL {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    })
}

fn accept(listener: &UnixListener, child: &mut Child) -> Result<UnixStream> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        if let Some(status) = child.try_wait()? {
            return Err(anyhow!("the emulator exited before starting ({status})"));
        }
        if Instant::now() > deadline {
            return Err(anyhow!("the emulator did not start in time"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn config(runner: PathBuf) -> SessionConfig {
        SessionConfig {
            runner,
            core: "/cores/snes9x_libretro.dylib".into(),
            rom: "/roms/Pokémon \"Blue\" (USA).gb".into(),
            system_dir: "/data/system".into(),
            save_dir: "/data/saves/7".into(),
            jit: false,
            load_slot: None,
        }
    }

    #[test]
    fn runner_args_preserve_unicode_paths() {
        let mut cfg = config("/bin/runner".into());
        cfg.jit = true;
        cfg.load_slot = Some(2);
        let args = runner_args(&cfg, Path::new("/tmp/c.sock"), "/cart-1");
        let expected: Vec<OsString> = [
            "--core",
            "/cores/snes9x_libretro.dylib",
            "--rom",
            "/roms/Pokémon \"Blue\" (USA).gb",
            "--system-dir",
            "/data/system",
            "--save-dir",
            "/data/saves/7",
            "--socket",
            "/tmp/c.sock",
            "--frames",
            "/cart-1",
            "--load-slot",
            "2",
            "--jit",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        assert_eq!(args, expected);
    }

    #[test]
    fn start_reports_runner_stderr_when_runner_exits_early() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-runner");
        std::fs::write(&script, "#!/bin/sh\necho 'core exploded' >&2\nexit 3\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = Session::start(&config(script))
            .err()
            .expect("start should fail");
        let text = format!("{err:#}");
        assert!(text.contains("core exploded"), "{text}");
    }

    #[test]
    fn log_survives_non_utf8_stderr_lines() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-runner");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'title \\377\\376\\n' >&2\necho 'still logging' >&2\nexit 3\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = Session::start(&config(script)).err().unwrap();
        let text = format!("{err:#}");
        assert!(text.contains("still logging"), "{text}");
    }

    #[test]
    fn start_fails_cleanly_for_missing_runner() {
        let err = Session::start(&config("/nonexistent/cartridge-runner".into()))
            .err()
            .unwrap();
        assert!(format!("{err:#}").contains("/nonexistent/cartridge-runner"));
    }
}
