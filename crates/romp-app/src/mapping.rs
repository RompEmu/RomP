use crate::gamepads::Button;
use crate::gettext_noop;
use crate::input::{A, B, DOWN, L, L2, L3, LEFT, R, R2, R3, RIGHT, SELECT, START, UP, X, Y};
use crate::pad_labels::{Family, CHOICES};
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
    (START, gettext_noop!("Start")),
    (SELECT, gettext_noop!("Select")),
    (UP, gettext_noop!("Up")),
    (DOWN, gettext_noop!("Down")),
    (LEFT, gettext_noop!("Left")),
    (RIGHT, gettext_noop!("Right")),
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
    (Hotkey::Pause, "pause", gettext_noop!("Pause")),
    (Hotkey::SaveState, "save", gettext_noop!("Save state")),
    (Hotkey::LoadState, "load", gettext_noop!("Load state")),
    (
        Hotkey::NextSlot,
        "next_slot",
        gettext_noop!("Next save slot"),
    ),
    (
        Hotkey::Fullscreen,
        "fullscreen",
        gettext_noop!("Full screen"),
    ),
    (
        Hotkey::Screenshot,
        "screenshot",
        gettext_noop!("Screenshot"),
    ),
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
    /// A shortcut already uses the key, and a console's buttons cannot take it
    /// from the all-consoles shortcuts. Holds the shortcut's label.
    Taken(String),
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
        return tr::tr!("Not set");
    }
    NAMES
        .iter()
        .find(|(k, _)| key_char(*k) == text)
        .map_or_else(|| text.to_uppercase(), |(_, n)| (*n).to_string())
}

/// What one console changed on top of the all-consoles layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Layout {
    keyboard: BTreeMap<String, String>,
    pads: BTreeMap<String, BTreeMap<String, String>>,
}

/// The keyboard, or one pad model, of a layout.
#[derive(Debug, Clone, Copy)]
enum Side<'a> {
    Keyboard,
    Pad(&'a str),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mappings {
    keyboard: BTreeMap<String, String>,
    hotkeys: BTreeMap<String, String>,
    pads: BTreeMap<String, BTreeMap<String, String>>,
    labels: BTreeMap<String, String>,
    pub nintendo_labels: bool,
    pub stick_dpad: bool,
    consoles: BTreeMap<String, Layout>,
    #[serde(skip)]
    swaps: &'static [(u32, u32)],
}

/// Buttons that trade default places on a console, because its core reads the
/// ones on the top and left of the pad for what the game uses most. In plain
/// Wiimote mode dolphin_libretro reads the Wiimote's 1 and 2 from X and Y
/// (Input.cpp); in sideways mode it uses B and A for them instead.
fn builtin_swaps(console: &str) -> &'static [(u32, u32)] {
    match console {
        "wii" => &[(X, B), (Y, A)],
        _ => &[],
    }
}

impl Mappings {
    pub fn from_json(text: Option<&str>) -> Self {
        text.and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("mappings serialize")
    }

    /// The mappings in effect while `console` runs. Shortcuts and labels stay
    /// global, so only the read-only result of this ever differs per console.
    pub fn for_console(&self, console: &str) -> Mappings {
        self.resolve(console, true)
    }

    /// `with_changes` off gives the all-consoles layout without what `console`
    /// saved, which is what a console's own reset falls back to.
    fn resolve(&self, console: &str, with_changes: bool) -> Mappings {
        let mut out = self.clone();
        out.swaps = builtin_swaps(console);
        if let Some(layout) = self.consoles.get(console).filter(|_| with_changes) {
            out.keyboard.extend(layout.keyboard.clone());
            for (model, entries) in &layout.pads {
                out.pads
                    .entry(model.clone())
                    .or_default()
                    .extend(entries.clone());
            }
        }
        out
    }

    /// The button this one trades default places with, or itself.
    fn partner(&self, button: u32) -> u32 {
        for &(a, b) in self.swaps {
            if button == a {
                return b;
            }
            if button == b {
                return a;
            }
        }
        button
    }

    /// The button whose default place `button` takes, unless the user has
    /// touched its pair: then both fall back to the plain defaults, because a
    /// saved entry on one of them may have been handed the other's built-in
    /// place and keeping the swap would put two buttons on one key.
    fn default_source(&self, button: u32, saved: impl Fn(u32) -> bool) -> u32 {
        let partner = self.partner(button);
        if saved(button) || saved(partner) {
            button
        } else {
            partner
        }
    }

    pub fn key_for(&self, button: u32) -> String {
        self.keyboard.get(name(button)).cloned().unwrap_or_else(|| {
            let saved = |b| self.keyboard.contains_key(name(b));
            default_key(self.default_source(button, saved))
        })
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
        self.edit_everywhere(Side::Keyboard, |m| m.assign_key(button, text))
    }

    fn assign_key(&mut self, button: u32, text: &str) -> Assigned {
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
        self.edit_everywhere(Side::Keyboard, |m| {
            if is_reserved(text) {
                return Assigned::Reserved;
            }
            let text = normalize_key(text);
            let previous = m.hotkey_key(hotkey);
            let outcome = m.give_away(&text, previous, None, Some(hotkey));
            m.hotkeys.insert(hotkey_entry(hotkey).1.into(), text);
            outcome
        })
    }

    /// Runs an all-consoles edit, then mends every console it put two buttons
    /// on one place in: consoles save only what differs, so the edit can move
    /// a button a console follows onto a place the console saved.
    fn edit_everywhere<R>(&mut self, side: Side, edit: impl FnOnce(&mut Self) -> R) -> R {
        let before: Vec<(String, Mappings)> = self
            .consoles
            .keys()
            .map(|c| (c.clone(), self.for_console(c)))
            .collect();
        let outcome = edit(self);
        for (console, before) in before {
            self.keep_apart(&console, side, &before);
        }
        outcome
    }

    /// Gives each clashing button the console follows the place it had
    /// before, the same swap a console's own edit would make. Each round saves
    /// one more button, and saved buttons all keep their old, distinct places,
    /// so it ends; resetting that side of the console is the fallback anyway.
    fn keep_apart(&mut self, console: &str, side: Side, before: &Mappings) {
        for _ in 0..=BUTTONS.len() {
            let now = self.for_console(console);
            let troubled = now.troubled(side);
            if troubled.is_empty() {
                return;
            }
            let Some(button) = troubled
                .into_iter()
                .find(|b| !self.console_saves(console, side, *b))
            else {
                break;
            };
            // Its pair goes too, as saving one member moves the other back to
            // the plain default.
            for b in [button, now.partner(button)] {
                if !self.console_saves(console, side, b) {
                    let place = before.place(side, b);
                    self.console_side(console, side)
                        .insert(name(b).into(), place);
                }
            }
        }
        match side {
            Side::Keyboard => self.reset_console_keyboard(console),
            Side::Pad(model) => self.reset_console_pad(console, model),
        }
    }

    fn place(&self, side: Side, button: u32) -> String {
        match side {
            Side::Keyboard => self.key_for(button),
            Side::Pad(model) => physical_name(self.pad_button(model, button)),
        }
    }

    /// Buttons that share their place with another.
    fn troubled(&self, side: Side) -> Vec<u32> {
        let places: Vec<(u32, String)> = BUTTONS
            .iter()
            .map(|(b, _)| (*b, self.place(side, *b)))
            .collect();
        places
            .iter()
            .filter(|(b, p)| !p.is_empty() && places.iter().any(|(o, q)| o != b && q == p))
            .map(|(b, _)| *b)
            .collect()
    }

    fn console_saves(&self, console: &str, side: Side, button: u32) -> bool {
        self.consoles.get(console).is_some_and(|layout| match side {
            Side::Keyboard => layout.keyboard.contains_key(name(button)),
            Side::Pad(model) => layout
                .pads
                .get(model)
                .is_some_and(|m| m.contains_key(name(button))),
        })
    }

    fn console_side(&mut self, console: &str, side: Side) -> &mut BTreeMap<String, String> {
        let layout = self.consoles.entry(console.into()).or_default();
        match side {
            Side::Keyboard => &mut layout.keyboard,
            Side::Pad(model) => layout.pads.entry(model.into()).or_default(),
        }
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

    /// Writes the swapped defaults down so an edit sees every button's real
    /// place and its swapping keeps them apart.
    fn spell_out(&mut self, model: Option<&str>) {
        // Read every place before writing any: a written entry would switch its
        // pair back to the plain defaults mid-way.
        let places: Vec<(&str, String)> = BUTTONS
            .iter()
            .map(|(b, n)| match model {
                None => (*n, self.key_for(*b)),
                Some(model) => (*n, physical_name(self.pad_button(model, *b))),
            })
            .collect();
        let target = match model {
            None => &mut self.keyboard,
            Some(model) => self.pads.entry(model.into()).or_default(),
        };
        for (n, place) in places {
            target.insert(n.into(), place);
        }
        self.swaps = &[];
    }

    /// Edits run on the console's own layout so swaps stay inside it, and only
    /// what differs from the all-consoles layout is kept, so every other button
    /// still follows later changes to that layout.
    pub fn set_console_key(&mut self, console: &str, button: u32, text: &str) -> Assigned {
        if is_reserved(text) {
            return Assigned::Reserved;
        }
        if let Some(hotkey) = self.hotkey_for(&normalize_key(text)) {
            return Assigned::Taken(hotkey_label(hotkey).into());
        }
        let mut edited = self.for_console(console);
        edited.spell_out(None);
        let outcome = edited.assign_key(button, text);
        self.keep_console_keyboard(console, &edited);
        outcome
    }

    pub fn set_console_pad_button(
        &mut self,
        console: &str,
        model: &str,
        button: u32,
        physical: Button,
    ) {
        let mut edited = self.for_console(console);
        edited.spell_out(Some(model));
        edited.assign_pad_button(model, button, physical);
        self.keep_console_pad(console, model, &edited);
    }

    pub fn reset_console_keyboard(&mut self, console: &str) {
        if let Some(layout) = self.consoles.get_mut(console) {
            layout.keyboard.clear();
        }
        self.drop_empty_console(console);
    }

    pub fn reset_console_pad(&mut self, console: &str, model: &str) {
        if let Some(layout) = self.consoles.get_mut(console) {
            layout.pads.remove(model);
        }
        self.drop_empty_console(console);
    }

    fn keep_console_keyboard(&mut self, console: &str, edited: &Mappings) {
        let base = self.resolve(console, false);
        let changed = BUTTONS
            .iter()
            .map(|(b, _)| *b)
            .filter(|b| edited.key_for(*b) != base.key_for(*b))
            // A pair is saved whole, since one saved member makes the other
            // fall back to the plain default.
            .flat_map(|b| [b, base.partner(b)])
            .map(|b| (name(b).to_string(), edited.key_for(b)))
            .collect();
        self.consoles.entry(console.into()).or_default().keyboard = changed;
        self.drop_empty_console(console);
    }

    fn keep_console_pad(&mut self, console: &str, model: &str, edited: &Mappings) {
        let base = self.resolve(console, false);
        let changed: BTreeMap<String, String> = BUTTONS
            .iter()
            .map(|(b, _)| *b)
            .filter(|b| edited.pad_button(model, *b) != base.pad_button(model, *b))
            .flat_map(|b| [b, base.partner(b)])
            .map(|b| {
                (
                    name(b).to_string(),
                    physical_name(edited.pad_button(model, b)),
                )
            })
            .collect();
        let layout = self.consoles.entry(console.into()).or_default();
        if changed.is_empty() {
            layout.pads.remove(model);
        } else {
            layout.pads.insert(model.into(), changed);
        }
        self.drop_empty_console(console);
    }

    fn drop_empty_console(&mut self, console: &str) {
        if self
            .consoles
            .get(console)
            .is_some_and(|l| *l == Layout::default())
        {
            self.consoles.remove(console);
        }
    }

    fn keys_clash(&self) -> bool {
        HOTKEYS
            .iter()
            .any(|(h, _, _)| self.button_for_key(&self.hotkey_key(*h)).is_some())
    }

    pub fn reset_keyboard(&mut self) {
        self.edit_everywhere(Side::Keyboard, |m| {
            m.keyboard.clear();
            if m.keys_clash() {
                m.hotkeys.clear();
            }
        });
    }

    pub fn reset_hotkeys(&mut self) {
        self.edit_everywhere(Side::Keyboard, |m| {
            m.hotkeys.clear();
            if m.keys_clash() {
                m.keyboard.clear();
            }
        });
    }

    pub fn pad_button(&self, model: &str, button: u32) -> Button {
        self.pads
            .get(model)
            .and_then(|m| m.get(name(button)))
            .and_then(|p| physical_named(p))
            .unwrap_or_else(|| {
                let saved = |b| {
                    self.pads
                        .get(model)
                        .is_some_and(|m| m.contains_key(name(b)))
                };
                default_physical(self.default_source(button, saved))
            })
    }

    pub fn set_pad_button(&mut self, model: &str, button: u32, physical: Button) {
        self.edit_everywhere(Side::Pad(model), |m| {
            m.assign_pad_button(model, button, physical);
        });
    }

    fn assign_pad_button(&mut self, model: &str, button: u32, physical: Button) {
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
        self.edit_everywhere(Side::Pad(model), |m| {
            m.pads.remove(model);
            m.labels.remove(model);
        });
    }

    /// The family whose labels a model was given instead of its own.
    pub fn labels(&self, model: &str) -> Option<Family> {
        self.labels.get(model).and_then(|f| Family::saved_as(f))
    }

    pub fn set_labels(&mut self, model: &str, family: Option<Family>) {
        let saved = family.and_then(|family| {
            CHOICES
                .iter()
                .find(|(f, _, _)| *f == family)
                .map(|(_, name, _)| *name)
        });
        match saved {
            Some(name) => self.labels.insert(model.into(), name.into()),
            None => self.labels.remove(model),
        };
    }

    pub fn family(&self, model: &str, detected: Family) -> Family {
        self.labels(model).unwrap_or(detected)
    }
}

pub fn model_of(pad_key: &str) -> &str {
    pad_key.rsplit_once('#').map_or(pad_key, |(model, _)| model)
}

/// A menu button as the person sees it: left and right trade places when the layout is mirrored.
pub fn as_seen(button: u32, rtl: bool) -> u32 {
    match button {
        LEFT if rtl => RIGHT,
        RIGHT if rtl => LEFT,
        other => other,
    }
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
    fn settings_saved_before_consoles_existed_still_load() {
        let m = Mappings::from_json(Some(r#"{"keyboard":{"A":"q"}}"#));
        assert_eq!(m.key_for(A), "q");
        assert_eq!(m.for_console("snes").key_for(A), "q");
    }

    #[test]
    fn a_console_nobody_changed_follows_the_all_consoles_layout() {
        let mut m = Mappings::default();
        m.set_key(B, "i");
        m.set_pad_button("pad", B, Button::LeftTrigger);
        for console in ["nes", "anything-unknown"] {
            let c = m.for_console(console);
            assert_eq!(c.key_for(B), "i");
            assert_eq!(c.pad_button("pad", B), Button::LeftTrigger);
        }
    }

    #[test]
    fn for_console_keeps_shortcuts_global() {
        let mut m = Mappings::default();
        m.set_hotkey(Hotkey::Pause, "o");
        assert_eq!(m.for_console("nes").hotkey_key(Hotkey::Pause), "o");
    }

    #[test]
    fn console_layouts_survive_a_save() {
        let m = Mappings::from_json(Some(r#"{"consoles":{"nes":{"keyboard":{"A":"k"}}}}"#));
        assert_eq!(m.for_console("nes").key_for(A), "k");
        assert_eq!(m.for_console("snes").key_for(A), default_key(A));
        assert_eq!(Mappings::from_json(Some(&m.to_json())), m);
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
    fn a_model_can_show_the_labels_of_another_family() {
        use crate::pad_labels::Family;
        let mut m = Mappings::default();
        assert_eq!(m.family("sn30", Family::Ps4), Family::Ps4);
        m.set_labels("sn30", Some(Family::Nintendo));
        assert_eq!(m.labels("sn30"), Some(Family::Nintendo));
        assert_eq!(m.family("sn30", Family::Ps4), Family::Nintendo);
        assert_eq!(m.family("other", Family::Ps4), Family::Ps4);
        let restored = Mappings::from_json(Some(&m.to_json()));
        assert_eq!(restored, m);
        m.set_labels("sn30", None);
        assert_eq!(m.family("sn30", Family::Ps4), Family::Ps4);
        m.set_labels("sn30", Some(Family::Xbox));
        m.reset_pad("sn30");
        assert_eq!(m.labels("sn30"), None);
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

    #[test]
    fn menus_take_left_and_right_as_seen_when_reading_right_to_left() {
        assert_eq!(as_seen(LEFT, true), RIGHT);
        assert_eq!(as_seen(RIGHT, true), LEFT);
        assert_eq!(as_seen(UP, true), UP);
        assert_eq!(as_seen(LEFT, false), LEFT);
    }

    #[test]
    fn a_console_key_leaves_other_consoles_and_all_consoles_alone() {
        let mut m = Mappings::default();
        m.set_console_key("nes", A, "k");
        assert_eq!(m.for_console("nes").key_for(A), "k");
        assert_eq!(m.for_console("snes").key_for(A), default_key(A));
        assert_eq!(m.key_for(A), default_key(A));
    }

    #[test]
    fn a_console_key_swaps_inside_the_console() {
        let mut m = Mappings::default();
        assert_eq!(
            m.set_console_key("nes", A, &default_key(B)),
            Assigned::Swapped("B".into())
        );
        let nes = m.for_console("nes");
        assert_eq!(nes.key_for(A), default_key(B));
        assert_eq!(nes.key_for(B), default_key(A));
        assert_eq!(m.key_for(B), default_key(B));
    }

    #[test]
    fn buttons_a_console_never_touched_follow_later_changes() {
        let mut m = Mappings::default();
        m.set_console_key("nes", A, "k");
        m.set_key(Y, "j");
        assert_eq!(m.for_console("nes").key_for(Y), "j");
    }

    #[test]
    fn a_console_cannot_take_a_shortcut_key() {
        let mut m = Mappings::default();
        let before = m.to_json();
        assert_eq!(
            m.set_console_key("nes", A, "p"),
            Assigned::Taken("Pause".into())
        );
        assert_eq!(m.to_json(), before);
        assert_eq!(
            m.set_console_key("nes", A, &key(Key::Escape)),
            Assigned::Reserved
        );
        assert_eq!(m.to_json(), before);
    }

    #[test]
    fn a_console_pad_layout_is_per_console_and_model_and_swaps() {
        let mut m = Mappings::default();
        m.set_console_pad_button("snes", "pad", A, Button::RightTrigger);
        let snes = m.for_console("snes");
        assert_eq!(snes.pad_button("pad", A), Button::RightTrigger);
        assert_eq!(snes.pad_button("pad", R), default_physical(A));
        assert_eq!(
            m.for_console("nes").pad_button("pad", A),
            default_physical(A)
        );
        assert_eq!(m.pad_button("pad", A), default_physical(A));
        assert_eq!(snes.pad_button("other", A), default_physical(A));
    }

    #[test]
    fn resetting_a_console_returns_it_to_the_all_consoles_layout() {
        let mut m = Mappings::default();
        m.set_console_key("nes", A, &default_key(B));
        m.set_console_pad_button("snes", "pad", A, Button::RightTrigger);
        m.reset_console_keyboard("nes");
        m.reset_console_pad("snes", "pad");
        assert_eq!(m.for_console("nes"), m.resolve("nes", false));
        assert_eq!(m.for_console("snes"), m.resolve("snes", false));
        let json = m.to_json();
        assert!(!json.contains("nes"), "{json}");
    }

    #[test]
    fn wii_puts_its_main_buttons_on_the_bottom_and_right() {
        let m = Mappings::default();
        let wii = m.for_console("wii");
        assert_eq!(wii.pad_button("pad", X), default_physical(B));
        assert_eq!(wii.pad_button("pad", Y), default_physical(A));
        assert_eq!(wii.pad_button("pad", B), default_physical(X));
        assert_eq!(wii.pad_button("pad", A), default_physical(Y));
        assert_eq!(wii.key_for(X), default_key(B));
        assert_eq!(wii.key_for(Y), default_key(A));
        assert_eq!(wii.key_for(B), default_key(X));
        assert_eq!(wii.key_for(A), default_key(Y));
        for plain in [m.for_console("ngc"), m.clone()] {
            assert_eq!(plain.pad_button("pad", X), default_physical(X));
            assert_eq!(plain.key_for(A), default_key(A));
        }
    }

    #[test]
    fn a_layout_you_set_for_everything_beats_the_wii_default() {
        let mut m = Mappings::default();
        m.set_pad_button("pad", X, Button::LeftTrigger);
        let wii = m.for_console("wii");
        assert_eq!(wii.pad_button("pad", X), Button::LeftTrigger);
        assert_eq!(wii.pad_button("pad", Y), default_physical(A));
    }

    #[test]
    fn changing_one_wii_button_keeps_the_rest_of_the_wii_default() {
        let mut m = Mappings::default();
        m.set_console_pad_button("wii", "pad", L, Button::Mode);
        let wii = m.for_console("wii");
        assert_eq!(wii.pad_button("pad", L), Button::Mode);
        assert_eq!(wii.pad_button("pad", X), default_physical(B));
        m.reset_console_pad("wii", "pad");
        let wii = m.for_console("wii");
        assert_eq!(wii.pad_button("pad", L), default_physical(L));
        assert_eq!(wii.pad_button("pad", X), default_physical(B));
    }

    #[test]
    fn wii_reset_returns_to_the_wii_default_not_the_global_one() {
        let mut m = Mappings::default();
        m.set_console_key("wii", A, "k");
        m.reset_console_keyboard("wii");
        assert_eq!(m.for_console("wii").key_for(A), default_key(Y));
    }

    fn all_distinct<T: Ord>(values: impl Iterator<Item = T>) -> bool {
        let values: Vec<T> = values.collect();
        let unique: std::collections::BTreeSet<&T> = values.iter().collect();
        unique.len() == values.len()
    }

    const MODELS: [&str; 2] = ["pad", "other"];

    fn no_clashes(m: &Mappings) -> bool {
        MODELS.iter().all(|model| {
            all_distinct(
                BUTTONS
                    .iter()
                    .map(|(b, _)| physical_name(m.pad_button(model, *b))),
            )
        }) && all_distinct(
            BUTTONS
                .iter()
                .map(|(b, _)| m.key_for(*b))
                .filter(|k| !k.is_empty()),
        )
    }

    #[test]
    fn a_partial_remap_for_everything_never_doubles_up_a_wii_button() {
        let mut m = Mappings::default();
        m.set_pad_button("pad", X, Button::LeftTrigger);
        m.set_key(X, "k");
        assert!(no_clashes(&m.for_console("wii")));
        let mut m = Mappings::default();
        m.set_pad_button("pad", B, Button::Start);
        m.set_key(B, "k");
        assert!(no_clashes(&m.for_console("wii")));
    }

    #[test]
    fn a_wii_edit_never_doubles_up_a_button_and_leaves_others_alone() {
        let mut m = Mappings::default();
        m.set_console_pad_button("wii", "pad", X, Button::LeftTrigger);
        m.set_console_key("wii", X, "k");
        let wii = m.for_console("wii");
        assert!(no_clashes(&wii));
        assert_eq!(wii.pad_button("pad", X), Button::LeftTrigger);
        assert_eq!(wii.key_for(X), "k");
        assert_eq!(m.pad_button("pad", X), default_physical(X));
        assert_eq!(m.for_console("ngc").key_for(X), default_key(X));
    }

    #[test]
    fn setting_a_wii_button_to_its_built_in_place_stores_nothing() {
        let mut m = Mappings::default();
        m.set_console_pad_button("wii", "pad", X, default_physical(B));
        m.set_console_key("wii", X, &default_key(B));
        assert!(!m.to_json().contains("wii"), "{}", m.to_json());
    }

    fn assert_consoles_apart(m: &Mappings) {
        for console in m.consoles.keys() {
            assert!(
                no_clashes(&m.for_console(console)),
                "{console}: {}",
                m.to_json()
            );
        }
    }

    #[test]
    fn a_later_key_for_everything_never_doubles_up_a_console_key() {
        let mut m = Mappings::default();
        m.set_console_key("nes", A, "k");
        assert_consoles_apart(&m);
        m.set_key(B, "k");
        assert_consoles_apart(&m);
        let nes = m.for_console("nes");
        assert_eq!(nes.key_for(A), "k");
        assert_eq!(nes.key_for(B), default_key(B));
        assert_eq!(m.key_for(B), "k");
    }

    #[test]
    fn a_later_pad_button_for_everything_never_doubles_up_a_console_button() {
        let mut m = Mappings::default();
        m.set_console_pad_button("snes", "pad", A, Button::RightTrigger);
        assert_consoles_apart(&m);
        m.set_pad_button("pad", START, Button::RightTrigger);
        assert_consoles_apart(&m);
        let snes = m.for_console("snes");
        assert_eq!(snes.pad_button("pad", A), Button::RightTrigger);
        assert_eq!(snes.pad_button("pad", START), Button::Start);
        assert_eq!(m.pad_button("pad", START), Button::RightTrigger);
    }

    #[test]
    fn a_later_pad_button_for_everything_keeps_a_saved_wii_pair_apart() {
        let mut m = Mappings::default();
        m.set_console_pad_button("wii", "pad", X, Button::East);
        let wii = m.for_console("wii");
        assert_eq!(
            [X, B, Y, A].map(|b| wii.pad_button("pad", b)),
            [Button::East, Button::North, Button::South, Button::West]
        );
        m.set_pad_button("pad", B, Button::LeftTrigger);
        assert_consoles_apart(&m);
        assert_eq!(m.for_console("wii").pad_button("pad", Y), Button::South);
    }

    #[test]
    fn putting_a_console_button_back_stores_nothing() {
        let mut m = Mappings::default();
        m.set_console_key("nes", A, "k");
        m.set_console_key("nes", A, &default_key(A));
        m.set_console_pad_button("nes", "pad", A, Button::LeftTrigger);
        m.set_console_pad_button("nes", "pad", A, default_physical(A));
        assert!(!m.to_json().contains("nes"), "{}", m.to_json());
    }

    #[test]
    fn resetting_one_console_leaves_the_others_alone() {
        let mut m = Mappings::default();
        m.set_console_key("nes", A, "k");
        m.set_console_key("snes", A, "j");
        m.set_console_pad_button("nes", "pad", A, Button::LeftTrigger);
        m.set_console_pad_button("snes", "pad", A, Button::Mode);
        let snes = m.consoles["snes"].clone();
        m.reset_console_keyboard("nes");
        m.reset_console_pad("nes", "pad");
        assert_eq!(m.consoles["snes"], snes);
        assert!(!m.consoles.contains_key("nes"));
    }

    /// A small deterministic generator, so a failing sequence repeats.
    struct Lcg(u64);

    impl Lcg {
        fn pick(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) as usize % n
        }
    }

    fn random_edit(m: &mut Mappings, rng: &mut Lcg) -> String {
        const CONSOLES: [&str; 3] = ["nes", "snes", "wii"];
        const KEYS: [&str; 9] = ["x", "z", "a", "s", "q", "k", "j", "p", "o"];
        const PADS: [Button; 6] = [
            Button::South,
            Button::East,
            Button::North,
            Button::LeftTrigger,
            Button::Start,
            Button::Mode,
        ];
        let button = BUTTONS[rng.pick(6)].0;
        let console = CONSOLES[rng.pick(CONSOLES.len())];
        let model = MODELS[rng.pick(MODELS.len())];
        let key = KEYS[rng.pick(KEYS.len())];
        let physical = PADS[rng.pick(PADS.len())];
        let hotkey = HOTKEYS[rng.pick(2)].0;
        match rng.pick(10) {
            0 => {
                m.set_key(button, key);
                format!("set_key({}, {key})", name(button))
            }
            1 => {
                m.set_console_key(console, button, key);
                format!("set_console_key({console}, {}, {key})", name(button))
            }
            2 => {
                m.set_pad_button(model, button, physical);
                format!("set_pad_button({model}, {}, {physical:?})", name(button))
            }
            3 => {
                m.set_console_pad_button(console, model, button, physical);
                format!(
                    "set_console_pad_button({console}, {model}, {}, {physical:?})",
                    name(button)
                )
            }
            4 => {
                m.set_hotkey(hotkey, key);
                format!("set_hotkey({hotkey:?}, {key})")
            }
            5 => {
                m.reset_keyboard();
                "reset_keyboard".into()
            }
            6 => {
                m.reset_hotkeys();
                "reset_hotkeys".into()
            }
            7 => {
                m.reset_pad(model);
                format!("reset_pad({model})")
            }
            8 => {
                m.reset_console_keyboard(console);
                format!("reset_console_keyboard({console})")
            }
            _ => {
                m.reset_console_pad(console, model);
                format!("reset_console_pad({console}, {model})")
            }
        }
    }

    #[test]
    fn no_sequence_of_edits_doubles_up_a_button_in_any_console() {
        for seed in 0..300 {
            let mut rng = Lcg(seed);
            let mut m = Mappings::default();
            let mut steps = Vec::new();
            for _ in 0..40 {
                steps.push(random_edit(&mut m, &mut rng));
                for console in m.consoles.keys().map(String::as_str).chain([""]) {
                    let c = if console.is_empty() {
                        m.clone()
                    } else {
                        m.for_console(console)
                    };
                    assert!(
                        no_clashes(&c),
                        "seed {seed}, console {console:?} after {steps:#?}\n{}",
                        m.to_json()
                    );
                }
            }
        }
    }
}
