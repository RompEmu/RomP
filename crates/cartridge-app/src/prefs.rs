use serde::{Deserialize, Serialize};

pub const UI_SCALES: [u8; 3] = [100, 150, 200];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub pause_unfocused: bool,
    pub resume: bool,
    pub fullscreen: bool,
    pub sharp_pixels: bool,
    pub volume: u8,
    pub ui_scale: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            pause_unfocused: true,
            resume: true,
            fullscreen: false,
            sharp_pixels: true,
            volume: 100,
            ui_scale: 100,
        }
    }
}

impl Preferences {
    pub fn from_json(text: Option<&str>) -> Self {
        let mut prefs: Self = text
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();
        prefs.volume = prefs.volume.min(100);
        if !UI_SCALES.contains(&prefs.ui_scale) {
            prefs.ui_scale = 100;
        }
        prefs
    }

    pub fn scale_factor(self) -> f32 {
        f32::from(self.ui_scale) / 100.0
    }

    pub fn to_json(self) -> String {
        serde_json::to_string(&self).expect("preferences serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_favour_a_calm_desktop_setup() {
        let p = Preferences::default();
        assert!(p.pause_unfocused && p.resume && p.sharp_pixels);
        assert!(!p.fullscreen);
        assert_eq!(p.volume, 100);
    }

    #[test]
    fn saved_choices_round_trip_and_missing_fields_use_defaults() {
        let p = Preferences {
            fullscreen: true,
            volume: 40,
            ..Preferences::default()
        };
        assert_eq!(Preferences::from_json(Some(&p.to_json())), p);
        let partial = Preferences::from_json(Some(r#"{"volume": 70}"#));
        assert_eq!(partial.volume, 70);
        assert!(partial.pause_unfocused);
        assert_eq!(
            Preferences::from_json(Some("nonsense")),
            Preferences::default()
        );
        assert!(Preferences::from_json(Some(r#"{"volume": 250}"#)).volume <= 100);
    }

    #[test]
    fn interface_scale_is_one_of_the_offered_sizes() {
        assert_eq!(Preferences::default().ui_scale, 100);
        assert_eq!(Preferences::default().scale_factor(), 1.0);
        let large = Preferences::from_json(Some(r#"{"ui_scale": 150}"#));
        assert_eq!(large.scale_factor(), 1.5);
        assert_eq!(
            Preferences::from_json(Some(r#"{"ui_scale": 200}"#)).scale_factor(),
            2.0
        );
        assert_eq!(
            Preferences::from_json(Some(r#"{"ui_scale": 900}"#)).ui_scale,
            100
        );
        assert_eq!(
            Preferences::from_json(Some(r#"{"ui_scale": 0}"#)).ui_scale,
            100
        );
    }
}
