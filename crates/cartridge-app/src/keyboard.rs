use slint::platform::Key;

pub const MOD_SHIFT: u16 = 1;
pub const MOD_CTRL: u16 = 2;
pub const MOD_ALT: u16 = 4;
pub const MOD_META: u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetroKey {
    pub code: u32,
    pub character: u32,
}

const SPECIAL: [(Key, u32, u32); 32] = [
    (Key::Backspace, 8, 8),
    (Key::Tab, 9, 9),
    (Key::Return, 13, 13),
    (Key::Escape, 27, 27),
    (Key::Delete, 127, 127),
    (Key::UpArrow, 273, 0),
    (Key::DownArrow, 274, 0),
    (Key::RightArrow, 275, 0),
    (Key::LeftArrow, 276, 0),
    (Key::Insert, 277, 0),
    (Key::Home, 278, 0),
    (Key::End, 279, 0),
    (Key::PageUp, 280, 0),
    (Key::PageDown, 281, 0),
    (Key::F1, 282, 0),
    (Key::F2, 283, 0),
    (Key::F3, 284, 0),
    (Key::F4, 285, 0),
    (Key::F5, 286, 0),
    (Key::F6, 287, 0),
    (Key::F7, 288, 0),
    (Key::F8, 289, 0),
    (Key::F9, 290, 0),
    (Key::F10, 291, 0),
    (Key::F11, 292, 0),
    (Key::F12, 293, 0),
    (Key::CapsLock, 301, 0),
    (Key::ShiftR, 303, 0),
    (Key::Shift, 304, 0),
    (Key::ControlR, 305, 0),
    (Key::Control, 306, 0),
    (Key::Alt, 308, 0),
];

const SHIFTED: [(char, char); 21] = [
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('~', '`'),
];

pub fn retro_key(text: &str) -> Option<RetroKey> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if let Some((_, code, character)) = SPECIAL.iter().find(|(k, _, _)| char::from(*k) == c) {
        return Some(RetroKey {
            code: *code,
            character: *character,
        });
    }
    if !c.is_ascii() || c.is_ascii_control() {
        return None;
    }
    let base = SHIFTED
        .iter()
        .find(|(shifted, _)| *shifted == c)
        .map_or(c.to_ascii_lowercase(), |(_, base)| *base);
    Some(RetroKey {
        code: u32::from(base),
        character: u32::from(c),
    })
}

pub fn modifier_bit(code: u32) -> u16 {
    match code {
        303 | 304 => MOD_SHIFT,
        305 | 306 => MOD_CTRL,
        307 | 308 => MOD_ALT,
        309..=312 => MOD_META,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key) -> String {
        char::from(k).to_string()
    }

    #[test]
    fn letters_digits_and_symbols_use_their_unshifted_key() {
        assert_eq!(
            retro_key("a"),
            Some(RetroKey {
                code: 97,
                character: 97
            })
        );
        assert_eq!(
            retro_key("A"),
            Some(RetroKey {
                code: 97,
                character: 65
            })
        );
        assert_eq!(retro_key("7").unwrap().code, 55);
        assert_eq!(retro_key("!").unwrap().code, 49);
        assert_eq!(retro_key("?").unwrap().code, 47);
        assert_eq!(retro_key(" ").unwrap().code, 32);
        assert_eq!(retro_key("é"), None);
    }

    #[test]
    fn special_keys_map_to_libretro_codes() {
        let code = |k| retro_key(&key(k)).unwrap().code;
        assert_eq!(code(Key::UpArrow), 273);
        assert_eq!(code(Key::LeftArrow), 276);
        assert_eq!(code(Key::Return), 13);
        assert_eq!(code(Key::Escape), 27);
        assert_eq!(code(Key::Backspace), 8);
        assert_eq!(code(Key::F1), 282);
        assert_eq!(code(Key::F12), 293);
        assert_eq!(code(Key::Shift), 304);
        assert_eq!(code(Key::Control), 306);
        assert_eq!(code(Key::Alt), 308);
        assert_eq!(retro_key(&key(Key::Return)).unwrap().character, 13);
        assert_eq!(retro_key(&key(Key::UpArrow)).unwrap().character, 0);
    }

    #[test]
    fn modifier_keys_have_modifier_bits() {
        assert_eq!(modifier_bit(304), MOD_SHIFT);
        assert_eq!(modifier_bit(305), MOD_CTRL);
        assert_eq!(modifier_bit(308), MOD_ALT);
        assert_eq!(modifier_bit(97), 0);
    }
}
