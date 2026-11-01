use std::time::Duration;

/// Time spent in one running iteration, including pacing and work outside the core.
#[derive(Default)]
pub struct FrameTimes {
    pub sleep: Duration,
    pub controls: Duration,
    pub core: Duration,
    pub resume: Duration,
    pub readback: Duration,
    /// GPU wait and CPU conversion are details within readback, not additional loop stages.
    pub readback_timing: Option<(Duration, Duration)>,
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
    "frame copy",
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
    readback_frames: u32,
    gpu_wait: Duration,
    gpu_wait_max: Duration,
    cpu_convert: Duration,
    cpu_convert_max: Duration,
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
            readback_frames: 0,
            gpu_wait: Duration::ZERO,
            gpu_wait_max: Duration::ZERO,
            cpu_convert: Duration::ZERO,
            cpu_convert_max: Duration::ZERO,
            audio_frames: 0,
            fill: 0.0,
            fill_min: 1.0,
        }
    }

    /// Call only for running iterations: time spent paused must not dilute FPS or audio rate.
    pub fn record(&mut self, times: FrameTimes, audio_frames: u64, fill: f32) {
        if let Some((gpu_wait, cpu_convert)) = times.readback_timing {
            self.readback_frames += 1;
            self.gpu_wait += gpu_wait;
            self.gpu_wait_max = self.gpu_wait_max.max(gpu_wait);
            self.cpu_convert += cpu_convert;
            self.cpu_convert_max = self.cpu_convert_max.max(cpu_convert);
        }
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
                let mut stage = format!(
                    "{name} {:.1}/{:.1}",
                    ms(self.times[i]) / frames,
                    ms(self.maxima[i])
                );
                if i == 4 && self.readback_frames > 0 {
                    stage.push_str(&format!(
                        " (GPU wait {:.1}/{:.1}, CPU convert {:.1}/{:.1})",
                        ms(self.gpu_wait) / frames,
                        ms(self.gpu_wait_max),
                        ms(self.cpu_convert) / frames,
                        ms(self.cpu_convert_max)
                    ));
                }
                stage
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
                    readback_timing: Some((Duration::from_millis(100), Duration::from_millis(20))),
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
        assert!(
            line.contains("frame copy 250.0/250.0 (GPU wait 100.0/100.0, CPU convert 20.0/20.0)"),
            "{line}"
        );
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

    #[test]
    fn readback_details_share_the_frame_window_and_reset() {
        let mut stats = FrameStats::new(Duration::from_secs(1));
        stats.record(
            FrameTimes {
                total: Duration::from_millis(500),
                readback: Duration::from_millis(10),
                readback_timing: Some((Duration::from_millis(5), Duration::from_millis(2))),
                ..Default::default()
            },
            0,
            0.0,
        );
        assert_eq!(stats.report(), None);
        stats.record(
            FrameTimes {
                total: Duration::from_millis(500),
                ..Default::default()
            },
            0,
            0.0,
        );
        let line = stats.report().unwrap();
        assert!(
            line.contains("frame copy 5.0/10.0 (GPU wait 2.5/5.0, CPU convert 1.0/2.0)"),
            "{line}"
        );
        assert_eq!(stats.readback_frames, 0);
        stats.record(
            FrameTimes {
                total: Duration::from_secs(1),
                ..Default::default()
            },
            0,
            0.0,
        );
        let line = stats.report().unwrap();
        assert!(!line.contains("GPU wait"), "{line}");
    }
}
