use crate::audio::AudioProducer;
use crate::input::InputState;
use cartridge_libretro as lr;
use ringbuf::traits::{Observer, Producer};
use std::time::{Duration, Instant};

pub struct VideoFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub pitch: usize,
}

pub struct Frontend {
    pub video: Option<VideoFrame>,
    pub video_dirty: bool,
    pub video_format: lr::PixelFormat,
    pub aspect: f32,
    pub audio: Option<AudioProducer>,
    pub input: InputState,
    pub shutdown: bool,
    pub hw_frame_dirty: bool,
    pub hw_frame_width: u32,
    pub hw_frame_height: u32,
}

impl Frontend {
    pub fn new() -> Self {
        Self {
            video: None,
            video_dirty: false,
            video_format: lr::PixelFormat::Xrgb8888,
            aspect: 0.0,
            audio: None,
            input: InputState::default(),
            shutdown: false,
            hw_frame_dirty: false,
            hw_frame_width: 0,
            hw_frame_height: 0,
        }
    }

    pub fn audio_fill(&self) -> f32 {
        match &self.audio {
            Some(a) => a.occupied_len() as f32 / a.capacity().get() as f32,
            None => 1.0,
        }
    }
}

impl Default for Frontend {
    fn default() -> Self {
        Self::new()
    }
}

impl lr::Frontend for Frontend {
    fn video_refresh_hw(&mut self, width: u32, height: u32) {
        self.hw_frame_dirty = true;
        self.hw_frame_width = width;
        self.hw_frame_height = height;
    }

    fn video_refresh(&mut self, frame: Option<lr::VideoFrame<'_>>) {
        let Some(f) = frame else { return };
        let take = (f.height as usize * f.pitch).min(f.data.len());
        let video = self.video.get_or_insert_with(|| VideoFrame {
            data: Vec::new(),
            width: 0,
            height: 0,
            pitch: 0,
        });
        video.data.clear();
        video.data.extend_from_slice(&f.data[..take]);
        video.width = f.width;
        video.height = f.height;
        video.pitch = f.pitch;
        self.video_dirty = true;
    }

    fn audio_sample_batch(&mut self, samples: &[i16]) -> usize {
        if let Some(a) = &mut self.audio {
            // Blocking here paces cores that emulate on their own thread.
            let deadline = Instant::now() + Duration::from_millis(500);
            let mut pushed = 0;
            while pushed < samples.len() && Instant::now() < deadline {
                pushed += a.push_slice(&samples[pushed..]);
                if pushed < samples.len() {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        samples.len() / 2
    }

    fn input_poll(&mut self) {}

    fn input_state(&mut self, port: u32, device: u32, index: u32, id: u32) -> i16 {
        match device {
            lr::RETRO_DEVICE_ANALOG => self.input.analog(port, index, id),
            lr::RETRO_DEVICE_KEYBOARD | lr::RETRO_DEVICE_MOUSE | lr::RETRO_DEVICE_POINTER => 0,
            _ => i16::from(self.input.is_pressed(port, id)),
        }
    }

    fn set_pixel_format(&mut self, fmt: lr::PixelFormat) -> bool {
        self.video_format = fmt;
        true
    }

    fn shutdown(&mut self) {
        self.shutdown = true;
    }

    fn set_geometry(&mut self, geometry: lr::Geometry) {
        self.aspect = geometry.aspect_ratio;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cartridge_libretro::Frontend as _;

    #[test]
    fn geometry_changes_update_the_aspect_ratio() {
        let mut frontend = Frontend::new();
        frontend.set_geometry(lr::Geometry {
            base_width: 640,
            base_height: 448,
            max_width: 640,
            max_height: 480,
            aspect_ratio: 4.0 / 3.0,
        });
        assert_eq!(frontend.aspect, 4.0 / 3.0);
    }
}
