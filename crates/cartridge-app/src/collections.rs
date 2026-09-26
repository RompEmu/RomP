use crate::romm::client::{Client, Error};
use crate::romm::types::RemoteCollection;
use crate::store::{CollectionKind, CollectionRecord, Store};
use std::sync::Mutex;

pub const AUTO: [(&str, CollectionKind); 3] = [
    ("collection", CollectionKind::Series),
    ("franchise", CollectionKind::Franchise),
    ("genre", CollectionKind::Genre),
];

pub fn record(c: RemoteCollection, kind: CollectionKind) -> CollectionRecord {
    let prefix = match kind {
        CollectionKind::Smart => "s",
        CollectionKind::Series | CollectionKind::Franchise | CollectionKind::Genre => "v",
        _ => "c",
    };
    CollectionRecord {
        key: format!("{prefix}:{}", c.id),
        kind,
        remote_id: c.id.to_string(),
        name: c.name,
        owner: c.owner_username,
        rom_ids: c.rom_ids,
    }
}

pub fn records(
    user_id: Option<i64>,
    regular: Vec<RemoteCollection>,
    smart: Vec<RemoteCollection>,
    auto: Vec<(CollectionKind, Vec<RemoteCollection>)>,
) -> Vec<CollectionRecord> {
    let mut have_favorites = false;
    let mut out: Vec<CollectionRecord> = regular
        .into_iter()
        .map(|c| {
            let owned = user_id.is_some() && c.user_id == user_id;
            let kind = if owned && c.is_favorite && !have_favorites {
                have_favorites = true;
                CollectionKind::Favorites
            } else if owned {
                CollectionKind::Mine
            } else {
                CollectionKind::Shared
            };
            record(c, kind)
        })
        .collect();
    out.extend(smart.into_iter().map(|c| record(c, CollectionKind::Smart)));
    for (kind, list) in auto {
        out.extend(list.into_iter().map(|c| record(c, kind)));
    }
    out
}

pub async fn sync_collections(client: &Client, store: &Mutex<Store>) -> Result<(), Error> {
    let user_id = client.me().await?.id;
    let regular = client.collections().await?;
    let smart = client.smart_collections().await?;
    let mut auto = Vec::new();
    for (kind_name, kind) in AUTO {
        auto.push((kind, client.virtual_collections(kind_name).await?));
    }
    let records = records(user_id, regular, smart, auto);
    store.lock().unwrap().replace_collections(&records);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::client::tests::base_of;
    use crate::romm::types::CollectionId;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn remote(id: i64, name: &str, user: i64, favorite: bool) -> RemoteCollection {
        RemoteCollection {
            id: CollectionId::Number(id),
            name: name.into(),
            rom_ids: vec![id * 10],
            is_favorite: favorite,
            is_smart: false,
            user_id: Some(user),
            owner_username: Some(format!("user{user}")),
            kind: None,
        }
    }

    #[test]
    fn ownership_decides_favorites_mine_and_shared() {
        let out = records(
            Some(1),
            vec![
                remote(1, "Favourites", 1, true),
                remote(2, "Their favourites", 2, true),
                remote(3, "Couch", 1, false),
                remote(4, "Public", 2, false),
            ],
            vec![remote(5, "Unplayed", 1, false)],
            vec![(
                CollectionKind::Genre,
                vec![RemoteCollection {
                    id: CollectionId::Text("Zz==".into()),
                    kind: Some("genre".into()),
                    ..remote(0, "Shooter", 0, false)
                }],
            )],
        );
        let summary: Vec<_> = out.iter().map(|r| (r.key.as_str(), r.kind)).collect();
        assert_eq!(
            summary,
            [
                ("c:1", CollectionKind::Favorites),
                ("c:2", CollectionKind::Shared),
                ("c:3", CollectionKind::Mine),
                ("c:4", CollectionKind::Shared),
                ("s:5", CollectionKind::Smart),
                ("v:Zz==", CollectionKind::Genre),
            ]
        );
        assert_eq!(out[5].remote_id, "Zz==");
    }

    #[test]
    fn unknown_user_owns_nothing() {
        let out = records(None, vec![remote(1, "Favourites", 1, true)], vec![], vec![]);
        assert_eq!(out[0].kind, CollectionKind::Shared);
    }

    #[tokio::test]
    async fn sync_stores_every_collection_kind() {
        let server = MockServer::start().await;
        let json = |body: serde_json::Value| ResponseTemplate::new(200).set_body_json(body);
        Mock::given(method("GET"))
            .and(path("/api/users/me"))
            .respond_with(json(serde_json::json!({"id": 1, "username": "me"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/collections"))
            .respond_with(json(serde_json::json!([
                {"id": 1, "name": "Favourites", "rom_ids": [7], "is_favorite": true, "user_id": 1}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/collections/smart"))
            .respond_with(json(serde_json::json!([])))
            .mount(&server)
            .await;
        for kind in ["collection", "franchise", "genre"] {
            Mock::given(method("GET"))
                .and(path("/api/collections/virtual"))
                .and(query_param("type", kind))
                .respond_with(json(serde_json::json!([
                    {"id": format!("{kind}=="), "name": kind, "type": kind, "rom_ids": [7]}
                ])))
                .mount(&server)
                .await;
        }
        let store = Mutex::new(Store::open_in_memory().unwrap());
        store
            .lock()
            .unwrap()
            .upsert_games(&[crate::romm::types::rom(7, 1, "Game", "t")]);
        let client = Client::new(base_of(&server, "/")).with_token("t".into());
        sync_collections(&client, &store).await.unwrap();
        let kinds: Vec<_> = store
            .lock()
            .unwrap()
            .collections(false)
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                CollectionKind::Favorites,
                CollectionKind::Series,
                CollectionKind::Franchise,
                CollectionKind::Genre
            ]
        );
    }
}
