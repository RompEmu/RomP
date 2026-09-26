use super::save_sync::PendingLaunch;
use super::{on_ui, with_controller, Controller, SCREEN_GAME, SCREEN_LIBRARY};
use crate::bios;
use crate::cores::{core_for_platform, BUILDBOT};
use crate::details;
use crate::download::DownloadError;
use crate::fetch::download_game;
use crate::paths;
use crate::play::{self, CoreIdentity, GameOptions};
use crate::romm::client::Error;
use crate::saves::{self, SramOutcome};
use crate::store::GameDetail;
use crate::{GameCard, Shot};
use slint::{Image, Model, ModelRc, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

const SIMILAR_LIMIT: u32 = 12;

pub(super) struct GameState {
    detail: GameDetail,
    shots: Rc<VecModel<Shot>>,
    similar: Rc<VecModel<GameCard>>,
}

fn load_image(path: &Path) -> Option<Image> {
    Image::load_from_path(path).ok()
}

fn downloaded_path(detail: &GameDetail) -> Option<PathBuf> {
    detail
        .local_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

fn download_dir(detail: &GameDetail) -> Option<PathBuf> {
    let roms = paths::roms_dir().canonicalize().ok()?;
    let file = PathBuf::from(detail.local_path.as_deref()?)
        .canonicalize()
        .ok()?;
    let parts: Vec<_> = file.strip_prefix(&roms).ok()?.components().collect();
    let id = detail.id.to_string();
    let depth = (1..=2).find(|&i| parts.get(i).is_some_and(|c| c.as_os_str() == id.as_str()))?;
    Some(parts[..=depth].iter().fold(roms, |dir, c| dir.join(c)))
}

impl Controller {
    fn downloading_id(&self) -> Option<i64> {
        self.downloading.borrow().as_ref().map(|(id, _)| *id)
    }

    pub(super) fn server(&self) -> String {
        self.client
            .borrow()
            .as_ref()
            .map(|c| c.base().to_string())
            .unwrap_or_default()
    }

    pub(super) fn current_game_id(&self) -> Option<i64> {
        self.game.borrow().as_ref().map(|g| g.detail.id)
    }

    fn current_game(&self) -> Option<GameDetail> {
        self.game.borrow().as_ref().map(|g| g.detail.clone())
    }

    pub(super) fn open_game(&self, id: i64) {
        self.clear_conflict();
        let Some(detail) = self.shared.store.lock().unwrap().game(id) else {
            return;
        };
        let shots = Rc::new(VecModel::from(
            detail
                .screenshots
                .iter()
                .map(|url| {
                    let image = self
                        .shared
                        .covers
                        .cached_screenshot(id, url)
                        .and_then(|p| load_image(&p));
                    Shot {
                        loaded: image.is_some(),
                        image: image.unwrap_or_default(),
                    }
                })
                .collect::<Vec<_>>(),
        ));
        let similar = Rc::new(VecModel::default());
        *self.game.borrow_mut() = Some(GameState {
            detail: detail.clone(),
            shots: shots.clone(),
            similar: similar.clone(),
        });
        self.refresh_game_page(true);
        if let Some(ui) = self.ui() {
            ui.set_game_status("".into());
            ui.set_game_shots(ModelRc::from(shots));
            ui.set_game_similar(ModelRc::from(similar));
            ui.set_screen(SCREEN_GAME);
        }
        self.load_game_cover(&detail);
        self.load_screenshots(&detail);
        self.load_similar(id);
    }

    fn with_game<T>(&self, id: i64, f: impl FnOnce(&GameState) -> T) -> Option<T> {
        self.game
            .borrow()
            .as_ref()
            .filter(|g| g.detail.id == id)
            .map(f)
    }

    fn load_screenshots(&self, detail: &GameDetail) {
        let missing: Vec<(usize, String)> = detail
            .screenshots
            .iter()
            .enumerate()
            .filter(|(_, url)| {
                self.shared
                    .covers
                    .cached_screenshot(detail.id, url)
                    .is_none()
            })
            .map(|(i, url)| (i, url.clone()))
            .collect();
        let client = self.client.borrow().clone();
        let Some(client) = client.filter(|_| !self.offline.get() && !missing.is_empty()) else {
            return;
        };
        let covers = self.shared.covers.clone();
        let id = detail.id;
        self.shared.rt.spawn(async move {
            for (index, url) in missing {
                if let Ok(path) = covers.ensure_screenshot(&client, id, &url).await {
                    on_ui(move |c| c.show_screenshot(id, index, &path));
                }
            }
        });
    }

    fn show_screenshot(&self, id: i64, index: usize, path: &Path) {
        let Some(image) = load_image(path) else {
            return;
        };
        self.with_game(id, |g| {
            if index < g.shots.row_count() {
                g.shots.set_row_data(
                    index,
                    Shot {
                        image,
                        loaded: true,
                    },
                );
            }
        });
    }

    fn load_similar(&self, id: i64) {
        let client = self.client.borrow().clone();
        let Some(client) = client.filter(|_| !self.offline.get()) else {
            return;
        };
        self.shared.rt.spawn(async move {
            match client.similar(id, SIMILAR_LIMIT).await {
                Ok(ids) => on_ui(move |c| c.show_similar(id, ids)),
                Err(e) => tracing::warn!("similar games for {id}: {e}"),
            }
        });
    }

    fn show_similar(&self, id: i64, ids: Vec<i64>) {
        let games: Vec<GameDetail> = {
            let store = self.shared.store.lock().unwrap();
            ids.iter().filter_map(|&i| store.game(i)).collect()
        };
        let mut wanted = Vec::new();
        let cards: Vec<GameCard> = games
            .iter()
            .map(|g| {
                let image = g.cover_small.as_deref().and_then(|cover| {
                    let path = self.shared.covers.path_for(g.id, cover);
                    let image = path.exists().then(|| load_image(&path)).flatten();
                    if image.is_none() {
                        wanted.push((g.id, cover.to_string()));
                    }
                    image
                });
                GameCard {
                    id: g.id as i32,
                    title: g.title.clone().into(),
                    platform: g.platform.clone().into(),
                    has_cover: image.is_some(),
                    cover: image.unwrap_or_default(),
                    downloaded: g.local_path.is_some(),
                }
            })
            .collect();
        if self.with_game(id, |g| g.similar.set_vec(cards)).is_none() {
            return;
        }
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        let covers = self.shared.covers.clone();
        self.shared.rt.spawn(async move {
            for (game, cover) in wanted {
                if let Ok(path) = covers.ensure(&client, game, &cover).await {
                    on_ui(move |c| c.show_similar_cover(id, game, &path));
                }
            }
        });
    }

    fn show_similar_cover(&self, id: i64, game: i64, path: &Path) {
        let Some(image) = load_image(path) else {
            return;
        };
        self.with_game(id, |g| {
            let index = g.similar.iter().position(|c| i64::from(c.id) == game);
            if let Some(index) = index {
                let mut card = g.similar.row_data(index).unwrap();
                card.cover = image;
                card.has_cover = true;
                g.similar.set_row_data(index, card);
            }
        });
    }

    pub(super) fn refresh_game_page(&self, reload: bool) {
        let Some(ui) = self.ui() else { return };
        let Some(mut detail) = self.current_game() else {
            return;
        };
        if reload {
            if let Some(fresh) = self.shared.store.lock().unwrap().game(detail.id) {
                detail = fresh;
                if let Some(state) = self.game.borrow_mut().as_mut() {
                    state.detail = detail.clone();
                }
            }
        }
        let playable = core_for_platform(&detail.platform_slug).is_some();
        let busy = self.downloading_id() == Some(detail.id);
        ui.set_game_title(detail.title.clone().into());
        ui.set_game_category(
            detail
                .platform_category
                .as_deref()
                .unwrap_or_default()
                .to_uppercase()
                .into(),
        );
        ui.set_game_subtitle(details::subtitle(&detail).into());
        ui.set_game_summary(detail.summary.clone().unwrap_or_default().into());
        ui.set_game_facts(ModelRc::new(VecModel::from(
            details::facts(&detail)
                .into_iter()
                .map(|(label, value)| crate::Fact {
                    label: label.into(),
                    value: value.into(),
                })
                .collect::<Vec<_>>(),
        )));
        ui.set_game_downloaded(downloaded_path(&detail).is_some());
        ui.set_game_busy(busy);
        if busy {
            ui.set_game_progress(self.download_fraction.get());
        }
        ui.set_game_playable(playable);
        self.refresh_collection_controls(detail.id);
        ui.set_game_can_download(!self.offline.get() && self.downloading_id().is_none());
        if !playable {
            ui.set_game_status(
                format!("Cartridge can't play {} games yet.", detail.platform).into(),
            );
        } else if self.downloading_id().is_some_and(|d| d != detail.id) {
            ui.set_game_status("Another download is in progress.".into());
        } else if self.offline.get() && downloaded_path(&detail).is_none() {
            ui.set_game_status("Connect to your server to download this game.".into());
        }
    }

    fn load_game_cover(&self, detail: &GameDetail) {
        let Some(ui) = self.ui() else { return };
        ui.set_has_game_cover(false);
        let small = detail
            .cover_small
            .as_deref()
            .map(|c| self.shared.covers.path_for(detail.id, c))
            .filter(|p| p.exists());
        if let Some(image) = small.and_then(|p| Image::load_from_path(&p).ok()) {
            ui.set_game_cover(image);
            ui.set_has_game_cover(true);
        }
        let Some(cover) = detail.cover_large.clone() else {
            return;
        };
        if let Some(cached) = self.shared.covers.cached_large(detail.id, &cover) {
            self.show_game_cover(detail.id, &cached);
            return;
        }
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        let covers = self.shared.covers.clone();
        let id = detail.id;
        self.shared.rt.spawn(async move {
            if let Ok(path) = covers.ensure_large(&client, id, &cover).await {
                on_ui(move |c| c.show_game_cover(id, &path));
            }
        });
    }

    fn show_game_cover(&self, id: i64, path: &Path) {
        if self.current_game().map(|g| g.id) != Some(id) {
            return;
        }
        let (Some(ui), Ok(image)) = (self.ui(), Image::load_from_path(path)) else {
            return;
        };
        ui.set_game_cover(image);
        ui.set_has_game_cover(true);
    }

    pub(super) fn back_to_library(&self) {
        self.clear_conflict();
        self.close_settings();
        if let Some(ui) = self.ui() {
            ui.set_screen(SCREEN_LIBRARY);
        }
        self.reload_games();
    }

    pub(super) fn download_game(&self) {
        let Some(detail) = self.current_game() else {
            return;
        };
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        if self.downloading_id().is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.downloading.borrow_mut() = Some((detail.id, cancel.clone()));
        self.download_fraction.set(0.0);
        if let Some(ui) = self.ui() {
            ui.set_game_progress(0.0);
            ui.set_game_status("Downloading…".into());
        }
        self.refresh_game_page(false);
        let covers = self.shared.covers.clone();
        let id = detail.id;
        self.shared.rt.spawn(async move {
            let last = AtomicU64::new(u64::MAX);
            let progress = |done: u64, total: u64| {
                let permille = done.saturating_mul(1000) / total.max(1);
                if last.swap(permille / 10, Ordering::Relaxed) != permille / 10 {
                    on_ui(move |c| c.download_progress(id, permille as f32 / 1000.0));
                }
            };
            let roms = paths::server_roms_dir(client.base().as_str());
            let result = download_game(&client, id, &roms, &progress, &cancel).await;
            let mut missing = Vec::new();
            if result.is_ok() {
                if let Some(cover) = detail.cover_large.as_deref() {
                    let _ = covers.ensure_large(&client, id, cover).await;
                }
                for url in &detail.screenshots {
                    let _ = covers.ensure_screenshot(&client, id, url).await;
                }
                let system = paths::system_dir();
                missing = match bios::ensure(
                    &client,
                    detail.platform_id,
                    &detail.platform_slug,
                    &system,
                    &cancel,
                )
                .await
                {
                    Ok(missing) => missing,
                    Err(e) => {
                        tracing::warn!("fetching BIOS for {id}: {e}");
                        bios::missing(&detail.platform_slug, &system)
                    }
                };
            }
            on_ui(move |c| c.download_finished(id, result, missing));
        });
    }

    fn download_progress(&self, id: i64, fraction: f32) {
        self.download_fraction.set(fraction);
        if self.current_game().map(|g| g.id) == Some(id) {
            if let Some(ui) = self.ui() {
                ui.set_game_progress(fraction);
            }
        }
    }

    fn download_finished(
        &self,
        id: i64,
        result: Result<PathBuf, DownloadError>,
        missing: Vec<String>,
    ) {
        self.downloading.borrow_mut().take();
        let status = match result {
            Ok(path) => {
                self.shared
                    .store
                    .lock()
                    .unwrap()
                    .set_local_path(id, Some(&path.to_string_lossy()));
                if missing.is_empty() {
                    String::new()
                } else {
                    bios::missing_message(&missing)
                }
            }
            Err(DownloadError::Cancelled) => {
                "Download paused. Choose Download to continue.".to_string()
            }
            Err(e) => e.to_string(),
        };
        if self.current_game().map(|g| g.id) == Some(id) {
            if let Some(ui) = self.ui() {
                ui.set_game_status(status.into());
            }
        }
        self.refresh_game_page(true);
    }

    pub(super) fn cancel_download(&self) {
        if let Some((_, cancel)) = self.downloading.borrow().as_ref() {
            cancel.store(true, Ordering::SeqCst);
        }
    }

    pub(super) fn play_game(&self) {
        let Some(ui) = self.ui() else { return };
        let Some(detail) = self.current_game() else {
            return;
        };
        if self.running.borrow().is_some() || self.preparing.get() {
            ui.set_game_status("A game is already running.".into());
            return;
        }
        if self.syncing_game.get() == Some(detail.id) {
            ui.set_game_status("Still syncing your saves. Try again in a moment.".into());
            return;
        }
        let Some(core) = core_for_platform(&detail.platform_slug) else {
            return;
        };
        let Some(rom) = downloaded_path(&detail) else {
            return;
        };
        self.preparing.set(true);
        ui.set_game_status("Getting ready…".into());
        let cores = self.shared.cores.clone();
        let http = self.shared.http.clone();
        let client = self.client.borrow().clone();
        let offline = self.offline.get();
        let id = detail.id;
        let sync = self
            .sync_device()
            .zip(self.game_saves(&detail))
            .filter(|_| !offline);
        self.shared.rt.spawn(async move {
            let core_path = match cores.installed(core) {
                Some(path) => Ok(path),
                None => {
                    let name = core.name;
                    on_ui(move |c| c.game_status(id, format!("Installing {name}…")));
                    cores.install(&http, BUILDBOT, core).await
                }
            };
            let system = paths::system_dir();
            let mut missing = bios::missing(&detail.platform_slug, &system);
            if !missing.is_empty() && !offline {
                if let Some(client) = &client {
                    if let Ok(still) = bios::ensure(
                        client,
                        detail.platform_id,
                        &detail.platform_slug,
                        &system,
                        &AtomicBool::new(false),
                    )
                    .await
                    {
                        missing = still;
                    }
                }
            }
            let sram = match (&client, sync) {
                (Some(client), Some((device, game))) => {
                    on_ui(move |c| c.game_status(id, "Syncing your save…".into()));
                    Some(saves::sync_sram(client, &device, &game).await)
                }
                _ => None,
            };
            on_ui(move |c| c.launch_ready(detail, rom, core.jit, core_path, missing, sram));
        });
    }

    pub(super) fn game_status(&self, id: i64, text: String) {
        if self.current_game().map(|g| g.id) == Some(id) {
            if let Some(ui) = self.ui() {
                ui.set_game_status(text.into());
            }
        }
    }

    fn launch_ready(
        &self,
        detail: GameDetail,
        rom: PathBuf,
        jit: bool,
        core_path: Result<PathBuf, String>,
        missing: Vec<String>,
        sram: Option<Result<SramOutcome, Error>>,
    ) {
        let core = match core_path {
            Ok(path) => path,
            Err(e) => {
                self.preparing.set(false);
                return self.game_status(detail.id, e);
            }
        };
        if !missing.is_empty() {
            self.preparing.set(false);
            return self.game_status(detail.id, bios::missing_message(&missing));
        }
        match sram {
            Some(Ok(SramOutcome::Conflict(conflict))) => {
                return self.show_conflict(PendingLaunch {
                    detail,
                    rom,
                    jit,
                    core,
                    conflict,
                });
            }
            Some(Err(e)) => tracing::warn!("save sync before launch: {e}"),
            _ => {}
        }
        self.start_game(detail, rom, jit, core);
    }

    pub(super) fn start_game(&self, detail: GameDetail, rom: PathBuf, jit: bool, core: PathBuf) {
        self.preparing.set(false);
        let options = GameOptions {
            core,
            rom,
            save_dir: paths::game_save_dir(&paths::data_dir(), &self.server(), detail.id),
            title: detail.title.clone(),
            jit,
            options: core_for_platform(&detail.platform_slug)
                .map(|core| crate::cores::default_options(core.id))
                .unwrap_or_default(),
            split_screens: core_for_platform(&detail.platform_slug)
                .is_some_and(|core| core.id == "desmume"),
            gamepads: self.gamepads.clone(),
            players: self.players.clone(),
        };
        let on_closed = move |identity| {
            let _ =
                slint::invoke_from_event_loop(move || with_controller(|c| c.game_closed(identity)));
        };
        match play::launch(options, on_closed) {
            Ok(running) => {
                *self.running.borrow_mut() = Some(running);
                self.game_status(detail.id, String::new());
                *self.playing.borrow_mut() = Some(detail);
            }
            Err(e) => self.game_status(detail.id, format!("Could not start the game: {e:#}")),
        }
    }

    fn game_closed(&self, identity: Option<CoreIdentity>) {
        self.running.borrow_mut().take();
        self.save_players();
        let playing = self.playing.borrow_mut().take();
        if let Some(detail) = playing {
            self.after_play(detail, identity);
        }
    }

    pub(super) fn delete_game(&self) {
        let Some(detail) = self.current_game() else {
            return;
        };
        if self.running.borrow().is_some() || self.preparing.get() {
            return self.game_status(detail.id, "Close the running game first.".into());
        }
        if let Some(dir) = download_dir(&detail) {
            let _ = std::fs::remove_dir_all(dir);
        }
        self.shared
            .store
            .lock()
            .unwrap()
            .set_local_path(detail.id, None);
        self.game_status(detail.id, String::new());
        self.refresh_game_page(true);
    }
}
