pub const JOYPAD: u32 = 1;
const MOUSE: u32 = 2;
const LIGHTGUN: u32 = 4;
const MASK: u32 = 0xff;
pub const MENU_PORTS: usize = 4;

pub type Options = Vec<(String, u32)>;

pub fn choose(options: &Options, saved: Option<u32>) -> u32 {
    let offered = |id: u32| options.iter().any(|(_, d)| *d == id);
    match saved {
        Some(id) if offered(id) => id,
        _ if offered(JOYPAD) || options.is_empty() => JOYPAD,
        _ => options[0].1,
    }
}

pub fn cycle(options: &Options, current: u32, step: i32) -> u32 {
    if options.is_empty() {
        return current;
    }
    let len = options.len() as i32;
    let index = options.iter().position(|(_, d)| *d == current).unwrap_or(0) as i32;
    options[(index + step).rem_euclid(len) as usize].1
}

pub fn pickable(ports: &[Options]) -> Vec<usize> {
    ports
        .iter()
        .enumerate()
        .filter(|(_, options)| options.len() > 1)
        .map(|(i, _)| i)
        .take(MENU_PORTS)
        .collect()
}

pub fn name_of(options: &Options, device: u32) -> String {
    options
        .iter()
        .find(|(_, d)| *d == device)
        .map_or_else(|| tr::tr!("Controller"), |(name, _)| name.clone())
}

pub fn uses_mouse(devices: &[u32]) -> bool {
    devices.iter().any(|d| d & MASK == MOUSE)
}

const POINTER: u32 = 6;

pub fn uses_lightgun(devices: &[u32]) -> bool {
    devices
        .iter()
        .any(|d| matches!(d & MASK, LIGHTGUN | POINTER))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snes() -> Vec<Options> {
        vec![
            vec![("RetroPad".into(), 1), ("SNES Mouse".into(), 0x102)],
            vec![
                ("RetroPad".into(), 1),
                ("SNES Mouse".into(), 0x102),
                ("Super Scope".into(), 0x104),
            ],
            vec![("RetroPad".into(), 1)],
        ]
    }

    #[test]
    fn saved_choices_apply_only_when_the_core_offers_them() {
        let ports = snes();
        assert_eq!(choose(&ports[1], Some(0x104)), 0x104);
        assert_eq!(choose(&ports[0], Some(0x104)), JOYPAD);
        assert_eq!(choose(&ports[0], None), JOYPAD);
        assert_eq!(choose(&vec![("Keyboard".into(), 3)], None), 3);
    }

    #[test]
    fn cycling_wraps_through_the_offered_devices() {
        let ports = snes();
        assert_eq!(cycle(&ports[1], JOYPAD, 1), 0x102);
        assert_eq!(cycle(&ports[1], 0x104, 1), JOYPAD);
        assert_eq!(cycle(&ports[1], JOYPAD, -1), 0x104);
        assert_eq!(name_of(&ports[1], 0x104), "Super Scope");
    }

    #[test]
    fn only_ports_with_a_choice_are_listed() {
        assert_eq!(pickable(&snes()), [0, 1]);
    }

    #[test]
    fn mouse_and_light_gun_use_is_read_from_the_device_type() {
        assert!(uses_mouse(&[JOYPAD, 0x102]));
        assert!(!uses_mouse(&[JOYPAD, 0x104]));
        assert!(uses_lightgun(&[JOYPAD, 0x104]));
        assert!(!uses_lightgun(&[JOYPAD]));
        assert!(uses_lightgun(&[JOYPAD, 0x106]));
    }
}
