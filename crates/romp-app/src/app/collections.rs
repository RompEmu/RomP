use super::{on_ui, Controller};
use crate::collections::record;
use crate::romm::client::Error;
use crate::romm::types::RemoteCollection;
use crate::store::{CollectionItem, CollectionKind, PlatformItem, Scope};
use crate::{Membership, SidebarEntry};
use slint::{Image, ModelRc, VecModel};
use tr::tr;

const AUTO_SECTIONS: [(&str, &str, CollectionKind); 3] = [
    (
        "series",
        crate::gettext_noop!("Series"),
        CollectionKind::Series,
    ),
    (
        "franchises",
        crate::gettext_noop!("Franchises"),
        CollectionKind::Franchise,
    ),
    (
        "genres",
        crate::gettext_noop!("Genres"),
        CollectionKind::Genre,
    ),
];

const PAIR_KEY: &str = "pair";
pub(super) const RECENT_KEY: &str = "recent";
pub(super) const FOR_YOU_KEY: &str = "for-you";
pub(super) const NOW_PLAYING_KEY: &str = "now-playing";
pub(super) const BACKLOG_KEY: &str = "backlog";

pub(super) enum DialogAction {
    Create { add_game: Option<i64> },
    Rename(String),
    Delete(String),
    DeleteState(u8),
}

/// RomM 5.4 answers a name that's taken with 409 and one over 400 characters with 422.
fn collection_error(e: &Error) -> String {
    match e {
        Error::Conflict => tr!("a collection with that name already exists"),
        Error::Status(422) => tr!("that name is too long"),
        e => e.to_string(),
    }
}

/// Whether something narrows the library: a game search, a console filter, or a sidebar pick
/// other than All games. Downloaded only isn't counted: it's a mode, and offline turns it on.
pub(super) fn filters_active(search: &str, platform_filter: &str, selected: &str) -> bool {
    !search.trim().is_empty() || !platform_filter.trim().is_empty() || selected != "all"
}

fn initial_of(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

/// Lower case without accents, so "poke" finds "Pokémon".
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à'..='å' => 'a',
            'ç' => 'c',
            'è'..='ë' => 'e',
            'ì'..='ï' => 'i',
            'ñ' => 'n',
            'ò'..='ö' | 'ø' => 'o',
            'ù'..='ü' => 'u',
            'ý' | 'ÿ' => 'y',
            c => c,
        })
        .collect()
}

/// The consoles whose name, RomM slug or another name RomM knows holds the filter.
pub(super) fn filter_platforms<'a>(
    platforms: &'a [PlatformItem],
    filter: &str,
) -> Vec<&'a PlatformItem> {
    let filter = fold(filter.trim());
    platforms
        .iter()
        .filter(|p| {
            filter.is_empty()
                || std::iter::once(&p.name)
                    .chain([&p.slug])
                    .chain(&p.aliases)
                    .any(|name| fold(name).contains(&filter))
        })
        .collect()
}

fn item(key: &str, name: &str, count: i64, glyph: &str) -> SidebarEntry {
    SidebarEntry {
        header: false,
        key: key.into(),
        name: name.into(),
        initial: initial_of(name).into(),
        count: count as i32,
        icon: Image::default(),
        has_icon: false,
        glyph: glyph.into(),
        expanded: false,
        can_add: false,
        editable: false,
        note: false,
    }
}

fn header(section: &str, label: &str, expanded: bool, can_add: bool) -> SidebarEntry {
    SidebarEntry {
        header: true,
        key: format!("section:{section}").into(),
        expanded,
        can_add,
        ..item("", &label.to_uppercase(), 0, "")
    }
}

pub(super) fn heading_for(key: &str, entries: &[(String, String)]) -> String {
    match key {
        "all" => tr!("All games"),
        "favorites" | PAIR_KEY => tr!("Favorites"),
        RECENT_KEY => tr!("Recently played"),
        FOR_YOU_KEY => tr!("For you"),
        NOW_PLAYING_KEY => tr!("Now playing"),
        BACKLOG_KEY => tr!("Backlog"),
        _ => entries
            .iter()
            .find(|(k, _)| k == key)
            .map_or_else(|| tr!("Games"), |(_, name)| name.clone()),
    }
}

pub(super) fn games_label(count: usize) -> String {
    let digits = count.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    match count {
        0 => tr!("No games"),
        _ => tr!("{0} game" | "{0} games" % count, grouped),
    }
}

pub(super) fn scope_for(key: &str) -> Scope {
    match key {
        "all" => Scope::All,
        RECENT_KEY => Scope::Recent,
        FOR_YOU_KEY => Scope::ForYou,
        NOW_PLAYING_KEY => Scope::NowPlaying,
        BACKLOG_KEY => Scope::Backlog,
        _ => match key.strip_prefix("p:").and_then(|id| id.parse().ok()) {
            Some(id) => Scope::Platform(id),
            None => Scope::Collection(key.to_string()),
        },
    }
}

fn recent_icon() -> Image {
    Image::load_from_svg_data(include_bytes!("../../ui/icons/recent.svg")).unwrap_or_default()
}

fn now_playing_icon() -> Image {
    Image::load_from_svg_data(include_bytes!("../../ui/icons/gamepad.svg")).unwrap_or_default()
}

fn backlog_icon() -> Image {
    Image::load_from_svg_data(include_bytes!("../../ui/icons/backlog.svg")).unwrap_or_default()
}

fn for_you_icon() -> Image {
    Image::load_from_svg_data(include_bytes!("../../ui/icons/for-you.svg")).unwrap_or_default()
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

    pub(super) fn refresh_recommendations(&self) {
        let Some(client) = self.client.borrow().clone().filter(|_| !self.offline.get()) else {
            return;
        };
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            match client.recommendations(24).await {
                Ok(picks) => {
                    store.lock().unwrap().replace_recommendations(&picks);
                    on_ui(|c| {
                        c.reload_sidebar();
                        if *c.selected.borrow() == FOR_YOU_KEY {
                            c.reload_games();
                        }
                    });
                }
                Err(e) => tracing::debug!("recommendations: {e}"),
            }
        });
    }

    fn can_edit_collections(&self) -> bool {
        self.has_scope("collections.write") && !self.offline.get() && self.client.borrow().is_some()
    }

    pub(super) fn reload_sidebar(&self) {
        let Some(ui) = self.ui() else { return };
        let downloaded_only = self.library.borrow().filter.downloaded_only;
        let show_collections = self.has_scope("collections.read");
        let can_edit = self.can_edit_collections();
        let (platforms, collections, recent, for_you, (now_playing, backlog)) = {
            let store = self.shared.store.lock().unwrap();
            let collections = if show_collections {
                store.collections(downloaded_only)
            } else {
                Vec::new()
            };
            (
                store.platforms(downloaded_only),
                collections,
                store.recent_count(downloaded_only),
                store.recommended_count(downloaded_only),
                store.status_counts(downloaded_only),
            )
        };
        let expanded = self.expanded.borrow().clone();
        let total: i64 = platforms.iter().map(|p| p.count).sum();
        let mut entries = vec![item("all", &tr!("All games"), total, "")];
        let mut keys = vec!["all".to_string()];
        if show_collections {
            let favorites = collections
                .iter()
                .find(|c| c.kind == CollectionKind::Favorites);
            let key = favorites.map_or("favorites", |f| f.key.as_str());
            entries.push(item(
                key,
                &tr!("Favorites"),
                favorites.map_or(0, |f| f.count),
                "♥",
            ));
            keys.push(key.to_string());
        } else if self.client.borrow().is_some() {
            entries.push(item(PAIR_KEY, &tr!("Favorites"), -1, "♥"));
        }
        if recent > 0 {
            entries.push(SidebarEntry {
                has_icon: true,
                icon: recent_icon(),
                ..item(RECENT_KEY, &tr!("Recently played"), recent, "")
            });
            keys.push(RECENT_KEY.to_string());
        }
        if now_playing > 0 {
            entries.push(SidebarEntry {
                has_icon: true,
                icon: now_playing_icon(),
                ..item(NOW_PLAYING_KEY, &tr!("Now playing"), now_playing, "")
            });
            keys.push(NOW_PLAYING_KEY.to_string());
        }
        if backlog > 0 {
            entries.push(SidebarEntry {
                has_icon: true,
                icon: backlog_icon(),
                ..item(BACKLOG_KEY, &tr!("Backlog"), backlog, "")
            });
            keys.push(BACKLOG_KEY.to_string());
        }
        if for_you > 0 {
            entries.push(SidebarEntry {
                has_icon: true,
                icon: for_you_icon(),
                ..item(FOR_YOU_KEY, &tr!("For you"), for_you, "")
            });
            keys.push(FOR_YOU_KEY.to_string());
        }

        let filter = self.platform_filter.borrow().clone();
        let filtering = !filter.trim().is_empty();
        // A filter shows its matches even in a collapsed section. Every console keeps its key,
        // so hiding the selected one doesn't change what the library shows.
        let open = expanded.contains("platforms") || filtering;
        entries.push(header("platforms", &tr!("Platforms"), open, false));
        let mut names: Vec<(String, String)> = Vec::new();
        for p in &platforms {
            let key = format!("p:{}", p.id);
            keys.push(key.clone());
            names.push((key, p.name.clone()));
        }
        let shown = if open {
            filter_platforms(&platforms, &filter)
        } else {
            Vec::new()
        };
        if filtering && shown.is_empty() {
            entries.push(SidebarEntry {
                note: true,
                ..item("", &tr!("No consoles match"), -1, "")
            });
        }
        let mut missing_icons = Vec::new();
        for p in shown {
            let key = format!("p:{}", p.id);
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
            entries.push(header("collections", &tr!("Collections"), open, can_edit));
            for c in listed {
                keys.push(c.key.clone());
                names.push((c.key.clone(), c.name.clone()));
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
            entries.push(header("collections", &tr!("Collections"), true, false));
            entries.push(item(PAIR_KEY, &tr!("Pair again to show them"), -1, ""));
        }
        for (section, label, kind) in AUTO_SECTIONS {
            let members: Vec<&CollectionItem> =
                collections.iter().filter(|c| c.kind == kind).collect();
            if members.is_empty() {
                continue;
            }
            let open = expanded.contains(section);
            entries.push(header(section, &crate::i18n::translate(label), open, false));
            for c in members {
                keys.push(c.key.clone());
                names.push((c.key.clone(), c.name.clone()));
                if open {
                    entries.push(item(&c.key, &c.name, c.count, ""));
                }
            }
        }
        ui.set_sidebar(ModelRc::new(VecModel::from(entries)));
        *self.sidebar_names.borrow_mut() = names;
        self.update_heading();
        self.fetch_icons(missing_icons);
        let selected = self.selected.borrow().clone();
        if !keys.contains(&selected) && selected != "favorites" {
            self.select("all".into());
        }
    }

    /// Named from every console and collection, as the sidebar may be hiding the selected one.
    pub(super) fn update_heading(&self) {
        let Some(ui) = self.ui() else { return };
        let names = self.sidebar_names.borrow();
        ui.set_heading(heading_for(&self.selected.borrow(), &names).into());
        ui.set_filters_active(filters_active(
            &self.library.borrow().filter.search,
            &self.platform_filter.borrow(),
            &self.selected.borrow(),
        ));
        ui.set_heading_detail(games_label(self.library.borrow().games.len()).into());
    }

    pub(super) fn select(&self, key: String) {
        if key == PAIR_KEY {
            return self.pair_again();
        }
        if key.is_empty() {
            return;
        }
        self.library.borrow_mut().filter.scope = scope_for(&key);
        if let Some(ui) = self.ui() {
            ui.set_selected_key(key.clone().into());
        }
        *self.selected.borrow_mut() = key;
        self.reload_games();
    }

    /// Back to everything: no search, no console filter, All games.
    pub(super) fn clear_filters(&self) {
        if let Some(ui) = self.ui() {
            ui.set_search("".into());
            ui.set_platform_filter("".into());
        }
        self.library.borrow_mut().filter.search.clear();
        self.platform_filter.borrow_mut().clear();
        self.reload_sidebar();
        self.select("all".into());
    }

    pub(super) fn set_platform_filter(&self, text: String) {
        *self.platform_filter.borrow_mut() = text;
        self.reload_sidebar();
    }

    pub(super) fn toggle_section(&self, key: String) {
        let section = key.trim_start_matches("section:").to_string();
        // The filter holds the section open, so collapsing it lets the filter go too.
        if section == "platforms" && !self.platform_filter.borrow().trim().is_empty() {
            self.platform_filter.borrow_mut().clear();
            self.expanded.borrow_mut().remove(&section);
            if let Some(ui) = self.ui() {
                ui.set_platform_filter("".into());
            }
            return self.reload_sidebar();
        }
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
                    c.report(tr!("Couldn't update \"{0}\": {1}.", item.name, e));
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
            Err(e) => {
                let e = collection_error(&e);
                return self.report(tr!("Couldn't create the collection: {e}.", e));
            }
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

    pub(super) fn open_dialog(&self, action: DialogAction) {
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
                tr!("New collection"),
                String::new(),
                true,
                String::new(),
                tr!("Create"),
                false,
            ),
            DialogAction::Rename(key) => (
                tr!("Rename collection"),
                String::new(),
                true,
                name_of(key),
                tr!("Rename"),
                false,
            ),
            DialogAction::Delete(key) => (
                tr!("Delete \"{}\"?", name_of(key)),
                tr!("The collection is removed from your RomM server. Its games stay in your library."),
                false,
                String::new(),
                tr!("Delete"),
                true,
            ),
            DialogAction::DeleteState(slot) => (
                tr!("Delete \"{}\"?", crate::slots::label(*slot)),
                tr!("The save state is removed from this computer and from your RomM server. A backup stays in the game's save folder."),
                false,
                String::new(),
                tr!("Delete"),
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
                self.create_collection(name, false, add_game);
            }
            DialogAction::Rename(key) if !name.is_empty() => self.rename(key, name),
            DialogAction::Delete(key) => self.delete(key),
            DialogAction::DeleteState(slot) => self.delete_state(slot),
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
                    c.report(tr!(
                        "Couldn't rename \"{0}\": {1}.",
                        item.name,
                        collection_error(&e)
                    ));
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
                Err(e) => c.report(tr!("Couldn't delete \"{0}\": {1}.", item.name, e)),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_name_the_selection_and_count_its_games() {
        let entries = vec![
            ("p:3".to_string(), "Super Nintendo".to_string()),
            ("c:1".to_string(), "Favorites".to_string()),
        ];
        assert_eq!(heading_for("all", &entries), "All games");
        assert_eq!(heading_for("p:3", &entries), "Super Nintendo");
        assert_eq!(heading_for("favorites", &entries), "Favorites");
        assert_eq!(heading_for(RECENT_KEY, &entries), "Recently played");
        assert_eq!(heading_for(FOR_YOU_KEY, &entries), "For you");
        assert_eq!(heading_for(NOW_PLAYING_KEY, &entries), "Now playing");
        assert_eq!(heading_for(BACKLOG_KEY, &entries), "Backlog");
        assert_eq!(heading_for("v:gone", &entries), "Games");
        assert_eq!(games_label(0), "No games");
        assert_eq!(games_label(1), "1 game");
        assert_eq!(games_label(1234), "1,234 games");
    }

    #[test]
    fn compact_sidebar_shows_the_first_letter_of_a_name() {
        assert_eq!(initial_of("Super Nintendo"), "S");
        assert_eq!(initial_of("  émulateurs"), "É");
        assert_eq!(initial_of("3DO"), "3");
        assert_eq!(initial_of("\"Couch\" co-op"), "C");
        assert_eq!(initial_of(""), "");
    }

    fn console(id: i64, slug: &str, name: &str) -> PlatformItem {
        PlatformItem {
            id,
            slug: slug.into(),
            name: name.into(),
            count: 1,
            aliases: Vec::new(),
        }
    }

    #[test]
    fn consoles_filter_by_name_or_slug_ignoring_case_and_accents() {
        let consoles = [
            console(1, "snes", "Super Nintendo Entertainment System"),
            console(2, "pokemon-mini", "Pokémon Mini"),
            console(3, "genesis", "Sega Mega Drive/Genesis"),
            console(4, "n64", "Nintendo 64"),
        ];
        let names = |filter: &str| -> Vec<i64> {
            filter_platforms(&consoles, filter)
                .iter()
                .map(|p| p.id)
                .collect()
        };
        assert_eq!(names(""), [1, 2, 3, 4]);
        assert_eq!(names("   "), [1, 2, 3, 4]);
        assert_eq!(names("SNES"), [1]);
        assert_eq!(names("nintendo"), [1, 4]);
        assert_eq!(names("pokemon"), [2]);
        assert_eq!(names("POKÉ"), [2]);
        assert_eq!(names(" mega "), [3]);
        assert!(names("xbox").is_empty());
    }

    #[test]
    fn consoles_also_match_romms_short_and_other_names() {
        let consoles = [
            PlatformItem {
                aliases: vec!["MD".into(), "Mega Drive".into()],
                ..console(1, "genesis", "Sega Genesis")
            },
            console(2, "snes", "Super Nintendo Entertainment System"),
        ];
        let names = |filter: &str| -> Vec<i64> {
            filter_platforms(&consoles, filter)
                .iter()
                .map(|p| p.id)
                .collect()
        };
        assert_eq!(names("mega drive"), [1]);
        assert_eq!(names("md"), [1]);
    }

    #[test]
    fn the_heading_names_a_console_the_sidebar_is_not_showing() {
        let names = vec![("p:3".to_string(), "Super Nintendo".to_string())];
        assert_eq!(heading_for("p:3", &names), "Super Nintendo");
    }

    #[test]
    fn collection_name_errors_say_what_was_wrong() {
        assert_eq!(
            collection_error(&Error::Conflict),
            "a collection with that name already exists"
        );
        assert_eq!(
            collection_error(&Error::Status(422)),
            "that name is too long"
        );
        assert_eq!(
            collection_error(&Error::Unreachable),
            Error::Unreachable.to_string()
        );
    }

    #[test]
    fn the_view_is_narrowed_by_a_search_a_console_filter_or_a_pick() {
        assert!(!filters_active("", "", "all"));
        assert!(
            !filters_active("  ", " ", "all"),
            "blank text doesn't count"
        );
        assert!(filters_active("zelda", "", "all"));
        assert!(filters_active("", "nintendo", "all"));
        assert!(filters_active("", "", "p:3"));
        assert!(filters_active("", "", RECENT_KEY));
    }

    #[test]
    fn sidebar_keys_map_to_library_scopes() {
        assert_eq!(scope_for("all"), Scope::All);
        assert_eq!(scope_for(RECENT_KEY), Scope::Recent);
        assert_eq!(scope_for(FOR_YOU_KEY), Scope::ForYou);
        assert_eq!(scope_for(NOW_PLAYING_KEY), Scope::NowPlaying);
        assert_eq!(scope_for(BACKLOG_KEY), Scope::Backlog);
        assert_eq!(scope_for("p:12"), Scope::Platform(12));
        assert_eq!(scope_for("c:3"), Scope::Collection("c:3".into()));
        assert_eq!(scope_for("v:eyJu"), Scope::Collection("v:eyJu".into()));
        assert_eq!(
            scope_for("favorites"),
            Scope::Collection("favorites".into())
        );
    }
}
