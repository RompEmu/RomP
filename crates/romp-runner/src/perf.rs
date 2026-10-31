use std::time::Duration;

/// Time spent in one running iteration, including pacing and work outside the core.
#[derive(Default)]
pub struct FrameTimes {
    pub sleep: Duration,
    pub controls: Duration,
    pub core: Duration,
    pub resume: Duration,
    pub readback: Duration,
    pub publish: Duration,
    pub audio: Duration,
    pub saves: Duration,
    pub achievements: Duration,
    pub total: Duration,
}

const STAGES: [&str; 10] = [
    "sleep",
    "controls",
    "core",
    "resume",
    "GPU readback",
    "frame publish",
    "audio",
    "saves",
    "achievements",
    "other",
];

pub struct FrameStats {
    window: Duration,
    elapsed: Duration,
    frames: u32,
    times: [Duration; 10],
    maxima: [Duration; 10],
    loop_max: Duration,
    audio_frames: u64,
    fill: f64,
    fill_min: f32,
}

impl FrameStats {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            elapsed: Duration::ZERO,
            frames: 0,
            times: [Duration::ZERO; 10],
            maxima: [Duration::ZERO; 10],
            loop_max: Duration::ZERO,
            audio_frames: 0,
            fill: 0.0,
            fill_min: 1.0,
        }
    }

    /// Call only for running iterations: time spent paused must not dilute FPS or audio rate.
    pub fn record(&mut self, times: FrameTimes, audio_frames: u64, fill: f32) {
        let mut stages = [
            times.sleep,
            times.controls,
            times.core,
            times.resume,
            times.readback,
            times.publish,
            times.audio,
            times.saves,
            times.achievements,
            Duration::ZERO,
        ];
        stages[9] = times.total.saturating_sub(stages.iter().sum());
        for (i, time) in stages.into_iter().enumerate() {
            self.times[i] += time;
            self.maxima[i] = self.maxima[i].max(time);
        }
        self.elapsed += times.total;
        self.loop_max = self.loop_max.max(times.total);
        self.frames += 1;
        self.audio_frames += audio_frames;
        self.fill += f64::from(fill);
        self.fill_min = self.fill_min.min(fill);
    }

    pub fn report(&mut self) -> Option<String> {
        if self.elapsed < self.window || self.frames == 0 {
            return None;
        }
        let frames = f64::from(self.frames);
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let stages = STAGES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                format!(
                    "{name} {:.1}/{:.1}",
                    ms(self.times[i]) / frames,
                    ms(self.maxima[i])
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let line = format!(
            "{:.1} fps; loop {:.1}/{:.1} ms avg/max; stages ms avg/max: {stages}; audio {:.0} Hz, buffer {:.0}% avg, {:.0}% min",
            frames / self.elapsed.as_secs_f64(),
            ms(self.elapsed) / frames,
            ms(self.loop_max),
            self.audio_frames as f64 / self.elapsed.as_secs_f64(),
            self.fill * 100.0 / frames,
            self.fill_min * 100.0
        );
        *self = Self::new(self.window);
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_time_includes_maintenance_and_pacing() {
        let mut stats = FrameStats::new(Duration::from_secs(1));
        for _ in 0..2 {
            stats.record(
                FrameTimes {
                    total: Duration::from_millis(500),
                    core: Duration::from_millis(2),
                    readback: Duration::from_millis(250),
                    saves: Duration::from_millis(100),
                    sleep: Duration::from_millis(10),
                    ..Default::default()
                },
                1000,
                0.25,
            );
        }
        let line = stats.report().unwrap();
        assert!(line.starts_with("2.0 fps; loop 500.0/500.0"), "{line}");
        assert!(line.contains("saves 100.0/100.0"), "{line}");
        assert!(line.contains("other 138.0/138.0"), "{line}");
        assert!(line.contains("audio 2000 Hz"), "{line}");
        assert_eq!(stats.report(), None);
    }

    #[test]
    fn only_running_iterations_advance_the_reporting_window() {
        let mut stats = FrameStats::new(Duration::from_secs(2));
        stats.record(
            FrameTimes {
                total: Duration::from_secs(1),
                ..Default::default()
            },
            100,
            0.5,
        );
        // Paused iterations have no record, even if report is polled during the pause.
        assert_eq!(stats.report(), None);
        assert_eq!(stats.report(), None);
        stats.record(
            FrameTimes {
                total: Duration::from_secs(1),
                ..Default::default()
            },
            100,
            0.5,
        );
        assert!(stats.report().unwrap().starts_with("1.0 fps;"));
    }
}
