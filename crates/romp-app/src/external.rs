use serde::{Deserialize, Serialize};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOG_TAIL: usize = 40;

#[derive(Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: String,
    exe: String,
}

/// An emulator Romp downloads from its GitHub releases and starts as its own program.
pub struct Emulator {
    pub name: &'static str,
    pub dir: PathBuf,
    pub pick: fn(&[Asset]) -> Option<&Asset>,
    pub exe_in: fn(&Path, &str) -> PathBuf,
}

impl Emulator {
    pub fn installed(&self) -> Option<PathBuf> {
        let manifest: Manifest =
            serde_json::from_slice(&std::fs::read(self.dir.join("manifest.json")).ok()?).ok()?;
        let exe = self.dir.join(manifest.exe);
        exe.is_file().then_some(exe)
    }

    pub async fn install(&self, http: &reqwest::Client, releases: &str) -> Result<PathBuf, String> {
        let name = self.name;
        let unreachable = |_| format!("Could not reach GitHub to download {name}");
        let release: Release = http
            .get(releases)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(unreachable)?
            .error_for_status()
            .map_err(|e| format!("{name} is not available right now ({e})"))?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let asset = (self.pick)(&release.assets)
            .ok_or_else(|| format!("{name} has no download for this computer"))?;
        let bytes = http
            .get(&asset.browser_download_url)
            .send()
            .await
            .map_err(unreachable)?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let version_dir = self.dir.join(&release.tag_name);
        let file = asset.name.clone();
        let dir = version_dir.clone();
        tokio::task::spawn_blocking(move || unpack(&bytes, &dir, &file))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Could not unpack {name}: {e}"))?;
        let exe = (self.exe_in)(&version_dir, &asset.name);
        if !exe.is_file() {
            return Err(format!("The {name} download did not contain the emulator"));
        }
        let manifest = Manifest {
            version: release.tag_name.clone(),
            exe: exe
                .strip_prefix(&self.dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned(),
        };
        std::fs::write(
            self.dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest json"),
        )
        .map_err(|e| e.to_string())?;
        Ok(exe)
    }
}

fn unpack(bytes: &[u8], dir: &Path, name: &str) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if name.ends_with(".zip") && cfg!(target_os = "macos") {
        unpack_with(bytes, dir, name, |archive| {
            let mut ditto = std::process::Command::new("/usr/bin/ditto");
            ditto.args(["-x", "-k"]).arg(archive).arg(dir);
            ditto
        })
    } else if name.ends_with(".zip") {
        zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .and_then(|mut archive| archive.extract(dir))
            .map_err(|e| e.to_string())
    } else if name.ends_with(".7z") {
        unpack_7z(bytes, dir, name)
    } else {
        let exe = dir.join(name);
        std::fs::write(&exe, bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Unpacks with a macOS tool, which keeps the symbolic links inside app bundles.
fn unpack_with(
    bytes: &[u8],
    dir: &Path,
    name: &str,
    command: impl FnOnce(&Path) -> std::process::Command,
) -> Result<(), String> {
    let archive = dir.join(name);
    std::fs::write(&archive, bytes).map_err(|e| e.to_string())?;
    let status = command(&archive).status().map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&archive);
    match status? {
        s if s.success() => Ok(()),
        s => Err(format!("unpacking failed ({s})")),
    }
}

#[cfg(target_os = "macos")]
fn unpack_7z(bytes: &[u8], dir: &Path, name: &str) -> Result<(), String> {
    unpack_with(bytes, dir, name, |archive| {
        let mut tar = std::process::Command::new("/usr/bin/tar");
        tar.arg("-xf").arg(archive).arg("-C").arg(dir);
        tar
    })
}

#[cfg(not(target_os = "macos"))]
fn unpack_7z(bytes: &[u8], dir: &Path, name: &str) -> Result<(), String> {
    extract(std::io::Cursor::new(bytes), name, dir)
}

/// Unpacks a .zip or .7z into `dir`, refusing entries that would land outside it.
pub fn extract<R: Read + Seek>(reader: R, name: &str, dir: &Path) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        return zip::ZipArchive::new(reader)
            .and_then(|mut archive| archive.extract(dir))
            .map_err(|e| e.to_string());
    }
    if !lower.ends_with(".7z") {
        return Err(format!("{name} is not a zip or 7z archive"));
    }
    let mut archive = sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::empty())
        .map_err(|e| e.to_string())?;
    archive
        .for_each_entries(|entry, data| {
            let rel = Path::new(entry.name());
            if rel.as_os_str().is_empty()
                || !rel
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
            {
                return Err(std::io::Error::other(format!("unsafe path {}", entry.name())).into());
            }
            let path = dir.join(rel);
            if entry.is_directory() {
                std::fs::create_dir_all(&path)?;
                return Ok(true);
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(data, &mut std::fs::File::create(&path)?)?;
            Ok(true)
        })
        .map_err(|e| e.to_string())
}

pub struct Running {
    child: Arc<Mutex<std::process::Child>>,
}

impl Running {
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

/// Starts the emulator and calls `on_exit` with its exit code and the last lines it printed.
pub fn launch(
    mut command: std::process::Command,
    label: &'static str,
    on_exit: impl FnOnce(Option<i32>, Vec<String>) + Send + 'static,
) -> std::io::Result<Running> {
    use std::io::BufRead;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    let stderr = child.stderr.take().expect("stderr is piped");
    let child = Arc::new(Mutex::new(child));
    let waiter = child.clone();
    std::thread::spawn(move || {
        let mut tail = std::collections::VecDeque::new();
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            tracing::debug!("[{label}] {line}");
            if tail.len() == LOG_TAIL {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        let code = loop {
            match waiter.lock().unwrap().try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) => {}
                Err(_) => break None,
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        on_exit(code, tail.into());
    });
    Ok(Running { child })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn seven_zip_archives_keep_app_bundle_links() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("A.app/Contents/Versions/A")).unwrap();
        std::fs::write(src.join("A.app/Contents/Versions/A/lib"), b"code").unwrap();
        std::os::unix::fs::symlink("A", src.join("A.app/Contents/Versions/Current")).unwrap();
        let archive = dir.path().join("a.7z");
        let status = std::process::Command::new("/usr/bin/tar")
            .args(["--format", "7zip", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg("A.app")
            .status()
            .unwrap();
        if !status.success() {
            return;
        }
        let out = dir.path().join("out");
        unpack(&std::fs::read(&archive).unwrap(), &out, "a.7z").unwrap();
        let link = out.join("A.app/Contents/Versions/Current");
        assert!(link.is_symlink());
        assert_eq!(std::fs::read(link.join("lib")).unwrap(), b"code");
        assert!(!out.join("a.7z").exists(), "the archive is removed");
    }

    #[test]
    fn plain_downloads_become_executables() {
        let dir = tempfile::tempdir().unwrap();
        unpack(b"elf", dir.path(), "emu.AppImage").unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("emu.AppImage")).unwrap(),
            b"elf"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("emu.AppImage"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }
}
