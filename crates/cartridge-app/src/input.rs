use cartridge_proto::msg::{AppMsg, PadState};
use slint::platform::Key;

pub const B: u32 = 0;
pub const Y: u32 = 1;
pub const SELECT: u32 = 2;
pub const START: u32 = 3;
pub const UP: u32 = 4;
pub const DOWN: u32 = 5;
pub const LEFT: u32 = 6;
pub const RIGHT: u32 = 7;
pub const A: u32 = 8;
pub const X: u32 = 9;
pub const L: u32 = 10;
pub const R: u32 = 11;
pub const L2: u32 = 12;
pub const R2: u32 = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Button(u32),
    SaveSlot(u8),
    LoadSlot(u8),
    Quit,
}

pub fn map_key(text: &str) -> Option<KeyAction> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let is = |k: Key| char::from(k) == c;
    let action = if is(Key::UpArrow) {
        KeyAction::Button(UP)
    } else if is(Key::DownArrow) {
        KeyAction::Button(DOWN)
    } else if is(Key::LeftArrow) {
        KeyAction::Button(LEFT)
    } else if is(Key::RightArrow) {
        KeyAction::Button(RIGHT)
    } else if is(Key::Return) {
        KeyAction::Button(START)
    } else if is(Key::Backspace) {
        KeyAction::Button(SELECT)
    } else if is(Key::F5) {
        KeyAction::SaveSlot(1)
    } else if is(Key::F7) {
        KeyAction::LoadSlot(1)
    } else if is(Key::Escape) {
        KeyAction::Quit
    } else {
        KeyAction::Button(match c.to_ascii_lowercase() {
            'z' => B,
            'x' => A,
            'a' => Y,
            's' => X,
            'q' => L,
            'w' => R,
            'd' => L2,
            'f' => R2,
            _ => return None,
        })
    };
    Some(action)
}

#[derive(Default)]
pub struct Pad {
    state: PadState,
}

impl Pad {
    pub fn set(&mut self, button: u32, pressed: bool) -> Option<PadState> {
        let before = self.state.buttons;
        if pressed {
            self.state.buttons |= 1 << button;
        } else {
            self.state.buttons &= !(1 << button);
        }
        (self.state.buttons != before).then_some(self.state)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Send(AppMsg),
    Quit,
}

#[derive(Default)]
pub struct Controls {
    pad: Pad,
}

impl Controls {
    pub fn key(&mut self, text: &str, pressed: bool, repeat: bool) -> Option<Option<Command>> {
        let action = map_key(text)?;
        let command = match action {
            KeyAction::Button(button) => self
                .pad
                .set(button, pressed)
                .map(|state| Command::Send(AppMsg::Pad { port: 0, state })),
            _ if !pressed || repeat => None,
            KeyAction::SaveSlot(slot) => Some(Command::Send(AppMsg::SaveSlot(slot))),
            KeyAction::LoadSlot(slot) => Some(Command::Send(AppMsg::LoadSlot(slot))),
            KeyAction::Quit => Some(Command::Quit),
        };
        Some(command)
    }

    pub fn release_all(&mut self) -> Option<Command> {
        if self.pad.state == PadState::default() {
            return None;
        }
        self.pad = Pad::default();
        Some(Command::Send(AppMsg::Pad {
            port: 0,
            state: PadState::default(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key) -> String {
        char::from(k).to_string()
    }

    #[test]
    fn arrows_map_to_dpad() {
        assert_eq!(map_key(&key(Key::UpArrow)), Some(KeyAction::Button(UP)));
        assert_eq!(map_key(&key(Key::DownArrow)), Some(KeyAction::Button(DOWN)));
        assert_eq!(map_key(&key(Key::LeftArrow)), Some(KeyAction::Button(LEFT)));
        assert_eq!(
            map_key(&key(Key::RightArrow)),
            Some(KeyAction::Button(RIGHT))
        );
    }

    #[test]
    fn letters_map_case_insensitively() {
        assert_eq!(map_key("x"), Some(KeyAction::Button(A)));
        assert_eq!(map_key("X"), Some(KeyAction::Button(A)));
        assert_eq!(map_key("z"), Some(KeyAction::Button(B)));
        assert_eq!(map_key("a"), Some(KeyAction::Button(Y)));
        assert_eq!(map_key("s"), Some(KeyAction::Button(X)));
    }

    #[test]
    fn start_select_and_hotkeys() {
        assert_eq!(map_key(&key(Key::Return)), Some(KeyAction::Button(START)));
        assert_eq!(
            map_key(&key(Key::Backspace)),
            Some(KeyAction::Button(SELECT))
        );
        assert_eq!(map_key(&key(Key::F5)), Some(KeyAction::SaveSlot(1)));
        assert_eq!(map_key(&key(Key::F7)), Some(KeyAction::LoadSlot(1)));
        assert_eq!(map_key(&key(Key::Escape)), Some(KeyAction::Quit));
    }

    #[test]
    fn unknown_or_multi_char_is_none() {
        assert_eq!(map_key("p"), None);
        assert_eq!(map_key(""), None);
        assert_eq!(map_key("xz"), None);
    }

    #[test]
    fn pad_reports_only_changes() {
        let mut pad = Pad::default();
        assert_eq!(pad.set(A, true).unwrap().buttons, 1 << A);
        assert_eq!(pad.set(A, true), None);
        assert_eq!(pad.set(B, true).unwrap().buttons, 1 << A | 1 << B);
        assert_eq!(pad.set(A, false).unwrap().buttons, 1 << B);
    }

    fn f5() -> String {
        key(Key::F5)
    }

    #[test]
    fn held_hotkey_fires_once() {
        let mut controls = Controls::default();
        assert_eq!(
            controls.key(&f5(), true, false),
            Some(Some(Command::Send(AppMsg::SaveSlot(1))))
        );
        assert_eq!(controls.key(&f5(), true, true), Some(None));
        assert_eq!(controls.key(&f5(), false, false), Some(None));
    }

    #[test]
    fn buttons_send_pad_state_once_per_change() {
        let mut controls = Controls::default();
        let pressed = PadState {
            buttons: 1 << A,
            axes: [0; 6],
        };
        assert_eq!(
            controls.key("x", true, false),
            Some(Some(Command::Send(AppMsg::Pad {
                port: 0,
                state: pressed
            })))
        );
        assert_eq!(controls.key("x", true, true), Some(None));
    }

    #[test]
    fn escape_quits_and_unknown_keys_are_not_handled() {
        let mut controls = Controls::default();
        assert_eq!(
            controls.key(&key(Key::Escape), true, false),
            Some(Some(Command::Quit))
        );
        assert_eq!(controls.key("p", true, false), None);
    }

    #[test]
    fn release_all_clears_held_buttons_once() {
        let mut controls = Controls::default();
        controls.key("x", true, false);
        controls.key(&key(Key::UpArrow), true, false);
        assert_eq!(
            controls.release_all(),
            Some(Command::Send(AppMsg::Pad {
                port: 0,
                state: PadState::default()
            }))
        );
        assert_eq!(controls.release_all(), None);
    }
}
