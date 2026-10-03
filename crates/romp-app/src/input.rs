use crate::mapping::{combo, Hotkey, Mappings, Mods};
use crate::players::PLAYERS;
use romp_proto::msg::{AppMsg, PadState};
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
    Save,
    Load,
    NextSlot,
    Menu,
    TogglePause,
    ToggleFullscreen,
    Screenshot,
}

pub const SLOTS: u8 = 4;

pub fn map_key(text: &str, mods: Mods, mappings: &Mappings) -> Option<KeyAction> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if c == char::from(Key::Escape) {
        return Some(KeyAction::Menu);
    }
    let action = match mappings.hotkey_for(&combo(mods, text)) {
        Some(Hotkey::SaveState) => KeyAction::Save,
        Some(Hotkey::NextSlot) => KeyAction::NextSlot,
        Some(Hotkey::LoadState) => KeyAction::Load,
        Some(Hotkey::Fullscreen) => KeyAction::ToggleFullscreen,
        Some(Hotkey::Screenshot) => KeyAction::Screenshot,
        Some(Hotkey::Pause) => KeyAction::TogglePause,
        None => KeyAction::Button(mappings.button_for_key(text)?),
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

pub const L3: u32 = 14;
pub const R3: u32 = 15;
const DEADZONE: f32 = 0.15;

pub fn retro_button(button: crate::gamepads::Button) -> Option<u32> {
    use crate::gamepads::Button::*;
    Some(match button {
        South => B,
        East => A,
        West => Y,
        North => X,
        LeftTrigger => L,
        RightTrigger => R,
        LeftTrigger2 => L2,
        RightTrigger2 => R2,
        Select => SELECT,
        Start => START,
        DPadUp => UP,
        DPadDown => DOWN,
        DPadLeft => LEFT,
        DPadRight => RIGHT,
        LeftThumb => L3,
        RightThumb => R3,
        _ => return None,
    })
}

pub fn stick(value: f32, invert: bool) -> i16 {
    if value.abs() < DEADZONE {
        return 0;
    }
    let v = (value.clamp(-1.0, 1.0) * 32767.0) as i16;
    if invert {
        -v
    } else {
        v
    }
}

pub fn pointer_coords(u: f32, v: f32, bottom_half: bool) -> (i16, i16) {
    let v = if bottom_half { 0.5 + v / 2.0 } else { v };
    let scale = |t: f32| ((t.clamp(0.0, 1.0) * 2.0 - 1.0) * 32767.0).round() as i16;
    (scale(u), scale(v))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Send(AppMsg),
    Menu,
    TogglePause,
    ToggleFullscreen,
    Screenshot,
    SlotChanged(u8),
}

pub struct Controls {
    keyboard: Pad,
    keyboard_player: Option<u8>,
    pads: Vec<(Option<u8>, PadState)>,
    sent: [PadState; PLAYERS as usize],
    slot: u8,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            keyboard: Pad::default(),
            keyboard_player: Some(1),
            pads: Vec::new(),
            sent: [PadState::default(); PLAYERS as usize],
            slot: 1,
        }
    }
}

impl Controls {
    pub fn set_keyboard_player(&mut self, player: Option<u8>) -> Vec<Command> {
        self.keyboard_player = player;
        self.push()
    }

    /// Releases always reach the game, so a button let go while a modifier is held never sticks.
    pub fn key(
        &mut self,
        text: &str,
        mods: Mods,
        pressed: bool,
        repeat: bool,
        mappings: &Mappings,
    ) -> Option<Vec<Command>> {
        let mods = if pressed { mods } else { Mods::default() };
        let action = map_key(text, mods, mappings)?;
        let commands = match action {
            KeyAction::Button(button) => {
                self.keyboard.set(button, pressed);
                self.push()
            }
            _ if !pressed || repeat => Vec::new(),
            KeyAction::Save => vec![Command::Send(AppMsg::SaveSlot(self.slot))],
            KeyAction::Load => vec![Command::Send(AppMsg::LoadSlot(self.slot))],
            KeyAction::NextSlot => {
                self.slot = self.slot % SLOTS + 1;
                vec![Command::SlotChanged(self.slot)]
            }
            KeyAction::Menu => vec![Command::Menu],
            KeyAction::TogglePause => vec![Command::TogglePause],
            KeyAction::ToggleFullscreen => vec![Command::ToggleFullscreen],
            KeyAction::Screenshot => vec![Command::Screenshot],
        };
        Some(commands)
    }

    pub fn slot(&self) -> u8 {
        self.slot
    }

    pub fn set_slot(&mut self, slot: u8) {
        self.slot = slot.clamp(1, SLOTS);
    }

    pub fn set_gamepads(&mut self, pads: Vec<(Option<u8>, PadState)>) -> Vec<Command> {
        self.pads = pads;
        self.push()
    }

    pub fn release_all(&mut self) -> Vec<Command> {
        self.keyboard = Pad::default();
        self.pads.clear();
        self.push()
    }

    fn merged(&self, player: u8) -> PadState {
        let keyboard = (self.keyboard_player == Some(player)).then_some(self.keyboard.state);
        let devices: Vec<PadState> = keyboard
            .into_iter()
            .chain(
                self.pads
                    .iter()
                    .filter(|(p, _)| *p == Some(player))
                    .map(|(_, s)| *s),
            )
            .collect();
        PadState {
            buttons: devices.iter().fold(0, |b, s| b | s.buttons),
            axes: devices
                .iter()
                .map(|s| s.axes)
                .find(|a| a.iter().any(|v| *v != 0))
                .unwrap_or_default(),
        }
    }

    fn push(&mut self) -> Vec<Command> {
        let mut commands = Vec::new();
        for player in 1..=PLAYERS {
            let state = self.merged(player);
            let slot = &mut self.sent[player as usize - 1];
            if *slot != state {
                *slot = state;
                commands.push(Command::Send(AppMsg::Pad {
                    port: player - 1,
                    state,
                }));
            }
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Mappings {
        Mappings::default()
    }

    fn key(k: Key) -> String {
        char::from(k).to_string()
    }

    #[test]
    fn arrows_map_to_dpad() {
        assert_eq!(
            map_key(&key(Key::UpArrow), Mods::default(), &m()),
            Some(KeyAction::Button(UP))
        );
        assert_eq!(
            map_key(&key(Key::DownArrow), Mods::default(), &m()),
            Some(KeyAction::Button(DOWN))
        );
        assert_eq!(
            map_key(&key(Key::LeftArrow), Mods::default(), &m()),
            Some(KeyAction::Button(LEFT))
        );
        assert_eq!(
            map_key(&key(Key::RightArrow), Mods::default(), &m()),
            Some(KeyAction::Button(RIGHT))
        );
    }

    #[test]
    fn letters_map_case_insensitively() {
        assert_eq!(
            map_key("x", Mods::default(), &m()),
            Some(KeyAction::Button(A))
        );
        assert_eq!(
            map_key("X", Mods::default(), &m()),
            Some(KeyAction::Button(A))
        );
        assert_eq!(
            map_key("z", Mods::default(), &m()),
            Some(KeyAction::Button(B))
        );
        assert_eq!(
            map_key("a", Mods::default(), &m()),
            Some(KeyAction::Button(Y))
        );
        assert_eq!(
            map_key("s", Mods::default(), &m()),
            Some(KeyAction::Button(X))
        );
    }

    #[test]
    fn start_select_and_hotkeys() {
        assert_eq!(
            map_key(&key(Key::Return), Mods::default(), &m()),
            Some(KeyAction::Button(START))
        );
        assert_eq!(
            map_key(&key(Key::Backspace), Mods::default(), &m()),
            Some(KeyAction::Button(SELECT))
        );
        assert_eq!(
            map_key(&key(Key::F5), Mods::default(), &m()),
            Some(KeyAction::Save)
        );
        assert_eq!(
            map_key(&key(Key::F6), Mods::default(), &m()),
            Some(KeyAction::NextSlot)
        );
        assert_eq!(
            map_key(&key(Key::F7), Mods::default(), &m()),
            Some(KeyAction::Load)
        );
        assert_eq!(
            map_key(&key(Key::F11), Mods::default(), &m()),
            Some(KeyAction::ToggleFullscreen)
        );
        assert_eq!(
            map_key(&key(Key::F12), Mods::default(), &m()),
            Some(KeyAction::Screenshot)
        );
        assert_eq!(
            map_key(&key(Key::Escape), Mods::default(), &m()),
            Some(KeyAction::Menu)
        );
        assert_eq!(
            map_key("P", Mods::default(), &m()),
            Some(KeyAction::TogglePause)
        );
    }

    #[test]
    fn a_ctrl_combination_saves_and_releases_still_reach_the_game() {
        let mut mappings = Mappings::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        mappings.set_hotkey(Hotkey::SaveState, &combo(ctrl, "s"));
        let mut controls = Controls::default();
        let commands = controls.key("s", ctrl, true, false, &mappings).unwrap();
        assert!(matches!(commands[..], [Command::Send(AppMsg::SaveSlot(1))]));

        let pressed = controls
            .key("s", Mods::default(), true, false, &mappings)
            .unwrap();
        assert!(matches!(pressed[..], [Command::Send(AppMsg::Pad { .. })]));
        let released = controls.key("s", ctrl, false, false, &mappings).unwrap();
        assert!(
            matches!(released[..], [Command::Send(AppMsg::Pad { state, .. })] if state.buttons == 0),
            "letting go of S with Ctrl held still releases X"
        );
    }

    #[test]
    fn changed_hotkeys_take_effect_in_game() {
        let mut mappings = Mappings::default();
        mappings.set_hotkey(Hotkey::Screenshot, "k");
        mappings.set_hotkey(Hotkey::SaveState, "x");
        assert_eq!(
            map_key("K", Mods::default(), &mappings),
            Some(KeyAction::Screenshot)
        );
        assert_eq!(
            map_key("x", Mods::default(), &mappings),
            Some(KeyAction::Save)
        );
        assert_eq!(
            map_key(&key(Key::F5), Mods::default(), &mappings),
            Some(KeyAction::Button(A))
        );
        assert_eq!(map_key(&key(Key::F12), Mods::default(), &mappings), None);
        assert_eq!(
            map_key(&key(Key::Escape), Mods::default(), &mappings),
            Some(KeyAction::Menu)
        );
    }

    #[test]
    fn unknown_or_multi_char_is_none() {
        assert_eq!(map_key("k", Mods::default(), &m()), None);
        assert_eq!(map_key("", Mods::default(), &m()), None);
        assert_eq!(map_key("xz", Mods::default(), &m()), None);
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
            controls.key(&f5(), Mods::default(), true, false, &m()),
            Some(vec![Command::Send(AppMsg::SaveSlot(1))])
        );
        assert_eq!(
            controls.key(&f5(), Mods::default(), true, true, &m()),
            Some(vec![])
        );
        assert_eq!(
            controls.key(&f5(), Mods::default(), false, false, &m()),
            Some(vec![])
        );
    }

    #[test]
    fn buttons_send_pad_state_once_per_change() {
        let mut controls = Controls::default();
        let pressed = PadState {
            buttons: 1 << A,
            axes: [0; 6],
        };
        assert_eq!(
            controls.key("x", Mods::default(), true, false, &m()),
            Some(vec![Command::Send(AppMsg::Pad {
                port: 0,
                state: pressed
            })])
        );
        assert_eq!(
            controls.key("x", Mods::default(), true, true, &m()),
            Some(vec![])
        );
    }

    #[test]
    fn escape_opens_the_menu_and_unknown_keys_are_not_handled() {
        let mut controls = Controls::default();
        assert_eq!(
            controls.key(&key(Key::Escape), Mods::default(), true, false, &m()),
            Some(vec![Command::Menu])
        );
        assert_eq!(controls.key("k", Mods::default(), true, false, &m()), None);
    }

    #[test]
    fn f6_cycles_the_slot_used_by_save_and_load() {
        let mut controls = Controls::default();
        assert_eq!(
            controls.key(&key(Key::F6), Mods::default(), true, false, &m()),
            Some(vec![Command::SlotChanged(2)])
        );
        assert_eq!(
            controls.key(&f5(), Mods::default(), true, false, &m()),
            Some(vec![Command::Send(AppMsg::SaveSlot(2))])
        );
        controls.set_slot(4);
        assert_eq!(
            controls.key(&key(Key::F6), Mods::default(), true, false, &m()),
            Some(vec![Command::SlotChanged(1)])
        );
        assert_eq!(
            controls.key(&key(Key::F7), Mods::default(), true, false, &m()),
            Some(vec![Command::Send(AppMsg::LoadSlot(1))])
        );
    }

    #[test]
    fn release_all_clears_held_buttons_once() {
        let mut controls = Controls::default();
        controls.key("x", Mods::default(), true, false, &m());
        controls.key(&key(Key::UpArrow), Mods::default(), true, false, &m());
        assert_eq!(
            controls.release_all(),
            [Command::Send(AppMsg::Pad {
                port: 0,
                state: PadState::default()
            })]
        );
        assert!(controls.release_all().is_empty());
    }

    #[test]
    fn gamepad_buttons_map_to_retropad() {
        use crate::gamepads::Button;
        assert_eq!(retro_button(Button::South), Some(B));
        assert_eq!(retro_button(Button::East), Some(A));
        assert_eq!(retro_button(Button::Start), Some(START));
        assert_eq!(retro_button(Button::DPadLeft), Some(LEFT));
        assert_eq!(retro_button(Button::LeftThumb), Some(L3));
        assert_eq!(retro_button(Button::Mode), None);
    }

    #[test]
    fn sticks_have_a_deadzone_and_full_range() {
        assert_eq!(stick(0.1, false), 0);
        assert_eq!(stick(1.0, false), 32767);
        assert_eq!(stick(1.0, true), -32767);
        assert_eq!(stick(-1.0, false), -32767);
    }

    #[test]
    fn keyboard_and_gamepad_are_merged() {
        let mut controls = Controls::default();
        controls.key("x", Mods::default(), true, false, &m());
        let pad = PadState {
            buttons: 1 << B,
            axes: [100, 0, 0, 0, 0, 0],
        };
        assert_eq!(
            controls.set_gamepads(vec![(Some(1), pad)]),
            [Command::Send(AppMsg::Pad {
                port: 0,
                state: PadState {
                    buttons: 1 << A | 1 << B,
                    axes: [100, 0, 0, 0, 0, 0]
                }
            })]
        );
        assert!(controls.set_gamepads(vec![(Some(1), pad)]).is_empty());
        assert_eq!(
            controls.release_all(),
            [Command::Send(AppMsg::Pad {
                port: 0,
                state: PadState::default()
            })]
        );
    }

    fn buttons(b: u16) -> PadState {
        PadState {
            buttons: b,
            axes: [0; 6],
        }
    }

    #[test]
    fn each_player_gets_its_own_port() {
        let mut controls = Controls::default();
        let sent = controls.set_gamepads(vec![
            (Some(1), buttons(1 << A)),
            (Some(2), buttons(1 << B)),
            (None, buttons(1 << X)),
        ]);
        assert_eq!(
            sent,
            [
                Command::Send(AppMsg::Pad {
                    port: 0,
                    state: buttons(1 << A)
                }),
                Command::Send(AppMsg::Pad {
                    port: 1,
                    state: buttons(1 << B)
                }),
            ]
        );
    }

    #[test]
    fn moving_the_keyboard_releases_its_old_player() {
        let mut controls = Controls::default();
        controls.key("x", Mods::default(), true, false, &m());
        assert_eq!(
            controls.set_keyboard_player(Some(3)),
            [
                Command::Send(AppMsg::Pad {
                    port: 0,
                    state: PadState::default()
                }),
                Command::Send(AppMsg::Pad {
                    port: 2,
                    state: buttons(1 << A)
                }),
            ]
        );
    }

    #[test]
    fn pointer_maps_to_libretro_range() {
        assert_eq!(pointer_coords(0.5, 0.5, false), (0, 0));
        assert_eq!(pointer_coords(0.0, 0.0, false), (-32767, -32767));
        assert_eq!(pointer_coords(1.0, 1.0, false), (32767, 32767));
        assert_eq!(pointer_coords(0.5, 0.0, true), (0, 0));
        assert_eq!(pointer_coords(0.5, 1.0, true), (0, 32767));
        assert_eq!(pointer_coords(2.0, -1.0, false), (32767, -32767));
    }
}
