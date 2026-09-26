use crate::romm::types::PollOutcome;
use std::time::{Duration, Instant};

pub const SCOPES: [&str; 10] = [
    "me.read",
    "platforms.read",
    "roms.read",
    "firmware.read",
    "assets.read",
    "assets.write",
    "devices.read",
    "devices.write",
    "collections.read",
    "collections.write",
];

#[derive(Debug, PartialEq, Eq)]
pub enum PollStep {
    Wait(Duration),
    Done,
}

pub struct Poller {
    interval: Duration,
    deadline: Instant,
}

impl Poller {
    pub fn new(interval_secs: u64, expires_in_secs: u64, now: Instant) -> Self {
        Self {
            interval: Duration::from_secs(interval_secs.max(1)),
            deadline: now + Duration::from_secs(expires_in_secs),
        }
    }

    pub fn next(&mut self, outcome: &PollOutcome, now: Instant) -> PollStep {
        if now >= self.deadline {
            return PollStep::Done;
        }
        match outcome {
            PollOutcome::Pending => PollStep::Wait(self.interval),
            PollOutcome::SlowDown => {
                self.interval += Duration::from_secs(5);
                PollStep::Wait(self.interval)
            }
            _ => PollStep::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_waits_one_interval() {
        let now = Instant::now();
        let mut p = Poller::new(5, 600, now);
        assert_eq!(
            p.next(&PollOutcome::Pending, now),
            PollStep::Wait(Duration::from_secs(5))
        );
    }

    #[test]
    fn slow_down_backs_off_by_five_seconds() {
        let now = Instant::now();
        let mut p = Poller::new(5, 600, now);
        assert_eq!(
            p.next(&PollOutcome::SlowDown, now),
            PollStep::Wait(Duration::from_secs(10))
        );
        assert_eq!(
            p.next(&PollOutcome::Pending, now),
            PollStep::Wait(Duration::from_secs(10))
        );
    }

    #[test]
    fn final_outcomes_stop() {
        let now = Instant::now();
        for o in [
            PollOutcome::Denied,
            PollOutcome::Expired,
            PollOutcome::Approved {
                token: "rmm_x".into(),
                scopes: vec![],
            },
        ] {
            assert_eq!(Poller::new(5, 600, now).next(&o, now), PollStep::Done);
        }
    }

    #[test]
    fn poller_stops_at_deadline() {
        let now = Instant::now();
        let mut p = Poller::new(5, 600, now);
        assert_eq!(
            p.next(&PollOutcome::Pending, now + Duration::from_secs(600)),
            PollStep::Done
        );
    }

    #[test]
    fn zero_interval_is_clamped() {
        let now = Instant::now();
        assert_eq!(
            Poller::new(0, 600, now).next(&PollOutcome::Pending, now),
            PollStep::Wait(Duration::from_secs(1))
        );
    }
}
