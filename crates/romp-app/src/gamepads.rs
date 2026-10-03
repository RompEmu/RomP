use crate::input;
use crate::mapping::{self, Mappings, BUTTONS};
use romp_proto::msg::PadState;
use sdl3::event::Event;
use sdl3::gamepad::{Axis, Button as SdlButton};
use sdl3::joystick::JoystickId;
use sdl3::{EventPump, GamepadSubsystem};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const ACTIVE_FOR: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PadInfo {
    pub key: String,
    pub name: String,
}

pub fn pad_keys(uuids: &[[u8; 16]], names: &[&str]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    uuids
        .iter()
        .zip(names)
        .map(|(uuid, name)| {
            let base = if uuid.iter().all(|b| *b == 0) {
                format!("{:016x}", crate::paths::fnv1a(name.as_bytes()))
            } else {
                uuid.iter().map(|b| format!("{b:02x}")).collect()
            };
            let n = seen.entry(base.clone()).or_default();
            *n += 1;
            format!("{base}#{n}")
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct PadInput {
    pub key: String,
    pub state: PadState,
    pub guide: bool,
}

/// A controller's buttons by where they sit. The names are the ones saved in mappings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftTrigger,
    RightTrigger,
    LeftTrigger2,
    RightTrigger2,
    LeftThumb,
    RightThumb,
    Start,
    Select,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Mode,
    Unknown,
}

const SDL_BUTTONS: [(Button, SdlButton); 15] = [
    (Button::South, SdlButton::South),
    (Button::East, SdlButton::East),
    (Button::West, SdlButton::West),
    (Button::North, SdlButton::North),
    (Button::LeftTrigger, SdlButton::LeftShoulder),
    (Button::RightTrigger, SdlButton::RightShoulder),
    (Button::LeftThumb, SdlButton::LeftStick),
    (Button::RightThumb, SdlButton::RightStick),
    (Button::Start, SdlButton::Start),
    (Button::Select, SdlButton::Back),
    (Button::DPadUp, SdlButton::DPadUp),
    (Button::DPadDown, SdlButton::DPadDown),
    (Button::DPadLeft, SdlButton::DPadLeft),
    (Button::DPadRight, SdlButton::DPadRight),
    (Button::Mode, SdlButton::Guide),
];

const TRIGGERS: [(Button, Axis); 2] = [
    (Button::LeftTrigger2, Axis::TriggerLeft),
    (Button::RightTrigger2, Axis::TriggerRight),
];

/// How far a trigger goes down before it counts as pressed.
const TRIGGER_PRESS: f32 = 0.75;

fn from_sdl(button: SdlButton) -> Option<Button> {
    SDL_BUTTONS
        .iter()
        .find(|(_, b)| *b == button)
        .map(|(ours, _)| *ours)
}

fn axis_value(raw: i16) -> f32 {
    f32::from(raw) / 32767.0
}

/// The id gilrs gave a model, which saved layouts and players are keyed by.
fn model_uuid(bus: u16, vendor: u16, product: u16, version: u16) -> [u8; 16] {
    if vendor == 0 && product == 0 && version == 0 {
        return [0; 16];
    }
    let bus = if cfg!(target_os = "linux") { bus } else { 0x03 };
    let version = if cfg!(windows) { 0 } else { version };
    let mut uuid = [0; 16];
    for (at, value) in [(0, bus), (4, vendor), (8, product), (12, version)] {
        uuid[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    uuid
}

struct Pad {
    id: JoystickId,
    uuid: [u8; 16],
    name: String,
    triggers: [bool; 2],
    gamepad: sdl3::gamepad::Gamepad,
}

impl Pad {
    fn trigger(&self, axis: Axis) -> f32 {
        axis_value(self.gamepad.axis(axis)).max(0.0)
    }

    fn value(&self, button: Button) -> f32 {
        if let Some((_, axis)) = TRIGGERS.iter().find(|(b, _)| *b == button) {
            return self.trigger(*axis);
        }
        if self.pressed(button) {
            1.0
        } else {
            0.0
        }
    }

    fn pressed(&self, button: Button) -> bool {
        if let Some((_, axis)) = TRIGGERS.iter().find(|(b, _)| *b == button) {
            return self.trigger(*axis) >= TRIGGER_PRESS;
        }
        SDL_BUTTONS
            .iter()
            .find(|(b, _)| *b == button)
            .is_some_and(|(_, sdl)| self.gamepad.button(*sdl))
    }
}

struct Sdl {
    pads: Vec<Pad>,
    events: EventPump,
    subsystem: GamepadSubsystem,
    _context: sdl3::Sdl,
}

impl Sdl {
    fn new() -> Result<Self, sdl3::Error> {
        sdl3::hint::set("SDL_NO_SIGNAL_HANDLERS", "1");
        sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
        let context = sdl3::init()?;
        let subsystem = context.gamepad()?;
        let events = context.event_pump()?;
        let mut sdl = Self {
            pads: Vec::new(),
            events,
            subsystem,
            _context: context,
        };
        for id in sdl.subsystem.gamepads()? {
            sdl.open(id);
        }
        Ok(sdl)
    }

    fn open(&mut self, id: JoystickId) {
        if self.pads.iter().any(|p| p.id == id) {
            return;
        }
        let gamepad = match self.subsystem.open(id) {
            Ok(gamepad) => gamepad,
            Err(e) => {
                tracing::warn!("couldn't open a controller: {e}");
                return;
            }
        };
        let bus = {
            let data = self.subsystem.guid_for_id(id).raw().data;
            u16::from_le_bytes([data[0], data[1]])
        };
        let uuid = model_uuid(
            bus,
            gamepad.vendor_id().unwrap_or(0),
            gamepad.product_id().unwrap_or(0),
            gamepad.product_version().unwrap_or(0),
        );
        let name = gamepad.name().unwrap_or_else(|| "Controller".into());
        tracing::info!("controller connected: {name}");
        self.pads.push(Pad {
            id,
            uuid,
            name,
            triggers: [false; 2],
            gamepad,
        });
        self.pads.sort_by_key(|p| p.id);
    }
}

pub struct Gamepads {
    sdl: Option<Sdl>,
    active: HashMap<JoystickId, Instant>,
    presses: Vec<(JoystickId, Button)>,
}

impl Gamepads {
    pub fn new() -> Self {
        let sdl = Sdl::new()
            .map_err(|e| tracing::warn!("gamepads unavailable: {e}"))
            .ok();
        Self {
            sdl,
            active: HashMap::new(),
            presses: Vec::new(),
        }
    }

    fn pressed(&mut self, id: JoystickId, button: Button) {
        self.active.insert(id, Instant::now());
        self.presses.push((id, button));
        if self.presses.len() > 32 {
            self.presses.remove(0);
        }
    }

    pub fn poll(&mut self) {
        let Some(sdl) = self.sdl.as_mut() else {
            return;
        };
        let mut pressed = Vec::new();
        let events: Vec<Event> = sdl.events.poll_iter().collect();
        for event in events {
            match event {
                Event::GamepadAdded { which, .. } => sdl.open(which),
                Event::GamepadRemoved { which, .. } => sdl.pads.retain(|p| p.id != which),
                Event::GamepadButtonDown { which, button, .. } => {
                    if let Some(button) = from_sdl(button) {
                        pressed.push((which, button));
                    }
                }
                _ => {}
            }
        }
        for pad in &mut sdl.pads {
            for (i, (button, axis)) in TRIGGERS.iter().enumerate() {
                let down = pad.trigger(*axis) >= TRIGGER_PRESS;
                if down && !pad.triggers[i] {
                    pressed.push((pad.id, *button));
                }
                pad.triggers[i] = down;
            }
        }
        for (id, button) in pressed {
            self.pressed(id, button);
        }
    }

    fn pads(&self) -> &[Pad] {
        self.sdl.as_ref().map_or(&[], |sdl| &sdl.pads)
    }

    fn keys(&self) -> Vec<String> {
        let pads = self.pads();
        let uuids: Vec<[u8; 16]> = pads.iter().map(|p| p.uuid).collect();
        let names: Vec<&str> = pads.iter().map(|p| p.name.as_str()).collect();
        pad_keys(&uuids, &names)
    }

    pub fn connected(&self) -> Vec<PadInfo> {
        self.keys()
            .into_iter()
            .zip(self.pads())
            .map(|(key, pad)| PadInfo {
                key,
                name: pad.name.clone(),
            })
            .collect()
    }

    pub fn states(&self, mappings: &Mappings, nintendo: bool) -> Vec<PadInput> {
        self.keys()
            .into_iter()
            .zip(self.pads())
            .map(|(key, pad)| {
                let mut state = pad_state(pad, mappings, mapping::model_of(&key));
                if mappings.stick_dpad {
                    state = mapping::stick_to_dpad(state);
                }
                if nintendo && mappings.nintendo_labels {
                    state = mapping::swap_face(state);
                }
                PadInput {
                    key,
                    state,
                    guide: pad.pressed(Button::Mode),
                }
            })
            .collect()
    }

    pub fn take_presses(&mut self) -> Vec<(String, Button)> {
        let keys: HashMap<JoystickId, String> =
            self.pads().iter().map(|p| p.id).zip(self.keys()).collect();
        std::mem::take(&mut self.presses)
            .into_iter()
            .filter_map(|(id, button)| Some((keys.get(&id)?.clone(), button)))
            .collect()
    }

    pub fn recently_active(&self) -> Vec<String> {
        let now = Instant::now();
        self.keys()
            .into_iter()
            .zip(self.pads())
            .filter(|(_, pad)| {
                self.active
                    .get(&pad.id)
                    .is_some_and(|t| now.duration_since(*t) < ACTIVE_FOR)
            })
            .map(|(key, _)| key)
            .collect()
    }
}

fn pad_state(pad: &Pad, mappings: &Mappings, model: &str) -> PadState {
    let mut state = PadState::default();
    for (button, _) in BUTTONS {
        if pad.pressed(mappings.pad_button(model, button)) {
            state.buttons |= 1 << button;
        }
    }
    let axis = |a: Axis| axis_value(pad.gamepad.axis(a));
    let trigger = |b: Button| (pad.value(b) * 32767.0) as i16;
    state.axes = [
        input::stick(axis(Axis::LeftX), false),
        input::stick(axis(Axis::LeftY), false),
        input::stick(axis(Axis::RightX), false),
        input::stick(axis(Axis::RightY), false),
        trigger(mappings.pad_button(model, input::L2)),
        trigger(mappings.pad_button(model, input::R2)),
    ];
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_pads_get_distinct_keys_and_unknown_ids_use_the_name() {
        let a = [1u8; 16];
        let b = [2u8; 16];
        let keys = pad_keys(&[a, b, a, [0; 16]], &["Pad", "Other", "Pad", "Mystery"]);
        let hex_a = "01".repeat(16);
        assert_eq!(keys[0], format!("{hex_a}#1"));
        assert_eq!(keys[1], format!("{}#1", "02".repeat(16)));
        assert_eq!(keys[2], format!("{hex_a}#2"));
        assert!(keys[3].ends_with("#1") && keys[3].len() == 18);
    }

    #[test]
    fn models_keep_the_ids_gilrs_gave_them() {
        let uuid = model_uuid(0x05, 0x057e, 0x2009, 0x0001);
        let bus = if cfg!(target_os = "linux") {
            0x05
        } else {
            0x03
        };
        let version = if cfg!(windows) { 0x00 } else { 0x01 };
        assert_eq!(
            uuid,
            [bus, 0, 0, 0, 0x7e, 0x05, 0, 0, 0x09, 0x20, 0, 0, version, 0, 0, 0]
        );
        assert_eq!(model_uuid(0x03, 0, 0, 0), [0; 16]);
    }

    #[test]
    fn sdl_buttons_map_to_physical_positions() {
        assert_eq!(from_sdl(SdlButton::South), Some(Button::South));
        assert_eq!(from_sdl(SdlButton::Back), Some(Button::Select));
        assert_eq!(from_sdl(SdlButton::LeftShoulder), Some(Button::LeftTrigger));
        assert_eq!(from_sdl(SdlButton::Guide), Some(Button::Mode));
        assert_eq!(from_sdl(SdlButton::Touchpad), None);
    }
}
