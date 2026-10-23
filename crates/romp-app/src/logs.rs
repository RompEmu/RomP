use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const FILE: &str = "romp.log";
const KEEP: usize = 5;
const LIMIT: u64 = 20 * 1024 * 1024;

fn older(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("romp.{n}.log"))
}

/// Moves the earlier launches' logs along, keeping the newest few, and returns this launch's.
pub fn rotate(dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let current = dir.join(FILE);
    for n in (1..KEEP).rev() {
        let from = if n == 1 {
            current.clone()
        } else {
            older(dir, n - 1)
        };
        if from.exists() {
            std::fs::rename(from, older(dir, n))?;
        }
    }
    Ok(current)
}

/// This launch's log, after moving the earlier ones along.
pub fn open(dir: &Path) -> io::Result<Capped<std::fs::File>> {
    Ok(Capped {
        inner: std::fs::File::create(rotate(dir)?)?,
        written: 0,
        limit: LIMIT,
    })
}

/// A log file that stops growing at its limit, so a runaway loop can't fill the disk.
pub struct Capped<W> {
    inner: W,
    written: u64,
    limit: u64,
}

impl<W: Write> Write for Capped<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written >= self.limit {
            return Ok(buf.len());
        }
        self.inner.write_all(buf)?;
        self.written += buf.len() as u64;
        if self.written >= self.limit {
            self.inner
                .write_all(b"\nRomP stopped writing this log at its size limit.\n")?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_launch_keeps_the_newest_logs() {
        let dir = tempfile::tempdir().unwrap();
        for launch in 0..7 {
            let path = rotate(dir.path()).unwrap();
            std::fs::write(&path, format!("launch {launch}")).unwrap();
        }
        let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
        assert_eq!(read(FILE), "launch 6");
        assert_eq!(read("romp.1.log"), "launch 5");
        assert_eq!(read("romp.4.log"), "launch 2");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), KEEP);
    }

    #[test]
    fn a_missing_folder_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let path = rotate(&dir.path().join("logs")).unwrap();
        assert_eq!(path, dir.path().join("logs").join(FILE));
        assert!(path.parent().unwrap().is_dir());
    }

    #[test]
    fn the_log_stops_at_its_limit_and_says_so() {
        let mut log = Capped {
            inner: Vec::new(),
            written: 0,
            limit: 10,
        };
        log.write_all(b"12345678").unwrap();
        log.write_all(b"90abc").unwrap();
        log.write_all(b"more").unwrap();
        let text = String::from_utf8(log.inner).unwrap();
        assert!(text.starts_with("1234567890abc"));
        assert!(text.ends_with("RomP stopped writing this log at its size limit.\n"));
        assert!(!text.contains("more"));
    }
}
