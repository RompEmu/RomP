use std::collections::VecDeque;
use std::ops::Range;

pub const DEFAULT_COVER_CAPACITY: usize = 150;
pub const RECENT_ROWS: usize = 16;

pub fn row_count(items: usize, columns: usize) -> usize {
    items.div_ceil(columns.max(1))
}

pub fn row_range(row: usize, items: usize, columns: usize) -> Range<usize> {
    let columns = columns.max(1);
    let start = (row * columns).min(items);
    start..(start + columns).min(items)
}

pub const ROW_CHROME: f32 = 76.0;

pub fn row_span(shelves: &[f32], row: usize) -> Option<(f32, f32)> {
    let height = shelves.get(row)? + ROW_CHROME;
    let top: f32 = shelves[..row].iter().map(|s| s + ROW_CHROME).sum();
    Some((top, top + height))
}

pub struct CoverSlots {
    capacity: usize,
    order: VecDeque<i64>,
}

impl CoverSlots {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_COVER_CAPACITY)
    }

    pub fn touch(&mut self, game_id: i64) -> Vec<i64> {
        if let Some(pos) = self.order.iter().position(|&id| id == game_id) {
            self.order.remove(pos);
        }
        self.order.push_back(game_id);
        let mut evicted = Vec::new();
        while self.order.len() > self.capacity {
            evicted.extend(self.order.pop_front());
        }
        evicted
    }

    pub fn contains(&self, game_id: i64) -> bool {
        self.order.contains(&game_id)
    }

    pub fn clear(&mut self) {
        self.order.clear();
    }
}

pub struct RecentRows {
    capacity: usize,
    rows: VecDeque<usize>,
}

impl RecentRows {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            rows: VecDeque::new(),
        }
    }

    pub fn push(&mut self, row: usize) {
        if let Some(pos) = self.rows.iter().position(|&r| r == row) {
            self.rows.remove(pos);
        }
        self.rows.push_back(row);
        while self.rows.len() > self.capacity {
            self.rows.pop_front();
        }
    }

    #[cfg(test)]
    pub fn contains(&self, row: usize) -> bool {
        self.rows.contains(&row)
    }

    pub fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().copied()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_stack_by_their_own_heights() {
        let shelves = [230.0, 172.0, 125.0];
        assert_eq!(row_span(&shelves, 0), Some((0.0, 306.0)));
        assert_eq!(row_span(&shelves, 1), Some((306.0, 554.0)));
        assert_eq!(row_span(&shelves, 2), Some((554.0, 755.0)));
        assert_eq!(row_span(&shelves, 3), None);
    }

    #[test]
    fn rows_cover_all_items() {
        assert_eq!(row_count(0, 4), 0);
        assert_eq!(row_count(5, 2), 3);
        assert_eq!(row_count(4, 2), 2);
        assert_eq!(row_range(2, 5, 2), 4..5);
        assert_eq!(row_range(0, 5, 2), 0..2);
        assert_eq!(row_count(3, 0), 3);
    }

    #[test]
    fn cover_slots_evict_oldest() {
        let mut s = CoverSlots::new(2);
        assert!(s.touch(1).is_empty());
        assert!(s.touch(2).is_empty());
        assert!(s.touch(1).is_empty());
        assert_eq!(s.touch(3), vec![2]);
        assert!(s.contains(1) && s.contains(3) && !s.contains(2));
    }

    #[test]
    fn recent_rows_keep_the_latest_distinct_rows() {
        let mut r = RecentRows::new(3);
        for row in [1, 2, 3, 2, 4] {
            r.push(row);
        }
        assert!(!r.contains(1));
        assert!(r.contains(2) && r.contains(3) && r.contains(4));
        assert_eq!(r.rows().count(), 3);
    }
}
