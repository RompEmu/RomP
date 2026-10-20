use crate::audio::AudioProducer;
use crate::resample::{apply_volume, LowPass, RateControl, Resampler, TARGET_FILL};
use parking_lot::Mutex;
use ringbuf::traits::{Observer, Producer};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PUSH_TIMEOUT: Duration = Duration::from_millis(500);

pub struct AudioPipe {
    producer: Option<AudioProducer>,
    resampler: Resampler,
    low_pass: LowPass,
    /// Device frames per core frame, before the rate control nudges it.
    conversion: f64,
    rate: RateControl,
    volume: u8,
    frames: u64,
}

impl Default for AudioPipe {
    fn default() -> Self {
        Self {
            producer: None,
            resampler: Resampler::default(),
            low_pass: LowPass::new(1.0, 1.0),
            conversion: 1.0,
            rate: RateControl::default(),
            volume: 100,
            frames: 0,
        }
    }
}

pub type SharedAudio = Arc<Mutex<AudioPipe>>;

impl AudioPipe {
    pub fn set_producer(&mut self, producer: AudioProducer) {
        self.producer = Some(producer);
    }

    pub fn set_rates(&mut self, core: u32, device: u32) {
        self.conversion = f64::from(device) / f64::from(core);
        self.low_pass = LowPass::new(f64::from(core), f64::from(device));
    }

    /// Tops the buffer up with silence, at the start and after a pause has let it run dry, so the
    /// sound doesn't have to be stretched to catch up.
    pub fn refill(&mut self) {
        let Some(producer) = self.producer.as_mut() else {
            return;
        };
        let target = (producer.capacity().get() as f32 * TARGET_FILL) as usize & !1;
        let missing = target.saturating_sub(producer.occupied_len());
        producer.push_slice(&vec![0; missing]);
        self.rate.refilled();
    }

    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume;
    }

    pub fn take_frames(&mut self) -> u64 {
        std::mem::take(&mut self.frames)
    }

    pub fn take_stats(&mut self) -> (u64, f32) {
        (self.take_frames(), self.fill())
    }

    pub fn fill(&self) -> f32 {
        match &self.producer {
            Some(p) => p.occupied_len() as f32 / p.capacity().get() as f32,
            None => 1.0,
        }
    }
}

// Blocking here paces cores that emulate on their own thread; the lock is released while waiting.
pub fn push(audio: &SharedAudio, samples: &[i16]) -> usize {
    let frames = samples.len() / 2;
    let prepared = {
        let mut pipe = audio.lock();
        pipe.frames += frames as u64;
        if pipe.producer.is_none() {
            return frames;
        }
        let ratio = {
            let fill = pipe.fill();
            pipe.rate.ratio(fill) * pipe.conversion
        };
        let AudioPipe {
            resampler,
            low_pass,
            volume,
            ..
        } = &mut *pipe;
        let mut smooth = Vec::with_capacity(samples.len());
        low_pass.process(samples, &mut smooth);
        let mut out = Vec::with_capacity((samples.len() as f64 * ratio) as usize + 8);
        resampler.process(&smooth, ratio, &mut out);
        apply_volume(&mut out, *volume);
        out
    };
    let deadline = Instant::now() + PUSH_TIMEOUT;
    let mut pushed = 0;
    while pushed < prepared.len() {
        pushed += audio
            .lock()
            .producer
            .as_mut()
            .map_or(prepared.len() - pushed, |p| {
                p.push_slice(&prepared[pushed..])
            });
        if pushed < prepared.len() {
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::traits::Consumer;
    use ringbuf::{Cons, HeapRb, Prod};

    fn ring(capacity: usize) -> (AudioProducer, Cons<Arc<HeapRb<i16>>>) {
        let rb = Arc::new(HeapRb::<i16>::new(capacity));
        (Prod::new(rb.clone()), Cons::new(rb))
    }

    #[test]
    fn samples_from_several_threads_all_reach_the_output() {
        let (producer, mut consumer) = ring(4096);
        let audio: SharedAudio = Arc::default();
        audio.lock().set_producer(producer);
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let audio = audio.clone();
                std::thread::spawn(move || push(&audio, &[7; 200]))
            })
            .collect();
        let accepted: usize = threads.into_iter().map(|t| t.join().unwrap()).sum();
        assert_eq!(accepted, 200);
        assert_eq!(audio.lock().take_frames(), 200);
        let mut out = vec![0i16; 4096];
        let got = consumer.pop_slice(&mut out);
        assert!(got >= 400, "{got}");
        assert!(out[..got].iter().all(|s| *s == 7));
    }

    #[test]
    fn stats_report_frames_once_with_the_current_fill() {
        let (producer, _consumer) = ring(8);
        let audio: SharedAudio = Arc::default();
        audio.lock().set_producer(producer);
        push(&audio, &[1, 1]);
        let (frames, fill) = audio.lock().take_stats();
        assert_eq!(frames, 1);
        assert!(fill > 0.0);
        assert_eq!(audio.lock().take_stats().0, 0);
    }

    #[test]
    fn a_core_rate_the_device_lacks_is_converted() {
        let (producer, mut consumer) = ring(1 << 18);
        let audio: SharedAudio = Arc::default();
        audio.lock().set_producer(producer);
        audio.lock().set_rates(131_072, 48_000);
        for _ in 0..60 {
            push(&audio, &[100; 2 * 131_072 / 60]);
        }
        let mut out = vec![0i16; 1 << 18];
        let frames = consumer.pop_slice(&mut out) / 2;
        assert!((47_000..53_000).contains(&frames), "{frames}");
    }

    #[test]
    fn refilling_tops_the_buffer_up_to_its_target_with_silence() {
        let (producer, mut consumer) = ring(1000);
        let audio: SharedAudio = Arc::default();
        audio.lock().set_producer(producer);
        audio.lock().refill();
        assert!((audio.lock().fill() - TARGET_FILL).abs() < 0.01);
        audio.lock().refill();
        let mut out = vec![1i16; 1000];
        let got = consumer.pop_slice(&mut out);
        assert_eq!(got, 500);
        assert!(out[..got].iter().all(|s| *s == 0));
    }

    #[test]
    fn volume_applies_to_pushed_samples() {
        let (producer, mut consumer) = ring(64);
        let audio: SharedAudio = Arc::default();
        audio.lock().set_producer(producer);
        audio.lock().set_volume(50);
        push(&audio, &[1000, -1000]);
        let mut out = [0i16; 2];
        consumer.pop_slice(&mut out);
        assert_eq!(out, [500, -500]);
    }
}
