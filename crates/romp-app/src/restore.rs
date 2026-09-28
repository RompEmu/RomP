use serde::{Deserialize, Serialize};

pub const WINDOW_KEY: &str = "window";
pub const VIEW_KEY: &str = "last_view";
const MIN_SIZE: u32 = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

pub struct Observed {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    pub fullscreen: bool,
    pub minimized: bool,
}

pub fn game_window_key(platform_slug: &str, index: usize) -> String {
    match index {
        0 => format!("window:game:{platform_slug}"),
        i => format!("window:game:{platform_slug}:{}", i + 1),
    }
}

impl Observed {
    pub fn of(window: &slint::Window) -> Self {
        let position = window.position();
        let size = window.size();
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized: window.is_maximized(),
            fullscreen: window.is_fullscreen(),
            minimized: window.is_minimized(),
        }
    }
}

impl Placement {
    pub fn apply(self, window: &slint::Window) {
        window.set_size(slint::PhysicalSize::new(self.width, self.height));
        window.set_position(slint::PhysicalPosition::new(self.x, self.y));
        window.set_maximized(self.maximized);
    }

    pub fn from_json(text: Option<&str>) -> Option<Self> {
        let placement: Self = serde_json::from_str(text?).ok()?;
        (placement.width >= MIN_SIZE && placement.height >= MIN_SIZE).then_some(placement)
    }

    pub fn to_json(self) -> String {
        serde_json::to_string(&self).expect("placement serializes")
    }

    pub fn update(previous: Option<Self>, now: &Observed) -> Option<Self> {
        if now.fullscreen || now.minimized {
            return previous;
        }
        let current = Self {
            x: now.x,
            y: now.y,
            width: now.width,
            height: now.height,
            maximized: now.maximized,
        };
        match previous {
            Some(normal) if now.maximized => Some(Self {
                maximized: true,
                ..normal
            }),
            _ => Some(current),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastView {
    pub selected: String,
    pub game: Option<i64>,
    #[serde(default)]
    pub top_game: Option<i64>,
}

impl LastView {
    pub fn from_json(text: Option<&str>) -> Option<Self> {
        text.and_then(|t| serde_json::from_str(t).ok())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("last view serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed(width: u32, maximized: bool, fullscreen: bool, minimized: bool) -> Observed {
        Observed {
            x: 40,
            y: 60,
            width,
            height: 800,
            maximized,
            fullscreen,
            minimized,
        }
    }

    #[test]
    fn each_game_window_has_its_own_key() {
        assert_eq!(game_window_key("snes", 0), "window:game:snes");
        assert_eq!(game_window_key("nds", 1), "window:game:nds:2");
    }

    #[test]
    fn a_normal_window_is_remembered_as_it_is() {
        let saved = Placement::update(None, &observed(1200, false, false, false)).unwrap();
        assert_eq!(
            saved,
            Placement {
                x: 40,
                y: 60,
                width: 1200,
                height: 800,
                maximized: false
            }
        );
        assert_eq!(Placement::from_json(Some(&saved.to_json())), Some(saved));
    }

    #[test]
    fn maximized_windows_keep_their_normal_size_for_later() {
        let normal = Placement::update(None, &observed(1200, false, false, false));
        let maximized = Placement::update(normal, &observed(2800, true, false, false)).unwrap();
        assert!(maximized.maximized);
        assert_eq!(maximized.width, 1200);
        let first_run = Placement::update(None, &observed(2800, true, false, false)).unwrap();
        assert!(first_run.maximized);
    }

    #[test]
    fn full_screen_and_minimized_windows_leave_the_saved_place_alone() {
        let normal = Placement::update(None, &observed(1200, false, false, false));
        assert_eq!(
            Placement::update(normal, &observed(3000, false, true, false)),
            normal
        );
        assert_eq!(
            Placement::update(normal, &observed(0, false, false, true)),
            normal
        );
        assert_eq!(
            Placement::update(None, &observed(0, false, false, true)),
            None
        );
    }

    #[test]
    fn nonsense_is_not_restored() {
        assert_eq!(Placement::from_json(None), None);
        assert_eq!(Placement::from_json(Some("oops")), None);
        let tiny = Placement {
            width: 10,
            height: 10,
            ..Placement::default()
        };
        assert_eq!(Placement::from_json(Some(&tiny.to_json())), None);
    }

    #[test]
    fn the_last_view_round_trips() {
        let view = LastView {
            selected: "p:3".into(),
            game: Some(42),
            top_game: Some(7),
        };
        assert_eq!(LastView::from_json(Some(&view.to_json())), Some(view));
        let older = LastView::from_json(Some(r#"{"selected":"all","game":null}"#)).unwrap();
        assert_eq!(older.top_game, None);
        assert_eq!(LastView::from_json(Some("{")), None);
    }
}
