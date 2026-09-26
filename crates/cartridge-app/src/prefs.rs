use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub pause_unfocused: bool,
    pub resume: bool,
    pub fullscreen: bool,
    pub sharp_pixels: bool,
    pub volume: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            pause_unfocused: true,
            resume: true,
            fullscreen: false,
            sharp_pixels: true,
            volume: 100,
        }
    }
}

impl Preferences {
    pub fn from_json(text: Option<&str>) -> Self {
        let mut prefs: Self = text
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();
        prefs.volume = prefs.volume.min(100);
        prefs
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
}
