use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

pub fn move_in_grid(index: usize, total: usize, columns: usize, dir: Dir) -> usize {
    if total == 0 {
        return 0;
    }
    let columns = columns.max(1);
    let index = index.min(total - 1);
    let column = index % columns;
    match dir {
        Dir::Right if column + 1 < columns && index + 1 < total => index + 1,
        Dir::Left if column > 0 => index - 1,
        Dir::Up if index >= columns => index - columns,
        Dir::Down if index + columns < total => index + columns,
        Dir::Down if index / columns < (total - 1) / columns => total - 1,
        _ => index,
    }
}

const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(110);

#[derive(Debug, Default)]
pub struct Repeater {
    held: u16,
    since: Option<Instant>,
    last: Option<Instant>,
}

impl Repeater {
    pub fn update(&mut self, buttons: u16, repeatable: u16, now: Instant) -> u16 {
        let pressed = buttons & !self.held;
        self.held = buttons;
        let held = buttons & repeatable;
        if held == 0 {
            self.since = None;
            self.last = None;
            return pressed;
        }
        if pressed & repeatable != 0 {
            self.since = Some(now);
            self.last = Some(now);
            return pressed;
        }
        let (Some(since), Some(last)) = (self.since, self.last) else {
            return pressed;
        };
        if now.duration_since(since) >= REPEAT_DELAY && now.duration_since(last) >= REPEAT_EVERY {
            self.last = Some(now);
            return pressed | held;
        }
        pressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_moves_by_rows_and_columns_and_stays_inside() {
        assert_eq!(move_in_grid(0, 10, 4, Dir::Right), 1);
        assert_eq!(move_in_grid(3, 10, 4, Dir::Right), 3);
        assert_eq!(move_in_grid(4, 10, 4, Dir::Left), 4);
        assert_eq!(move_in_grid(5, 10, 4, Dir::Left), 4);
        assert_eq!(move_in_grid(1, 10, 4, Dir::Down), 5);
        assert_eq!(move_in_grid(6, 10, 4, Dir::Down), 9);
        assert_eq!(move_in_grid(9, 10, 4, Dir::Down), 9);
        assert_eq!(move_in_grid(5, 10, 4, Dir::Up), 1);
        assert_eq!(move_in_grid(1, 10, 4, Dir::Up), 1);
        assert_eq!(move_in_grid(9, 10, 4, Dir::Right), 9);
        assert_eq!(move_in_grid(0, 0, 4, Dir::Down), 0);
    }

    #[test]
    fn held_directions_repeat_after_a_delay() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut r = Repeater::default();
        let up = 1 << 4;
        let a = 1 << 8;
        assert_eq!(r.update(up | a, up, at(0)), up | a);
        assert_eq!(r.update(up | a, up, at(100)), 0);
        assert_eq!(r.update(up | a, up, at(450)), up);
        assert_eq!(r.update(up | a, up, at(500)), 0);
        assert_eq!(r.update(up | a, up, at(570)), up);
        assert_eq!(r.update(0, up, at(600)), 0);
        assert_eq!(r.update(a, up, at(610)), a);
    }
}
