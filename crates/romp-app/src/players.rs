use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PLAYERS: u8 = 4;
pub const KEYBOARD: &str = "keyboard";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignments {
    devices: BTreeMap<String, u8>,
}

impl Assignments {
    pub fn from_json(text: Option<&str>) -> Self {
        text.and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("assignments serialize")
    }

    pub fn player(&self, device: &str) -> Option<u8> {
        let default = if device == KEYBOARD { 1 } else { 0 };
        let player = self.devices.get(device).copied().unwrap_or(default);
        (1..=PLAYERS).contains(&player).then_some(player)
    }

    pub fn set(&mut self, device: &str, player: Option<u8>) {
        self.devices.insert(device.to_string(), player.unwrap_or(0));
    }

    pub fn connect(&mut self, pad: &str, connected: &[String]) -> Option<u8> {
        if self.devices.contains_key(pad) {
            return self.player(pad);
        }
        let taken: Vec<u8> = connected
            .iter()
            .filter(|other| *other != pad)
            .filter_map(|other| self.player(other))
            .collect();
        let player = (1..=PLAYERS).find(|p| !taken.contains(p));
        self.set(pad, player);
        player
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pads(keys: &[&str]) -> Vec<String> {
        keys.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn keyboard_starts_as_player_one() {
        assert_eq!(Assignments::default().player(KEYBOARD), Some(1));
    }

    #[test]
    fn new_pads_take_the_lowest_player_without_a_pad() {
        let mut a = Assignments::default();
        assert_eq!(a.connect("a#1", &[]), Some(1));
        assert_eq!(a.connect("b#1", &pads(&["a#1"])), Some(2));
        assert_eq!(a.connect("a#2", &pads(&["a#1", "b#1"])), Some(3));
        assert_eq!(a.connect("c#1", &pads(&["a#1", "b#1", "a#2"])), Some(4));
        assert_eq!(a.connect("d#1", &pads(&["a#1", "b#1", "a#2", "c#1"])), None);
    }

    #[test]
    fn a_freed_slot_is_reused_and_known_pads_keep_theirs() {
        let mut a = Assignments::default();
        a.connect("a#1", &[]);
        a.connect("b#1", &pads(&["a#1"]));
        assert_eq!(a.connect("c#1", &pads(&["b#1"])), Some(1));
        assert_eq!(a.connect("a#1", &pads(&["b#1", "c#1"])), Some(1));
    }

    #[test]
    fn manual_choices_persist_including_off() {
        let mut a = Assignments::default();
        a.set(KEYBOARD, Some(2));
        a.connect("a#1", &[]);
        a.set("a#1", None);
        let restored = Assignments::from_json(Some(&a.to_json()));
        assert_eq!(restored.player(KEYBOARD), Some(2));
        assert_eq!(restored.player("a#1"), None);
        let mut restored = restored;
        assert_eq!(restored.connect("a#1", &[]), None);
    }

    #[test]
    fn garbage_json_falls_back_to_defaults() {
        assert_eq!(
            Assignments::from_json(Some("{nope")),
            Assignments::default()
        );
        assert_eq!(Assignments::from_json(None).player(KEYBOARD), Some(1));
    }
}
