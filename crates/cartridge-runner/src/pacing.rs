use std::time::Duration;

pub fn paced_step(frame: Duration, fill: f32, nudge: f64) -> Duration {
    if fill < 0.25 {
        frame.mul_f64(1.0 - nudge)
    } else if fill < 0.5 {
        frame.mul_f64(1.0 - nudge / 2.0)
    } else if fill > 0.9 {
        frame.mul_f64(1.0 + nudge / 2.0)
    } else {
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_micros(16_667);

    #[test]
    fn steady_when_ring_is_healthy() {
        assert_eq!(paced_step(FRAME, 0.7, 0.02), FRAME);
    }

    #[test]
    fn runs_faster_when_ring_drains() {
        assert!(paced_step(FRAME, 0.1, 0.02) < paced_step(FRAME, 0.4, 0.02));
        assert!(paced_step(FRAME, 0.4, 0.02) < FRAME);
    }

    #[test]
    fn eases_off_when_ring_is_full() {
        assert!(paced_step(FRAME, 0.95, 0.02) > FRAME);
    }
}
