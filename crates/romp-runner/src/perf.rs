use std::time::{Duration, Instant};

pub struct FrameStats {
    window: Duration,
    started: Instant,
    frames: u32,
    run: Duration,
    run_max: Duration,
    readback: Duration,
    audio_frames: u64,
    fill: f64,
    fill_min: f32,
}

impl FrameStats {
    pub fn new(window: Duration, now: Instant) -> Self {
        Self {
            window,
            started: now,
            frames: 0,
            run: Duration::ZERO,
            run_max: Duration::ZERO,
            readback: Duration::ZERO,
            audio_frames: 0,
            fill: 0.0,
            fill_min: 1.0,
        }
    }

    pub fn record(&mut self, run: Duration, readback: Duration, audio_frames: u64, fill: f32) {
        self.frames += 1;
        self.audio_frames += audio_frames;
        self.fill += f64::from(fill);
        self.fill_min = self.fill_min.min(fill);
        self.run += run;
        self.run_max = self.run_max.max(run);
        self.readback += readback;
    }

    pub fn report(&mut self, now: Instant) -> Option<String> {
        let elapsed = now.duration_since(self.started);
        if elapsed < self.window {
            return None;
        }
        let frames = self.frames;
        let ms = |d: Duration| d.as_secs_f64() * 1000.0 / f64::from(frames.max(1));
        let line = format!(
            "{:.1} fps; core {:.1} ms avg, {:.1} ms max; frame copy {:.1} ms avg; audio {:.0} Hz, buffer {:.0}% avg, {:.0}% min",
            f64::from(frames) / elapsed.as_secs_f64(),
            ms(self.run),
            self.run_max.as_secs_f64() * 1000.0,
            ms(self.readback),
            self.audio_frames as f64 / elapsed.as_secs_f64(),
            self.fill * 100.0 / f64::from(frames.max(1)),
            self.fill_min * 100.0
        );
        *self = Self::new(self.window, now);
        (frames > 0).then_some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_once_per_window_and_resets() {
        let start = Instant::now();
        let mut stats = FrameStats::new(Duration::from_secs(5), start);
        for (ms, fill) in [(10, 0.5), (20, 0.25), (30, 0.75)] {
            stats.record(
                Duration::from_millis(ms),
                Duration::from_millis(2),
                1000,
                fill,
            );
        }
        assert_eq!(stats.report(start + Duration::from_secs(1)), None);
        assert_eq!(
            stats.report(start + Duration::from_secs(6)).as_deref(),
            Some("0.5 fps; core 20.0 ms avg, 30.0 ms max; frame copy 2.0 ms avg; audio 500 Hz, buffer 50% avg, 25% min")
        );
        assert_eq!(stats.report(start + Duration::from_secs(12)), None);
    }
}
