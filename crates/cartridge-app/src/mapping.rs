use crate::input::{A, B, DOWN, L, L2, L3, LEFT, R, R2, R3, RIGHT, SELECT, START, UP, X, Y};
use cartridge_proto::msg::PadState;
use gilrs::Button;
use serde::{Deserialize, Serialize};
use slint::platform::Key;
use std::collections::BTreeMap;

pub const BUTTONS: [(u32, &str); 16] = [
    (A, "A"),
    (B, "B"),
    (X, "X"),
    (Y, "Y"),
    (L, "L"),
    (R, "R"),
    (L2, "L2"),
    (R2, "R2"),
    (L3, "L3"),
    (R3, "R3"),
    (START, "Start"),
    (SELECT, "Select"),
    (UP, "Up"),
    (DOWN, "Down"),
    (LEFT, "Left"),
    (RIGHT, "Right"),
];

const PHYSICAL: [(Button, &str); 17] = [
    (Button::South, "Bottom face button"),
    (Button::East, "Right face button"),
    (Button::West, "Left face button"),
    (Button::North, "Top face button"),
    (Button::LeftTrigger, "Left bumper"),
    (Button::RightTrigger, "Right bumper"),
    (Button::LeftTrigger2, "Left trigger"),
    (Button::RightTrigger2, "Right trigger"),
    (Button::LeftThumb, "Left stick press"),
    (Button::RightThumb, "Right stick press"),
    (Button::Start, "Start"),
    (Button::Select, "Select"),
    (Button::DPadUp, "D-pad up"),
    (Button::DPadDown, "D-pad down"),
    (Button::DPadLeft, "D-pad left"),
    (Button::DPadRight, "D-pad right"),
    (Button::Mode, "Guide"),
];

pub fn name(button: u32) -> &'static str {
    BUTTONS
        .iter()
        .find(|(b, _)| *b == button)
        .map_or("?", |(_, n)| n)
}

fn physical_name(button: Button) -> String {
    format!("{button:?}")
}

fn physical_named(name: &str) -> Option<Button> {
    PHYSICAL
        .iter()
        .map(|(b, _)| *b)
        .find(|b| physical_name(*b) == name)
}

pub fn physical_label(button: Button) -> &'static str {
    PHYSICAL
        .iter()
        .find(|(b, _)| *b == button)
        .map_or("Unknown", |(_, label)| label)
}

pub fn default_physical(button: u32) -> Button {
    PHYSICAL
        .iter()
        .map(|(b, _)| *b)
        .find(|b| crate::input::retro_button(*b) == Some(button))
        .unwrap_or(Button::Unknown)
}

fn key_char(key: Key) -> String {
    char::from(key).to_string()
}

pub fn default_key(button: u32) -> String {
    match button {
        UP => key_char(Key::UpArrow),
        DOWN => key_char(Key::DownArrow),
        LEFT => key_char(Key::LeftArrow),
        RIGHT => key_char(Key::RightArrow),
        START => key_char(Key::Return),
        SELECT => key_char(Key::Backspace),
        B => "z".into(),
        A => "x".into(),
        Y => "a".into(),
        X => "s".into(),
        L => "q".into(),
        R => "w".into(),
        L2 => "d".into(),
        R2 => "f".into(),
        _ => String::new(),
    }
}

pub fn normalize_key(text: &str) -> String {
    text.to_lowercase()
}

pub fn is_hotkey(text: &str) -> bool {
    [Key::Escape, Key::F5, Key::F6, Key::F7, Key::F11]
        .iter()
        .any(|k| key_char(*k) == text)
        || normalize_key(text) == "p"
}

pub fn key_label(text: &str) -> String {
    const NAMES: [(Key, &str); 14] = [
        (Key::UpArrow, "↑"),
        (Key::DownArrow, "↓"),
        (Key::LeftArrow, "←"),
        (Key::RightArrow, "→"),
        (Key::Return, "Enter"),
        (Key::Backspace, "Backspace"),
        (Key::Tab, "Tab"),
        (Key::Space, "Space"),
        (Key::Shift, "Shift"),
        (Key::ShiftR, "Right Shift"),
        (Key::Control, "Ctrl"),
        (Key::ControlR, "Right Ctrl"),
        (Key::Alt, "Alt"),
        (Key::Meta, "Cmd"),
    ];
    if text.is_empty() {
        return "Not set".into();
    }
    NAMES
        .iter()
        .find(|(k, _)| key_char(*k) == text)
        .map_or_else(|| text.to_uppercase(), |(_, n)| (*n).to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mappings {
    keyboard: BTreeMap<String, String>,
    pads: BTreeMap<String, BTreeMap<String, String>>,
    pub nintendo_labels: bool,
    pub stick_dpad: bool,
}

impl Mappings {
    pub fn from_json(text: Option<&str>) -> Self {
        text.and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("mappings serialize")
    }

    pub fn key_for(&self, button: u32) -> String {
        self.keyboard
            .get(name(button))
            .cloned()
            .unwrap_or_else(|| default_key(button))
    }

    pub fn button_for_key(&self, text: &str) -> Option<u32> {
        let text = normalize_key(text);
        BUTTONS
            .iter()
            .map(|(b, _)| *b)
            .find(|b| normalize_key(&self.key_for(*b)) == text)
    }

    pub fn set_key(&mut self, button: u32, text: &str) -> bool {
        if text.is_empty() || is_hotkey(text) {
            return false;
        }
        let text = normalize_key(text);
        let previous = self.key_for(button);
        if let Some(other) = self.button_for_key(&text).filter(|b| *b != button) {
            self.keyboard.insert(name(other).into(), previous);
        }
        self.keyboard.insert(name(button).into(), text);
        true
    }

    pub fn reset_keyboard(&mut self) {
        self.keyboard.clear();
    }

    pub fn pad_button(&self, model: &str, button: u32) -> Button {
        self.pads
            .get(model)
            .and_then(|m| m.get(name(button)))
            .and_then(|p| physical_named(p))
            .unwrap_or_else(|| default_physical(button))
    }

    pub fn set_pad_button(&mut self, model: &str, button: u32, physical: Button) {
        let previous = self.pad_button(model, button);
        let other = BUTTONS
            .iter()
            .map(|(b, _)| *b)
            .find(|b| *b != button && self.pad_button(model, *b) == physical);
        let layout = self.pads.entry(model.to_string()).or_default();
        if let Some(other) = other {
            layout.insert(name(other).into(), physical_name(previous));
        }
        layout.insert(name(button).into(), physical_name(physical));
    }

    pub fn reset_pad(&mut self, model: &str) {
        self.pads.remove(model);
    }
}

pub fn model_of(pad_key: &str) -> &str {
    pad_key.rsplit_once('#').map_or(pad_key, |(model, _)| model)
}

fn swap_bits(buttons: u16, a: u32, b: u32) -> u16 {
    let bit_a = buttons >> a & 1;
    let bit_b = buttons >> b & 1;
    let cleared = buttons & !(1 << a) & !(1 << b);
    cleared | bit_a << b | bit_b << a
}

pub fn swap_face(state: PadState) -> PadState {
    PadState {
        buttons: swap_bits(swap_bits(state.buttons, A, B), X, Y),
        ..state
    }
}

const STICK_THRESHOLD: i16 = 16_000;

pub fn stick_to_dpad(state: PadState) -> PadState {
    let [x, y, ..] = state.axes;
    let mut buttons = state.buttons;
    for (on, button) in [
        (x < -STICK_THRESHOLD, LEFT),
        (x > STICK_THRESHOLD, RIGHT),
        (y < -STICK_THRESHOLD, UP),
        (y > STICK_THRESHOLD, DOWN),
    ] {
        if on {
            buttons |= 1 << button;
        }
    }
    PadState { buttons, ..state }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key) -> String {
        key_char(k)
    }

    #[test]
    fn defaults_match_the_built_in_layout() {
        let m = Mappings::default();
        assert_eq!(m.button_for_key("x"), Some(A));
        assert_eq!(m.button_for_key("X"), Some(A));
        assert_eq!(m.button_for_key(&key(Key::UpArrow)), Some(UP));
        assert_eq!(m.button_for_key("k"), None);
        assert_eq!(m.pad_button("model", A), Button::East);
        assert_eq!(m.pad_button("model", START), Button::Start);
    }

    #[test]
    fn rebinding_a_key_swaps_with_the_button_that_had_it() {
        let mut m = Mappings::default();
        assert!(m.set_key(A, "Z"));
        assert_eq!(m.button_for_key("z"), Some(A));
        assert_eq!(m.button_for_key("x"), Some(B));
        assert_eq!(m.key_for(B), "x");
        m.reset_keyboard();
        assert_eq!(m.button_for_key("x"), Some(A));
    }

    #[test]
    fn shortcut_keys_cannot_be_bound() {
        let mut m = Mappings::default();
        assert!(!m.set_key(A, &key(Key::Escape)));
        assert!(!m.set_key(A, "p"));
        assert!(!m.set_key(A, &key(Key::F5)));
        assert_eq!(m.button_for_key("x"), Some(A));
    }

    #[test]
    fn pad_layouts_are_per_model_and_swap_duplicates() {
        let mut m = Mappings::default();
        m.set_pad_button("xbox", A, Button::South);
        assert_eq!(m.pad_button("xbox", A), Button::South);
        assert_eq!(m.pad_button("xbox", B), Button::East);
        assert_eq!(m.pad_button("other", A), Button::East);
        let restored = Mappings::from_json(Some(&m.to_json()));
        assert_eq!(restored, m);
        m.reset_pad("xbox");
        assert_eq!(m.pad_button("xbox", A), Button::East);
    }

    #[test]
    fn identical_pads_share_a_model() {
        assert_eq!(model_of("0303#1"), "0303");
        assert_eq!(model_of("0303#2"), "0303");
    }

    fn pad(buttons: &[u32], axes: [i16; 6]) -> PadState {
        PadState {
            buttons: buttons.iter().fold(0, |b, id| b | 1 << id),
            axes,
        }
    }

    #[test]
    fn nintendo_labels_swap_the_face_buttons() {
        assert_eq!(swap_face(pad(&[A, Y], [0; 6])), pad(&[B, X], [0; 6]));
        assert_eq!(swap_face(pad(&[START], [0; 6])), pad(&[START], [0; 6]));
    }

    #[test]
    fn left_stick_can_press_the_dpad() {
        let axes = |x: i16, y: i16| [x, y, 0, 0, 0, 0];
        assert_eq!(stick_to_dpad(pad(&[], axes(-30000, 0))).buttons, 1 << LEFT);
        assert_eq!(stick_to_dpad(pad(&[], axes(0, -30000))).buttons, 1 << UP);
        assert_eq!(
            stick_to_dpad(pad(&[], axes(30000, 30000))).buttons,
            1 << RIGHT | 1 << DOWN
        );
        assert_eq!(stick_to_dpad(pad(&[], axes(8000, 0))).buttons, 0);
    }

    #[test]
    fn labels_are_readable() {
        assert_eq!(key_label(&key(Key::UpArrow)), "↑");
        assert_eq!(key_label("x"), "X");
        assert_eq!(key_label(""), "Not set");
        assert_eq!(physical_label(Button::South), "Bottom face button");
        assert_eq!(name(L2), "L2");
    }
}
