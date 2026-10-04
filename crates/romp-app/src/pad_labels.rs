use crate::gamepads::Button;
use crate::gettext_noop;

/// Whose names a controller's buttons carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Family {
    #[default]
    Other,
    Xbox360,
    Xbox,
    Ps3,
    Ps4,
    Ps5,
    Nintendo,
}

/// The families a controller's labels can be set to, with the names they're saved under.
pub const CHOICES: [(Family, &str, &str); 3] = [
    (Family::Xbox, "xbox", "Xbox"),
    (Family::Ps4, "playstation", "PlayStation"),
    (Family::Nintendo, "nintendo", "Nintendo"),
];

impl Family {
    pub fn saved_as(saved: &str) -> Option<Self> {
        CHOICES
            .iter()
            .find(|(_, name, _)| *name == saved)
            .map(|(family, _, _)| *family)
    }

    /// The name of this family among the choices, so Xbox 360 reads as Xbox.
    pub fn choice(self) -> Option<usize> {
        let family = match self {
            Self::Xbox360 => Self::Xbox,
            Self::Ps3 | Self::Ps5 => Self::Ps4,
            other => other,
        };
        CHOICES.iter().position(|(f, _, _)| *f == family)
    }

    fn playstation(self) -> bool {
        matches!(self, Self::Ps3 | Self::Ps4 | Self::Ps5)
    }

    /// Menus confirm with the bottom button, as Steam does, except on Nintendo pads where A
    /// sits on the right.
    pub fn confirm(self) -> Button {
        if self == Self::Nintendo {
            Button::East
        } else {
            Button::South
        }
    }

    pub fn back(self) -> Button {
        if self == Self::Nintendo {
            Button::South
        } else {
            Button::East
        }
    }
}

/// A PlayStation face button's symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Cross,
    Circle,
    Square,
    Triangle,
}

impl Shape {
    pub fn key(self) -> &'static str {
        match self {
            Self::Cross => "cross",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Triangle => "triangle",
        }
    }
}

/// How a button shows in a hint: what's printed on it, or a PlayStation symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Text(&'static str),
    Shape(Shape),
}

/// The button's name as its maker prints or calls it.
pub fn name(family: Family, button: Button) -> &'static str {
    use Button::*;
    use Family::*;
    match (family, button) {
        (_, DPadUp) => gettext_noop!("D-pad up"),
        (_, DPadDown) => gettext_noop!("D-pad down"),
        (_, DPadLeft) => gettext_noop!("D-pad left"),
        (_, DPadRight) => gettext_noop!("D-pad right"),
        (_, Unknown) => gettext_noop!("Unknown"),
        (Other, South) => gettext_noop!("Bottom face button"),
        (Other, East) => gettext_noop!("Right face button"),
        (Other, West) => gettext_noop!("Left face button"),
        (Other, North) => gettext_noop!("Top face button"),
        (Other, LeftTrigger) => gettext_noop!("Left bumper"),
        (Other, RightTrigger) => gettext_noop!("Right bumper"),
        (Other, LeftTrigger2) => gettext_noop!("Left trigger"),
        (Other, RightTrigger2) => gettext_noop!("Right trigger"),
        (Other, Start) => gettext_noop!("Start"),
        (Other, Select) => gettext_noop!("Select"),
        (Other, Mode) => gettext_noop!("Guide"),
        (Xbox360 | Xbox, South) => "A",
        (Xbox360 | Xbox, East) => "B",
        (Xbox360 | Xbox, West) => "X",
        (Xbox360 | Xbox, North) => "Y",
        (Xbox360 | Xbox, LeftTrigger) => "LB",
        (Xbox360 | Xbox, RightTrigger) => "RB",
        (Xbox360 | Xbox, LeftTrigger2) => "LT",
        (Xbox360 | Xbox, RightTrigger2) => "RT",
        (Xbox360, Start) => gettext_noop!("Start"),
        (Xbox360, Select) => gettext_noop!("Back"),
        (Xbox360, Mode) => gettext_noop!("Guide"),
        (Xbox, Start) => gettext_noop!("Menu"),
        (Xbox, Select) => gettext_noop!("View"),
        (Xbox, Mode) => gettext_noop!("Xbox button"),
        (Ps3 | Ps4 | Ps5, South) => gettext_noop!("Cross"),
        (Ps3 | Ps4 | Ps5, East) => gettext_noop!("Circle"),
        (Ps3 | Ps4 | Ps5, West) => gettext_noop!("Square"),
        (Ps3 | Ps4 | Ps5, North) => gettext_noop!("Triangle"),
        (Ps3 | Ps4 | Ps5, LeftTrigger) => "L1",
        (Ps3 | Ps4 | Ps5, RightTrigger) => "R1",
        (Ps3 | Ps4 | Ps5, LeftTrigger2) => "L2",
        (Ps3 | Ps4 | Ps5, RightTrigger2) => "R2",
        (Ps3 | Ps4 | Ps5, LeftThumb) => "L3",
        (Ps3 | Ps4 | Ps5, RightThumb) => "R3",
        (Ps3, Start) => gettext_noop!("Start"),
        (Ps3, Select) => gettext_noop!("Select"),
        (Ps4 | Ps5, Start) => gettext_noop!("Options"),
        (Ps4, Select) => gettext_noop!("Share"),
        (Ps5, Select) => gettext_noop!("Create"),
        (Ps3 | Ps4 | Ps5, Mode) => gettext_noop!("PS button"),
        (Nintendo, South) => "B",
        (Nintendo, East) => "A",
        (Nintendo, West) => "Y",
        (Nintendo, North) => "X",
        (Nintendo, LeftTrigger) => "L",
        (Nintendo, RightTrigger) => "R",
        (Nintendo, LeftTrigger2) => "ZL",
        (Nintendo, RightTrigger2) => "ZR",
        (Nintendo, Start) => "+",
        (Nintendo, Select) => "−",
        (Nintendo, Mode) => gettext_noop!("Home"),
        (_, LeftThumb) => gettext_noop!("Left stick press"),
        (_, RightThumb) => gettext_noop!("Right stick press"),
    }
}

/// The short form shown on a button badge in hints.
pub fn badge(family: Family, button: Button) -> Badge {
    use Button::*;
    let shape = match button {
        South => Some(Shape::Cross),
        East => Some(Shape::Circle),
        West => Some(Shape::Square),
        North => Some(Shape::Triangle),
        _ => None,
    };
    if let Some(shape) = shape.filter(|_| family.playstation()) {
        return Badge::Shape(shape);
    }
    let family = if family == Family::Other {
        Family::Xbox
    } else {
        family
    };
    Badge::Text(match (family, button) {
        (_, DPadUp) => "↑",
        (_, DPadDown) => "↓",
        (_, DPadLeft) => "←",
        (_, DPadRight) => "→",
        (Family::Xbox, Mode) => "Xbox",
        (Family::Ps3 | Family::Ps4 | Family::Ps5, Mode) => "PS",
        (_, LeftThumb) if !family.playstation() => "LS",
        (_, RightThumb) if !family.playstation() => "RS",
        _ => name(family, button),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_family_names_buttons_as_its_maker_does() {
        assert_eq!(name(Family::Xbox, Button::South), "A");
        assert_eq!(name(Family::Ps4, Button::South), "Cross");
        assert_eq!(name(Family::Nintendo, Button::South), "B");
        assert_eq!(name(Family::Nintendo, Button::East), "A");
        assert_eq!(name(Family::Ps5, Button::Select), "Create");
        assert_eq!(name(Family::Ps4, Button::Select), "Share");
        assert_eq!(name(Family::Xbox, Button::Select), "View");
        assert_eq!(name(Family::Nintendo, Button::LeftTrigger2), "ZL");
        assert_eq!(name(Family::Other, Button::South), "Bottom face button");
        assert_eq!(name(Family::Other, Button::Mode), "Guide");
    }

    #[test]
    fn menus_confirm_with_the_bottom_button_except_on_nintendo_pads() {
        assert_eq!(Family::Xbox.confirm(), Button::South);
        assert_eq!(Family::Ps4.confirm(), Button::South);
        assert_eq!(Family::Other.confirm(), Button::South);
        assert_eq!(Family::Xbox.back(), Button::East);
        assert_eq!(Family::Nintendo.confirm(), Button::East);
        assert_eq!(Family::Nintendo.back(), Button::South);
    }

    #[test]
    fn playstation_face_buttons_show_their_symbols() {
        assert_eq!(
            badge(Family::Ps4, Button::South),
            Badge::Shape(Shape::Cross)
        );
        assert_eq!(
            badge(Family::Ps5, Button::North),
            Badge::Shape(Shape::Triangle)
        );
        assert_eq!(badge(Family::Ps4, Button::LeftTrigger), Badge::Text("L1"));
        assert_eq!(badge(Family::Xbox, Button::South), Badge::Text("A"));
        assert_eq!(badge(Family::Other, Button::East), Badge::Text("B"));
        assert_eq!(badge(Family::Nintendo, Button::Start), Badge::Text("+"));
    }
}
