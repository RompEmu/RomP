use cartridge_libretro as lr;
use cartridge_proto::msg::PadState;
use parking_lot::Mutex;
use std::sync::Arc;

pub const PORTS: usize = 8;
const BUTTONS: u32 = 16;
const LEFT_X: usize = 0;
const LEFT_Y: usize = 1;
const RIGHT_X: usize = 2;
const RIGHT_Y: usize = 3;
const L2: usize = 4;
const R2: usize = 5;
const TRIGGER_DIGITAL_THRESHOLD: i16 = 0x4000;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub x: i16,
    pub y: i16,
    pub pressed: bool,
}

#[derive(Clone, Default)]
pub struct InputState {
    ports: Arc<Mutex<[PadState; PORTS]>>,
    pointer: Arc<Mutex<Pointer>>,
}

impl InputState {
    pub fn apply_pointer(&self, pointer: Pointer) {
        *self.pointer.lock() = pointer;
    }

    pub fn pointer_state(&self, id: u32) -> i16 {
        let p = *self.pointer.lock();
        match id {
            lr::RETRO_DEVICE_ID_POINTER_X => p.x,
            lr::RETRO_DEVICE_ID_POINTER_Y => p.y,
            lr::RETRO_DEVICE_ID_POINTER_PRESSED | lr::RETRO_DEVICE_ID_POINTER_COUNT => {
                i16::from(p.pressed)
            }
            _ => 0,
        }
    }

    pub fn apply_pad(&self, port: u8, state: PadState) {
        if let Some(slot) = self.ports.lock().get_mut(port as usize) {
            *slot = state;
        }
    }

    fn get(&self, port: u32) -> Option<PadState> {
        self.ports.lock().get(port as usize).copied()
    }

    pub fn is_pressed(&self, port: u32, id: u32) -> bool {
        let Some(st) = self.get(port) else {
            return false;
        };
        if id >= BUTTONS {
            return false;
        }
        if st.buttons & (1 << id) != 0 {
            return true;
        }
        match id {
            lr::RETRO_DEVICE_ID_JOYPAD_L2 => st.axes[L2] >= TRIGGER_DIGITAL_THRESHOLD,
            lr::RETRO_DEVICE_ID_JOYPAD_R2 => st.axes[R2] >= TRIGGER_DIGITAL_THRESHOLD,
            _ => false,
        }
    }

    pub fn analog(&self, port: u32, index: u32, id: u32) -> i16 {
        let Some(st) = self.get(port) else {
            return 0;
        };
        let digital = |b: u32| i16::from(b < BUTTONS && st.buttons & (1 << b) != 0);
        let synth = |pos: u32, neg: u32| (digital(pos) - digital(neg)) * 0x7fff;
        let or = |v: i16, fallback: i16| if v != 0 { v } else { fallback };
        match (index, id) {
            (lr::RETRO_DEVICE_INDEX_ANALOG_LEFT, lr::RETRO_DEVICE_ID_ANALOG_X) => or(
                st.axes[LEFT_X],
                synth(
                    lr::RETRO_DEVICE_ID_JOYPAD_RIGHT,
                    lr::RETRO_DEVICE_ID_JOYPAD_LEFT,
                ),
            ),
            (lr::RETRO_DEVICE_INDEX_ANALOG_LEFT, lr::RETRO_DEVICE_ID_ANALOG_Y) => or(
                st.axes[LEFT_Y],
                synth(
                    lr::RETRO_DEVICE_ID_JOYPAD_DOWN,
                    lr::RETRO_DEVICE_ID_JOYPAD_UP,
                ),
            ),
            (lr::RETRO_DEVICE_INDEX_ANALOG_RIGHT, lr::RETRO_DEVICE_ID_ANALOG_X) => st.axes[RIGHT_X],
            (lr::RETRO_DEVICE_INDEX_ANALOG_RIGHT, lr::RETRO_DEVICE_ID_ANALOG_Y) => st.axes[RIGHT_Y],
            (lr::RETRO_DEVICE_INDEX_ANALOG_BUTTON, lr::RETRO_DEVICE_ID_JOYPAD_L2) => {
                st.axes[L2].max(digital(id) * 0x7fff)
            }
            (lr::RETRO_DEVICE_INDEX_ANALOG_BUTTON, lr::RETRO_DEVICE_ID_JOYPAD_R2) => {
                st.axes[R2].max(digital(id) * 0x7fff)
            }
            (lr::RETRO_DEVICE_INDEX_ANALOG_BUTTON, _) => digital(id) * 0x7fff,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(buttons: &[u32], axes: [i16; 6]) -> PadState {
        PadState {
            buttons: buttons.iter().fold(0, |b, id| b | 1 << id),
            axes,
        }
    }

    #[test]
    fn buttons_follow_latest_pad_state() {
        let input = InputState::default();
        input.apply_pad(1, pad(&[lr::RETRO_DEVICE_ID_JOYPAD_A], [0; 6]));
        assert!(input.is_pressed(1, lr::RETRO_DEVICE_ID_JOYPAD_A));
        assert!(!input.is_pressed(0, lr::RETRO_DEVICE_ID_JOYPAD_A));
        input.apply_pad(1, PadState::default());
        assert!(!input.is_pressed(1, lr::RETRO_DEVICE_ID_JOYPAD_A));
    }

    #[test]
    fn out_of_range_port_or_id_is_ignored() {
        let input = InputState::default();
        input.apply_pad(PORTS as u8, pad(&[0], [0; 6]));
        assert!(!input.is_pressed(PORTS as u32, 0));
        assert!(!input.is_pressed(0, 99));
    }

    #[test]
    fn analog_trigger_past_threshold_reads_as_digital() {
        let input = InputState::default();
        input.apply_pad(0, pad(&[], [0, 0, 0, 0, 0x5000, 0x1000]));
        assert!(input.is_pressed(0, lr::RETRO_DEVICE_ID_JOYPAD_L2));
        assert!(!input.is_pressed(0, lr::RETRO_DEVICE_ID_JOYPAD_R2));
    }

    #[test]
    fn dpad_synthesises_left_stick_when_centred() {
        let input = InputState::default();
        input.apply_pad(
            0,
            pad(
                &[
                    lr::RETRO_DEVICE_ID_JOYPAD_RIGHT,
                    lr::RETRO_DEVICE_ID_JOYPAD_UP,
                ],
                [0; 6],
            ),
        );
        let x = input.analog(
            0,
            lr::RETRO_DEVICE_INDEX_ANALOG_LEFT,
            lr::RETRO_DEVICE_ID_ANALOG_X,
        );
        let y = input.analog(
            0,
            lr::RETRO_DEVICE_INDEX_ANALOG_LEFT,
            lr::RETRO_DEVICE_ID_ANALOG_Y,
        );
        assert_eq!((x, y), (0x7fff, -0x7fff));
        input.apply_pad(
            0,
            pad(&[lr::RETRO_DEVICE_ID_JOYPAD_RIGHT], [-100, 0, 0, 0, 0, 0]),
        );
        assert_eq!(
            input.analog(
                0,
                lr::RETRO_DEVICE_INDEX_ANALOG_LEFT,
                lr::RETRO_DEVICE_ID_ANALOG_X
            ),
            -100
        );
    }

    #[test]
    fn pointer_reports_position_press_and_count() {
        let input = InputState::default();
        input.apply_pointer(Pointer {
            x: -100,
            y: 200,
            pressed: true,
        });
        assert_eq!(input.pointer_state(lr::RETRO_DEVICE_ID_POINTER_X), -100);
        assert_eq!(input.pointer_state(lr::RETRO_DEVICE_ID_POINTER_Y), 200);
        assert_eq!(input.pointer_state(lr::RETRO_DEVICE_ID_POINTER_PRESSED), 1);
        assert_eq!(input.pointer_state(lr::RETRO_DEVICE_ID_POINTER_COUNT), 1);
        input.apply_pointer(Pointer {
            x: -100,
            y: 200,
            pressed: false,
        });
        assert_eq!(input.pointer_state(lr::RETRO_DEVICE_ID_POINTER_PRESSED), 0);
        assert_eq!(input.pointer_state(lr::RETRO_DEVICE_ID_POINTER_COUNT), 0);
    }
}
