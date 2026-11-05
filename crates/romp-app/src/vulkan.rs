use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Support {
    None,
    NoSurface,
    Full,
}

static SUPPORT: OnceLock<Support> = OnceLock::new();

/// Starts finding out whether Vulkan works here, so the answer is ready before the first game.
pub fn start_probe() {
    std::thread::spawn(available);
}

/// Whether this computer can create a Vulkan device, found out once by asking the runner, so a
/// failing driver takes down that process and not the app.
pub fn available() -> bool {
    support() != Support::None
}

/// Whether Vulkan cores also get a surface to present through, which Dolphin needs to show a
/// picture. Some drivers offer no surface without a window.
pub fn presents() -> bool {
    support() == Support::Full
}

fn support() -> Support {
    *SUPPORT.get_or_init(|| {
        let support = if cfg!(target_os = "macos") {
            Support::Full
        } else {
            probe()
        };
        tracing::info!(?support, "Vulkan");
        support
    })
}

fn probe() -> Support {
    let Ok(runner) = crate::paths::runner_exe() else {
        return Support::None;
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
        return Support::None;
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return support_from_exit(status.code()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return Support::None;
            }
        }
    }
}

fn support_from_exit(code: Option<i32>) -> Support {
    match code {
        Some(0) => Support::Full,
        Some(2) => Support::NoSurface,
        _ => Support::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_exit_code_tells_whether_cores_get_a_surface() {
        assert_eq!(support_from_exit(Some(0)), Support::Full);
        assert_eq!(support_from_exit(Some(2)), Support::NoSurface);
        assert_eq!(support_from_exit(Some(1)), Support::None);
        assert_eq!(support_from_exit(None), Support::None);
    }
}
