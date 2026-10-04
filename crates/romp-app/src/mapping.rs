use crate::gamepads::Button;
use crate::input::{A, B, DOWN, L, L2, L3, LEFT, R, R2, R3, RIGHT, SELECT, START, UP, X, Y};
use romp_proto::msg::PadState;
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

const PHYSICAL: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::LeftThumb,
    Button::RightThumb,
    Button::Start,
    Button::Select,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Mode,
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
    PHYSICAL.into_iter().find(|b| physical_name(*b) == name)
}

pub fn default_physical(button: u32) -> Button {
    PHYSICAL
        .into_iter()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    Pause,
    SaveState,
    LoadState,
    NextSlot,
    Fullscreen,
    Screenshot,
}

/// Each hotkey with the name it is saved under and the label people see.
pub const HOTKEYS: [(Hotkey, &str, &str); 6] = [
    (Hotkey::Pause, "pause", "Pause"),
    (Hotkey::SaveState, "save", "Save state"),
    (Hotkey::LoadState, "load", "Load state"),
    (Hotkey::NextSlot, "next_slot", "Next save slot"),
    (Hotkey::Fullscreen, "fullscreen", "Full screen"),
    (Hotkey::Screenshot, "screenshot", "Screenshot"),
];

fn hotkey_entry(hotkey: Hotkey) -> (Hotkey, &'static str, &'static str) {
    *HOTKEYS
        .iter()
        .find(|(h, _, _)| *h == hotkey)
        .expect("every hotkey is listed")
}

pub fn hotkey_label(hotkey: Hotkey) -> &'static str {
    hotkey_entry(hotkey).2
}

pub fn default_hotkey(hotkey: Hotkey) -> String {
    match hotkey {
        Hotkey::Pause => "p".into(),
        Hotkey::SaveState => key_char(Key::F5),
        Hotkey::LoadState => key_char(Key::F7),
        Hotkey::NextSlot => key_char(Key::F6),
        Hotkey::Fullscreen => key_char(Key::F11),
        Hotkey::Screenshot => key_char(Key::F12),
    }
}

/// The modifier keys held with a key press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

const MOD_PREFIXES: [(&str, &str); 4] = [
    ("ctrl+", "Ctrl+"),
    ("alt+", "Alt+"),
    ("shift+", "Shift+"),
    ("cmd+", "Cmd+"),
];

pub fn is_modifier(text: &str) -> bool {
    [
        Key::Shift,
        Key::ShiftR,
        Key::Control,
        Key::ControlR,
        Key::Alt,
        Key::AltGr,
        Key::Meta,
        Key::MetaR,
    ]
    .iter()
    .any(|k| key_char(*k) == text)
}

/// A key press with its modifiers, as shortcuts are stored, like `ctrl+s`.
pub fn combo(mods: Mods, text: &str) -> String {
    let mut chars = text.chars();
    let key = match (chars.next(), chars.next()) {
        (Some(c @ '\u{1}'..='\u{1a}'), None) if mods.ctrl => {
            char::from(c as u8 + b'a' - 1).to_string()
        }
        _ => normalize_key(text),
    };
    let held = [mods.ctrl, mods.alt, mods.shift, mods.meta];
    MOD_PREFIXES
        .iter()
        .zip(held)
        .filter(|(_, on)| *on)
        .map(|((prefix, _), _)| *prefix)
        .chain(std::iter::once(key.as_str()))
        .collect()
}

/// Splits a combination into its modifier prefixes and the key itself.
fn split_combo(combo: &str) -> (Vec<&'static str>, &str) {
    let mut rest = combo;
    let mut labels = Vec::new();
    for (prefix, label) in MOD_PREFIXES {
        if rest.len() > prefix.len() && rest.starts_with(prefix) {
            rest = &rest[prefix.len()..];
            labels.push(label);
        }
    }
    (labels, rest)
}

pub fn combo_label(combo: &str) -> String {
    let (mods, key) = split_combo(combo);
    format!("{}{}", mods.concat(), key_label(key))
}

/// Escape always opens the game menu, so it can never be taken by anything else.
pub fn is_reserved(text: &str) -> bool {
    let (_, key) = split_combo(text);
    key.is_empty() || key == key_char(Key::Escape)
}

/// What happened when a key was given to a button or hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assigned {
    Set,
    /// The key belonged to this button or hotkey, which now has the old key instead.
    Swapped(String),
    Reserved,
}

pub fn key_label(text: &str) -> String {
    const NAMES: [(Key, &str); 29] = [
        (Key::Escape, "Esc"),
        (Key::F1, "F1"),
        (Key::F2, "F2"),
        (Key::F3, "F3"),
        (Key::F4, "F4"),
        (Key::F5, "F5"),
        (Key::F6, "F6"),
        (Key::F7, "F7"),
        (Key::F8, "F8"),
        (Key::F9, "F9"),
        (Key::F10, "F10"),
        (Key::F11, "F11"),
        (Key::F12, "F12"),
        (Key::Delete, "Delete"),
        (Key::Home, "Home"),
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
    hotkeys: BTreeMap<String, String>,
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

    pub fn hotkey_key(&self, hotkey: Hotkey) -> String {
        self.hotkeys
            .get(hotkey_entry(hotkey).1)
            .cloned()
            .unwrap_or_else(|| default_hotkey(hotkey))
    }

    pub fn hotkey_for(&self, text: &str) -> Option<Hotkey> {
        let text = normalize_key(text);
        HOTKEYS
            .iter()
            .map(|(h, _, _)| *h)
            .find(|h| normalize_key(&self.hotkey_key(*h)) == text)
    }

    /// Hands `text` to `button`; whatever had it before gets the button's old key.
    pub fn set_key(&mut self, button: u32, text: &str) -> Assigned {
        if is_reserved(text) {
            return Assigned::Reserved;
        }
        let text = normalize_key(text);
        let previous = self.key_for(button);
        let outcome = self.give_away(&text, previous, Some(button), None);
        self.keyboard.insert(name(button).into(), text);
        outcome
    }

    /// Hands `text` to `hotkey`; whatever had it before gets the hotkey's old key.
    pub fn set_hotkey(&mut self, hotkey: Hotkey, text: &str) -> Assigned {
        if is_reserved(text) {
            return Assigned::Reserved;
        }
        let text = normalize_key(text);
        let previous = self.hotkey_key(hotkey);
        let outcome = self.give_away(&text, previous, None, Some(hotkey));
        self.hotkeys.insert(hotkey_entry(hotkey).1.into(), text);
        outcome
    }

    fn give_away(
        &mut self,
        text: &str,
        previous: String,
        button: Option<u32>,
        hotkey: Option<Hotkey>,
    ) -> Assigned {
        let plain = split_combo(text).0.is_empty();
        if let Some(other) = self
            .button_for_key(text)
            .filter(|b| plain && Some(*b) != button)
        {
            self.keyboard.insert(name(other).into(), previous);
            return Assigned::Swapped(name(other).into());
        }
        if let Some(other) = self.hotkey_for(text).filter(|h| Some(*h) != hotkey) {
            self.hotkeys.insert(hotkey_entry(other).1.into(), previous);
            return Assigned::Swapped(hotkey_label(other).into());
        }
        Assigned::Set
    }

    fn keys_clash(&self) -> bool {
        HOTKEYS
            .iter()
            .any(|(h, _, _)| self.button_for_key(&self.hotkey_key(*h)).is_some())
    }

    pub fn reset_keyboard(&mut self) {
        self.keyboard.clear();
        if self.keys_clash() {
            self.hotkeys.clear();
        }
    }

    pub fn reset_hotkeys(&mut self) {
        self.hotkeys.clear();
        if self.keys_clash() {
            self.keyboard.clear();
        }
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
        assert_eq!(m.set_key(A, "Z"), Assigned::Swapped("B".into()));
        assert_eq!(m.button_for_key("z"), Some(A));
        assert_eq!(m.button_for_key("x"), Some(B));
        assert_eq!(m.key_for(B), "x");
        m.reset_keyboard();
        assert_eq!(m.button_for_key("x"), Some(A));
    }

    #[test]
    fn hotkeys_default_to_the_function_keys_and_p() {
        let m = Mappings::default();
        assert_eq!(m.hotkey_for(&key(Key::F5)), Some(Hotkey::SaveState));
        assert_eq!(m.hotkey_for(&key(Key::F6)), Some(Hotkey::NextSlot));
        assert_eq!(m.hotkey_for(&key(Key::F7)), Some(Hotkey::LoadState));
        assert_eq!(m.hotkey_for(&key(Key::F11)), Some(Hotkey::Fullscreen));
        assert_eq!(m.hotkey_for(&key(Key::F12)), Some(Hotkey::Screenshot));
        assert_eq!(m.hotkey_for("P"), Some(Hotkey::Pause));
        assert_eq!(m.hotkey_for("x"), None);
        assert_eq!(m.hotkey_for(&key(Key::Escape)), None);
    }

    #[test]
    fn a_hotkey_and_a_button_trade_keys_instead_of_sharing_one() {
        let mut m = Mappings::default();
        assert_eq!(
            m.set_hotkey(Hotkey::Screenshot, "x"),
            Assigned::Swapped("A".into())
        );
        assert_eq!(m.hotkey_for("x"), Some(Hotkey::Screenshot));
        assert_eq!(m.key_for(A), key(Key::F12));
        assert_eq!(m.button_for_key("x"), None);

        assert_eq!(
            m.set_key(B, &key(Key::F5)),
            Assigned::Swapped("Save state".into())
        );
        assert_eq!(m.hotkey_key(Hotkey::SaveState), "z");
        assert_eq!(m.hotkey_for(&key(Key::F5)), None);

        assert_eq!(
            m.set_hotkey(Hotkey::Pause, &key(Key::F11)),
            Assigned::Swapped("Full screen".into())
        );
        assert_eq!(m.hotkey_key(Hotkey::Fullscreen), "p");
        assert_eq!(m.set_hotkey(Hotkey::Pause, "k"), Assigned::Set);
        let restored = Mappings::from_json(Some(&m.to_json()));
        assert_eq!(restored, m);
    }

    fn ctrl() -> Mods {
        Mods {
            ctrl: true,
            ..Mods::default()
        }
    }

    #[test]
    fn shortcuts_can_be_key_combinations() {
        assert_eq!(combo(ctrl(), "s"), "ctrl+s");
        assert_eq!(
            combo(ctrl(), "\u{13}"),
            "ctrl+s",
            "Ctrl+S may arrive as a control character"
        );
        let every = Mods {
            ctrl: true,
            alt: true,
            shift: true,
            meta: true,
        };
        assert_eq!(
            combo(every, &key(Key::F5)),
            format!("ctrl+alt+shift+cmd+{}", key(Key::F5))
        );
        assert_eq!(combo(Mods::default(), "P"), "p");
        assert!(is_modifier(&key(Key::Control)));
        assert!(!is_modifier("s"));
        assert_eq!(combo_label("ctrl+s"), "Ctrl+S");
        assert_eq!(
            combo_label(&combo(every, &key(Key::F5))),
            "Ctrl+Alt+Shift+Cmd+F5"
        );
        assert_eq!(combo_label("ctrl++"), "Ctrl++");
        assert_eq!(combo_label("p"), "P");
    }

    #[test]
    fn a_combination_leaves_its_plain_key_to_the_game() {
        let mut m = Mappings::default();
        assert_eq!(
            m.set_hotkey(Hotkey::SaveState, &combo(ctrl(), "s")),
            Assigned::Set
        );
        assert_eq!(m.hotkey_for("ctrl+s"), Some(Hotkey::SaveState));
        assert_eq!(m.hotkey_for("s"), None);
        assert_eq!(m.button_for_key("s"), Some(X), "S still presses X");
        assert_eq!(m.set_key(Y, "s"), Assigned::Swapped("X".into()));
        assert_eq!(m.hotkey_key(Hotkey::SaveState), "ctrl+s");
        assert_eq!(
            m.set_hotkey(Hotkey::Pause, "ctrl+s"),
            Assigned::Swapped("Save state".into())
        );
        assert_eq!(
            m.set_hotkey(Hotkey::Pause, &combo(ctrl(), &key(Key::Escape))),
            Assigned::Reserved
        );
    }

    #[test]
    fn escape_stays_the_menu_key() {
        let mut m = Mappings::default();
        assert_eq!(m.set_key(A, &key(Key::Escape)), Assigned::Reserved);
        assert_eq!(
            m.set_hotkey(Hotkey::Pause, &key(Key::Escape)),
            Assigned::Reserved
        );
        assert_eq!(m.set_hotkey(Hotkey::Pause, ""), Assigned::Reserved);
        assert_eq!(m.button_for_key("x"), Some(A));
        assert_eq!(m.hotkey_for("p"), Some(Hotkey::Pause));
    }

    #[test]
    fn resetting_one_side_never_leaves_two_meanings_on_a_key() {
        let mut m = Mappings::default();
        m.set_hotkey(Hotkey::Screenshot, "x");
        m.reset_keyboard();
        assert_eq!(m.button_for_key("x"), Some(A));
        assert_eq!(m.hotkey_for("x"), None);
        assert_eq!(m.hotkey_key(Hotkey::Screenshot), key(Key::F12));

        m.set_key(A, "p");
        m.reset_hotkeys();
        assert_eq!(m.hotkey_for("p"), Some(Hotkey::Pause));
        assert_eq!(m.key_for(A), "x");
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
        assert_eq!(key_label(&key(Key::F12)), "F12");
        assert_eq!(key_label(&key(Key::Escape)), "Esc");
        assert_eq!(key_label(""), "Not set");
        assert_eq!(name(L2), "L2");
    }
}
