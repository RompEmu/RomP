use std::time::{Duration, Instant};

pub struct FrameStats {
    window: Duration,
    started: Instant,
    frames: u32,
    run: Duration,
    run_max: Duration,
    readback: Duration,
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
    pub fn new(window: Duration, now: Instant) -> Self {
        Self {
            window,
            started: now,
            frames: 0,
            run: Duration::ZERO,
            run_max: Duration::ZERO,
            readback: Duration::ZERO,
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

    pub fn record(
        &mut self,
        run: Duration,
        readback: Duration,
        readback_timing: Option<(Duration, Duration)>,
        audio_frames: u64,
        fill: f32,
    ) {
        if let Some((gpu_wait, cpu_convert)) = readback_timing {
            self.readback_frames += 1;
            self.gpu_wait += gpu_wait;
            self.gpu_wait_max = self.gpu_wait_max.max(gpu_wait);
            self.cpu_convert += cpu_convert;
            self.cpu_convert_max = self.cpu_convert_max.max(cpu_convert);
        }
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
        let readback_detail = if self.readback_frames > 0 {
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            let frames = f64::from(frames);
            format!(
                " (GPU wait {:.1}/{:.1}, CPU convert {:.1}/{:.1} ms avg/max)",
                ms(self.gpu_wait) / frames,
                ms(self.gpu_wait_max),
                ms(self.cpu_convert) / frames,
                ms(self.cpu_convert_max)
            )
        } else {
            String::new()
        };
        let line = format!(
            "{:.1} fps; core {:.1} ms avg, {:.1} ms max; frame copy {:.1} ms avg{readback_detail}; audio {:.0} Hz, buffer {:.0}% avg, {:.0}% min",
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
                None,
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

    #[test]
    fn readback_details_share_the_frame_window_and_reset() {
        let start = Instant::now();
        let mut stats = FrameStats::new(Duration::from_secs(1), start);
        stats.record(
            Duration::ZERO,
            Duration::from_millis(10),
            Some((Duration::from_millis(5), Duration::from_millis(2))),
            0,
            0.0,
        );
        stats.record(
            Duration::ZERO,
            Duration::from_millis(10),
            Some((Duration::from_millis(3), Duration::from_millis(4))),
            0,
            0.0,
        );
        stats.record(Duration::ZERO, Duration::ZERO, None, 0, 0.0);
        assert_eq!(stats.report(start + Duration::from_millis(500)), None);
        let line = stats.report(start + Duration::from_secs(1)).unwrap();
        assert!(
            line.contains(
                "frame copy 6.7 ms avg (GPU wait 2.7/5.0, CPU convert 2.0/4.0 ms avg/max)"
            ),
            "{line}"
        );
        stats.record(Duration::ZERO, Duration::ZERO, None, 0, 0.0);
        let line = stats.report(start + Duration::from_secs(2)).unwrap();
        assert!(!line.contains("GPU wait"), "{line}");
        assert_eq!(stats.readback_frames, 0);
    }
}
