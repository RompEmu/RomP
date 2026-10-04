use crate::gamepads::Button;

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

impl Family {
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
        (_, DPadUp) => "D-pad up",
        (_, DPadDown) => "D-pad down",
        (_, DPadLeft) => "D-pad left",
        (_, DPadRight) => "D-pad right",
        (_, Unknown) => "Unknown",
        (Other, South) => "Bottom face button",
        (Other, East) => "Right face button",
        (Other, West) => "Left face button",
        (Other, North) => "Top face button",
        (Other, LeftTrigger) => "Left bumper",
        (Other, RightTrigger) => "Right bumper",
        (Other, LeftTrigger2) => "Left trigger",
        (Other, RightTrigger2) => "Right trigger",
        (Other, Start) => "Start",
        (Other, Select) => "Select",
        (Other, Mode) => "Guide",
        (Xbox360 | Xbox, South) => "A",
        (Xbox360 | Xbox, East) => "B",
        (Xbox360 | Xbox, West) => "X",
        (Xbox360 | Xbox, North) => "Y",
        (Xbox360 | Xbox, LeftTrigger) => "LB",
        (Xbox360 | Xbox, RightTrigger) => "RB",
        (Xbox360 | Xbox, LeftTrigger2) => "LT",
        (Xbox360 | Xbox, RightTrigger2) => "RT",
        (Xbox360, Start) => "Start",
        (Xbox360, Select) => "Back",
        (Xbox360, Mode) => "Guide",
        (Xbox, Start) => "Menu",
        (Xbox, Select) => "View",
        (Xbox, Mode) => "Xbox button",
        (Ps3 | Ps4 | Ps5, South) => "Cross",
        (Ps3 | Ps4 | Ps5, East) => "Circle",
        (Ps3 | Ps4 | Ps5, West) => "Square",
        (Ps3 | Ps4 | Ps5, North) => "Triangle",
        (Ps3 | Ps4 | Ps5, LeftTrigger) => "L1",
        (Ps3 | Ps4 | Ps5, RightTrigger) => "R1",
        (Ps3 | Ps4 | Ps5, LeftTrigger2) => "L2",
        (Ps3 | Ps4 | Ps5, RightTrigger2) => "R2",
        (Ps3 | Ps4 | Ps5, LeftThumb) => "L3",
        (Ps3 | Ps4 | Ps5, RightThumb) => "R3",
        (Ps3, Start) => "Start",
        (Ps3, Select) => "Select",
        (Ps4 | Ps5, Start) => "Options",
        (Ps4, Select) => "Share",
        (Ps5, Select) => "Create",
        (Ps3 | Ps4 | Ps5, Mode) => "PS button",
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
        (Nintendo, Mode) => "Home",
        (_, LeftThumb) => "Left stick press",
        (_, RightThumb) => "Right stick press",
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
