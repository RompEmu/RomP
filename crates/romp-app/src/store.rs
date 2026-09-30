mod collections;

pub use collections::{CollectionItem, CollectionKind, CollectionRecord};

use crate::romm::types::{Platform, Rom, RomMetadata};
use rusqlite::{params, Connection};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformItem {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameItem {
    pub id: i64,
    pub title: String,
    pub platform: String,
    pub platform_slug: String,
    pub cover: Option<String>,
    pub downloaded: bool,
    pub favorite: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaySession {
    pub id: i64,
    pub rom_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameDetail {
    pub id: i64,
    pub title: String,
    pub platform_id: i64,
    pub platform_slug: String,
    pub platform: String,
    pub platform_category: Option<String>,
    pub summary: Option<String>,
    pub size_bytes: i64,
    pub cover_small: Option<String>,
    pub cover_large: Option<String>,
    pub local_path: Option<String>,
    pub meta: RomMetadata,
    pub screenshots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateRecord {
    pub local_md5: String,
    pub remote_id: Option<i64>,
    pub remote_updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Platform(i64),
    Collection(String),
    Recent,
    ForYou,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortOrder {
    #[default]
    Name,
    Added,
    LastPlayed,
}

impl SortOrder {
    const ALL: [Self; 3] = [Self::Name, Self::Added, Self::LastPlayed];

    pub fn from_index(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or_default()
    }

    pub fn index(self) -> i32 {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as i32
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameFilter {
    pub scope: Scope,
    pub search: String,
    pub downloaded_only: bool,
    pub sort: SortOrder,
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

fn json_column<T: serde::de::DeserializeOwned + Default>(text: Option<String>) -> T {
    text.and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

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
        if version < 3 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE games ADD COLUMN meta TEXT;
                 ALTER TABLE games ADD COLUMN screenshots TEXT;
                 ALTER TABLE platforms ADD COLUMN category TEXT;
                 DELETE FROM kv WHERE key = 'last_sync_at';
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        if version < 4 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS collections (key TEXT PRIMARY KEY, kind INTEGER NOT NULL,
                   remote_id TEXT NOT NULL, name TEXT NOT NULL, owner TEXT, position INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS collection_roms (key TEXT NOT NULL, rom_id INTEGER NOT NULL,
                   PRIMARY KEY (key, rom_id));
                 CREATE INDEX IF NOT EXISTS collection_roms_rom ON collection_roms(rom_id);
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }
        if version < 5 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE games ADD COLUMN added_at INTEGER;
                 ALTER TABLE games ADD COLUMN last_played INTEGER;
                 DELETE FROM kv WHERE key = 'last_sync_at';
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
        }
        if version < 6 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS play_sessions (id INTEGER PRIMARY KEY AUTOINCREMENT,
                   rom_id INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL);
                 PRAGMA user_version = 6;
                 COMMIT;",
            )?;
        }
        if version < 7 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS recommendations (rom_id INTEGER PRIMARY KEY,
                   rank INTEGER NOT NULL, reason TEXT NOT NULL);
                 PRAGMA user_version = 7;
                 COMMIT;",
            )?;
        }
        if version < 8 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS pending_screenshots (path TEXT PRIMARY KEY,
                   rom_id INTEGER NOT NULL);
                 PRAGMA user_version = 8;
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
                "INSERT INTO platforms (id, slug, name, category) VALUES (?1, ?2, ?3, ?4)",
                params![p.id, p.slug, p.display_name, p.category],
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
                 (id, platform_id, title, summary, updated_at, cover_small, cover_large, size_bytes,
                  meta, screenshots, added_at, last_played)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                   platform_id = excluded.platform_id, title = excluded.title,
                   summary = excluded.summary, updated_at = excluded.updated_at,
                   cover_small = excluded.cover_small, cover_large = excluded.cover_large,
                   size_bytes = excluded.size_bytes, meta = excluded.meta,
                   screenshots = excluded.screenshots, added_at = excluded.added_at,
                   last_played = NULLIF(MAX(COALESCE(last_played, 0),
                                            COALESCE(excluded.last_played, 0)), 0)",
                params![
                    r.id,
                    r.platform_id,
                    r.title(),
                    r.summary,
                    r.updated_at,
                    non_empty(&r.path_cover_small),
                    non_empty(&r.path_cover_large),
                    r.fs_size_bytes,
                    r.metadatum
                        .as_ref()
                        .and_then(|m| serde_json::to_string(m).ok()),
                    serde_json::to_string(&r.merged_screenshots).ok(),
                    r.added_at(),
                    r.last_played()
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
                "SELECT p.id, p.slug, p.name, COUNT(g.id) FROM platforms p
                 JOIN games g ON g.platform_id = p.id
                 WHERE ?1 = 0 OR g.local_path IS NOT NULL
                 GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
            )
            .expect("prepare");
        stmt.query_map([downloaded_only], |r| {
            Ok(PlatformItem {
                id: r.get(0)?,
                slug: r.get(1)?,
                name: r.get(2)?,
                count: r.get(3)?,
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
        let sort = if filter.scope == Scope::Recent {
            SortOrder::LastPlayed
        } else {
            filter.sort
        };
        let order = match sort {
            _ if filter.scope == Scope::ForYou => "r.rank",
            SortOrder::Name => "g.title COLLATE NOCASE, g.id",
            SortOrder::Added => "g.added_at IS NULL, g.added_at DESC, g.title COLLATE NOCASE, g.id",
            SortOrder::LastPlayed => {
                "g.last_played IS NULL, g.last_played DESC, g.title COLLATE NOCASE, g.id"
            }
        };
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT g.id, g.title, COALESCE(p.name, ''), g.cover_small, g.local_path IS NOT NULL,
                        EXISTS (SELECT 1 FROM collection_roms m JOIN collections c ON c.key = m.key
                                WHERE m.rom_id = g.id AND c.kind = 0),
                        COALESCE(p.slug, '')
                 FROM games g LEFT JOIN platforms p ON p.id = g.platform_id
                   LEFT JOIN recommendations r ON r.rom_id = g.id
                 WHERE (?1 IS NULL OR g.platform_id = ?1) AND g.title LIKE ?2 ESCAPE '\\'
                   AND (?3 = 0 OR g.local_path IS NOT NULL)
                   AND (?4 IS NULL OR g.id IN (SELECT rom_id FROM collection_roms WHERE key = ?4))
                   AND (?5 = 0 OR g.last_played IS NOT NULL)
                   AND (?6 = 0 OR r.rom_id IS NOT NULL)
                 ORDER BY {order}",
            ))
            .expect("prepare");
        let (platform, collection) = match &filter.scope {
            Scope::All | Scope::Recent | Scope::ForYou => (None, None),
            Scope::Platform(id) => (Some(*id), None),
            Scope::Collection(key) => (None, Some(key.as_str())),
        };
        let recent = filter.scope == Scope::Recent;
        let for_you = filter.scope == Scope::ForYou;
        stmt.query_map(
            params![
                platform,
                pattern,
                filter.downloaded_only,
                collection,
                recent,
                for_you
            ],
            |r| {
                Ok(GameItem {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    platform: r.get(2)?,
                    cover: r.get(3)?,
                    downloaded: r.get(4)?,
                    favorite: r.get(5)?,
                    platform_slug: r.get(6)?,
                })
            },
        )
        .expect("query")
        .filter_map(Result::ok)
        .collect()
    }

    pub fn rebase_local_paths(&self, old: &str, new: &str) -> usize {
        self.conn
            .execute(
                "UPDATE games SET local_path = ?2 || substr(local_path, length(?1) + 1)
                 WHERE substr(local_path, 1, length(?1)) = ?1",
                params![old, new],
            )
            .expect("rebase local paths")
    }

    pub fn recent_count(&self, downloaded_only: bool) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM games WHERE last_played IS NOT NULL
                   AND (?1 = 0 OR local_path IS NOT NULL)",
                [downloaded_only],
                |r| r.get(0),
            )
            .expect("count recent")
    }

    pub fn add_pending_screenshot(&self, rom_id: i64, path: &str) {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO pending_screenshots (path, rom_id) VALUES (?1, ?2)",
                params![path, rom_id],
            )
            .expect("add pending screenshot");
    }

    pub fn pending_screenshots(&self) -> Vec<(i64, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT rom_id, path FROM pending_screenshots ORDER BY rowid")
            .expect("prepare");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .filter_map(Result::ok)
            .collect()
    }

    pub fn remove_pending_screenshot(&self, path: &str) {
        self.conn
            .execute("DELETE FROM pending_screenshots WHERE path = ?1", [path])
            .expect("remove pending screenshot");
    }

    pub fn replace_recommendations(&mut self, picks: &[(i64, String)]) {
        let tx = self.conn.transaction().expect("tx");
        tx.execute("DELETE FROM recommendations", [])
            .expect("clear recommendations");
        for (rank, (rom_id, reason)) in picks.iter().enumerate() {
            tx.execute(
                "INSERT OR IGNORE INTO recommendations (rom_id, rank, reason) VALUES (?1, ?2, ?3)",
                params![rom_id, rank as i64, reason],
            )
            .expect("add recommendation");
        }
        tx.commit().expect("commit");
    }

    pub fn recommended_count(&self, downloaded_only: bool) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM recommendations r JOIN games g ON g.id = r.rom_id
                 WHERE ?1 = 0 OR g.local_path IS NOT NULL",
                [downloaded_only],
                |r| r.get(0),
            )
            .expect("count recommendations")
    }

    pub fn reason(&self, rom_id: i64) -> Option<String> {
        self.conn
            .query_row(
                "SELECT reason FROM recommendations WHERE rom_id = ?1",
                [rom_id],
                |r| r.get(0),
            )
            .ok()
    }

    #[cfg(test)]
    pub fn last_played(&self, id: i64) -> Option<i64> {
        self.conn
            .query_row("SELECT last_played FROM games WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .ok()
            .flatten()
    }

    pub fn mark_played(&mut self, id: i64, when: i64) {
        self.merge_last_played(&[(id, when)]);
    }

    pub fn merge_last_played(&mut self, plays: &[(i64, i64)]) {
        let tx = self.conn.transaction().expect("tx");
        for (id, when) in plays {
            tx.execute(
                "UPDATE games SET last_played = MAX(COALESCE(last_played, 0), ?2) WHERE id = ?1",
                params![id, when],
            )
            .expect("mark played");
        }
        tx.commit().expect("commit");
    }

    pub fn game(&self, id: i64) -> Option<GameDetail> {
        self.conn
            .query_row(
                "SELECT g.id, g.title, g.platform_id, COALESCE(p.slug, ''), COALESCE(p.name, ''),
                        g.summary, g.size_bytes, g.cover_small, g.cover_large, g.local_path,
                        p.category, g.meta, g.screenshots
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
                        platform_category: r.get(10)?,
                        meta: json_column(r.get(11)?),
                        screenshots: json_column(r.get(12)?),
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

    pub fn add_play_session(&self, rom_id: i64, start_ms: i64, end_ms: i64) {
        self.conn
            .execute(
                "INSERT INTO play_sessions (rom_id, start_ms, end_ms) VALUES (?1, ?2, ?3)",
                [rom_id, start_ms, end_ms],
            )
            .expect("add play session");
    }

    pub fn play_sessions(&self) -> Vec<PlaySession> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, rom_id, start_ms, end_ms FROM play_sessions ORDER BY id")
            .expect("prepare");
        stmt.query_map([], |r| {
            Ok(PlaySession {
                id: r.get(0)?,
                rom_id: r.get(1)?,
                start_ms: r.get(2)?,
                end_ms: r.get(3)?,
            })
        })
        .expect("query")
        .filter_map(Result::ok)
        .collect()
    }

    pub fn remove_play_sessions(&mut self, ids: &[i64]) {
        let tx = self.conn.transaction().expect("tx");
        for id in ids {
            tx.execute("DELETE FROM play_sessions WHERE id = ?1", [id])
                .expect("remove play session");
        }
        tx.commit().expect("commit");
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
                 DELETE FROM play_sessions; DELETE FROM recommendations;
                 DELETE FROM pending_screenshots;
                 DELETE FROM collections; DELETE FROM collection_roms;
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
            category: None,
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
        let f = |platform: Option<i64>, search: &str| GameFilter {
            scope: platform.map_or(Scope::All, Scope::Platform),
            search: search.into(),
            ..GameFilter::default()
        };
        assert_eq!(ids(&s, f(Some(1), "")), [11, 10]);
        assert_eq!(ids(&s, f(None, "TRI")), [11, 12]);
        assert_eq!(ids(&s, f(Some(2), "zel")), Vec::<i64>::new());
        assert_eq!(ids(&s, f(None, "50%_")), Vec::<i64>::new());
    }

    fn played(id: i64, platform: i64, name: &str, added: &str, last: Option<&str>) -> Rom {
        let mut r = rom(id, platform, name, "t");
        r.created_at = Some(added.into());
        r.rom_user = Some(crate::romm::types::RomUser {
            last_played: last.map(String::from),
        });
        r
    }

    fn sorted(s: &Store, scope: Scope, sort: SortOrder) -> Vec<i64> {
        ids(
            s,
            GameFilter {
                scope,
                sort,
                ..GameFilter::default()
            },
        )
    }

    #[test]
    fn sort_order_round_trips_through_its_index() {
        for sort in [SortOrder::Name, SortOrder::Added, SortOrder::LastPlayed] {
            assert_eq!(SortOrder::from_index(sort.index()), sort);
        }
        assert_eq!(SortOrder::from_index(7), SortOrder::Name);
        assert_eq!(SortOrder::from_index(-1), SortOrder::Name);
    }

    #[test]
    fn games_sort_by_name_date_added_or_last_played() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_games(&[
            played(1, 1, "Banjo", "2026-03-01T00:00:00Z", None),
            played(
                2,
                1,
                "Aladdin",
                "2026-01-01T00:00:00Z",
                Some("2026-09-01T00:00:00Z"),
            ),
            played(
                3,
                1,
                "Castlevania",
                "2026-02-01T00:00:00Z",
                Some("2026-09-05T00:00:00Z"),
            ),
        ]);
        assert_eq!(sorted(&s, Scope::All, SortOrder::Name), [2, 1, 3]);
        assert_eq!(sorted(&s, Scope::All, SortOrder::Added), [1, 3, 2]);
        assert_eq!(sorted(&s, Scope::All, SortOrder::LastPlayed), [3, 2, 1]);
        assert_eq!(
            sorted(&s, Scope::Recent, SortOrder::Name),
            [3, 2],
            "recent is newest first"
        );
        assert_eq!(s.recent_count(false), 2);
        assert_eq!(s.recent_count(true), 0);
    }

    #[test]
    fn recommendations_keep_the_servers_order_and_reasons() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_games(&[
            rom(1, 1, "Aladdin", "t"),
            rom(2, 1, "Banjo", "t"),
            rom(3, 1, "Castlevania", "t"),
        ]);
        assert_eq!(s.recommended_count(false), 0);
        s.replace_recommendations(&[
            (3, "Because you played Contra".into()),
            (1, "Because you played Bomberman".into()),
            (99, "Not in the library".into()),
        ]);
        assert_eq!(sorted(&s, Scope::ForYou, SortOrder::Name), [3, 1]);
        assert_eq!(s.recommended_count(false), 2);
        assert_eq!(s.recommended_count(true), 0);
        assert_eq!(s.reason(1).as_deref(), Some("Because you played Bomberman"));
        assert_eq!(s.reason(2), None);
        s.replace_recommendations(&[(2, "New".into())]);
        assert_eq!(sorted(&s, Scope::ForYou, SortOrder::Name), [2]);
        s.clear_library();
        assert_eq!(s.reason(2), None);
    }

    #[test]
    fn a_play_here_or_on_the_server_only_moves_last_played_forward() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_games(&[played(
            1,
            1,
            "A",
            "2026-01-01T00:00:00Z",
            Some("2026-09-01T00:00:00Z"),
        )]);
        let later = crate::sync::parse_iso("2026-09-10T00:00:00Z").unwrap();
        s.mark_played(1, later);
        s.upsert_games(&[played(
            1,
            1,
            "A",
            "2026-01-01T00:00:00Z",
            Some("2026-09-01T00:00:00Z"),
        )]);
        assert_eq!(s.last_played(1), Some(later));
        s.merge_last_played(&[(1, later + 5)]);
        assert_eq!(s.last_played(1), Some(later + 5));
        s.merge_last_played(&[(1, later - 5)]);
        assert_eq!(s.last_played(1), Some(later + 5));
    }

    #[test]
    fn upsert_replaces_existing_rows() {
        let mut s = seeded();
        s.upsert_games(&[rom(12, 2, "Tetris DX", "2026-02-01T00:00:00+00:00")]);
        let g = s.games(&GameFilter {
            scope: Scope::Platform(2),
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
                    slug: "game boy".into(),
                    name: "Game Boy".into(),
                    count: 1
                },
                PlatformItem {
                    id: 1,
                    slug: "snes".into(),
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
    fn downloaded_paths_follow_a_moved_data_folder() {
        let mut s = seeded();
        s.set_local_path(10, Some("/data/Cartridge/roms/10/Zelda.sfc"));
        s.set_local_path(11, Some("/elsewhere/Chrono.sfc"));
        s.set_local_path(12, Some("/data/Cartridge2/roms/12/Tetris.gb"));
        assert_eq!(s.rebase_local_paths("/data/Cartridge/", "/data/Romp/"), 1);
        let path = |id| s.game(id).unwrap().local_path.unwrap();
        assert_eq!(path(10), "/data/Romp/roms/10/Zelda.sfc");
        assert_eq!(path(11), "/elsewhere/Chrono.sfc");
        assert_eq!(path(12), "/data/Cartridge2/roms/12/Tetris.gb");
        assert_eq!(s.rebase_local_paths("/data/Cartridge/", "/data/Romp/"), 0);
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
                slug: "snes".into(),
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
    fn play_sessions_wait_until_uploaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db");
        {
            let s = Store::open(&path).unwrap();
            s.add_play_session(7, 1_000, 61_000);
            s.add_play_session(3, 5_000, 95_000);
        }
        let mut s = Store::open(&path).unwrap();
        let sessions = s.play_sessions();
        assert_eq!(
            sessions
                .iter()
                .map(|p| (p.rom_id, p.start_ms, p.end_ms))
                .collect::<Vec<_>>(),
            [(7, 1_000, 61_000), (3, 5_000, 95_000)]
        );
        s.remove_play_sessions(&[sessions[0].id]);
        assert_eq!(s.play_sessions().len(), 1);
        s.clear_library();
        assert!(s.play_sessions().is_empty());
    }

    #[test]
    fn screenshots_wait_to_upload() {
        let mut s = Store::open_in_memory().unwrap();
        s.add_pending_screenshot(7, "/shots/b.png");
        s.add_pending_screenshot(3, "/shots/a.png");
        s.add_pending_screenshot(3, "/shots/a.png");
        assert_eq!(
            s.pending_screenshots(),
            [(7, "/shots/b.png".into()), (3, "/shots/a.png".into())]
        );
        s.remove_pending_screenshot("/shots/b.png");
        assert_eq!(s.pending_screenshots().len(), 1);
        s.clear_library();
        assert!(s.pending_screenshots().is_empty());
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
    fn game_detail_carries_metadata_screenshots_and_category() {
        let mut s = Store::open_in_memory().unwrap();
        s.replace_platforms(&[Platform {
            category: Some("Console".into()),
            ..platform(1, "SNES")
        }]);
        let mut r = rom(5, 1, "Chrono Trigger", "t");
        r.metadatum = Some(RomMetadata {
            genres: vec!["Role-playing (RPG)".into()],
            player_count: Some("1".into()),
            first_release_date: Some(795_052_800_000),
            ..RomMetadata::default()
        });
        r.merged_screenshots = vec!["/a.jpg".into(), "/b.jpg".into()];
        s.upsert_games(&[r]);
        let g = s.game(5).unwrap();
        assert_eq!(g.platform_category.as_deref(), Some("Console"));
        assert_eq!(g.meta.genres, ["Role-playing (RPG)"]);
        assert_eq!(g.meta.first_release_date, Some(795_052_800_000));
        assert_eq!(g.screenshots, ["/a.jpg", "/b.jpg"]);
        s.upsert_games(&[rom(6, 1, "Plain", "t")]);
        let plain = s.game(6).unwrap();
        assert_eq!(plain.meta, RomMetadata::default());
        assert!(plain.screenshots.is_empty());
    }

    #[test]
    fn v2_database_migrates_to_v3_and_forgets_the_sync_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v2.db");
        {
            let s = Store::open(&path).unwrap();
            s.set("last_sync_at", "t");
            s.conn
                .execute_batch(
                    "ALTER TABLE games DROP COLUMN meta; ALTER TABLE games DROP COLUMN screenshots;
                     ALTER TABLE games DROP COLUMN added_at; ALTER TABLE games DROP COLUMN last_played;
                     ALTER TABLE platforms DROP COLUMN category; PRAGMA user_version = 2;",
                )
                .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.get("last_sync_at"), None);
        s.set("last_sync_at", "u");
        drop(s);
        assert_eq!(
            Store::open(&path).unwrap().get("last_sync_at").as_deref(),
            Some("u")
        );
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
