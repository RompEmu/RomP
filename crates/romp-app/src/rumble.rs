use crate::players::PLAYERS;
use std::time::{Duration, Instant};

/// How long each request to a controller rumbles for. Short, so a game that quits or crashes
/// mid-rumble doesn't leave the controller buzzing.
pub const RUMBLE_FOR: Duration = Duration::from_millis(1000);
/// How often a running motor is asked again, well before its last request runs out.
const REFRESH: Duration = Duration::from_millis(400);

type Motors = [(u16, u16); PLAYERS as usize];

/// Each player's strong and weak motor, as the game set them, and when the controllers last heard.
#[derive(Default)]
pub struct Rumble {
    motors: Motors,
    quiet: bool,
    changed: bool,
    told: Option<Instant>,
}

impl Rumble {
    pub fn set(&mut self, port: u32, strong: u16, weak: u16) {
        let Some(motors) = self.motors.get_mut(port as usize) else {
            return;
        };
        if *motors != (strong, weak) {
            *motors = (strong, weak);
            self.changed = true;
        }
    }

    /// Stops every motor while the game is paused or behind its menu.
    pub fn set_quiet(&mut self, quiet: bool) {
        if self.quiet != quiet {
            self.quiet = quiet;
            self.changed = true;
        }
    }

    /// What each player's controllers should rumble with, when it's time to tell them.
    pub fn due(&mut self, now: Instant) -> Option<Motors> {
        let running = !self.quiet && self.motors.iter().any(|&m| m != (0, 0));
        let stale = self
            .told
            .is_none_or(|told| now.saturating_duration_since(told) >= REFRESH);
        if !self.changed && !(running && stale) {
            return None;
        }
        self.changed = false;
        self.told = Some(now);
        Some(if self.quiet {
            [(0, 0); PLAYERS as usize]
        } else {
            self.motors
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_due_at_once_and_only_once() {
        let now = Instant::now();
        let mut rumble = Rumble::default();
        assert_eq!(rumble.due(now), None);
        rumble.set(1, 40_000, 9_000);
        assert_eq!(
            rumble.due(now),
            Some([(0, 0), (40_000, 9_000), (0, 0), (0, 0)])
        );
        assert_eq!(rumble.due(now), None);
        rumble.set(1, 40_000, 9_000);
        assert_eq!(
            rumble.due(now),
            None,
            "the same strength again isn't a change"
        );
        rumble.set(9, 1, 1);
        assert_eq!(
            rumble.due(now),
            None,
            "ports past the last player are ignored"
        );
    }

    #[test]
    fn a_running_motor_is_asked_again_before_its_rumble_runs_out() {
        let now = Instant::now();
        let mut rumble = Rumble::default();
        rumble.set(0, 100, 0);
        assert!(rumble.due(now).is_some());
        assert_eq!(rumble.due(now + REFRESH / 2), None);
        assert!(rumble.due(now + REFRESH).is_some());
        assert!(REFRESH < RUMBLE_FOR);

        rumble.set(0, 0, 0);
        assert!(rumble.due(now + REFRESH).is_some());
        assert_eq!(
            rumble.due(now + REFRESH * 4),
            None,
            "still motors aren't asked again"
        );
    }

    #[test]
    fn quiet_stops_every_motor_until_it_ends() {
        let now = Instant::now();
        let mut rumble = Rumble::default();
        rumble.set(0, 100, 50);
        rumble.due(now);
        rumble.set_quiet(true);
        assert_eq!(rumble.due(now), Some([(0, 0); PLAYERS as usize]));
        assert_eq!(rumble.due(now + REFRESH * 2), None);
        rumble.set_quiet(false);
        assert_eq!(rumble.due(now), Some([(100, 50), (0, 0), (0, 0), (0, 0)]));
    }
}
