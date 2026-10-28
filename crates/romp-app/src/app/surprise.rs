use super::Controller;

/// A game from `ids` by `roll`, other than `last` while there's another to pick.
fn pick(ids: &[i64], last: Option<i64>, roll: u64) -> Option<i64> {
    let others: Vec<i64> = ids.iter().copied().filter(|&id| Some(id) != last).collect();
    let from = if others.is_empty() { ids } else { &others };
    let index = usize::try_from(roll % from.len().max(1) as u64).ok()?;
    from.get(index).copied()
}

fn roll() -> u64 {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).expect("system random numbers");
    u64::from_le_bytes(bytes)
}

impl Controller {
    /// Opens a random game from the ones the library shows.
    pub(super) fn surprise(&self) {
        let ids: Vec<i64> = self.library.borrow().games.iter().map(|g| g.id).collect();
        if let Some(id) = pick(&ids, self.surprise_last.get(), roll()) {
            self.surprise_last.set(Some(id));
            self.open_game(id);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_by_the_roll_from_the_games_shown() {
        assert_eq!(pick(&[10, 20, 30], None, 0), Some(10));
        assert_eq!(pick(&[10, 20, 30], None, 4), Some(20));
        assert_eq!(pick(&[], None, 4), None);
    }

    #[test]
    fn never_picks_the_last_pick_twice_in_a_row() {
        for roll in 0..12 {
            assert_ne!(pick(&[10, 20, 30], Some(20), roll), Some(20));
        }
        assert_eq!(
            pick(&[20], Some(20), 3),
            Some(20),
            "unless it's the only game"
        );
    }
}
