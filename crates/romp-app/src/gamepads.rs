use crate::input;
use crate::mapping::{self, Mappings, BUTTONS};
use romp_proto::msg::PadState;
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

pub struct Gamepads {
    gilrs: Option<gilrs::Gilrs>,
    active: HashMap<gilrs::GamepadId, Instant>,
    presses: Vec<(gilrs::GamepadId, gilrs::Button)>,
}

impl Gamepads {
    pub fn new() -> Self {
        let gilrs = gilrs::Gilrs::new()
            .map_err(|e| tracing::warn!("gamepads unavailable: {e}"))
            .ok();
        Self {
            gilrs,
            active: HashMap::new(),
            presses: Vec::new(),
        }
    }

    pub fn poll(&mut self) {
        let Some(gilrs) = self.gilrs.as_mut() else {
            return;
        };
        while let Some(event) = gilrs.next_event() {
            if let gilrs::EventType::ButtonPressed(button, _) = event.event {
                self.active.insert(event.id, Instant::now());
                self.presses.push((event.id, button));
                if self.presses.len() > 32 {
                    self.presses.remove(0);
                }
            }
        }
    }

    fn pads(&self) -> Vec<(gilrs::GamepadId, gilrs::Gamepad<'_>)> {
        let Some(gilrs) = self.gilrs.as_ref() else {
            return Vec::new();
        };
        let mut pads: Vec<_> = gilrs.gamepads().filter(|(_, g)| g.is_connected()).collect();
        pads.sort_by_key(|(id, _)| usize::from(*id));
        pads
    }

    pub fn connected(&self) -> Vec<PadInfo> {
        let pads = self.pads();
        let uuids: Vec<[u8; 16]> = pads.iter().map(|(_, g)| g.uuid()).collect();
        let names: Vec<&str> = pads.iter().map(|(_, g)| g.name()).collect();
        pad_keys(&uuids, &names)
            .into_iter()
            .zip(&names)
            .map(|(key, name)| PadInfo {
                key,
                name: name.to_string(),
            })
            .collect()
    }

    pub fn states(&self, mappings: &Mappings, nintendo: bool) -> Vec<PadInput> {
        let pads = self.pads();
        self.connected()
            .into_iter()
            .zip(pads)
            .map(|(info, (_, pad))| {
                let mut state = pad_state(&pad, mappings, mapping::model_of(&info.key));
                if mappings.stick_dpad {
                    state = mapping::stick_to_dpad(state);
                }
                if nintendo && mappings.nintendo_labels {
                    state = mapping::swap_face(state);
                }
                PadInput {
                    key: info.key,
                    state,
                    guide: pad.is_pressed(gilrs::Button::Mode),
                }
            })
            .collect()
    }

    pub fn take_presses(&mut self) -> Vec<(String, gilrs::Button)> {
        let keys: HashMap<gilrs::GamepadId, String> = self
            .pads()
            .iter()
            .map(|(id, _)| *id)
            .zip(self.connected().into_iter().map(|p| p.key))
            .collect();
        std::mem::take(&mut self.presses)
            .into_iter()
            .filter_map(|(id, button)| Some((keys.get(&id)?.clone(), button)))
            .collect()
    }

    pub fn recently_active(&self) -> Vec<String> {
        let now = Instant::now();
        self.connected()
            .into_iter()
            .zip(self.pads())
            .filter(|(_, (id, _))| {
                self.active
                    .get(id)
                    .is_some_and(|t| now.duration_since(*t) < ACTIVE_FOR)
            })
            .map(|(info, _)| info.key)
            .collect()
    }
}

fn pad_state(pad: &gilrs::Gamepad<'_>, mappings: &Mappings, model: &str) -> PadState {
    use gilrs::{Axis, Button};
    let mut state = PadState::default();
    for (button, _) in BUTTONS {
        if pad.is_pressed(mappings.pad_button(model, button)) {
            state.buttons |= 1 << button;
        }
    }
    let trigger = |b: Button| (pad.button_data(b).map_or(0.0, |d| d.value()) * 32767.0) as i16;
    state.axes = [
        input::stick(pad.value(Axis::LeftStickX), false),
        input::stick(pad.value(Axis::LeftStickY), true),
        input::stick(pad.value(Axis::RightStickX), false),
        input::stick(pad.value(Axis::RightStickY), true),
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
}
