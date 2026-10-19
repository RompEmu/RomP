pub const TARGET_FILL: f32 = 0.5;
pub const MAX_STRETCH: f64 = 0.08;

const SMOOTHING: f32 = 0.05;
const INTEGRAL_GAIN: f64 = 0.002;

fn fill_error(fill: f32) -> f64 {
    f64::from((TARGET_FILL - fill) / TARGET_FILL).clamp(-1.0, 1.0)
}

pub fn drc_ratio(fill: f32) -> f64 {
    1.0 + fill_error(fill) * MAX_STRETCH
}

pub struct RateControl {
    fill: f32,
    integral: f64,
}

impl Default for RateControl {
    fn default() -> Self {
        Self {
            fill: TARGET_FILL,
            integral: 0.0,
        }
    }
}

impl RateControl {
    pub fn ratio(&mut self, fill: f32) -> f64 {
        self.fill += (fill - self.fill) * SMOOTHING;
        let error = fill_error(self.fill);
        self.integral = (self.integral + error * INTEGRAL_GAIN).clamp(-MAX_STRETCH, MAX_STRETCH);
        (drc_ratio(self.fill) + self.integral).clamp(1.0 - MAX_STRETCH, 1.0 + MAX_STRETCH)
    }
}

pub fn apply_volume(samples: &mut [i16], volume: u8) {
    if volume >= 100 {
        return;
    }
    let scale = f32::from(volume) / 100.0;
    for s in samples {
        *s = (f32::from(*s) * scale).round() as i16;
    }
}

pub struct Resampler {
    prev: [f32; 2],
    pos: f64,
}

impl Default for Resampler {
    fn default() -> Self {
        Self {
            prev: [0.0; 2],
            pos: 1.0,
        }
    }
}

impl Resampler {
    pub fn process(&mut self, input: &[i16], ratio: f64, out: &mut Vec<i16>) {
        let frames = input.len() / 2;
        if frames == 0 {
            return;
        }
        let step = 1.0 / ratio.max(0.01);
        let frame = |i: usize, c: usize| -> f32 {
            if i == 0 {
                self.prev[c]
            } else {
                f32::from(input[(i - 1) * 2 + c])
            }
        };
        while self.pos <= frames as f64 {
            let i = self.pos.floor() as usize;
            let t = (self.pos - i as f64) as f32;
            for c in 0..2 {
                let a = frame(i, c);
                let b = if t > 0.0 { frame(i + 1, c) } else { a };
                out.push((a + (b - a) * t).round() as i16);
            }
            self.pos += step;
        }
        self.pos -= frames as f64;
        self.prev = [
            f32::from(input[(frames - 1) * 2]),
            f32::from(input[(frames - 1) * 2 + 1]),
        ];
    }
}

/// Cuts what a lower device rate can't carry before resampling down, so it doesn't fold back as noise.
pub struct LowPass {
    taps: Vec<f32>,
    history: Vec<[f32; 2]>,
    next: usize,
}

impl LowPass {
    const TAPS: usize = 63;

    pub fn new(from: f64, to: f64) -> Self {
        if to >= from {
            return Self {
                taps: vec![1.0],
                history: vec![[0.0; 2]],
                next: 0,
            };
        }
        // Below the device's Nyquist, leaving room for the filter's roll-off.
        let cutoff = 0.42 * to / from;
        let middle = (Self::TAPS - 1) as f64 / 2.0;
        let mut taps: Vec<f64> = (0..Self::TAPS)
            .map(|n| {
                let x = n as f64 - middle;
                let sinc = if x == 0.0 {
                    1.0
                } else {
                    let a = std::f64::consts::TAU * cutoff * x;
                    a.sin() / a
                };
                let w = std::f64::consts::TAU * n as f64 / (Self::TAPS - 1) as f64;
                sinc * (0.42 - 0.5 * w.cos() + 0.08 * (2.0 * w).cos())
            })
            .collect();
        let sum: f64 = taps.iter().sum();
        taps.iter_mut().for_each(|t| *t /= sum);
        Self {
            taps: taps.into_iter().map(|t| t as f32).collect(),
            history: vec![[0.0; 2]; Self::TAPS],
            next: 0,
        }
    }

    pub fn process(&mut self, input: &[i16], out: &mut Vec<i16>) {
        let len = self.taps.len();
        for frame in input.as_chunks::<2>().0 {
            self.history[self.next] = [f32::from(frame[0]), f32::from(frame[1])];
            self.next = (self.next + 1) % len;
            let mut acc = [0.0f32; 2];
            for (k, tap) in self.taps.iter().enumerate() {
                let h = self.history[(self.next + k) % len];
                acc[0] += h[0] * tap;
                acc[1] += h[1] * tap;
            }
            out.extend(acc.map(|v| v.round().clamp(-32768.0, 32767.0) as i16));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: i16) -> Vec<i16> {
        (0..frames).flat_map(|i| [i * 10, -i * 10]).collect()
    }

    #[test]
    fn unit_ratio_passes_samples_through() {
        let mut r = Resampler::default();
        let mut out = Vec::new();
        r.process(&ramp(8), 1.0, &mut out);
        assert_eq!(out, ramp(8));
    }

    #[test]
    fn stretching_interpolates_between_frames() {
        let mut r = Resampler::default();
        let mut out = Vec::new();
        r.process(&ramp(4), 2.0, &mut out);
        assert_eq!(
            out,
            [0, 0, 5, -5, 10, -10, 15, -15, 20, -20, 25, -25, 30, -30]
        );
    }

    #[test]
    fn output_is_continuous_across_batches() {
        let input = ramp(100);
        let mut whole = Vec::new();
        Resampler::default().process(&input, 1.05, &mut whole);
        let mut r = Resampler::default();
        let mut split = Vec::new();
        for chunk in input.chunks(14) {
            r.process(chunk, 1.05, &mut split);
        }
        assert_eq!(split, whole);
        let frames = split.len() / 2;
        assert!((104..=106).contains(&frames), "{frames}");
    }

    fn simulate(delivered: f64, ticks: usize) -> Vec<f32> {
        let capacity = 22_050.0;
        let consumed = 735.0;
        let mut ring = capacity * 0.4;
        let mut control = RateControl::default();
        (0..ticks)
            .map(|_| {
                let fill = (ring / capacity) as f32;
                let ratio = control.ratio(fill);
                ring = (ring + consumed * delivered * ratio).min(capacity);
                ring = (ring - consumed).max(0.0);
                fill
            })
            .collect()
    }

    #[test]
    fn rate_control_settles_near_target_when_a_core_drops_audio() {
        let fills = simulate(0.95, 6_000);
        let settled = &fills[3_000..];
        assert!(
            settled.iter().all(|f| (0.4..0.6).contains(f)),
            "{:?}",
            &settled[..5]
        );
    }

    #[test]
    fn rate_control_stays_neutral_for_a_healthy_core() {
        let fills = simulate(1.0, 6_000);
        assert!(fills[3_000..].iter().all(|f| (0.45..0.55).contains(f)));
        let mut control = RateControl::default();
        for _ in 0..1_000 {
            control.ratio(TARGET_FILL);
        }
        assert!((control.ratio(TARGET_FILL) - 1.0).abs() < 1e-9);
    }

    fn tone(hz: f64, rate: f64, frames: usize) -> Vec<i16> {
        (0..frames)
            .flat_map(|i| {
                let s = (8000.0 * (std::f64::consts::TAU * hz * i as f64 / rate).sin()) as i16;
                [s, s]
            })
            .collect()
    }

    fn rms(samples: &[i16]) -> f64 {
        (samples.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }

    fn filtered(input: &[i16], from: f64, to: f64) -> Vec<i16> {
        let mut out = Vec::new();
        LowPass::new(from, to).process(input, &mut out);
        out
    }

    #[test]
    fn low_pass_keeps_what_the_device_can_play() {
        let input = tone(1000.0, 131_072.0, 8192);
        let out = filtered(&input, 131_072.0, 48_000.0);
        assert_eq!(out.len(), input.len());
        let ratio = rms(&out[1024..]) / rms(&input[1024..]);
        assert!((0.97..1.03).contains(&ratio), "{ratio}");
    }

    #[test]
    fn low_pass_removes_what_would_fold_back_as_noise() {
        let input = tone(40_000.0, 131_072.0, 8192);
        let out = filtered(&input, 131_072.0, 48_000.0);
        let ratio = rms(&out[1024..]) / rms(&input[1024..]);
        assert!(ratio < 0.02, "{ratio}");
    }

    #[test]
    fn low_pass_is_continuous_across_batches() {
        let input = tone(5000.0, 131_072.0, 1000);
        let whole = filtered(&input, 131_072.0, 48_000.0);
        let mut lp = LowPass::new(131_072.0, 48_000.0);
        let mut split = Vec::new();
        for chunk in input.chunks(2 * 37) {
            lp.process(chunk, &mut split);
        }
        assert_eq!(split, whole);
    }

    #[test]
    fn volume_scales_samples_and_full_volume_is_untouched() {
        let mut samples = [1000, -1000, i16::MAX, i16::MIN];
        apply_volume(&mut samples, 100);
        assert_eq!(samples, [1000, -1000, i16::MAX, i16::MIN]);
        apply_volume(&mut samples, 50);
        assert_eq!(samples, [500, -500, 16384, -16384]);
        apply_volume(&mut samples, 0);
        assert_eq!(samples, [0; 4]);
    }

    #[test]
    fn ratio_stretches_when_the_buffer_runs_low() {
        assert_eq!(drc_ratio(TARGET_FILL), 1.0);
        assert!(drc_ratio(0.3) > 1.0);
        assert!(drc_ratio(0.7) < 1.0);
        assert_eq!(drc_ratio(0.0), 1.0 + MAX_STRETCH);
        assert_eq!(drc_ratio(1.0), 1.0 - MAX_STRETCH);
    }
}
