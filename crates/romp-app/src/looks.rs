use crate::shading::Stage;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How a console's picture is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Look {
    Sharp,
    Smooth,
    #[serde(alias = "CrtTv", alias = "CrtMonitor")]
    Crt,
    Handheld,
}

pub const LOOKS: [(Look, &str); 4] = [
    (Look::Sharp, "Sharp pixels"),
    (Look::Smooth, "Smooth"),
    (Look::Crt, "CRT"),
    (Look::Handheld, "Handheld screen"),
];

pub const SCREEN: [&str; 2] = ["TV", "Monitor"];
pub const CURVATURE: [&str; 3] = ["Off", "Subtle", "Strong"];
pub const SCANLINES: [&str; 3] = ["Light", "Medium", "Strong"];
pub const MASK: [&str; 3] = ["Off", "Light", "Strong"];
pub const MODELS: [&str; 3] = ["Pocket", "Original green", "Backlit"];
pub const COLOURS: [&str; 2] = ["Original", "Vivid"];
pub const DITHERING: [&str; 2] = ["Blend", "Off"];

/// The fine adjustments of a CRT look, each an index into its list of steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuning {
    #[serde(default)]
    pub screen: u8,
    pub curvature: u8,
    pub scanlines: u8,
    pub mask: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub look: Look,
    pub tuning: Tuning,
    /// Whether checkerboard dithering is blended into the transparency it stood for on a TV.
    #[serde(default = "yes")]
    pub blend_dithering: bool,
    /// Whether a handheld's colours are shown as stored, instead of as its screen showed them.
    #[serde(default)]
    pub vivid: bool,
    /// Which Game Boy's screen a Game Boy game is shown on, an index into `MODELS`.
    #[serde(default)]
    pub model: u8,
}

fn yes() -> bool {
    true
}

/// A line of the look panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Style,
    Screen,
    Curvature,
    Scanlines,
    Mask,
    Model,
    Colours,
    Dithering,
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

/// Consoles whose games drew transparency as a checkerboard a TV blurred together.
const DITHERED: [&str; 4] = ["genesis", "segacd", "sega32", "saturn"];

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

/// A TV for home consoles, and a flat monitor for arcade boards and computers.
pub fn default_tuning(platform: &str) -> Tuning {
    let monitor = MONITOR.contains(&platform);
    Tuning {
        screen: u8::from(monitor),
        curvature: u8::from(!monitor),
        scanlines: 1,
        mask: 1,
    }
}

/// What a console looks like until the player picks something else.
pub fn default_choice(platform: &str, sharp_pixels: bool) -> Choice {
    let look = if is_handheld(platform) {
        Look::Handheld
    } else if TELEVISION.contains(&platform) || MONITOR.contains(&platform) {
        Look::Crt
    } else if sharp_pixels {
        Look::Sharp
    } else {
        Look::Smooth
    };
    Choice {
        look,
        tuning: default_tuning(platform),
        blend_dithering: true,
        vivid: false,
        model: 0,
    }
}

pub fn has_tuning(look: Look) -> bool {
    look == Look::Crt
}

/// The shader preset for a look on a console, relative to the bundled shaders, or None for no shader.
pub fn preset(look: Look, platform: &str) -> Option<&'static str> {
    match look {
        Look::Sharp => Some("pixel-art-scaling/sharp-shimmerless.slangp"),
        Look::Smooth => None,
        Look::Crt => Some("crt/crt-guest-advanced.slangp"),
        Look::Handheld => Some(match platform {
            "gb" => "handheld/gameboy-pocket.slangp",
            "gbc" => "handheld/gameboy-color-dot-matrix.slangp",
            "gba" => "handheld/gameboy-advance-dot-matrix.slangp",
            _ => "handheld/lcd-grid-v2.slangp",
        }),
    }
}

/// The pass that turns a handheld's stored colours into the ones its screen showed.
fn colours_preset(platform: &str) -> Option<&'static str> {
    match platform {
        "gba" => Some("handheld/color-mod/gba-color.slangp"),
        "gbc" => Some("handheld/color-mod/gbc-color.slangp"),
        "nds" => Some("handheld/color-mod/nds-color.slangp"),
        "psp" => Some("handheld/color-mod/psp-color.slangp"),
        _ => None,
    }
}

/// The look panel's lines for this choice on this console.
pub fn rows(choice: &Choice, platform: &str) -> Vec<Row> {
    let mut rows = vec![Row::Style];
    if has_tuning(choice.look) {
        rows.extend([Row::Screen, Row::Curvature, Row::Scanlines, Row::Mask]);
    }
    if choice.look == Look::Handheld && platform == "gb" {
        rows.push(Row::Model);
    }
    if choice.look == Look::Handheld && colours_preset(platform).is_some() {
        rows.push(Row::Colours);
    }
    if DITHERED.contains(&platform) {
        rows.push(Row::Dithering);
    }
    rows
}

pub fn row_label(row: Row) -> &'static str {
    match row {
        Row::Style => "Style",
        Row::Screen => "Screen",
        Row::Curvature => "Curvature",
        Row::Scanlines => "Scanlines",
        Row::Mask => "Mask",
        Row::Model => "Model",
        Row::Colours => "Colours",
        Row::Dithering => "Dithering",
    }
}

pub fn row_value(choice: &Choice, row: Row) -> &'static str {
    let t = choice.tuning;
    let pick = |steps: &[&'static str], i: u8| steps.get(usize::from(i)).copied().unwrap_or("");
    match row {
        Row::Style => label(choice.look),
        Row::Screen => pick(&SCREEN, t.screen),
        Row::Curvature => pick(&CURVATURE, t.curvature),
        Row::Scanlines => pick(&SCANLINES, t.scanlines),
        Row::Mask => pick(&MASK, t.mask),
        Row::Model => pick(&MODELS, choice.model),
        Row::Colours => pick(&COLOURS, u8::from(choice.vivid)),
        Row::Dithering => pick(&DITHERING, u8::from(!choice.blend_dithering)),
    }
}

/// Moves a line of the look panel one step left or right.
pub fn step(choice: &mut Choice, platform: &str, row: Row, delta: i32) {
    let turn =
        |value: u8, steps: usize| (i32::from(value) + delta).clamp(0, steps as i32 - 1) as u8;
    let t = &mut choice.tuning;
    match row {
        Row::Style => {
            let styles = available(platform);
            let here = styles.iter().position(|l| *l == choice.look).unwrap_or(0) as i32;
            choice.look = styles[(here + delta).rem_euclid(styles.len() as i32) as usize];
        }
        Row::Screen => t.screen = turn(t.screen, SCREEN.len()),
        Row::Curvature => t.curvature = turn(t.curvature, CURVATURE.len()),
        Row::Scanlines => t.scanlines = turn(t.scanlines, SCANLINES.len()),
        Row::Mask => t.mask = turn(t.mask, MASK.len()),
        Row::Model => choice.model = turn(choice.model, MODELS.len()),
        Row::Colours => choice.vivid = turn(u8::from(choice.vivid), COLOURS.len()) == 1,
        Row::Dithering => {
            choice.blend_dithering = turn(u8::from(!choice.blend_dithering), DITHERING.len()) == 0;
        }
    }
}

/// What sets a TV apart from a monitor, before the player's curvature, scanline and mask steps.
fn character(tv: bool) -> &'static [(&'static str, f32)] {
    if tv {
        // A home TV over composite: soft, glowing, with a slot mask and darker edges.
        &[
            ("h_sharp", 2.0),
            ("s_sharp", 0.2),
            ("glow", 0.12),
            ("halation", 0.15),
            ("shadowMask", 0.0),
            ("vigstr", 0.25),
            ("brightboost", 1.5),
        ]
    } else {
        // A studio monitor over RGB: sharp, with an aperture grille and no glow.
        &[
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
        ]
    }
}

/// The shader's own settings for a look's screen and its curvature, scanline and mask steps.
pub fn params(choice: &Choice) -> Vec<(String, f32)> {
    if !has_tuning(choice.look) {
        return Vec::new();
    }
    let tv = choice.tuning.screen == 0;
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
    let mut params: Vec<(String, f32)> = character(tv)
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

/// The shaders to draw a console with under this choice, from `shaders`, in order.
pub fn stages(choice: &Choice, platform: &str, shaders: &Path) -> Vec<Stage> {
    let before = |preset: &str| Stage {
        preset: shaders.join(preset),
        params: Vec::new(),
        to_screen: false,
    };
    let mut stages = Vec::new();
    if DITHERED.contains(&platform) && choice.blend_dithering {
        stages.push(before("dithering/mdapt.slangp"));
    }
    if choice.look == Look::Handheld && !choice.vivid {
        stages.extend(colours_preset(platform).map(before));
    }
    stages.extend(screen_preset(choice, platform).map(|(p, params)| Stage {
        preset: shaders.join(p),
        params,
        to_screen: true,
    }));
    stages
}

/// The look's own shader for this console, with its settings.
fn screen_preset(choice: &Choice, platform: &str) -> Option<(&'static str, Vec<(String, f32)>)> {
    if choice.look != Look::Handheld || platform != "gb" {
        return preset(choice.look, platform).map(|p| (p, params(choice)));
    }
    // The dot matrix shader's own palettes: 1 is the Pocket's, 4 the original's green.
    let palette = match choice.model {
        0 => 1.0,
        1 => 4.0,
        _ => return Some(("handheld/dot.slangp", Vec::new())),
    };
    let params = [
        ("palette", palette),
        ("screen_light", 1.05),
        ("contrast", 0.95),
        ("pixel_size", 0.9),
        ("baseline_alpha", 0.05),
    ];
    Some((
        "handheld/gameboy-pocket.slangp",
        params.iter().map(|(n, v)| ((*n).to_string(), *v)).collect(),
    ))
}

pub fn store_key(platform: &str) -> String {
    format!("look:{platform}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consoles_start_with_the_screen_they_were_played_on() {
        assert_eq!(default_choice("snes", true).look, Look::Crt);
        assert_eq!(default_choice("snes", true).tuning.screen, 0);
        assert_eq!(default_choice("arcade", true).look, Look::Crt);
        assert_eq!(default_choice("arcade", true).tuning.screen, 1);
        assert_eq!(default_choice("arcade", true).tuning.curvature, 0);
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
            Some("handheld/gameboy-pocket.slangp")
        );
        assert_eq!(
            preset(Look::Handheld, "psp"),
            Some("handheld/lcd-grid-v2.slangp")
        );
        assert_eq!(preset(Look::Smooth, "snes"), None);
    }

    #[test]
    fn crt_steps_set_the_shader_parameters() {
        let flat = Choice {
            look: Look::Crt,
            tuning: Tuning {
                screen: 1,
                curvature: 0,
                scanlines: 2,
                mask: 0,
            },
            ..default_choice("arcade", true)
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
        let tv = params(&default_choice("snes", true));
        let tv_get = |name: &str| tv.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
        assert_eq!(tv_get("slotmask"), Some(0.3), "a TV has a slot mask");
        assert!(tv_get("glow").unwrap() > get("glow").unwrap());
        assert!(params(&default_choice("ps2", true)).is_empty());
    }

    #[test]
    fn looks_saved_before_screens_were_a_setting_still_load() {
        let saved = r#"{"look":"CrtMonitor","tuning":{"curvature":0,"scanlines":2,"mask":1}}"#;
        let choice: Choice = serde_json::from_str(saved).unwrap();
        assert_eq!(choice.look, Look::Crt);
        assert_eq!(choice.tuning.scanlines, 2);
    }

    #[test]
    fn every_bundled_preset_exists() {
        let shaders = Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
        for platform in [
            "snes", "arcade", "genesis", "gb", "gbc", "gba", "nds", "psp",
        ] {
            for ((look, _), model) in LOOKS.iter().flat_map(|l| (0..3).map(move |m| (l, m))) {
                let choice = Choice {
                    look: *look,
                    model,
                    ..default_choice(platform, true)
                };
                for stage in stages(&choice, platform, &shaders) {
                    assert!(
                        stage.preset.is_file(),
                        "{} is not bundled",
                        stage.preset.display()
                    );
                }
            }
        }
    }

    #[test]
    fn dithering_is_blended_before_the_look_on_consoles_that_used_it() {
        let shaders = Path::new("shaders");
        let mut choice = default_choice("genesis", true);
        let names = |choice: &Choice, platform| {
            stages(choice, platform, shaders)
                .into_iter()
                .map(|s| (s.preset, s.to_screen))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&choice, "genesis"),
            [
                (shaders.join("dithering/mdapt.slangp"), false),
                (shaders.join("crt/crt-guest-advanced.slangp"), true),
            ]
        );
        assert!(rows(&choice, "genesis").contains(&Row::Dithering));
        assert!(!rows(&default_choice("snes", true), "snes").contains(&Row::Dithering));
        step(&mut choice, "genesis", Row::Dithering, 1);
        assert_eq!(row_value(&choice, Row::Dithering), "Off");
        assert_eq!(names(&choice, "genesis").len(), 1);
    }

    #[test]
    fn handhelds_show_the_colours_their_screens_did() {
        let shaders = Path::new("shaders");
        let mut choice = default_choice("gba", true);
        assert_eq!(rows(&choice, "gba"), [Row::Style, Row::Colours]);
        assert_eq!(row_value(&choice, Row::Colours), "Original");
        assert_eq!(stages(&choice, "gba", shaders).len(), 2);
        step(&mut choice, "gba", Row::Colours, 1);
        assert_eq!(row_value(&choice, Row::Colours), "Vivid");
        assert_eq!(stages(&choice, "gba", shaders).len(), 1);
    }

    #[test]
    fn game_boy_games_pick_which_game_boy_screen() {
        let shaders = Path::new("shaders");
        let mut choice = default_choice("gb", true);
        assert_eq!(rows(&choice, "gb"), [Row::Style, Row::Model]);
        assert_eq!(row_value(&choice, Row::Model), "Pocket");
        let screen = |choice: &Choice| stages(choice, "gb", shaders).pop().unwrap();
        assert!(screen(&choice).params.contains(&("palette".into(), 1.0)));
        step(&mut choice, "gb", Row::Model, 1);
        assert!(screen(&choice).params.contains(&("palette".into(), 4.0)));
        step(&mut choice, "gb", Row::Model, 1);
        assert_eq!(screen(&choice).preset, shaders.join("handheld/dot.slangp"));
        assert!(!rows(&default_choice("gbc", true), "gbc").contains(&Row::Model));
    }

    #[test]
    fn smooth_pixels_with_no_passes_are_drawn_plainly() {
        let choice = Choice {
            look: Look::Smooth,
            ..default_choice("ps2", false)
        };
        assert!(stages(&choice, "ps2", Path::new("shaders")).is_empty());
    }
}
