use super::Store;
use rusqlite::params;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionKind {
    Favorites,
    Mine,
    Shared,
    Smart,
    Series,
    Franchise,
    Genre,
}

impl CollectionKind {
    const ALL: [Self; 7] = [
        Self::Favorites,
        Self::Mine,
        Self::Shared,
        Self::Smart,
        Self::Series,
        Self::Franchise,
        Self::Genre,
    ];

    fn code(self) -> i64 {
        Self::ALL.iter().position(|k| *k == self).unwrap() as i64
    }

    fn from_code(code: i64) -> Self {
        Self::ALL[code.clamp(0, Self::ALL.len() as i64 - 1) as usize]
    }

    pub fn editable(self) -> bool {
        matches!(self, Self::Favorites | Self::Mine)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRecord {
    pub key: String,
    pub kind: CollectionKind,
    pub remote_id: String,
    pub name: String,
    pub owner: Option<String>,
    pub rom_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionItem {
    pub key: String,
    pub kind: CollectionKind,
    pub remote_id: String,
    pub name: String,
    pub owner: Option<String>,
    pub count: i64,
}

impl Store {
    pub fn replace_collections(&mut self, records: &[CollectionRecord]) {
        let tx = self.conn.transaction().expect("tx");
        tx.execute_batch("DELETE FROM collections; DELETE FROM collection_roms;")
            .expect("clear collections");
        for (position, r) in records.iter().enumerate() {
            insert(&tx, r, position as i64);
        }
        tx.commit().expect("commit");
    }

    pub fn add_collection(&mut self, record: &CollectionRecord) {
        let tx = self.conn.transaction().expect("tx");
        let position: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM collections",
                [],
                |r| r.get(0),
            )
            .expect("position");
        tx.execute("DELETE FROM collection_roms WHERE key = ?1", [&record.key])
            .expect("clear members");
        insert(&tx, record, position);
        tx.commit().expect("commit");
    }

    pub fn collections(&self, downloaded_only: bool) -> Vec<CollectionItem> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT c.key, c.kind, c.remote_id, c.name, c.owner,
                        (SELECT COUNT(*) FROM collection_roms m JOIN games g ON g.id = m.rom_id
                         WHERE m.key = c.key AND (?1 = 0 OR g.local_path IS NOT NULL))
                 FROM collections c ORDER BY c.kind, c.name COLLATE NOCASE, c.position",
            )
            .expect("prepare");
        stmt.query_map([downloaded_only], |r| {
            Ok(CollectionItem {
                key: r.get(0)?,
                kind: CollectionKind::from_code(r.get(1)?),
                remote_id: r.get(2)?,
                name: r.get(3)?,
                owner: r.get(4)?,
                count: r.get(5)?,
            })
        })
        .expect("query")
        .filter_map(Result::ok)
        .filter(|c| c.count > 0 || c.kind.editable())
        .collect()
    }

    pub fn collection(&self, key: &str) -> Option<CollectionItem> {
        self.collections(false).into_iter().find(|c| c.key == key)
    }

    pub fn favorites(&self) -> Option<CollectionItem> {
        self.collections(false)
            .into_iter()
            .find(|c| c.kind == CollectionKind::Favorites)
    }

    pub fn set_member(&mut self, key: &str, rom_id: i64, member: bool) {
        let sql = if member {
            "INSERT OR IGNORE INTO collection_roms (key, rom_id) VALUES (?1, ?2)"
        } else {
            "DELETE FROM collection_roms WHERE key = ?1 AND rom_id = ?2"
        };
        self.conn
            .execute(sql, params![key, rom_id])
            .expect("set member");
    }

    pub fn memberships(&self, rom_id: i64) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM collection_roms WHERE rom_id = ?1 ORDER BY key")
            .expect("prepare");
        stmt.query_map([rom_id], |r| r.get(0))
            .expect("query")
            .filter_map(Result::ok)
            .collect()
    }

    pub fn rename_collection(&mut self, key: &str, name: &str) {
        self.conn
            .execute(
                "UPDATE collections SET name = ?2 WHERE key = ?1",
                params![key, name],
            )
            .expect("rename collection");
    }

    pub fn delete_collection(&mut self, key: &str) {
        self.conn
            .execute("DELETE FROM collections WHERE key = ?1", [key])
            .expect("delete collection");
        self.conn
            .execute("DELETE FROM collection_roms WHERE key = ?1", [key])
            .expect("delete members");
    }
}

fn insert(tx: &rusqlite::Transaction, r: &CollectionRecord, position: i64) {
    tx.execute(
        "INSERT OR REPLACE INTO collections (key, kind, remote_id, name, owner, position)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![r.key, r.kind.code(), r.remote_id, r.name, r.owner, position],
    )
    .expect("insert collection");
    for id in &r.rom_ids {
        tx.execute(
            "INSERT OR IGNORE INTO collection_roms (key, rom_id) VALUES (?1, ?2)",
            params![r.key, id],
        )
        .expect("insert member");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::types::rom;
    use crate::store::{GameFilter, Scope};

    fn record(key: &str, kind: CollectionKind, name: &str, rom_ids: &[i64]) -> CollectionRecord {
        CollectionRecord {
            key: key.into(),
            kind,
            remote_id: key[2..].into(),
            name: name.into(),
            owner: None,
            rom_ids: rom_ids.to_vec(),
        }
    }

    fn seeded() -> Store {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_games(&[
            rom(1, 1, "A", "t"),
            rom(2, 1, "B", "t"),
            rom(3, 1, "C", "t"),
        ]);
        s.replace_collections(&[
            record("v:Zz", CollectionKind::Genre, "Shooter", &[1, 2, 99]),
            record("c:4", CollectionKind::Mine, "Couch", &[]),
            record("c:1", CollectionKind::Favorites, "Favourites", &[3]),
            record("v:Yy", CollectionKind::Franchise, "Nobody", &[99]),
            record("s:2", CollectionKind::Smart, "Recent", &[2]),
        ]);
        s
    }

    #[test]
    fn collections_are_grouped_by_kind_with_local_counts() {
        let names: Vec<_> = seeded()
            .collections(false)
            .into_iter()
            .map(|c| (c.name, c.count))
            .collect();
        assert_eq!(
            names,
            [
                ("Favourites".to_string(), 1),
                ("Couch".to_string(), 0),
                ("Recent".to_string(), 1),
                ("Shooter".to_string(), 2),
            ]
        );
    }

    #[test]
    fn downloaded_only_counts_downloaded_members() {
        let mut s = seeded();
        s.set_local_path(2, Some("/x"));
        let counts: Vec<_> = s
            .collections(true)
            .into_iter()
            .map(|c| (c.key, c.count))
            .collect();
        assert_eq!(
            counts,
            [
                ("c:1".to_string(), 0),
                ("c:4".to_string(), 0),
                ("s:2".to_string(), 1),
                ("v:Zz".to_string(), 1),
            ]
        );
    }

    #[test]
    fn games_can_be_filtered_by_collection() {
        let s = seeded();
        let ids: Vec<i64> = s
            .games(&GameFilter {
                scope: Scope::Collection("v:Zz".into()),
                ..GameFilter::default()
            })
            .into_iter()
            .map(|g| g.id)
            .collect();
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn membership_edits_rename_and_delete() {
        let mut s = seeded();
        assert_eq!(s.favorites().unwrap().key, "c:1");
        s.set_member("c:4", 1, true);
        s.set_member("c:4", 1, true);
        s.set_member("c:1", 3, false);
        assert_eq!(s.memberships(1), ["c:4", "v:Zz"]);
        assert!(s.memberships(3).is_empty());
        s.rename_collection("c:4", "Party");
        assert_eq!(s.collection("c:4").unwrap().name, "Party");
        s.delete_collection("c:4");
        assert!(s.collection("c:4").is_none());
        assert_eq!(s.memberships(1), ["v:Zz"]);
    }

    #[test]
    fn added_collections_replace_their_members() {
        let mut s = seeded();
        s.add_collection(&record("c:9", CollectionKind::Mine, "New", &[2]));
        s.add_collection(&record("c:9", CollectionKind::Mine, "New", &[3]));
        assert_eq!(s.collection("c:9").unwrap().count, 1);
        assert_eq!(s.memberships(3), ["c:1", "c:9"]);
    }

    #[test]
    fn game_list_marks_favorites() {
        let s = seeded();
        let favorites: Vec<(i64, bool)> = s
            .games(&GameFilter::default())
            .into_iter()
            .map(|g| (g.id, g.favorite))
            .collect();
        assert_eq!(favorites, [(1, false), (2, false), (3, true)]);
    }

    #[test]
    fn clearing_the_library_drops_collections() {
        let mut s = seeded();
        s.clear_library();
        assert!(s.collections(false).is_empty());
    }
}
