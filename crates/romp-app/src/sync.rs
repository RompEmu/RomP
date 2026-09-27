use crate::romm::client::{Client, Error};
use crate::store::Store;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PAGE_SIZE: i64 = 500;
const CURSOR_MARGIN: Duration = Duration::from_secs(60);

pub fn cursor_from(time: SystemTime) -> String {
    iso_utc(time.checked_sub(CURSOR_MARGIN).unwrap_or(UNIX_EPOCH))
}

pub fn iso_utc(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn parse_iso(text: &str) -> Option<i64> {
    let text = text.trim();
    let num = |range: std::ops::Range<usize>| -> Option<i64> { text.get(range)?.parse().ok() };
    if text.len() < 19 || !matches!(text.as_bytes()[10], b'T' | b' ') {
        return None;
    }
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        let padded = format!("{:0<3}", &fraction[..digits.min(3)]);
        millis = padded.parse::<i64>().ok()?;
        rest = &fraction[digits..];
    }
    let offset_minutes = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours: i64 = rest.get(1..3)?.parse().ok()?;
            let minutes: i64 = rest
                .get(3..)
                .map(|m| m.trim_start_matches(':'))
                .unwrap_or("0")
                .parse()
                .ok()?;
            sign * (hours * 60 + minutes)
        }
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, mo as u32, d as u32);
    let seconds = days * 86_400 + h * 3600 + mi * 60 + s - offset_minutes * 60;
    Some(seconds * 1000 + millis)
}

pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub updated: usize,
    pub removed: usize,
}

fn write<T>(
    store: &Mutex<Store>,
    cancel: &AtomicBool,
    f: impl FnOnce(&mut Store) -> T,
) -> Result<T, Error> {
    let mut store = store.lock().unwrap();
    if cancel.load(Ordering::SeqCst) {
        return Err(Error::Cancelled);
    }
    Ok(f(&mut store))
}

async fn fetch_pages(
    client: &Client,
    store: &Mutex<Store>,
    page_size: i64,
    since: Option<&str>,
    cancel: &AtomicBool,
    report: &mut SyncReport,
) -> Result<Option<String>, Error> {
    let mut newest: Option<String> = None;
    let mut offset = 0;
    loop {
        let page = client.roms_page(offset, page_size, since).await?;
        let count = page.items.len();
        for rom in &page.items {
            if newest
                .as_deref()
                .is_none_or(|n| rom.updated_at.as_str() > n)
            {
                newest = Some(rom.updated_at.clone());
            }
        }
        write(store, cancel, |s| s.upsert_games(&page.items))?;
        report.updated += count;
        offset += count as i64;
        if (count as i64) < page_size {
            return Ok(newest);
        }
    }
}

pub async fn sync_library(
    client: &Client,
    store: &Mutex<Store>,
    page_size: i64,
    cancel: &AtomicBool,
) -> Result<SyncReport, Error> {
    let started = client.server_time().await;
    let platforms = client.platforms().await?;
    write(store, cancel, |s| s.replace_platforms(&platforms))?;

    let since = store.lock().unwrap().get("last_sync_at");
    let mut report = SyncReport::default();
    let mut newest = fetch_pages(
        client,
        store,
        page_size,
        since.as_deref(),
        cancel,
        &mut report,
    )
    .await?;

    let ids = client.rom_ids().await?;
    report.removed = write(store, cancel, |s| s.retain_games(&ids))?;
    if since.is_some() && store.lock().unwrap().game_count() < ids.len() {
        let full = fetch_pages(client, store, page_size, None, cancel, &mut report).await?;
        newest = newest.max(full);
    }

    if let Some(cursor) = started.map(cursor_from).or(newest) {
        write(store, cancel, |s| s.set("last_sync_at", &cursor))?;
    }
    Ok(report)
}

pub async fn sync_played(
    client: &Client,
    store: &Mutex<Store>,
    page_size: i64,
    cancel: &AtomicBool,
) -> Result<usize, Error> {
    let mut offset = 0;
    let mut merged = 0;
    loop {
        let page = client.played_roms(offset, page_size).await?;
        let count = page.items.len();
        let plays: Vec<(i64, i64)> = page
            .items
            .iter()
            .filter_map(|r| Some((r.id, r.last_played()?)))
            .collect();
        merged += plays.len();
        write(store, cancel, |s| s.merge_last_played(&plays))?;
        offset += count as i64;
        if (count as i64) < page_size {
            return Ok(merged);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times_parse_with_fractions_and_offsets() {
        let base = parse_iso("2026-09-26T10:00:00+00:00").unwrap();
        assert_eq!(parse_iso("2026-09-26T10:00:00Z"), Some(base));
        assert_eq!(parse_iso("2026-09-26T10:00:00.5Z"), Some(base + 500));
        assert_eq!(
            parse_iso("2026-09-26T10:00:00.123456+00:00"),
            Some(base + 123)
        );
        assert_eq!(parse_iso("2026-09-26T12:00:00+02:00"), Some(base));
        assert_eq!(parse_iso("2026-09-26 10:00:00"), Some(base));
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("yesterday"), None);
        assert_eq!(
            parse_iso(&iso_utc(UNIX_EPOCH + Duration::from_secs(1_790_000_000))),
            Some(1_790_000_000_000)
        );
    }
    use crate::romm::client::tests::base_of;
    use crate::romm::types::rom;
    use crate::store::GameFilter;
    use serde_json::{json, Value};
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn rom_json(id: i64, updated_at: &str) -> Value {
        json!({"id": id, "platform_id": 1, "name": format!("Game {id}"), "fs_name": format!("g{id}.sfc"),
               "summary": null, "updated_at": updated_at, "path_cover_small": "", "path_cover_large": "",
               "fs_size_bytes": 1})
    }

    async fn server_with(platforms: Value, ids: Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/platforms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(platforms))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/roms/identifiers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ids))
            .mount(&server)
            .await;
        server
    }

    fn page(items: Vec<Value>) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"items": items, "total": null}))
    }

    fn store() -> Mutex<Store> {
        Mutex::new(Store::open_in_memory().unwrap())
    }

    fn ids(store: &Mutex<Store>) -> Vec<i64> {
        let mut ids: Vec<_> = store
            .lock()
            .unwrap()
            .games(&GameFilter::default())
            .into_iter()
            .map(|g| g.id)
            .collect();
        ids.sort();
        ids
    }

    fn go() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn client(server: &MockServer) -> Client {
        Client::new(base_of(server, "/")).with_token("t".into())
    }

    #[tokio::test]
    async fn sync_pages_across_boundaries() {
        let server = server_with(
            json!([{"id": 1, "slug": "snes", "display_name": "SNES", "rom_count": 5}]),
            json!([1, 2, 3, 4, 5]),
        )
        .await;
        for (offset, items) in [
            (
                "0",
                vec![
                    rom_json(1, "2026-01-01T00:00:00+00:00"),
                    rom_json(2, "2026-01-03T00:00:00+00:00"),
                ],
            ),
            (
                "2",
                vec![
                    rom_json(3, "2026-01-02T00:00:00+00:00"),
                    rom_json(4, "2026-01-01T00:00:00+00:00"),
                ],
            ),
            ("4", vec![rom_json(5, "2026-01-01T00:00:00+00:00")]),
        ] {
            Mock::given(method("GET"))
                .and(path("/api/roms"))
                .and(query_param("offset", offset))
                .and(query_param_is_missing("updated_after"))
                .respond_with(page(items))
                .expect(1)
                .mount(&server)
                .await;
        }
        let store = store();
        let report = sync_library(&client(&server), &store, 2, &go())
            .await
            .unwrap();
        assert_eq!(
            report,
            SyncReport {
                updated: 5,
                removed: 0
            }
        );
        assert_eq!(ids(&store), [1, 2, 3, 4, 5]);
        let s = store.lock().unwrap();
        assert_eq!(
            s.get("last_sync_at").as_deref(),
            Some("2026-01-03T00:00:00+00:00")
        );
        assert_eq!(s.platforms(false)[0].count, 5);
    }

    #[tokio::test]
    async fn played_times_from_the_server_are_merged_page_by_page() {
        let server = MockServer::start().await;
        let played = |id: i64, when: &str| {
            let mut rom = rom_json(id, "t");
            rom["rom_user"] = json!({"last_played": when});
            rom
        };
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param("last_played", "true"))
            .and(query_param("offset", "0"))
            .respond_with(page(vec![
                played(1, "2026-09-01T00:00:00Z"),
                played(2, "2026-09-02T00:00:00Z"),
            ]))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param("last_played", "true"))
            .and(query_param("offset", "2"))
            .respond_with(page(vec![played(3, "2026-09-03T00:00:00Z")]))
            .expect(1)
            .mount(&server)
            .await;
        let store = store();
        store.lock().unwrap().upsert_games(&[
            rom(1, 1, "A", "x"),
            rom(2, 1, "B", "x"),
            rom(3, 1, "C", "x"),
        ]);
        let merged = sync_played(&client(&server), &store, 2, &go())
            .await
            .unwrap();
        assert_eq!(merged, 3);
        let s = store.lock().unwrap();
        assert_eq!(s.last_played(3), parse_iso("2026-09-03T00:00:00Z"));
        assert_eq!(s.recent_count(false), 3);
    }

    #[tokio::test]
    async fn second_sync_asks_for_changes_and_drops_deleted_games() {
        let server = server_with(
            json!([{"id": 1, "slug": "snes", "display_name": "SNES", "rom_count": 2}]),
            json!([1, 3]),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param("updated_after", "2026-01-03T00:00:00+00:00"))
            .respond_with(page(vec![rom_json(3, "2026-02-01T00:00:00+00:00")]))
            .expect(1)
            .mount(&server)
            .await;
        let store = store();
        {
            let mut s = store.lock().unwrap();
            s.set("last_sync_at", "2026-01-03T00:00:00+00:00");
            s.upsert_games(&[rom(1, 1, "A", "x"), rom(2, 1, "B", "x")]);
        }
        let report = sync_library(&client(&server), &store, 500, &go())
            .await
            .unwrap();
        assert_eq!(
            report,
            SyncReport {
                updated: 1,
                removed: 1
            }
        );
        assert_eq!(ids(&store), [1, 3]);
        assert_eq!(
            store.lock().unwrap().get("last_sync_at").as_deref(),
            Some("2026-02-01T00:00:00+00:00")
        );
    }

    #[tokio::test]
    async fn nothing_new_keeps_last_sync() {
        let server = server_with(json!([]), json!([])).await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .respond_with(page(vec![]))
            .mount(&server)
            .await;
        let store = store();
        store
            .lock()
            .unwrap()
            .set("last_sync_at", "2026-01-03T00:00:00+00:00");
        sync_library(&client(&server), &store, 500, &go())
            .await
            .unwrap();
        assert_eq!(
            store.lock().unwrap().get("last_sync_at").as_deref(),
            Some("2026-01-03T00:00:00+00:00")
        );
    }

    #[tokio::test]
    async fn unauthorized_sync_keeps_cache() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let store = store();
        store.lock().unwrap().upsert_games(&[rom(1, 1, "A", "x")]);
        assert!(matches!(
            sync_library(&client(&server), &store, 500, &go()).await,
            Err(Error::Unauthorized)
        ));
        assert_eq!(ids(&store), [1]);
    }

    #[tokio::test]
    async fn unreachable_server_leaves_cache_untouched() {
        let store = store();
        store.lock().unwrap().upsert_games(&[rom(1, 1, "A", "x")]);
        let client =
            Client::new(url::Url::parse("http://127.0.0.1:9/").unwrap()).with_token("t".into());
        assert!(matches!(
            sync_library(&client, &store, 500, &go()).await,
            Err(Error::Unreachable)
        ));
        assert_eq!(ids(&store), [1]);
    }

    #[tokio::test]
    #[ignore]
    async fn live_server_sync() {
        let Some(path) = std::env::var_os("ROMP_LIVE") else {
            return;
        };
        let cfg: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let base = crate::romm::client::server_candidates(cfg["server"].as_str().unwrap())
            .unwrap()
            .remove(0);
        let client = Client::new(base).with_token(cfg["token"].as_str().unwrap().into());
        let store = store();
        let first = sync_library(&client, &store, PAGE_SIZE, &go())
            .await
            .unwrap();
        assert!(first.updated > 0);
        let second = sync_library(&client, &store, PAGE_SIZE, &go())
            .await
            .unwrap();
        assert!(
            second.updated <= 1,
            "incremental sync refetched {} roms",
            second.updated
        );
        eprintln!(
            "live: {} games, {} platforms",
            first.updated,
            store.lock().unwrap().platforms(false).len()
        );
    }

    #[test]
    fn cursor_is_utc_iso_minus_margin() {
        let t = UNIX_EPOCH + Duration::from_secs(1_790_330_400);
        assert_eq!(cursor_from(t), "2026-09-25T09:59:00+00:00");
        assert_eq!(
            cursor_from(UNIX_EPOCH + Duration::from_secs(951_782_400 + 60)),
            "2000-02-29T00:00:00+00:00"
        );
    }

    #[tokio::test]
    async fn cursor_comes_from_server_clock_at_start() {
        let server = server_with(json!([]), json!([1])).await;
        Mock::given(method("GET"))
            .and(path("/api/heartbeat"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("date", "Fri, 25 Sep 2026 10:00:00 GMT")
                    .set_body_json(json!({"SYSTEM": {"VERSION": "5.3.1"}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .respond_with(page(vec![rom_json(1, "2026-09-25T10:30:00+00:00")]))
            .mount(&server)
            .await;
        let store = store();
        sync_library(&client(&server), &store, 500, &go())
            .await
            .unwrap();
        assert_eq!(
            store.lock().unwrap().get("last_sync_at").as_deref(),
            Some("2026-09-25T09:59:00+00:00")
        );
    }

    #[tokio::test]
    async fn games_missing_after_incremental_sync_trigger_a_full_pass() {
        let server = server_with(json!([]), json!([1, 2])).await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param("updated_after", "2026-01-03T00:00:00+00:00"))
            .respond_with(page(vec![]))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param_is_missing("updated_after"))
            .respond_with(page(vec![
                rom_json(1, "2026-01-01T00:00:00+00:00"),
                rom_json(2, "2026-01-01T00:00:00+00:00"),
            ]))
            .expect(1)
            .mount(&server)
            .await;
        let store = store();
        {
            let mut s = store.lock().unwrap();
            s.set("last_sync_at", "2026-01-03T00:00:00+00:00");
            s.upsert_games(&[rom(1, 1, "A", "x")]);
        }
        sync_library(&client(&server), &store, 500, &go())
            .await
            .unwrap();
        assert_eq!(ids(&store), [1, 2]);
    }

    #[tokio::test]
    async fn cancelled_sync_writes_nothing() {
        let server = server_with(
            json!([{"id": 1, "slug": "snes", "display_name": "SNES", "rom_count": 1}]),
            json!([1]),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .respond_with(page(vec![rom_json(1, "2026-01-01T00:00:00+00:00")]))
            .mount(&server)
            .await;
        let store = store();
        let result = sync_library(&client(&server), &store, 500, &AtomicBool::new(true)).await;
        assert!(matches!(result, Err(Error::Cancelled)));
        let s = store.lock().unwrap();
        assert_eq!(s.game_count(), 0);
        assert!(s.platforms(false).is_empty());
        assert_eq!(s.get("last_sync_at"), None);
    }
}
