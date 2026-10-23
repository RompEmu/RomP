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

const STARTED: &str = "── Playing ";
const ENDED: &str = "── Stopped playing ";

pub fn game_started(title: &str, core: &str) -> String {
    format!("{STARTED}{title} with {core} ──")
}

/// `code` is the emulator's exit code, None when the system stopped it, as after a crash.
pub fn game_ended(title: &str, code: Option<i32>) -> String {
    let how = match code {
        Some(0) => "quit".to_string(),
        Some(code) => format!("the emulator exited with code {code}"),
        None => "the emulator was stopped by the system, as after a crash".to_string(),
    };
    format!("{ENDED}{title}: {how} ──")
}

/// The newest game's part of a log, from its start line to its end line, or to the end of the
/// log when the game was still running or RomP stopped with it.
pub fn last_game(log: &str) -> Option<String> {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.iter().rposition(|l| l.contains(STARTED))?;
    let end = lines[start..]
        .iter()
        .position(|l| l.contains(ENDED))
        .map_or(lines.len(), |n| start + n + 1);
    Some(lines[start..end].join("\n"))
}

/// The newest game's part of this launch's log, or of an earlier launch's when none was played since.
pub fn last_game_in(dir: &Path) -> Option<String> {
    std::iter::once(dir.join(FILE))
        .chain((1..KEEP).map(|n| older(dir, n)))
        .filter_map(|path| std::fs::read(path).ok())
        .find_map(|bytes| last_game(&String::from_utf8_lossy(&bytes)))
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
    fn a_game_is_marked_where_it_starts_and_ends() {
        assert_eq!(
            game_started("Super Mario 64", "mupen64plus_next_libretro"),
            "── Playing Super Mario 64 with mupen64plus_next_libretro ──"
        );
        assert_eq!(
            game_ended("Doom", Some(0)),
            "── Stopped playing Doom: quit ──"
        );
        assert_eq!(
            game_ended("Doom", Some(3)),
            "── Stopped playing Doom: the emulator exited with code 3 ──"
        );
        assert_eq!(
            game_ended("Doom", None),
            "── Stopped playing Doom: the emulator was stopped by the system, as after a crash ──"
        );
    }

    #[test]
    fn the_last_game_is_cut_from_its_start_to_its_end() {
        let log = [
            "signed in",
            &game_started("Tetris", "mgba"),
            "tetris line",
            &game_ended("Tetris", Some(0)),
            &game_started("Doom", "dosbox_pure"),
            "doom line",
            &game_ended("Doom", None),
            "synced saves",
        ]
        .join("\n");
        assert_eq!(
            last_game(&log).unwrap(),
            [
                game_started("Doom", "dosbox_pure").as_str(),
                "doom line",
                &game_ended("Doom", None),
            ]
            .join("\n")
        );
        assert_eq!(last_game("signed in\nsynced saves"), None);
    }

    #[test]
    fn a_game_without_an_end_runs_to_the_end_of_the_log() {
        let log = format!("{}\nlast words", game_started("Doom", "dosbox_pure"));
        assert_eq!(last_game(&log).unwrap(), log);
    }

    #[test]
    fn the_last_game_can_come_from_an_earlier_launch() {
        let dir = tempfile::tempdir().unwrap();
        let played = format!("{}\nline", game_started("Doom", "dosbox_pure"));
        std::fs::write(dir.path().join("romp.2.log"), &played).unwrap();
        std::fs::write(dir.path().join("romp.1.log"), "no games").unwrap();
        std::fs::write(dir.path().join(FILE), "no games either").unwrap();
        assert_eq!(last_game_in(dir.path()).unwrap(), played);
        assert_eq!(last_game_in(&dir.path().join("missing")), None);
    }

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
