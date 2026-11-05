use crate::gettext_noop;
use crate::input::{A, B, DOWN, L, L2, L3, LEFT, R, R2, R3, RIGHT, SELECT, START, UP, X, Y};
use crate::mapping::BUTTONS;

/// NES, Famicom, Game Boy and their kin share one pad shape, so they share one list.
const NES_PAD: [(u32, &str); 8] = [
    (UP, gettext_noop!("Up")),
    (DOWN, gettext_noop!("Down")),
    (LEFT, gettext_noop!("Left")),
    (RIGHT, gettext_noop!("Right")),
    (A, "A"),
    (B, "B"),
    (SELECT, gettext_noop!("Select")),
    (START, gettext_noop!("Start")),
];

/// The buttons a console's controller has, under the names the console gives them, so
/// the controls page does not offer buttons the console cannot read. A controller that
/// came in versions lists the most buttons any version has.
// Nothing calls this until the controls page does.
#[cfg_attr(not(test), expect(dead_code))]
pub fn buttons(console: &str) -> Vec<(u32, &'static str)> {
    let dpad = [
        (UP, gettext_noop!("Up")),
        (DOWN, gettext_noop!("Down")),
        (LEFT, gettext_noop!("Left")),
        (RIGHT, gettext_noop!("Right")),
    ];
    let mut list = dpad.to_vec();
    match console {
        "nes" | "famicom" | "fds" | "gb" | "gbc" => return NES_PAD.to_vec(),
        "gba" => {
            list.extend([(A, "A"), (B, "B"), (L, "L"), (R, "R")]);
            list.extend([
                (SELECT, gettext_noop!("Select")),
                (START, gettext_noop!("Start")),
            ]);
        }
        "snes" | "sfam" => {
            list.extend([(A, "A"), (B, "B"), (X, "X"), (Y, "Y"), (L, "L"), (R, "R")]);
            list.extend([
                (SELECT, gettext_noop!("Select")),
                (START, gettext_noop!("Start")),
            ]);
        }
        "genesis" => {
            list.extend([(Y, "A"), (B, "B"), (A, "C"), (L, "X"), (X, "Y"), (R, "Z")]);
            list.extend([
                (SELECT, gettext_noop!("Mode")),
                (START, gettext_noop!("Start")),
            ]);
        }
        "sms" | "gamegear" | "sg1000" => {
            list.extend([(B, "1"), (A, "2"), (START, gettext_noop!("Start"))]);
        }
        "psx" => {
            list.extend([
                (B, gettext_noop!("Cross")),
                (A, gettext_noop!("Circle")),
                (Y, gettext_noop!("Square")),
                (X, gettext_noop!("Triangle")),
                (L, "L1"),
                (R, "R1"),
                (L2, "L2"),
                (R2, "R2"),
                (L3, "L3"),
                (R3, "R3"),
            ]);
            list.extend([
                (SELECT, gettext_noop!("Select")),
                (START, gettext_noop!("Start")),
            ]);
        }
        "wii" => {
            list.extend([(A, "A"), (B, "B"), (X, "X"), (Y, "Y"), (L, "L"), (R, "R")]);
            list.extend([
                (START, gettext_noop!("Start")),
                (SELECT, gettext_noop!("Select")),
            ]);
        }
        _ => return BUTTONS.to_vec(),
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_button_is_a_known_button_once() {
        for slug in [
            "nes", "famicom", "fds", "gb", "gbc", "gba", "snes", "sfam", "genesis", "sms",
            "gamegear", "sg1000", "psx", "wii",
        ] {
            let list = buttons(slug);
            let mut seen = Vec::new();
            for (id, _) in &list {
                assert!(BUTTONS.iter().any(|(b, _)| b == id), "{slug}");
                assert!(!seen.contains(id), "{slug} repeats a button");
                seen.push(*id);
            }
            for must in [UP, DOWN, LEFT, RIGHT, START] {
                assert!(seen.contains(&must), "{slug} lacks a required button");
            }
        }
    }

    #[test]
    fn genesis_names_its_buttons_the_way_the_console_does() {
        let list = buttons("genesis");
        for want in [(Y, "A"), (B, "B"), (A, "C"), (L, "X"), (X, "Y"), (R, "Z")] {
            assert!(list.contains(&want), "{want:?}");
        }
    }

    #[test]
    fn a_console_shows_the_most_buttons_of_its_versions() {
        let psx = buttons("psx");
        assert!(psx.iter().any(|(b, _)| *b == L3));
        assert!(psx.iter().any(|(b, _)| *b == R3));
        let snes = buttons("snes");
        assert!(snes.iter().any(|(b, _)| *b == X));
        assert!(snes.iter().any(|(b, _)| *b == Y));
        assert!(!snes.iter().any(|(b, _)| *b == L3));
    }

    #[test]
    fn a_console_without_an_entry_lists_every_button() {
        assert_eq!(buttons("atari7800").len(), BUTTONS.len());
        assert_eq!(buttons("not-a-console").len(), BUTTONS.len());
    }
}
