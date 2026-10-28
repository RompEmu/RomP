use crate::audio_pipe::{self, SharedAudio};
use crate::input::{InputState, PORTS};
use romp_libretro as lr;

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
    pub audio: SharedAudio,
    pub input: InputState,
    pub shutdown: bool,
    pub hw_frame_dirty: bool,
    pub hw_frame_width: u32,
    pub hw_frame_height: u32,
    /// Each port's strong and weak motor, as the core set them and as last sent to the app.
    rumble: [[u16; 2]; PORTS],
    sent_rumble: [[u16; 2]; PORTS],
}

impl Frontend {
    pub fn new() -> Self {
        Self {
            video: None,
            video_dirty: false,
            video_format: lr::PixelFormat::Xrgb8888,
            aspect: 0.0,
            audio: SharedAudio::default(),
            input: InputState::default(),
            shutdown: false,
            hw_frame_dirty: false,
            hw_frame_width: 0,
            hw_frame_height: 0,
            rumble: [[0; 2]; PORTS],
            sent_rumble: [[0; 2]; PORTS],
        }
    }
}

impl Frontend {
    /// The ports whose motors changed since last asked, with their strong and weak strengths.
    pub fn take_rumble(&mut self) -> Vec<(u32, u16, u16)> {
        let mut changed = Vec::new();
        for (port, (now, sent)) in self.rumble.iter().zip(&mut self.sent_rumble).enumerate() {
            if now != sent {
                *sent = *now;
                changed.push((port as u32, now[0], now[1]));
            }
        }
        changed
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
        audio_pipe::push(&self.audio, samples)
    }

    fn input_poll(&mut self) {
        self.input.latch_mouse();
    }

    fn input_state(&mut self, port: u32, device: u32, index: u32, id: u32) -> i16 {
        self.input.state(port, device, index, id)
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

    fn set_rumble(&mut self, port: u32, motor: lr::Motor, strength: u16) -> bool {
        let Some(motors) = self.rumble.get_mut(port as usize) else {
            return false;
        };
        motors[motor as usize] = strength;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use romp_libretro::Frontend as _;

    #[test]
    fn rumble_reports_each_port_once_per_change() {
        let mut frontend = Frontend::new();
        assert!(frontend.take_rumble().is_empty());
        assert!(frontend.set_rumble(1, lr::Motor::Strong, 40_000));
        assert!(frontend.set_rumble(1, lr::Motor::Weak, 9_000));
        assert_eq!(frontend.take_rumble(), [(1, 40_000, 9_000)]);
        assert!(frontend.take_rumble().is_empty());
        frontend.set_rumble(1, lr::Motor::Weak, 9_000);
        assert!(frontend.take_rumble().is_empty(), "no change, nothing to send");
        frontend.set_rumble(1, lr::Motor::Strong, 0);
        frontend.set_rumble(0, lr::Motor::Weak, 5);
        assert_eq!(frontend.take_rumble(), [(0, 0, 5), (1, 0, 9_000)]);
        assert!(!frontend.set_rumble(8, lr::Motor::Strong, 1));
    }

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
