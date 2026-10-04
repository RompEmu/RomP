use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tr::tr;

/// How much a game has been played: across every device RomM knows of, plus this computer's unsent time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayStats {
    pub total_ms: i64,
    pub sessions: u32,
    pub last_played_ms: Option<i64>,
}

impl PlayStats {
    pub fn add(&mut self, start_ms: i64, end_ms: i64) {
        self.total_ms += (end_ms - start_ms).max(0);
        self.sessions += 1;
        self.last_played_ms = Some(self.last_played_ms.map_or(end_ms, |last| last.max(end_ms)));
    }

    pub fn merged(mut self, other: PlayStats) -> PlayStats {
        self.total_ms += other.total_ms;
        self.sessions += other.sessions;
        self.last_played_ms = match (self.last_played_ms, other.last_played_ms) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self
    }
}

fn duration(ms: i64) -> String {
    let minutes = ms / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, 0) => tr!("under a minute"),
        (0, m) => tr!("{m} min", m),
        (h, 0) => tr!("{h} h", h),
        (h, m) => tr!("{h} h {m} min", h, m),
    }
}

/// "Played 12 h 30 min · 8 sessions · last played yesterday", or nothing before the first session.
pub fn describe(stats: &PlayStats, now: SystemTime) -> String {
    if stats.sessions == 0 {
        return String::new();
    }
    let sessions = tr!("{n} session" | "{n} sessions" % stats.sessions);
    let mut parts = vec![tr!("Played {}", duration(stats.total_ms)), sessions];
    if let Some(ms) = stats.last_played_ms {
        let when = crate::slots::when(
            UNIX_EPOCH + Duration::from_millis(u64::try_from(ms).unwrap_or(0)),
            now,
        );
        let mut chars = when.chars();
        let when: String = chars
            .next()
            .map(|c| c.to_lowercase().chain(chars).collect())
            .unwrap_or_default();
        parts.push(tr!("last played {when}", when));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;

    #[test]
    fn sessions_add_up() {
        let mut stats = PlayStats::default();
        stats.add(0, 2 * HOUR);
        stats.add(10 * HOUR, 10 * HOUR + 30 * 60_000);
        assert_eq!(stats.sessions, 2);
        assert_eq!(stats.total_ms, 2 * HOUR + 30 * 60_000);
        assert_eq!(stats.last_played_ms, Some(10 * HOUR + 30 * 60_000));
        let mut unsent = PlayStats::default();
        unsent.add(20 * HOUR, 21 * HOUR);
        let all = stats.merged(unsent);
        assert_eq!(
            (all.sessions, all.total_ms, all.last_played_ms),
            (3, 3 * HOUR + 30 * 60_000, Some(21 * HOUR))
        );
    }

    #[test]
    fn stats_read_as_one_line() {
        let now = UNIX_EPOCH + Duration::from_secs(1_790_850_000);
        let now_ms = 1_790_850_000_000;
        let stats = PlayStats {
            total_ms: 12 * HOUR + 30 * 60_000,
            sessions: 8,
            last_played_ms: Some(now_ms - 30 * HOUR),
        };
        assert_eq!(
            describe(&stats, now),
            "Played 12 h 30 min · 8 sessions · last played yesterday"
        );
        let once = PlayStats {
            total_ms: 45 * 60_000,
            sessions: 1,
            last_played_ms: Some(now_ms - 10_000),
        };
        assert_eq!(
            describe(&once, now),
            "Played 45 min · 1 session · last played just now"
        );
        assert_eq!(describe(&PlayStats::default(), now), "");
        assert_eq!(duration(2 * HOUR), "2 h");
        assert_eq!(duration(20_000), "under a minute");
    }
}
