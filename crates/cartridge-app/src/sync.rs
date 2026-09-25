use crate::romm::client::{Client, Error};
use crate::store::Store;
use std::sync::Mutex;

pub const PAGE_SIZE: i64 = 500;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub updated: usize,
    pub removed: usize,
}

pub async fn sync_library(
    client: &Client,
    store: &Mutex<Store>,
    page_size: i64,
) -> Result<SyncReport, Error> {
    let platforms = client.platforms().await?;
    store.lock().unwrap().replace_platforms(&platforms);

    let since = store.lock().unwrap().get("last_sync_at");
    let mut newest: Option<String> = None;
    let mut report = SyncReport::default();
    let mut offset = 0;
    loop {
        let page = client
            .roms_page(offset, page_size, since.as_deref())
            .await?;
        let count = page.items.len();
        for rom in &page.items {
            if newest
                .as_deref()
                .is_none_or(|n| rom.updated_at.as_str() > n)
            {
                newest = Some(rom.updated_at.clone());
            }
        }
        store.lock().unwrap().upsert_games(&page.items);
        report.updated += count;
        offset += count as i64;
        if (count as i64) < page_size {
            break;
        }
    }

    let ids = client.rom_ids().await?;
    report.removed = store.lock().unwrap().retain_games(&ids);
    if let Some(newest) = newest {
        store.lock().unwrap().set("last_sync_at", &newest);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let report = sync_library(&client(&server), &store, 2).await.unwrap();
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
        assert_eq!(s.platforms()[0].count, 5);
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
        let report = sync_library(&client(&server), &store, 500).await.unwrap();
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
        sync_library(&client(&server), &store, 500).await.unwrap();
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
            sync_library(&client(&server), &store, 500).await,
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
            sync_library(&client, &store, 500).await,
            Err(Error::Unreachable)
        ));
        assert_eq!(ids(&store), [1]);
    }

    #[tokio::test]
    #[ignore]
    async fn live_server_sync() {
        let Some(path) = std::env::var_os("CARTRIDGE_LIVE") else {
            return;
        };
        let cfg: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let base = crate::romm::client::server_candidates(cfg["server"].as_str().unwrap())
            .unwrap()
            .remove(0);
        let client = Client::new(base).with_token(cfg["token"].as_str().unwrap().into());
        let store = store();
        let first = sync_library(&client, &store, PAGE_SIZE).await.unwrap();
        assert!(first.updated > 0);
        let second = sync_library(&client, &store, PAGE_SIZE).await.unwrap();
        assert!(
            second.updated <= 1,
            "incremental sync refetched {} roms",
            second.updated
        );
        eprintln!(
            "live: {} games, {} platforms",
            first.updated,
            store.lock().unwrap().platforms().len()
        );
    }
}
