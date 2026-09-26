use super::{on_ui, Controller};
use crate::collections::record;
use crate::romm::client::Error;
use crate::romm::types::RemoteCollection;
use crate::store::{CollectionItem, CollectionKind, Scope};
use crate::{Membership, SidebarEntry};
use slint::{Image, ModelRc, VecModel};

const AUTO_SECTIONS: [(&str, &str, CollectionKind); 3] = [
    ("series", "SERIES", CollectionKind::Series),
    ("franchises", "FRANCHISES", CollectionKind::Franchise),
    ("genres", "GENRES", CollectionKind::Genre),
];

const PAIR_KEY: &str = "pair";

pub(super) enum DialogAction {
    Create { add_game: Option<i64> },
    Rename(String),
    Delete(String),
}

fn item(key: &str, name: &str, count: i64, glyph: &str) -> SidebarEntry {
    SidebarEntry {
        header: false,
        key: key.into(),
        name: name.into(),
        count: count as i32,
        icon: Image::default(),
        has_icon: false,
        glyph: glyph.into(),
        expanded: false,
        can_add: false,
        editable: false,
    }
}

fn header(section: &str, label: &str, expanded: bool, can_add: bool) -> SidebarEntry {
    SidebarEntry {
        header: true,
        key: format!("section:{section}").into(),
        expanded,
        can_add,
        ..item("", label, 0, "")
    }
}

pub(super) fn scope_for(key: &str) -> Scope {
    match key {
        "all" => Scope::All,
        _ => match key.strip_prefix("p:").and_then(|id| id.parse().ok()) {
            Some(id) => Scope::Platform(id),
            None => Scope::Collection(key.to_string()),
        },
    }
}

fn numeric_id(item: &CollectionItem) -> Option<i64> {
    item.remote_id.parse().ok()
}

impl Controller {
    pub(super) fn has_scope(&self, scope: &str) -> bool {
        self.shared
            .store
            .lock()
            .unwrap()
            .get("scopes")
            .is_some_and(|s| s.split(' ').any(|s| s == scope))
    }

    fn can_edit_collections(&self) -> bool {
        self.has_scope("collections.write") && !self.offline.get() && self.client.borrow().is_some()
    }

    pub(super) fn reload_sidebar(&self) {
        let Some(ui) = self.ui() else { return };
        let downloaded_only = self.library.borrow().filter.downloaded_only;
        let show_collections = self.has_scope("collections.read");
        let can_edit = self.can_edit_collections();
        let (platforms, collections) = {
            let store = self.shared.store.lock().unwrap();
            let collections = if show_collections {
                store.collections(downloaded_only)
            } else {
                Vec::new()
            };
            (store.platforms(downloaded_only), collections)
        };
        let expanded = self.expanded.borrow().clone();
        let total: i64 = platforms.iter().map(|p| p.count).sum();
        let mut entries = vec![item("all", "All games", total, "")];
        let mut keys = vec!["all".to_string()];
        if show_collections {
            let favorites = collections
                .iter()
                .find(|c| c.kind == CollectionKind::Favorites);
            let key = favorites.map_or("favorites", |f| f.key.as_str());
            entries.push(item(
                key,
                "Favorites",
                favorites.map_or(0, |f| f.count),
                "♥",
            ));
            keys.push(key.to_string());
        } else if self.client.borrow().is_some() {
            entries.push(item(PAIR_KEY, "Favorites", -1, "♥"));
        }

        let open = expanded.contains("platforms");
        entries.push(header("platforms", "PLATFORMS", open, false));
        let mut missing_icons = Vec::new();
        for p in platforms {
            let key = format!("p:{}", p.id);
            keys.push(key.clone());
            if !open {
                continue;
            }
            let image = self
                .shared
                .covers
                .cached_icon(&p.slug)
                .and_then(|icon| Image::load_from_path(&icon).ok());
            if image.is_none() && self.icon_requests.borrow_mut().insert(p.slug.clone()) {
                missing_icons.push(p.slug.clone());
            }
            entries.push(SidebarEntry {
                has_icon: image.is_some(),
                icon: image.unwrap_or_default(),
                ..item(&key, &p.name, p.count, "")
            });
        }

        let listed: Vec<&CollectionItem> = collections
            .iter()
            .filter(|c| {
                matches!(
                    c.kind,
                    CollectionKind::Mine | CollectionKind::Shared | CollectionKind::Smart
                )
            })
            .collect();
        if show_collections && (can_edit || !listed.is_empty()) {
            let open = expanded.contains("collections");
            entries.push(header("collections", "COLLECTIONS", open, can_edit));
            for c in listed {
                keys.push(c.key.clone());
                if !open {
                    continue;
                }
                let name = match (&c.kind, &c.owner) {
                    (CollectionKind::Shared, Some(owner)) => format!("{} · {owner}", c.name),
                    _ => c.name.clone(),
                };
                entries.push(SidebarEntry {
                    editable: c.kind == CollectionKind::Mine && can_edit,
                    ..item(&c.key, &name, c.count, "")
                });
            }
        }
        if !show_collections && self.client.borrow().is_some() {
            entries.push(header("collections", "COLLECTIONS", true, false));
            entries.push(item(PAIR_KEY, "Pair again to show them", -1, ""));
        }
        for (section, label, kind) in AUTO_SECTIONS {
            let members: Vec<&CollectionItem> =
                collections.iter().filter(|c| c.kind == kind).collect();
            if members.is_empty() {
                continue;
            }
            let open = expanded.contains(section);
            entries.push(header(section, label, open, false));
            for c in members {
                keys.push(c.key.clone());
                if open {
                    entries.push(item(&c.key, &c.name, c.count, ""));
                }
            }
        }
        ui.set_sidebar(ModelRc::new(VecModel::from(entries)));
        self.fetch_icons(missing_icons);
        let selected = self.selected.borrow().clone();
        if !keys.contains(&selected) && selected != "favorites" {
            self.select("all".into());
        }
    }

    pub(super) fn select(&self, key: String) {
        if key == PAIR_KEY {
            return self.pair_again();
        }
        self.library.borrow_mut().filter.scope = scope_for(&key);
        if let Some(ui) = self.ui() {
            ui.set_selected_key(key.clone().into());
        }
        *self.selected.borrow_mut() = key;
        self.reload_games();
    }

    pub(super) fn toggle_section(&self, key: String) {
        let section = key.trim_start_matches("section:").to_string();
        {
            let mut expanded = self.expanded.borrow_mut();
            if !expanded.remove(&section) {
                expanded.insert(section);
            }
        }
        self.reload_sidebar();
    }

    pub(super) fn refresh_collection_controls(&self, rom_id: i64) {
        let Some(ui) = self.ui() else { return };
        let show = self.has_scope("collections.read");
        ui.set_show_collections(show);
        ui.set_can_edit_collections(self.can_edit_collections());
        let (collections, member_of) = {
            let store = self.shared.store.lock().unwrap();
            (store.collections(false), store.memberships(rom_id))
        };
        ui.set_game_favorite(
            collections
                .iter()
                .any(|c| c.kind == CollectionKind::Favorites && member_of.contains(&c.key)),
        );
        let memberships: Vec<Membership> = collections
            .iter()
            .filter(|c| c.kind == CollectionKind::Mine)
            .map(|c| Membership {
                key: c.key.clone().into(),
                name: c.name.clone().into(),
                member: member_of.contains(&c.key),
            })
            .collect();
        ui.set_game_memberships(ModelRc::new(VecModel::from(memberships)));
    }

    fn collections_changed(&self, _key: &str) {
        self.reload_sidebar();
        self.reload_games();
        if let Some(id) = self.current_game_id() {
            self.refresh_collection_controls(id);
        }
    }

    fn report(&self, text: String) {
        match self.current_game_id() {
            Some(id) => self.game_status(id, text),
            None => {
                if let Some(ui) = self.ui() {
                    ui.set_sync_status(text.into());
                }
            }
        }
    }

    pub(super) fn toggle_favorite(&self) {
        let Some(rom_id) = self.current_game_id() else {
            return;
        };
        let favorites = self.shared.store.lock().unwrap().favorites();
        match favorites {
            Some(favorites) => {
                let member = !self
                    .shared
                    .store
                    .lock()
                    .unwrap()
                    .memberships(rom_id)
                    .contains(&favorites.key);
                self.set_membership(favorites, rom_id, member);
            }
            None => self.create_collection("Favorites".into(), true, Some(rom_id)),
        }
    }

    pub(super) fn toggle_membership(&self, key: String, member: bool) {
        let Some(rom_id) = self.current_game_id() else {
            return;
        };
        let item = self.shared.store.lock().unwrap().collection(&key);
        if let Some(item) = item {
            self.set_membership(item, rom_id, member);
        }
    }

    fn set_membership(&self, item: CollectionItem, rom_id: i64, member: bool) {
        let (Some(client), Some(id)) = (self.client.borrow().clone(), numeric_id(&item)) else {
            return;
        };
        if !self.can_edit_collections() {
            return;
        }
        self.shared
            .store
            .lock()
            .unwrap()
            .set_member(&item.key, rom_id, member);
        self.collections_changed(&item.key);
        self.shared.rt.spawn(async move {
            let result = client.set_collection_member(id, rom_id, member).await;
            on_ui(move |c| {
                if let Err(e) = result {
                    c.shared
                        .store
                        .lock()
                        .unwrap()
                        .set_member(&item.key, rom_id, !member);
                    c.collections_changed(&item.key);
                    c.report(format!("Couldn't update \"{}\": {e}.", item.name));
                }
            });
        });
    }

    fn create_collection(&self, name: String, favorite: bool, add_game: Option<i64>) {
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        self.shared.rt.spawn(async move {
            let result = client.create_collection(&name, favorite).await;
            on_ui(move |c| c.collection_created(result, favorite, add_game));
        });
    }

    fn collection_created(
        &self,
        result: Result<RemoteCollection, Error>,
        favorite: bool,
        add_game: Option<i64>,
    ) {
        let created = match result {
            Ok(created) => created,
            Err(e) => return self.report(format!("Couldn't create the collection: {e}.")),
        };
        let kind = if favorite {
            CollectionKind::Favorites
        } else {
            CollectionKind::Mine
        };
        let record = record(created, kind);
        let key = record.key.clone();
        self.shared.store.lock().unwrap().add_collection(&record);
        if favorite {
            self.select_if_placeholder(&key);
        }
        self.collections_changed(&key);
        let item = self.shared.store.lock().unwrap().collection(&key);
        if let (Some(item), Some(rom_id)) = (item, add_game) {
            self.set_membership(item, rom_id, true);
        }
    }

    fn select_if_placeholder(&self, key: &str) {
        if *self.selected.borrow() == "favorites" {
            self.select(key.to_string());
        }
    }

    fn open_dialog(&self, action: DialogAction) {
        let Some(ui) = self.ui() else { return };
        let name_of = |key: &str| {
            self.shared
                .store
                .lock()
                .unwrap()
                .collection(key)
                .map(|c| c.name)
                .unwrap_or_default()
        };
        let (title, message, asks_name, value, confirm, destructive) = match &action {
            DialogAction::Create { .. } => (
                "New collection".to_string(),
                String::new(),
                true,
                String::new(),
                "Create",
                false,
            ),
            DialogAction::Rename(key) => (
                "Rename collection".to_string(),
                String::new(),
                true,
                name_of(key),
                "Rename",
                false,
            ),
            DialogAction::Delete(key) => (
                format!("Delete \"{}\"?", name_of(key)),
                "The collection is removed from your RomM server. Its games stay in your library."
                    .to_string(),
                false,
                String::new(),
                "Delete",
                true,
            ),
        };
        ui.set_dialog_title(title.into());
        ui.set_dialog_message(message.into());
        ui.set_dialog_asks_name(asks_name);
        ui.set_dialog_value(value.into());
        ui.set_dialog_confirm(confirm.into());
        ui.set_dialog_destructive(destructive);
        ui.set_dialog_open(true);
        *self.dialog.borrow_mut() = Some(action);
    }

    pub(super) fn new_collection(&self, add_game: Option<i64>) {
        if self.can_edit_collections() {
            self.open_dialog(DialogAction::Create { add_game });
        }
    }

    pub(super) fn rename_collection(&self, key: String) {
        self.open_dialog(DialogAction::Rename(key));
    }

    pub(super) fn delete_collection(&self, key: String) {
        self.open_dialog(DialogAction::Delete(key));
    }

    pub(super) fn close_dialog(&self) {
        self.dialog.borrow_mut().take();
        if let Some(ui) = self.ui() {
            ui.set_dialog_open(false);
        }
    }

    pub(super) fn dialog_accepted(&self, value: String) {
        let Some(action) = self.dialog.borrow_mut().take() else {
            return;
        };
        self.close_dialog();
        let name = value.trim().to_string();
        match action {
            DialogAction::Create { add_game } if !name.is_empty() => {
                self.create_collection(name, false, add_game)
            }
            DialogAction::Rename(key) if !name.is_empty() => self.rename(key, name),
            DialogAction::Delete(key) => self.delete(key),
            _ => {}
        }
    }

    fn rename(&self, key: String, name: String) {
        let item = self.shared.store.lock().unwrap().collection(&key);
        let (Some(item), Some(client)) = (item, self.client.borrow().clone()) else {
            return;
        };
        let Some(id) = numeric_id(&item) else { return };
        self.shared
            .store
            .lock()
            .unwrap()
            .rename_collection(&key, &name);
        self.collections_changed(&key);
        self.shared.rt.spawn(async move {
            let result = client.rename_collection(id, &name).await;
            on_ui(move |c| {
                if let Err(e) = result {
                    c.shared
                        .store
                        .lock()
                        .unwrap()
                        .rename_collection(&key, &item.name);
                    c.collections_changed(&key);
                    c.report(format!("Couldn't rename \"{}\": {e}.", item.name));
                }
            });
        });
    }

    fn delete(&self, key: String) {
        let item = self.shared.store.lock().unwrap().collection(&key);
        let (Some(item), Some(client)) = (item, self.client.borrow().clone()) else {
            return;
        };
        let Some(id) = numeric_id(&item) else { return };
        self.shared.rt.spawn(async move {
            let result = client.delete_collection(id).await;
            on_ui(move |c| match result {
                Ok(()) => {
                    c.shared.store.lock().unwrap().delete_collection(&key);
                    c.collections_changed(&key);
                }
                Err(e) => c.report(format!("Couldn't delete \"{}\": {e}.", item.name)),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_keys_map_to_library_scopes() {
        assert_eq!(scope_for("all"), Scope::All);
        assert_eq!(scope_for("p:12"), Scope::Platform(12));
        assert_eq!(scope_for("c:3"), Scope::Collection("c:3".into()));
        assert_eq!(scope_for("v:eyJu"), Scope::Collection("v:eyJu".into()));
        assert_eq!(
            scope_for("favorites"),
            Scope::Collection("favorites".into())
        );
    }
}
