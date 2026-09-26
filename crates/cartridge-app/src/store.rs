use crate::romm::types::{Platform, Rom};
use rusqlite::{params, Connection};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformItem {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameItem {
    pub id: i64,
    pub title: String,
    pub platform: String,
    pub cover: Option<String>,
    pub downloaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDetail {
    pub id: i64,
    pub title: String,
    pub platform_id: i64,
    pub platform_slug: String,
    pub platform: String,
    pub summary: Option<String>,
    pub size_bytes: i64,
    pub cover_small: Option<String>,
    pub cover_large: Option<String>,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateRecord {
    pub local_md5: String,
    pub remote_id: Option<i64>,
    pub remote_updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameFilter {
    pub platform: Option<i64>,
    pub search: String,
    pub downloaded_only: bool,
}

pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS platforms (id INTEGER PRIMARY KEY, slug TEXT NOT NULL, name TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS games (
    id INTEGER PRIMARY KEY,
    platform_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    summary TEXT,
    updated_at TEXT NOT NULL,
    cover_small TEXT,
    cover_large TEXT,
    size_bytes INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS games_platform ON games(platform_id);
";

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| !s.is_empty())
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(SCHEMA)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "BEGIN; ALTER TABLE games ADD COLUMN local_path TEXT; PRAGMA user_version = 1; COMMIT;",
            )?;
        }
        if version < 2 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS state_sync (rom_id INTEGER NOT NULL, file TEXT NOT NULL,
                   local_md5 TEXT NOT NULL, remote_id INTEGER, remote_updated_at TEXT,
                   PRIMARY KEY (rom_id, file));
                 CREATE TABLE IF NOT EXISTS pending_saves (rom_id INTEGER PRIMARY KEY);
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
            .ok()
    }

    pub fn set(&self, key: &str, value: &str) {
        self.conn
            .execute(
                "INSERT INTO kv (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .expect("write kv");
    }

    pub fn remove(&self, key: &str) {
        self.conn
            .execute("DELETE FROM kv WHERE key = ?1", [key])
            .expect("delete kv");
    }

    pub fn replace_platforms(&mut self, platforms: &[Platform]) {
        let tx = self.conn.transaction().expect("tx");
        tx.execute("DELETE FROM platforms", [])
            .expect("clear platforms");
        for p in platforms {
            tx.execute(
                "INSERT INTO platforms (id, slug, name) VALUES (?1, ?2, ?3)",
                params![p.id, p.slug, p.display_name],
            )
            .expect("insert platform");
        }
        tx.commit().expect("commit");
    }

    pub fn upsert_games(&mut self, roms: &[Rom]) {
        let tx = self.conn.transaction().expect("tx");
        for r in roms {
            tx.execute(
                "INSERT INTO games
                 (id, platform_id, title, summary, updated_at, cover_small, cover_large, size_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                   platform_id = excluded.platform_id, title = excluded.title,
                   summary = excluded.summary, updated_at = excluded.updated_at,
                   cover_small = excluded.cover_small, cover_large = excluded.cover_large,
                   size_bytes = excluded.size_bytes",
                params![
                    r.id,
                    r.platform_id,
                    r.title(),
                    r.summary,
                    r.updated_at,
                    non_empty(&r.path_cover_small),
                    non_empty(&r.path_cover_large),
                    r.fs_size_bytes
                ],
            )
            .expect("upsert game");
        }
        tx.commit().expect("commit");
    }

    pub fn retain_games(&mut self, ids: &[i64]) -> usize {
        let tx = self.conn.transaction().expect("tx");
        tx.execute(
            "CREATE TEMP TABLE IF NOT EXISTS keep (id INTEGER PRIMARY KEY)",
            [],
        )
        .expect("temp table");
        tx.execute("DELETE FROM keep", []).expect("clear keep");
        for id in ids {
            tx.execute("INSERT OR IGNORE INTO keep (id) VALUES (?1)", [id])
                .expect("keep id");
        }
        let removed = tx
            .execute(
                "DELETE FROM games WHERE id NOT IN (SELECT id FROM keep) AND local_path IS NULL",
                [],
            )
            .expect("retain");
        tx.commit().expect("commit");
        removed
    }

    pub fn platforms(&self, downloaded_only: bool) -> Vec<PlatformItem> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT p.id, p.name, COUNT(g.id) FROM platforms p
                 JOIN games g ON g.platform_id = p.id
                 WHERE ?1 = 0 OR g.local_path IS NOT NULL
                 GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
            )
            .expect("prepare");
        stmt.query_map([downloaded_only], |r| {
            Ok(PlatformItem {
                id: r.get(0)?,
                name: r.get(1)?,
                count: r.get(2)?,
            })
        })
        .expect("query")
        .filter_map(Result::ok)
        .collect()
    }

    pub fn games(&self, filter: &GameFilter) -> Vec<GameItem> {
        let pattern = format!(
            "%{}%",
            filter
                .search
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self
            .conn
            .prepare(
                "SELECT g.id, g.title, COALESCE(p.name, ''), g.cover_small, g.local_path IS NOT NULL
                 FROM games g LEFT JOIN platforms p ON p.id = g.platform_id
                 WHERE (?1 IS NULL OR g.platform_id = ?1) AND g.title LIKE ?2 ESCAPE '\\'
                   AND (?3 = 0 OR g.local_path IS NOT NULL)
                 ORDER BY g.title COLLATE NOCASE, g.id",
            )
            .expect("prepare");
        stmt.query_map(
            params![filter.platform, pattern, filter.downloaded_only],
            |r| {
                Ok(GameItem {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    platform: r.get(2)?,
                    cover: r.get(3)?,
                    downloaded: r.get(4)?,
                })
            },
        )
        .expect("query")
        .filter_map(Result::ok)
        .collect()
    }

    pub fn game(&self, id: i64) -> Option<GameDetail> {
        self.conn
            .query_row(
                "SELECT g.id, g.title, g.platform_id, COALESCE(p.slug, ''), COALESCE(p.name, ''),
                        g.summary, g.size_bytes, g.cover_small, g.cover_large, g.local_path
                 FROM games g LEFT JOIN platforms p ON p.id = g.platform_id WHERE g.id = ?1",
                [id],
                |r| {
                    Ok(GameDetail {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        platform_id: r.get(2)?,
                        platform_slug: r.get(3)?,
                        platform: r.get(4)?,
                        summary: r.get(5)?,
                        size_bytes: r.get(6)?,
                        cover_small: r.get(7)?,
                        cover_large: r.get(8)?,
                        local_path: r.get(9)?,
                    })
                },
            )
            .ok()
    }

    pub fn set_local_path(&mut self, id: i64, path: Option<&str>) {
        self.conn
            .execute(
                "UPDATE games SET local_path = ?2 WHERE id = ?1",
                params![id, path],
            )
            .expect("set local path");
    }

    pub fn state_record(&self, rom_id: i64, file: &str) -> Option<StateRecord> {
        self.conn
            .query_row(
                "SELECT local_md5, remote_id, remote_updated_at FROM state_sync
                 WHERE rom_id = ?1 AND file = ?2",
                params![rom_id, file],
                |r| {
                    Ok(StateRecord {
                        local_md5: r.get(0)?,
                        remote_id: r.get(1)?,
                        remote_updated_at: r.get(2)?,
                    })
                },
            )
            .ok()
    }

    pub fn set_state_record(&mut self, rom_id: i64, file: &str, record: &StateRecord) {
        self.conn
            .execute(
                "INSERT INTO state_sync (rom_id, file, local_md5, remote_id, remote_updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(rom_id, file) DO UPDATE SET local_md5 = excluded.local_md5,
                   remote_id = excluded.remote_id, remote_updated_at = excluded.remote_updated_at",
                params![
                    rom_id,
                    file,
                    record.local_md5,
                    record.remote_id,
                    record.remote_updated_at
                ],
            )
            .expect("write state record");
    }

    pub fn add_pending(&mut self, rom_id: i64) {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO pending_saves (rom_id) VALUES (?1)",
                [rom_id],
            )
            .expect("add pending");
    }

    pub fn remove_pending(&mut self, rom_id: i64) {
        self.conn
            .execute("DELETE FROM pending_saves WHERE rom_id = ?1", [rom_id])
            .expect("remove pending");
    }

    pub fn pending(&self) -> Vec<i64> {
        let mut stmt = self
            .conn
            .prepare("SELECT rom_id FROM pending_saves ORDER BY rom_id")
            .expect("prepare");
        stmt.query_map([], |r| r.get(0))
            .expect("query")
            .filter_map(Result::ok)
            .collect()
    }

    pub fn switch_server(&mut self, server: &str) {
        if self.get("server").as_deref() != Some(server) {
            self.clear_library();
            self.set("server", server);
        }
    }

    pub fn game_count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM games", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    pub fn clear_library(&mut self) {
        self.conn
            .execute_batch(
                "DELETE FROM games; DELETE FROM platforms; DELETE FROM state_sync; DELETE FROM pending_saves;
                 DELETE FROM kv WHERE key = 'last_sync_at';",
            )
            .expect("clear library");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::types::rom;

    fn platform(id: i64, name: &str) -> Platform {
        Platform {
            id,
            slug: name.to_lowercase(),
            display_name: name.into(),
            rom_count: 0,
        }
    }

    fn seeded() -> Store {
        let mut s = Store::open_in_memory().unwrap();
        s.replace_platforms(&[
            platform(1, "SNES"),
            platform(2, "Game Boy"),
            platform(3, "Empty"),
        ]);
        s.upsert_games(&[
            rom(10, 1, "zelda", "2026-01-01T00:00:00+00:00"),
            rom(11, 1, "Chrono Trigger", "2026-01-01T00:00:00+00:00"),
            rom(12, 2, "Tetris", "2026-01-01T00:00:00+00:00"),
        ]);
        s
    }

    fn ids(s: &Store, f: GameFilter) -> Vec<i64> {
        s.games(&f).into_iter().map(|g| g.id).collect()
    }

    #[test]
    fn kv_round_trip() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get("server"), None);
        s.set("server", "http://a/");
        s.set("server", "http://b/");
        assert_eq!(s.get("server").as_deref(), Some("http://b/"));
        s.remove("server");
        assert_eq!(s.get("server"), None);
    }

    #[test]
    fn games_sorted_case_insensitively_with_platform_names() {
        let titles: Vec<_> = seeded()
            .games(&GameFilter::default())
            .into_iter()
            .map(|g| (g.title, g.platform))
            .collect();
        assert_eq!(
            titles,
            [
                ("Chrono Trigger".to_string(), "SNES".to_string()),
                ("Tetris".to_string(), "Game Boy".to_string()),
                ("zelda".to_string(), "SNES".to_string())
            ]
        );
    }

    #[test]
    fn filter_by_platform_and_search() {
        let s = seeded();
        let f = |platform, search: &str| GameFilter {
            platform,
            search: search.into(),
            downloaded_only: false,
        };
        assert_eq!(ids(&s, f(Some(1), "")), [11, 10]);
        assert_eq!(ids(&s, f(None, "TRI")), [11, 12]);
        assert_eq!(ids(&s, f(Some(2), "zel")), Vec::<i64>::new());
        assert_eq!(ids(&s, f(None, "50%_")), Vec::<i64>::new());
    }

    #[test]
    fn upsert_replaces_existing_rows() {
        let mut s = seeded();
        s.upsert_games(&[rom(12, 2, "Tetris DX", "2026-02-01T00:00:00+00:00")]);
        let g = s.games(&GameFilter {
            platform: Some(2),
            ..GameFilter::default()
        });
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].title, "Tetris DX");
    }

    #[test]
    fn platforms_list_only_non_empty_with_counts() {
        assert_eq!(
            seeded().platforms(false),
            [
                PlatformItem {
                    id: 2,
                    name: "Game Boy".into(),
                    count: 1
                },
                PlatformItem {
                    id: 1,
                    name: "SNES".into(),
                    count: 2
                }
            ]
        );
    }

    #[test]
    fn retain_removes_missing_games() {
        let mut s = seeded();
        assert_eq!(s.retain_games(&[10, 12]), 1);
        assert_eq!(ids(&s, GameFilter::default()), [12, 10]);
    }

    #[test]
    fn empty_cover_path_is_none() {
        let mut s = Store::open_in_memory().unwrap();
        let mut r = rom(1, 1, "A", "t");
        r.path_cover_small = Some(String::new());
        s.upsert_games(&[r]);
        assert_eq!(s.games(&GameFilter::default())[0].cover, None);
    }

    #[test]
    fn clear_library_drops_games_and_platforms_but_keeps_kv() {
        let mut s = seeded();
        s.set("device_id", "x");
        s.clear_library();
        assert!(s.games(&GameFilter::default()).is_empty());
        assert!(s.platforms(false).is_empty());
        assert_eq!(s.get("device_id").as_deref(), Some("x"));
    }

    #[test]
    fn reopening_a_file_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db");
        Store::open(&path).unwrap().set("k", "v");
        assert_eq!(Store::open(&path).unwrap().get("k").as_deref(), Some("v"));
    }

    #[test]
    fn switching_server_clears_the_library_only_when_it_changes() {
        let mut s = seeded();
        s.set("last_sync_at", "t");
        s.switch_server("http://a/");
        assert_eq!(s.game_count(), 0);
        assert_eq!(s.get("server").as_deref(), Some("http://a/"));
        assert_eq!(s.get("last_sync_at"), None);
        s.upsert_games(&[rom(1, 1, "A", "x")]);
        s.set("last_sync_at", "t");
        s.switch_server("http://a/");
        assert_eq!(s.game_count(), 1);
        assert_eq!(s.get("last_sync_at").as_deref(), Some("t"));
    }

    #[test]
    fn upsert_keeps_local_path() {
        let mut s = seeded();
        s.set_local_path(12, Some("/roms/gb/12/Tetris.gb"));
        s.upsert_games(&[rom(12, 2, "Tetris DX", "2026-02-01T00:00:00+00:00")]);
        let g = s.game(12).unwrap();
        assert_eq!(g.title, "Tetris DX");
        assert_eq!(g.local_path.as_deref(), Some("/roms/gb/12/Tetris.gb"));
    }

    #[test]
    fn retain_keeps_downloaded_games() {
        let mut s = seeded();
        s.set_local_path(11, Some("/x"));
        assert_eq!(s.retain_games(&[10]), 1);
        assert_eq!(ids(&s, GameFilter::default()), [11, 10]);
    }

    #[test]
    fn downloaded_only_filter_and_flag() {
        let mut s = seeded();
        s.set_local_path(10, Some("/x"));
        let f = GameFilter {
            downloaded_only: true,
            ..GameFilter::default()
        };
        let games = s.games(&f);
        assert_eq!(games.len(), 1);
        assert!(games[0].downloaded);
        s.set_local_path(10, None);
        assert!(s.games(&f).is_empty());
    }

    #[test]
    fn game_detail_joins_platform() {
        let s = seeded();
        let g = s.game(12).unwrap();
        assert_eq!(
            (g.platform_slug.as_str(), g.platform.as_str(), g.size_bytes),
            ("game boy", "Game Boy", 1024)
        );
        assert!(s.game(999).is_none());
    }

    #[test]
    fn migrating_a_v0_database_adds_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE games (id INTEGER PRIMARY KEY, platform_id INTEGER NOT NULL, title TEXT NOT NULL,
                 summary TEXT, updated_at TEXT NOT NULL, cover_small TEXT, cover_large TEXT, size_bytes INTEGER NOT NULL);
                 INSERT INTO games VALUES (1, 1, 'A', NULL, 't', NULL, NULL, 5);",
            )
            .unwrap();
        let mut s = Store::open(&path).unwrap();
        s.set_local_path(1, Some("/x"));
        assert_eq!(s.game(1).unwrap().local_path.as_deref(), Some("/x"));
        drop(s);
        Store::open(&path).unwrap();
    }

    #[test]
    fn downloaded_only_platform_counts() {
        let mut s = seeded();
        s.set_local_path(10, Some("/x"));
        assert_eq!(
            s.platforms(true),
            [PlatformItem {
                id: 1,
                name: "SNES".into(),
                count: 1
            }]
        );
    }

    #[test]
    fn state_records_round_trip() {
        let mut s = seeded();
        assert_eq!(s.state_record(10, "slot-1"), None);
        let record = StateRecord {
            local_md5: "abc".into(),
            remote_id: Some(3),
            remote_updated_at: Some("t".into()),
        };
        s.set_state_record(10, "slot-1", &record);
        assert_eq!(s.state_record(10, "slot-1"), Some(record.clone()));
        let newer = StateRecord {
            local_md5: "def".into(),
            ..record
        };
        s.set_state_record(10, "slot-1", &newer);
        assert_eq!(s.state_record(10, "slot-1"), Some(newer));
    }

    #[test]
    fn pending_saves_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db");
        {
            let mut s = Store::open(&path).unwrap();
            s.add_pending(7);
            s.add_pending(7);
            s.add_pending(3);
        }
        let mut s = Store::open(&path).unwrap();
        assert_eq!(s.pending(), [3, 7]);
        s.remove_pending(3);
        assert_eq!(s.pending(), [7]);
        s.clear_library();
        assert!(s.pending().is_empty());
    }

    #[test]
    fn v1_database_migrates_to_v2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.db");
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE games (id INTEGER PRIMARY KEY, platform_id INTEGER NOT NULL, title TEXT NOT NULL,
                 summary TEXT, updated_at TEXT NOT NULL, cover_small TEXT, cover_large TEXT, size_bytes INTEGER NOT NULL,
                 local_path TEXT);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        let mut s = Store::open(&path).unwrap();
        s.add_pending(1);
        assert_eq!(s.pending(), [1]);
    }
}
