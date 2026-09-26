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

    #[test]
    fn ratio_stretches_when_the_buffer_runs_low() {
        assert_eq!(drc_ratio(TARGET_FILL), 1.0);
        assert!(drc_ratio(0.3) > 1.0);
        assert!(drc_ratio(0.7) < 1.0);
        assert_eq!(drc_ratio(0.0), 1.0 + MAX_STRETCH);
        assert_eq!(drc_ratio(1.0), 1.0 - MAX_STRETCH);
    }
}
