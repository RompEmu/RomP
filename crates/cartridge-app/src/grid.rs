use std::collections::VecDeque;
use std::ops::Range;

pub const DEFAULT_COVER_CAPACITY: usize = 400;

pub fn row_count(items: usize, columns: usize) -> usize {
    items.div_ceil(columns.max(1))
}

pub fn row_range(row: usize, items: usize, columns: usize) -> Range<usize> {
    let columns = columns.max(1);
    let start = (row * columns).min(items);
    start..(start + columns).min(items)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
