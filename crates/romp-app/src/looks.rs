use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How a console's picture is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Look {
    Sharp,
    Smooth,
    CrtTv,
    CrtMonitor,
    Handheld,
}

pub const LOOKS: [(Look, &str); 5] = [
    (Look::Sharp, "Sharp pixels"),
    (Look::Smooth, "Smooth"),
    (Look::CrtTv, "CRT TV"),
    (Look::CrtMonitor, "CRT monitor"),
    (Look::Handheld, "Handheld screen"),
];

pub const CURVATURE: [&str; 3] = ["Off", "Subtle", "Strong"];
pub const SCANLINES: [&str; 3] = ["Light", "Medium", "Strong"];
pub const MASK: [&str; 3] = ["Off", "Light", "Strong"];

/// The fine adjustments of a CRT look, each an index into its list of steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuning {
    pub curvature: u8,
    pub scanlines: u8,
    pub mask: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub look: Look,
    pub tuning: Tuning,
}

const HANDHELDS: [&str; 11] = [
    "gb",
    "gbc",
    "gba",
    "gamegear",
    "neo-geo-pocket",
    "neo-geo-pocket-color",
    "lynx",
    "wonderswan",
    "wonderswan-color",
    "nds",
    "psp",
];

const TELEVISION: [&str; 20] = [
    "nes",
    "famicom",
    "fds",
    "snes",
    "sfam",
    "genesis",
    "sms",
    "sg1000",
    "segacd",
    "sega32",
    "tg16",
    "supergrafx",
    "turbografx-cd",
    "saturn",
    "atari2600",
    "colecovision",
    "intellivision",
    "odyssey-2",
    "3do",
    "neogeoaes",
];

const MONITOR: [&str; 11] = [
    "arcade",
    "neogeomvs",
    "cps1",
    "cps2",
    "cps3",
    "amiga",
    "c64",
    "msx",
    "msx2",
    "msx2plus",
    "zxs",
];

pub fn label(look: Look) -> &'static str {
    LOOKS
        .iter()
        .find(|(l, _)| *l == look)
        .map_or("", |(_, name)| name)
}

pub fn is_handheld(platform: &str) -> bool {
    HANDHELDS.contains(&platform)
}

/// The looks offered for a console: a handheld screen only for handhelds.
pub fn available(platform: &str) -> Vec<Look> {
    LOOKS
        .iter()
        .map(|(look, _)| *look)
        .filter(|look| *look != Look::Handheld || is_handheld(platform))
        .collect()
}

pub fn default_tuning(look: Look) -> Tuning {
    match look {
        Look::CrtMonitor => Tuning {
            curvature: 0,
            scanlines: 1,
            mask: 1,
        },
        _ => Tuning {
            curvature: 1,
            scanlines: 1,
            mask: 1,
        },
    }
}

/// What a console looks like until the player picks something else.
pub fn default_choice(platform: &str, sharp_pixels: bool) -> Choice {
    let look = if is_handheld(platform) {
        Look::Handheld
    } else if TELEVISION.contains(&platform) {
        Look::CrtTv
    } else if MONITOR.contains(&platform) {
        Look::CrtMonitor
    } else if sharp_pixels {
        Look::Sharp
    } else {
        Look::Smooth
    };
    Choice {
        look,
        tuning: default_tuning(look),
    }
}

pub fn has_tuning(look: Look) -> bool {
    matches!(look, Look::CrtTv | Look::CrtMonitor)
}

/// The shader preset for a look on a console, relative to the bundled shaders, or None for no shader.
pub fn preset(look: Look, platform: &str) -> Option<&'static str> {
    match look {
        Look::Sharp | Look::Smooth => None,
        Look::CrtTv => Some("crt/crt-guest-advanced-ntsc.slangp"),
        Look::CrtMonitor => Some("crt/crt-guest-advanced.slangp"),
        Look::Handheld => Some(match platform {
            "gb" => "handheld/gameboy.slangp",
            "gbc" => "handheld/gameboy-color-dot-matrix.slangp",
            "gba" => "handheld/gameboy-advance-dot-matrix.slangp",
            _ => "handheld/lcd-grid-v2.slangp",
        }),
    }
}

/// What sets each CRT look apart, before the player's curvature, scanline and mask steps.
fn character(look: Look) -> &'static [(&'static str, f32)] {
    match look {
        // A home TV over composite: soft, glowing, with a slot mask and darker edges.
        Look::CrtTv => &[
            ("glow", 0.12),
            ("halation", 0.15),
            ("shadowMask", 0.0),
            ("vigstr", 0.25),
            ("brightboost", 1.5),
        ],
        // A studio monitor over RGB: sharp, flat, with an aperture grille and no glow.
        Look::CrtMonitor => &[
            ("h_sharp", 8.0),
            ("s_sharp", 1.0),
            ("glow", 0.0),
            ("halation", 0.0),
            ("shadowMask", 6.0),
            ("slotmask", 0.0),
            ("slotmask1", 0.0),
            ("vigstr", 0.0),
            ("gsl", 1.0),
            ("brightboost", 1.8),
            ("brightboost1", 1.2),
        ],
        _ => &[],
    }
}

/// The shader's own settings for a look and its curvature, scanline and mask steps.
pub fn params(choice: &Choice) -> Vec<(String, f32)> {
    if !has_tuning(choice.look) {
        return Vec::new();
    }
    let tv = choice.look == Look::CrtTv;
    let t = choice.tuning;
    let (warp_x, warp_y, corner) = match t.curvature {
        0 => (0.0, 0.0, 0.0),
        1 => (0.03, 0.04, 0.02),
        _ => (0.06, 0.08, 0.04),
    };
    let (beam_min, beam_max) = match (tv, t.scanlines) {
        (true, 0) => (1.0, 0.8),
        (true, 1) => (1.2, 0.9),
        (true, _) => (1.6, 1.1),
        (false, 0) => (1.3, 1.0),
        (false, 1) => (1.6, 1.2),
        (false, _) => (2.2, 1.4),
    };
    let mask = match (tv, t.mask) {
        (_, 0) => 0.0,
        (true, 1) => 0.3,
        (true, _) => 0.5,
        (false, 1) => 0.45,
        (false, _) => 0.7,
    };
    let mut params: Vec<(String, f32)> = character(choice.look)
        .iter()
        .map(|(name, value)| ((*name).to_string(), *value))
        .collect();
    let slot = if tv {
        f32::from(t.mask.min(2)) * 0.3
    } else {
        0.0
    };
    params.extend(
        [
            ("warpX", warp_x),
            ("warpY", warp_y),
            ("csize", corner),
            ("beam_min", beam_min),
            ("beam_max", beam_max),
            ("maskstr", mask),
            ("slotmask", slot),
            ("slotmask1", slot),
        ]
        .into_iter()
        .filter(|(name, _)| tv || !name.starts_with("slotmask"))
        .map(|(name, value)| (name.to_string(), value)),
    );
    params
}

/// The shader to draw a console with under this choice, from `shaders`, with its settings.
pub fn shader(
    choice: &Choice,
    platform: &str,
    shaders: &Path,
) -> Option<(PathBuf, Vec<(String, f32)>)> {
    preset(choice.look, platform).map(|p| (shaders.join(p), params(choice)))
}

pub fn store_key(platform: &str) -> String {
    format!("look:{platform}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consoles_start_with_the_screen_they_were_played_on() {
        assert_eq!(default_choice("snes", true).look, Look::CrtTv);
        assert_eq!(default_choice("arcade", true).look, Look::CrtMonitor);
        assert_eq!(default_choice("gb", true).look, Look::Handheld);
        assert_eq!(default_choice("ps2", true).look, Look::Sharp);
        assert_eq!(default_choice("ps2", false).look, Look::Smooth);
        assert_eq!(
            default_choice("psx", false).look,
            Look::Smooth,
            "upscaled 3D would get twice the scanlines"
        );
    }

    #[test]
    fn handheld_screens_are_only_offered_for_handhelds() {
        assert!(available("gba").contains(&Look::Handheld));
        assert!(!available("snes").contains(&Look::Handheld));
        assert_eq!(
            preset(Look::Handheld, "gb"),
            Some("handheld/gameboy.slangp")
        );
        assert_eq!(
            preset(Look::Handheld, "psp"),
            Some("handheld/lcd-grid-v2.slangp")
        );
        assert_eq!(preset(Look::Sharp, "snes"), None);
    }

    #[test]
    fn crt_steps_set_the_shader_parameters() {
        let flat = Choice {
            look: Look::CrtMonitor,
            tuning: Tuning {
                curvature: 0,
                scanlines: 2,
                mask: 0,
            },
        };
        let monitor = params(&flat);
        let get = |name: &str| monitor.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert_eq!(get("warpX"), Some(0.0));
        assert_eq!(get("beam_min"), Some(2.2));
        assert_eq!(get("maskstr"), Some(0.0));
        assert_eq!(
            get("shadowMask"),
            Some(6.0),
            "a monitor has an aperture grille"
        );
        let tv = params(&Choice {
            look: Look::CrtTv,
            tuning: default_tuning(Look::CrtTv),
        });
        let tv_get = |name: &str| tv.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert_eq!(tv_get("slotmask"), Some(0.3), "a TV has a slot mask");
        assert!(tv_get("glow").unwrap() > get("glow").unwrap());
        assert!(params(&Choice {
            look: Look::Sharp,
            tuning: default_tuning(Look::Sharp)
        })
        .is_empty());
    }

    #[test]
    fn every_bundled_preset_exists() {
        let shaders = Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
        for platform in ["snes", "gb", "gbc", "gba", "psp"] {
            for (look, _) in LOOKS {
                if let Some(p) = preset(look, platform) {
                    assert!(shaders.join(p).is_file(), "{p} is not bundled");
                }
            }
        }
    }
}
