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

pub const MOUSE_LEFT: u8 = 1;
pub const MOUSE_RIGHT: u8 = 2;
pub const MOUSE_MIDDLE: u8 = 4;

#[derive(Clone, Copy, Default, Debug)]
struct Mouse {
    pending: (i32, i32),
    frame: (i16, i16),
    buttons: u8,
}

#[derive(Clone, Default)]
pub struct InputState {
    ports: Arc<Mutex<[PadState; PORTS]>>,
    pointer: Arc<Mutex<Pointer>>,
    mouse: Arc<Mutex<Mouse>>,
    devices: Arc<Mutex<[u32; PORTS]>>,
    keys: Arc<Mutex<std::collections::HashSet<u32>>>,
}

impl InputState {
    pub fn state(&self, port: u32, device: u32, index: u32, id: u32) -> i16 {
        match device & lr::RETRO_DEVICE_MASK {
            lr::RETRO_DEVICE_ANALOG => self.analog(port, index, id),
            lr::RETRO_DEVICE_POINTER => self.pointer_state(id),
            lr::RETRO_DEVICE_MOUSE => self.mouse_state(port, id),
            lr::RETRO_DEVICE_LIGHTGUN => self.lightgun_state(port, id),
            lr::RETRO_DEVICE_KEYBOARD => self.key_state(id),
            _ => i16::from(self.is_pressed(port, id)),
        }
    }

    pub fn set_key(&self, code: u32, down: bool) {
        let mut keys = self.keys.lock();
        if down {
            keys.insert(code);
        } else {
            keys.remove(&code);
        }
    }

    pub fn key_state(&self, code: u32) -> i16 {
        i16::from(self.keys.lock().contains(&code))
    }

    pub fn set_port_device(&self, port: u32, device: u32) {
        if let Some(slot) = self.devices.lock().get_mut(port as usize) {
            *slot = device;
        }
    }

    fn port_uses(&self, port: u32, base: u32) -> bool {
        self.devices
            .lock()
            .get(port as usize)
            .is_some_and(|d| d & lr::RETRO_DEVICE_MASK == base)
    }

    pub fn lightgun_state(&self, port: u32, id: u32) -> i16 {
        let p = *self.pointer.lock();
        let m = *self.mouse.lock();
        let held = |bit: u8| m.buttons & bit != 0;
        let offscreen = held(MOUSE_RIGHT);
        match id {
            lr::RETRO_DEVICE_ID_LIGHTGUN_SCREEN_X => p.x,
            lr::RETRO_DEVICE_ID_LIGHTGUN_SCREEN_Y => p.y,
            lr::RETRO_DEVICE_ID_LIGHTGUN_X => m.frame.0,
            lr::RETRO_DEVICE_ID_LIGHTGUN_Y => m.frame.1,
            lr::RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN | lr::RETRO_DEVICE_ID_LIGHTGUN_RELOAD => {
                i16::from(offscreen)
            }
            lr::RETRO_DEVICE_ID_LIGHTGUN_TRIGGER => i16::from(held(MOUSE_LEFT) || offscreen),
            lr::RETRO_DEVICE_ID_LIGHTGUN_AUX_A => i16::from(held(MOUSE_MIDDLE)),
            lr::RETRO_DEVICE_ID_LIGHTGUN_AUX_B => {
                i16::from(self.is_pressed(port, lr::RETRO_DEVICE_ID_JOYPAD_A))
            }
            lr::RETRO_DEVICE_ID_LIGHTGUN_START | lr::RETRO_DEVICE_ID_LIGHTGUN_PAUSE => {
                i16::from(self.is_pressed(port, lr::RETRO_DEVICE_ID_JOYPAD_START))
            }
            lr::RETRO_DEVICE_ID_LIGHTGUN_SELECT => {
                i16::from(self.is_pressed(port, lr::RETRO_DEVICE_ID_JOYPAD_SELECT))
            }
            _ => 0,
        }
    }

    pub fn apply_mouse(&self, dx: i16, dy: i16, buttons: u8) {
        let mut m = self.mouse.lock();
        m.pending.0 += i32::from(dx);
        m.pending.1 += i32::from(dy);
        m.buttons = buttons;
    }

    pub fn latch_mouse(&self) {
        let mut m = self.mouse.lock();
        let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        m.frame = (clamp(m.pending.0), clamp(m.pending.1));
        m.pending = (0, 0);
    }

    pub fn mouse_state(&self, port: u32, id: u32) -> i16 {
        if port != 0 && !self.port_uses(port, lr::RETRO_DEVICE_MOUSE) {
            return 0;
        }
        let m = *self.mouse.lock();
        let button = |bit: u8| i16::from(m.buttons & bit != 0);
        match id {
            lr::RETRO_DEVICE_ID_MOUSE_X => m.frame.0,
            lr::RETRO_DEVICE_ID_MOUSE_Y => m.frame.1,
            lr::RETRO_DEVICE_ID_MOUSE_LEFT => button(MOUSE_LEFT),
            lr::RETRO_DEVICE_ID_MOUSE_RIGHT => button(MOUSE_RIGHT),
            lr::RETRO_DEVICE_ID_MOUSE_MIDDLE => button(MOUSE_MIDDLE),
            _ => 0,
        }
    }

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

    #[test]
    fn mouse_motion_is_latched_once_per_frame() {
        let input = InputState::default();
        input.apply_mouse(3, -2, MOUSE_LEFT);
        input.apply_mouse(4, 1, MOUSE_LEFT | MOUSE_RIGHT);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_X), 0);
        input.latch_mouse();
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_X), 7);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_Y), -1);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_X), 7);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_LEFT), 1);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_RIGHT), 1);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_MIDDLE), 0);
        assert_eq!(input.mouse_state(1, lr::RETRO_DEVICE_ID_MOUSE_LEFT), 0);
        input.latch_mouse();
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_X), 0);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_LEFT), 1);
    }

    #[test]
    fn mouse_devices_on_other_ports_get_the_mouse() {
        let input = InputState::default();
        input.apply_mouse(5, 0, MOUSE_LEFT);
        input.latch_mouse();
        assert_eq!(input.mouse_state(1, lr::RETRO_DEVICE_ID_MOUSE_X), 0);
        input.set_port_device(1, lr::RETRO_DEVICE_MOUSE | 0x100);
        assert_eq!(input.mouse_state(1, lr::RETRO_DEVICE_ID_MOUSE_X), 5);
        assert_eq!(input.mouse_state(0, lr::RETRO_DEVICE_ID_MOUSE_X), 5);
    }

    #[test]
    fn light_gun_aims_with_the_pointer_and_fires_with_mouse_buttons() {
        let input = InputState::default();
        input.apply_pointer(Pointer {
            x: -1000,
            y: 2000,
            pressed: false,
        });
        input.apply_mouse(0, 0, MOUSE_LEFT);
        input.apply_pad(0, pad(&[lr::RETRO_DEVICE_ID_JOYPAD_START], [0; 6]));
        let gun = |id| input.lightgun_state(0, id);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_SCREEN_X), -1000);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_SCREEN_Y), 2000);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_TRIGGER), 1);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN), 0);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_START), 1);
        input.apply_mouse(0, 0, MOUSE_RIGHT);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN), 1);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_RELOAD), 1);
        assert_eq!(gun(lr::RETRO_DEVICE_ID_LIGHTGUN_TRIGGER), 1);
    }

    #[test]
    fn held_keys_are_reported_until_released() {
        let input = InputState::default();
        input.set_key(97, true);
        input.set_key(273, true);
        assert_eq!(input.key_state(97), 1);
        assert_eq!(input.key_state(98), 0);
        input.set_key(97, false);
        assert_eq!(input.key_state(97), 0);
        assert_eq!(input.key_state(273), 1);
    }
}
