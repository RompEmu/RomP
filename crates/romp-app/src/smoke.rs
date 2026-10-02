use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Frames the emulator runs before the picture is checked, so the core is past any start-up
/// screens. Counted by the emulator, as a window nobody looks at may pick up only some of them.
const FRAMES: u64 = 180;
const TIMEOUT: Duration = Duration::from_secs(90);
pub const SLOT: u8 = 1;
pub const FRAME_FILE: &str = "frame.png";
/// "passed", or what failed: for when the exit code doesn't reach whoever started Romp.
pub const RESULT_FILE: &str = "result.txt";

pub type Outcome = Rc<RefCell<Option<Result<(), String>>>>;

/// What the game should do next for the smoke test.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    SaveState,
    Finish,
}

/// A quick check that a packaged build plays a game: frames arrive and aren't blank, and a
/// state saves. Used by `romp --smoke`, which exits with an error when it fails.
pub struct Smoke {
    dir: PathBuf,
    deadline: Instant,
    saving: bool,
    outcome: Outcome,
}

impl Smoke {
    pub fn new(dir: PathBuf, now: Instant) -> (Self, Outcome) {
        let outcome = Outcome::default();
        let smoke = Self {
            dir,
            deadline: now + TIMEOUT,
            saving: false,
            outcome: outcome.clone(),
        };
        (smoke, outcome)
    }

    fn done(&self) -> bool {
        self.outcome.borrow().is_some()
    }

    fn end(&self, result: Result<(), String>) -> Step {
        if !self.done() {
            *self.outcome.borrow_mut() = Some(result);
        }
        Step::Finish
    }

    /// Called often, whether or not a frame arrived.
    pub fn tick(&self, now: Instant) -> Step {
        if !self.done() && now >= self.deadline {
            let stage = if self.saving {
                "saving a state"
            } else {
                "waiting for frames"
            };
            return self.end(Err(format!("timed out {stage}")));
        }
        Step::Wait
    }

    /// Called with each new frame the game shows.
    pub fn frame(&mut self, rgba: &[u8], width: u32, height: u32, frames: u64) -> Step {
        if self.done() || self.saving || frames < FRAMES {
            return Step::Wait;
        }
        if !rgba.as_chunks::<4>().0.iter().any(|p| p[..3] != [0, 0, 0]) {
            return self.end(Err(format!("frame {frames} is all black")));
        }
        let picture = image::RgbaImage::from_raw(width, height, rgba.to_vec());
        let written = picture
            .ok_or_else(|| "frame has the wrong size".to_string())
            .and_then(|p| {
                std::fs::create_dir_all(&self.dir)
                    .and_then(|()| {
                        p.save(self.dir.join(FRAME_FILE))
                            .map_err(std::io::Error::other)
                    })
                    .map_err(|e| format!("writing the frame: {e}"))
            });
        if let Err(e) = written {
            return self.end(Err(e));
        }
        self.saving = true;
        Step::SaveState
    }

    /// Called when the runner reports a state save.
    pub fn state_written(&self, slot: u8, ok: bool, save_dir: &Path) -> Step {
        if !self.saving || slot != SLOT || self.done() {
            return Step::Wait;
        }
        let file = crate::slots::state_path(save_dir, SLOT);
        let saved = std::fs::metadata(&file).is_ok_and(|m| m.len() > 0);
        self.end(if ok && saved {
            Ok(())
        } else {
            Err(format!("the state didn't save to {}", file.display()))
        })
    }

    /// Called when the emulator stops before the test finished.
    pub fn ended(&self, code: Option<i32>) {
        if !self.done() {
            let how = code.map_or("by a signal".to_string(), |c| format!("with code {c}"));
            self.end(Err(format!("the emulator stopped {how}")));
        }
    }
}

/// Writes how the test went next to its frame.
pub fn report(dir: &Path, result: &Result<(), String>) {
    let text = match result {
        Ok(()) => "passed\n".to_string(),
        Err(reason) => format!("failed: {reason}\n"),
    };
    let written =
        std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(RESULT_FILE), text));
    if let Err(e) = written {
        tracing::warn!("writing the smoke test result: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(colour: [u8; 4]) -> Vec<u8> {
        colour.repeat(4 * 4)
    }

    #[test]
    fn a_coloured_frame_and_a_saved_state_pass() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let (mut smoke, outcome) = Smoke::new(dir.path().join("out"), now);
        let blue = frame([30, 60, 200, 255]);
        assert_eq!(smoke.frame(&blue, 4, 4, FRAMES - 1), Step::Wait);
        assert_eq!(smoke.frame(&blue, 4, 4, FRAMES), Step::SaveState);
        assert!(dir.path().join("out").join(FRAME_FILE).is_file());
        assert_eq!(
            smoke.frame(&blue, 4, 4, FRAMES + 1),
            Step::Wait,
            "saves once"
        );
        std::fs::write(crate::slots::state_path(dir.path(), SLOT), b"state").unwrap();
        assert_eq!(smoke.state_written(SLOT, true, dir.path()), Step::Finish);
        assert_eq!(*outcome.borrow(), Some(Ok(())));
    }

    #[test]
    fn the_result_is_written_down() {
        let dir = tempfile::tempdir().unwrap();
        report(dir.path(), &Ok(()));
        assert_eq!(
            std::fs::read_to_string(dir.path().join(RESULT_FILE)).unwrap(),
            "passed\n"
        );
        report(dir.path(), &Err("frame 180 is all black".into()));
        assert!(std::fs::read_to_string(dir.path().join(RESULT_FILE))
            .unwrap()
            .starts_with("failed: frame 180"));
    }

    #[test]
    fn black_frames_fail() {
        let dir = tempfile::tempdir().unwrap();
        let (mut smoke, outcome) = Smoke::new(dir.path().to_path_buf(), Instant::now());
        assert_eq!(
            smoke.frame(&frame([0, 0, 0, 255]), 4, 4, FRAMES),
            Step::Finish
        );
        assert!(matches!(&*outcome.borrow(), Some(Err(e)) if e.contains("black")));
    }

    #[test]
    fn a_state_that_isnt_on_disk_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (mut smoke, outcome) = Smoke::new(dir.path().join("out"), Instant::now());
        smoke.frame(&frame([255; 4]), 4, 4, FRAMES);
        assert_eq!(smoke.state_written(SLOT, true, dir.path()), Step::Finish);
        assert!(matches!(&*outcome.borrow(), Some(Err(_))));
    }

    #[test]
    fn running_out_of_time_or_a_crash_fails() {
        let now = Instant::now();
        let (smoke, outcome) = Smoke::new(PathBuf::from("unused"), now);
        assert_eq!(smoke.tick(now + Duration::from_secs(1)), Step::Wait);
        assert_eq!(smoke.tick(now + TIMEOUT), Step::Finish);
        assert!(matches!(&*outcome.borrow(), Some(Err(e)) if e.contains("waiting for frames")));
        let (smoke, outcome) = Smoke::new(PathBuf::from("unused"), now);
        smoke.ended(Some(3));
        assert!(matches!(&*outcome.borrow(), Some(Err(e)) if e.contains("code 3")));
    }
}
