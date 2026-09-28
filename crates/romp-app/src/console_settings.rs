use crate::cores::{core_for_platform, default_options};
use std::collections::BTreeMap;

pub const STORE_KEY: &str = "console_settings";

pub struct Choice {
    pub label: &'static str,
    pub value: &'static str,
}

pub struct Setting {
    pub key: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub choices: &'static [Choice],
    pub default: usize,
}

pub struct Console {
    pub name: &'static str,
    pub platform: &'static str,
    pub core: &'static str,
    pub settings: &'static [Setting],
}

const fn choice(label: &'static str, value: &'static str) -> Choice {
    Choice { label, value }
}

const RESOLUTION_DETAIL: &str = "Draws the game at a multiple of the console's resolution. Higher is sharper and needs a faster computer.";
const ANISOTROPIC_DETAIL: &str = "Keeps textures sharp on floors and walls seen at an angle.";
const WIDESCREEN_DETAIL: &str = "Shows a wider picture in games that have a widescreen patch.";

static CONSOLES: &[Console] = &[
    Console {
        name: "PlayStation 2",
        platform: "ps2",
        core: "armsx2",
        settings: &[
            Setting {
                key: "armsx2_upscale",
                label: "Resolution",
                detail: RESOLUTION_DETAIL,
                choices: &[
                    choice("Native", "1x"),
                    choice("2x", "2x"),
                    choice("3x", "3x"),
                ],
                default: 2,
            },
            Setting {
                key: "armsx2_anisotropic_filtering",
                label: "Anisotropic filtering",
                detail: ANISOTROPIC_DETAIL,
                choices: &[
                    choice("Off", "0"),
                    choice("2x", "2"),
                    choice("4x", "4"),
                    choice("8x", "8"),
                    choice("16x", "16"),
                ],
                default: 4,
            },
            Setting {
                key: "armsx2_widescreen_patches",
                label: "Widescreen",
                detail: WIDESCREEN_DETAIL,
                choices: &[choice("Off", "disabled"), choice("On", "enabled")],
                default: 0,
            },
        ],
    },
    Console {
        name: "PlayStation 2",
        platform: "ps2",
        core: "pcsx2",
        settings: &[
            Setting {
                key: "pcsx2_upscale_multiplier",
                label: "Resolution",
                detail: RESOLUTION_DETAIL,
                choices: &[choice("Native", "1x (Native)"), choice("2x", "2x")],
                default: 0,
            },
            Setting {
                key: "pcsx2_anisotropic_filtering",
                label: "Anisotropic filtering",
                detail: ANISOTROPIC_DETAIL,
                choices: &[
                    choice("Off", "disabled"),
                    choice("2x", "2x"),
                    choice("4x", "4x"),
                    choice("8x", "8x"),
                    choice("16x", "16x"),
                ],
                default: 4,
            },
            Setting {
                key: "pcsx2_widescreen_hint",
                label: "Widescreen",
                detail: WIDESCREEN_DETAIL,
                choices: &[choice("Off", "disabled"), choice("On", "enabled (16:9)")],
                default: 0,
            },
        ],
    },
];

/// The consoles whose emulator on this computer has settings.
pub fn consoles() -> impl Iterator<Item = &'static Console> {
    CONSOLES
        .iter()
        .filter(|c| core_for_platform(c.platform).is_some_and(|core| core.id == c.core))
}

pub type Chosen = BTreeMap<String, String>;

pub fn chosen_from_json(text: Option<&str>) -> Chosen {
    text.and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_default()
}

pub fn selected(setting: &Setting, chosen: &Chosen) -> usize {
    chosen
        .get(setting.key)
        .and_then(|v| setting.choices.iter().position(|c| c.value == v))
        .unwrap_or(setting.default)
}

/// Sets `key` to its choice at `index`, returning false for an unknown setting or choice.
pub fn choose(chosen: &mut Chosen, key: &str, index: usize) -> bool {
    let Some(setting) = CONSOLES
        .iter()
        .flat_map(|c| c.settings)
        .find(|s| s.key == key)
    else {
        return false;
    };
    let Some(choice) = setting.choices.get(index) else {
        return false;
    };
    chosen.insert(key.to_string(), choice.value.to_string());
    true
}

/// The core options a game starts with: the core's own defaults, then its console settings.
pub fn core_options(core_id: &str, chosen: &Chosen) -> Vec<(String, String)> {
    let mut options = default_options(core_id);
    for setting in CONSOLES
        .iter()
        .filter(|c| c.core == core_id)
        .flat_map(|c| c.settings)
    {
        let value = setting.choices[selected(setting, chosen)].value.to_string();
        match options.iter_mut().find(|(k, _)| k == setting.key) {
            Some(existing) => existing.1 = value,
            None => options.push((setting.key.to_string(), value)),
        }
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_choices() {
        for setting in CONSOLES.iter().flat_map(|c| c.settings) {
            assert!(setting.default < setting.choices.len(), "{}", setting.key);
        }
    }

    #[test]
    fn upscaled_frames_fit_the_frame_buffer() {
        let widest = |console: &Console| {
            console.settings[0]
                .choices
                .iter()
                .filter_map(|c| c.value.split('x').next()?.parse::<u32>().ok())
                .max()
                .unwrap()
        };
        let armsx2 = CONSOLES.iter().find(|c| c.core == "armsx2").unwrap();
        assert!(640 * widest(armsx2) <= romp_proto::frame::MAX_W);
        assert!(512 * widest(armsx2) <= romp_proto::frame::MAX_H);
        let pcsx2 = CONSOLES.iter().find(|c| c.core == "pcsx2").unwrap();
        assert!(640 * widest(pcsx2) <= 1920);
    }

    #[test]
    fn a_chosen_setting_overrides_its_default() {
        let mut chosen = Chosen::new();
        let options = core_options("armsx2", &chosen);
        assert!(options.contains(&("armsx2_renderer".into(), "Vulkan".into())));
        assert!(options.contains(&("armsx2_upscale".into(), "3x".into())));
        assert!(options.contains(&("armsx2_anisotropic_filtering".into(), "16".into())));

        assert!(choose(&mut chosen, "armsx2_upscale", 0));
        assert!(choose(&mut chosen, "armsx2_widescreen_patches", 1));
        let options = core_options("armsx2", &chosen);
        assert!(options.contains(&("armsx2_upscale".into(), "1x".into())));
        assert!(options.contains(&("armsx2_widescreen_patches".into(), "enabled".into())));
    }

    #[test]
    fn unknown_or_stale_choices_fall_back_safely() {
        let mut chosen = Chosen::new();
        assert!(!choose(&mut chosen, "armsx2_upscale", 9));
        assert!(!choose(&mut chosen, "nope", 0));
        assert!(chosen.is_empty());
        chosen.insert("armsx2_upscale".into(), "8x".into());
        assert!(core_options("armsx2", &chosen).contains(&("armsx2_upscale".into(), "3x".into())));
        assert_eq!(chosen_from_json(Some("not json")), Chosen::new());
    }

    #[test]
    fn only_the_emulator_this_computer_uses_is_listed() {
        let listed: Vec<_> = consoles().map(|c| c.core).collect();
        let ps2 = core_for_platform("ps2").unwrap().id;
        assert!(listed.iter().all(|&core| core == ps2), "{listed:?}");
    }
}
