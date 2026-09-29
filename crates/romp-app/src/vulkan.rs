use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

static AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Starts finding out whether Vulkan works here, so the answer is ready before the first game.
pub fn start_probe() {
    std::thread::spawn(available);
}

/// Whether this computer can create a Vulkan device, found out once by asking the runner, so a
/// failing driver takes down that process and not the app.
pub fn available() -> bool {
    *AVAILABLE.get_or_init(|| {
        let works = cfg!(target_os = "macos") || probe();
        tracing::info!(works, "Vulkan");
        works
    })
}

fn probe() -> bool {
    let Ok(runner) = crate::paths::runner_exe() else {
        return false;
    };
    let mut command = Command::new(runner);
    command
        .arg("--probe-vulkan")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}
