mod game;

use crate::cores::Cores;
use crate::covers::Covers;
use crate::credentials::{Keychain, TokenStore};
use crate::grid::{row_count, row_range, CoverSlots, RecentRows, RECENT_ROWS};
use crate::romm::client::{check_version, server_candidates, Client, Error};
use crate::romm::pairing::{PollStep, Poller};
use crate::romm::types::{DeviceAuth, PollOutcome, User};
use crate::store::{GameFilter, GameItem, Store};
use crate::sync::{sync_library, SyncReport, PAGE_SIZE};
use crate::{identity, paths, qr, AppWindow, GameCard, GameRow, PlatformEntry};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode,
    VecModel, Weak,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;
use tokio::task::AbortHandle;

const PARALLEL_DOWNLOADS: usize = 6;
const OFFLINE_RETRY: Duration = Duration::from_secs(60);

const SCREEN_CONNECT: i32 = 0;
const SCREEN_PAIRING: i32 = 1;
const SCREEN_LIBRARY: i32 = 2;
const SCREEN_GAME: i32 = 3;

struct Shared {
    rt: Runtime,
    store: Arc<Mutex<Store>>,
    tokens: Arc<dyn TokenStore>,
    covers: Arc<Covers>,
    downloads: Arc<Semaphore>,
    cores: Arc<Cores>,
    http: reqwest::Client,
}

struct Library {
    filter: GameFilter,
    columns: usize,
    games: Vec<GameItem>,
    rows: Rc<VecModel<GameRow>>,
    covers: CoverSlots,
    loading: HashSet<i64>,
    recent: RecentRows,
    wanted: Arc<Mutex<HashSet<i64>>>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            filter: GameFilter::default(),
            columns: 1,
            games: Vec::new(),
            rows: Rc::new(VecModel::default()),
            covers: CoverSlots::with_default_capacity(),
            loading: HashSet::new(),
            recent: RecentRows::new(RECENT_ROWS),
            wanted: Arc::default(),
        }
    }
}

#[derive(Default)]
struct Pairing {
    client: Option<Client>,
    task: Option<AbortHandle>,
    countdown: Option<Timer>,
}

struct Controller {
    shared: Shared,
    ui: Weak<AppWindow>,
    pairing: RefCell<Pairing>,
    client: RefCell<Option<Client>>,
    library: RefCell<Library>,
    sync_generation: Cell<u64>,
    sync_cancel: RefCell<Arc<AtomicBool>>,
    game: RefCell<Option<game::GameState>>,
    downloading: RefCell<Option<(i64, Arc<AtomicBool>)>>,
    preparing: Cell<bool>,
    download_fraction: Cell<f32>,
    running: RefCell<Option<crate::play::RunningGame>>,
    offline: Cell<bool>,
    offline_retry: RefCell<Option<Timer>>,
}

thread_local! {
    static CONTROLLER: RefCell<Option<Rc<Controller>>> = const { RefCell::new(None) };
}

fn with_controller(f: impl FnOnce(&Rc<Controller>)) {
    let controller = CONTROLLER.with(|c| c.borrow().clone());
    if let Some(c) = controller {
        f(&c)
    }
}

fn on_ui(f: impl FnOnce(&Rc<Controller>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || with_controller(f));
}

pub fn run() -> anyhow::Result<()> {
    let shared = Shared {
        rt: Runtime::new()?,
        store: Arc::new(Mutex::new(Store::open(&paths::db_path())?)),
        tokens: Arc::new(Keychain),
        covers: Arc::new(Covers::new(paths::covers_dir())),
        downloads: Arc::new(Semaphore::new(PARALLEL_DOWNLOADS)),
        cores: Arc::new(Cores::new(paths::cores_dir())),
        http: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .build()?,
    };
    let ui = AppWindow::new()?;
    let controller = Rc::new(Controller {
        shared,
        ui: ui.as_weak(),
        pairing: RefCell::default(),
        client: RefCell::new(None),
        library: RefCell::default(),
        sync_generation: Cell::new(0),
        sync_cancel: RefCell::default(),
        game: RefCell::new(None),
        downloading: RefCell::new(None),
        preparing: Cell::new(false),
        download_fraction: Cell::new(0.0),
        running: RefCell::new(None),
        offline: Cell::new(false),
        offline_retry: RefCell::new(None),
    });
    ui.set_rows(ModelRc::from(controller.library.borrow().rows.clone()));
    CONTROLLER.with(|c| *c.borrow_mut() = Some(controller.clone()));
    controller.wire(&ui);
    controller.start();
    ui.run()?;
    CONTROLLER.with(|c| c.borrow_mut().take());
    Ok(())
}

impl Controller {
    fn ui(&self) -> Option<AppWindow> {
        self.ui.upgrade()
    }

    fn wire(&self, ui: &AppWindow) {
        ui.on_connect(|| with_controller(|c| c.connect()));
        ui.on_copy_link(|| with_controller(|c| c.copy_link()));
        ui.on_open_link(|| with_controller(|c| c.open_link()));
        ui.on_cancel_pairing(|| with_controller(|c| c.cancel_pairing()));
        ui.on_retry_pairing(|| with_controller(|c| c.connect()));
        ui.on_sign_out(|| with_controller(|c| c.sign_out()));
        ui.on_select_platform(|id| with_controller(|c| c.set_platform(id)));
        ui.on_search_edited(|text| with_controller(|c| c.set_search(text.to_string())));
        ui.on_columns_changed(|n| with_controller(|c| c.set_columns(n)));
        ui.on_row_shown(|i| with_controller(|c| c.row_shown(i.max(0) as usize)));
        ui.on_refresh(|| with_controller(|c| c.sync()));
        ui.on_open_game(|id| with_controller(|c| c.open_game(id as i64)));
        ui.on_back_to_library(|| with_controller(|c| c.back_to_library()));
        ui.on_download_game(|| with_controller(|c| c.download_game()));
        ui.on_cancel_download(|| with_controller(|c| c.cancel_download()));
        ui.on_play_game(|| with_controller(|c| c.play_game()));
        ui.on_delete_game(|| with_controller(|c| c.delete_game()));
    }

    fn start(&self) {
        let server = self.shared.store.lock().unwrap().get("server");
        let token = server.as_deref().and_then(|s| self.shared.tokens.load(s));
        match (server.as_deref().map(url::Url::parse), token) {
            (Some(Ok(base)), Some(token)) => {
                self.enter_library(Client::new(base).with_token(token))
            }
            _ => {
                if let Some(ui) = self.ui() {
                    let shown = server.unwrap_or_default();
                    ui.set_server_url(shown.trim_end_matches('/').into());
                    ui.set_screen(SCREEN_CONNECT);
                }
            }
        }
    }

    fn connect(&self) {
        let Some(ui) = self.ui() else { return };
        let candidates = match server_candidates(&ui.get_server_url()) {
            Ok(c) => c,
            Err(e) => return ui.set_connect_error(e.into()),
        };
        ui.set_connect_error("".into());
        ui.set_busy(true);
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            let result = begin_pairing(candidates, &store).await;
            on_ui(move |c| c.pairing_started(result));
        });
    }

    fn pairing_started(&self, result: Result<(Client, DeviceAuth), String>) {
        let Some(ui) = self.ui() else { return };
        ui.set_busy(false);
        let (client, auth) = match result {
            Ok(ok) => ok,
            Err(e) => {
                ui.set_connect_error(e.into());
                ui.set_screen(SCREEN_CONNECT);
                return;
            }
        };
        let link = client.url(&auth.verification_path_complete).to_string();
        if let Ok((pixels, side)) = qr::qr_pixels(&link, 8) {
            ui.set_qr(Image::from_rgba8(
                SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&pixels, side, side),
            ));
        }
        ui.set_pair_link(link.into());
        ui.set_user_code(auth.user_code.clone().into());
        ui.set_seconds_left(auth.expires_in as i32);
        ui.set_pair_status("".into());
        ui.set_pair_failed(false);
        ui.set_screen(SCREEN_PAIRING);

        let countdown = Timer::default();
        let weak = self.ui.clone();
        countdown.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_seconds_left((ui.get_seconds_left() - 1).max(0));
            }
        });

        let task = self.shared.rt.spawn(poll_until_done(client.clone(), auth));
        let mut pairing = self.pairing.borrow_mut();
        if let Some(old) = pairing.task.take() {
            old.abort();
        }
        *pairing = Pairing {
            client: Some(client),
            task: Some(task.abort_handle()),
            countdown: Some(countdown),
        };
    }

    fn pairing_finished(&self, outcome: PollOutcome) {
        let Some(ui) = self.ui() else { return };
        let client = {
            let mut pairing = self.pairing.borrow_mut();
            pairing.countdown = None;
            pairing.task = None;
            pairing.client.take()
        };
        let message = match (outcome, client) {
            (PollOutcome::Approved { token, .. }, Some(client)) => {
                let server = client.base().to_string();
                if let Err(e) = self.shared.tokens.save(&server, &token) {
                    ui.set_pair_failed(true);
                    ui.set_pair_status(e.into());
                    return;
                }
                self.stop_sync();
                self.shared.store.lock().unwrap().switch_server(&server);
                self.enter_library(client.with_token(token));
                return;
            }
            (PollOutcome::Denied, _) => "The request was declined.",
            _ => "The code expired.",
        };
        ui.set_pair_failed(true);
        ui.set_pair_status(message.into());
    }

    fn copy_link(&self) {
        if let Some(ui) = self.ui() {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(ui.get_pair_link().to_string());
            }
        }
    }

    fn open_link(&self) {
        if let Some(ui) = self.ui() {
            let _ = open::that_detached(ui.get_pair_link().as_str());
        }
    }

    fn cancel_pairing(&self) {
        let mut pairing = self.pairing.borrow_mut();
        if let Some(task) = pairing.task.take() {
            task.abort();
        }
        *pairing = Pairing::default();
        if let Some(ui) = self.ui() {
            ui.set_screen(SCREEN_CONNECT);
        }
    }

    fn enter_library(&self, client: Client) {
        *self.client.borrow_mut() = Some(client.clone());
        self.set_offline(false);
        self.library.borrow_mut().filter = GameFilter::default();
        if let Some(ui) = self.ui() {
            ui.set_selected_platform(-1);
            ui.set_search("".into());
            ui.set_screen(SCREEN_LIBRARY);
        }
        self.reload_sidebar();
        self.reload_games();
        self.sync();
        self.shared.rt.spawn(async move {
            let me = client.me().await;
            on_ui(move |c| c.signed_in(me));
        });
    }

    fn signed_in(&self, me: Result<User, Error>) {
        let Some(ui) = self.ui() else { return };
        match me {
            Ok(user) => ui.set_user_label(user.username.into()),
            Err(Error::Unauthorized) => self.needs_repair(),
            Err(_) => {}
        }
    }

    fn needs_repair(&self) {
        let Some(ui) = self.ui() else { return };
        ui.set_connect_error(
            "This device was signed out by the server. Pair it again to continue.".into(),
        );
        ui.set_screen(SCREEN_CONNECT);
    }

    fn stop_sync(&self) {
        self.sync_cancel.borrow().store(true, Ordering::SeqCst);
        self.sync_generation.set(self.sync_generation.get() + 1);
        if let Some(ui) = self.ui() {
            ui.set_syncing(false);
            ui.set_sync_status("".into());
        }
    }

    fn set_offline(&self, offline: bool) {
        self.library.borrow_mut().filter.downloaded_only = offline;
        if self.offline.replace(offline) == offline {
            return;
        }
        *self.offline_retry.borrow_mut() = offline.then(|| {
            let timer = Timer::default();
            timer.start(TimerMode::Repeated, OFFLINE_RETRY, || {
                with_controller(|c| c.sync())
            });
            timer
        });
        if self.game.borrow().is_some() {
            self.refresh_game_page(false);
        }
    }

    fn sign_out(&self) {
        self.stop_sync();
        self.set_offline(false);
        let server = self
            .client
            .borrow_mut()
            .take()
            .map(|c| c.base().to_string());
        if let Some(server) = server {
            self.shared.tokens.delete(&server);
        }
        {
            let mut store = self.shared.store.lock().unwrap();
            store.clear_library();
            store.remove("server");
        }
        self.library.borrow_mut().games.clear();
        self.rebuild_rows();
        self.reload_sidebar();
        if let Some(ui) = self.ui() {
            ui.set_user_label("".into());
            ui.set_connect_error("".into());
            ui.set_sync_status("".into());
            ui.set_screen(SCREEN_CONNECT);
        }
    }

    fn set_platform(&self, id: i32) {
        self.library.borrow_mut().filter.platform = (id >= 0).then_some(id as i64);
        if let Some(ui) = self.ui() {
            ui.set_selected_platform(id);
        }
        self.reload_games();
    }

    fn set_search(&self, text: String) {
        self.library.borrow_mut().filter.search = text;
        self.reload_games();
    }

    fn set_columns(&self, columns: i32) {
        let columns = columns.max(1) as usize;
        if self.library.borrow().columns != columns {
            self.library.borrow_mut().columns = columns;
            self.rebuild_rows();
        }
    }

    fn reload_sidebar(&self) {
        let Some(ui) = self.ui() else { return };
        let platforms = self
            .shared
            .store
            .lock()
            .unwrap()
            .platforms(self.offline.get());
        let total: i64 = platforms.iter().map(|p| p.count).sum();
        let entries: Vec<PlatformEntry> = platforms
            .into_iter()
            .map(|p| PlatformEntry {
                id: p.id as i32,
                name: p.name.into(),
                count: p.count as i32,
            })
            .collect();
        ui.set_platforms(ModelRc::new(VecModel::from(entries)));
        ui.set_total_games(total as i32);
    }

    fn reload_games(&self) {
        let filter = self.library.borrow().filter.clone();
        let games = self.shared.store.lock().unwrap().games(&filter);
        self.library.borrow_mut().games = games;
        self.rebuild_rows();
    }

    fn rebuild_rows(&self) {
        let mut lib = self.library.borrow_mut();
        lib.covers.clear();
        lib.recent.clear();
        lib.wanted.lock().unwrap().clear();
        let total = lib.games.len();
        let rows: Vec<GameRow> = (0..row_count(total, lib.columns))
            .map(|r| {
                let cards: Vec<GameCard> = lib.games[row_range(r, total, lib.columns)]
                    .iter()
                    .map(|g| GameCard {
                        id: g.id as i32,
                        title: g.title.clone().into(),
                        platform: g.platform.clone().into(),
                        cover: Image::default(),
                        has_cover: false,
                        downloaded: g.downloaded,
                    })
                    .collect();
                GameRow {
                    cards: ModelRc::new(VecModel::from(cards)),
                }
            })
            .collect();
        lib.rows.set_vec(rows);
    }

    fn row_shown(&self, row: usize) {
        let (games, wanted) = {
            let mut lib = self.library.borrow_mut();
            let total = lib.games.len();
            lib.recent.push(row);
            let visible: HashSet<i64> = lib
                .recent
                .rows()
                .flat_map(|r| {
                    lib.games[row_range(r, total, lib.columns)]
                        .iter()
                        .map(|g| g.id)
                })
                .collect();
            *lib.wanted.lock().unwrap() = visible;
            let games: Vec<(i64, String)> = lib.games[row_range(row, total, lib.columns)]
                .iter()
                .filter_map(|g| Some((g.id, g.cover.clone()?)))
                .collect();
            (games, lib.wanted.clone())
        };
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        for (id, cover) in games {
            let cached = self.shared.covers.path_for(id, &cover);
            if cached.exists() {
                self.show_cover(id, &cached);
                continue;
            }
            if !self.library.borrow_mut().loading.insert(id) {
                continue;
            }
            let covers = self.shared.covers.clone();
            let downloads = self.shared.downloads.clone();
            let wanted = wanted.clone();
            let client = client.clone();
            self.shared.rt.spawn(async move {
                let _permit = downloads.acquire_owned().await;
                let result = if wanted.lock().unwrap().contains(&id) {
                    covers.ensure(&client, id, &cover).await.map(Some)
                } else {
                    Ok(None)
                };
                on_ui(move |c| {
                    c.library.borrow_mut().loading.remove(&id);
                    match result {
                        Ok(Some(path)) => c.show_cover(id, &path),
                        Ok(None) => {}
                        Err(e) => tracing::debug!("cover {id}: {e}"),
                    }
                });
            });
        }
    }

    fn position_of(&self, id: i64) -> Option<(usize, usize)> {
        let lib = self.library.borrow();
        let index = lib.games.iter().position(|g| g.id == id)?;
        let columns = lib.columns.max(1);
        Some((index / columns, index % columns))
    }

    fn set_card_cover(&self, id: i64, image: Option<Image>) {
        let Some((row, col)) = self.position_of(id) else {
            return;
        };
        let rows = self.library.borrow().rows.clone();
        let Some(row_data) = rows.row_data(row) else {
            return;
        };
        let Some(mut card) = row_data.cards.row_data(col) else {
            return;
        };
        card.has_cover = image.is_some();
        card.cover = image.unwrap_or_default();
        row_data.cards.set_row_data(col, card);
    }

    fn show_cover(&self, id: i64, path: &Path) {
        {
            let lib = self.library.borrow();
            if lib.covers.contains(id) || !lib.wanted.lock().unwrap().contains(&id) {
                return;
            }
        }
        let Ok(image) = Image::load_from_path(path) else {
            return;
        };
        self.set_card_cover(id, Some(image));
        let evicted = self.library.borrow_mut().covers.touch(id);
        for old in evicted {
            self.set_card_cover(old, None);
        }
    }

    fn sync(&self) {
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        let Some(ui) = self.ui() else { return };
        if ui.get_syncing() {
            return;
        }
        ui.set_syncing(true);
        ui.set_sync_status("Syncing…".into());
        let generation = self.sync_generation.get() + 1;
        self.sync_generation.set(generation);
        let cancel = Arc::new(AtomicBool::new(false));
        *self.sync_cancel.borrow_mut() = cancel.clone();
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            let result = sync_library(&client, &store, PAGE_SIZE, &cancel).await;
            on_ui(move |c| c.sync_finished(generation, result));
        });
    }

    fn sync_finished(&self, generation: u64, result: Result<SyncReport, Error>) {
        if generation != self.sync_generation.get() {
            return;
        }
        let Some(ui) = self.ui() else { return };
        ui.set_syncing(false);
        match result {
            Ok(_) => {
                self.set_offline(false);
                ui.set_sync_status("".into());
            }
            Err(Error::Cancelled) => return,
            Err(Error::Unauthorized) => return self.needs_repair(),
            Err(Error::Unreachable) => {
                self.set_offline(true);
                ui.set_sync_status("Offline: showing downloaded games".into());
            }
            Err(e) => ui.set_sync_status(format!("Sync failed: {e}").into()),
        }
        self.reload_sidebar();
        self.reload_games();
    }
}

async fn begin_pairing(
    candidates: Vec<url::Url>,
    store: &Mutex<Store>,
) -> Result<(Client, DeviceAuth), String> {
    let (device_id, name) = {
        let store = store.lock().unwrap();
        (identity::device_id(&store), identity::device_name())
    };
    let mut last_error =
        String::from("Could not reach the server. Check the address and that RomM is running.");
    for base in candidates {
        let client = Client::new(base);
        match client.heartbeat().await {
            Ok(hb) => {
                check_version(&hb.system.version)?;
                return match client.device_init(&device_id, &name).await {
                    Ok(auth) => Ok((client, auth)),
                    Err(Error::Status(404)) => Err(
                        "This server does not support device sign-in. Update RomM to 5.0 or newer."
                            .into(),
                    ),
                    Err(e) => Err(format!("Could not start sign-in: {e}.")),
                };
            }
            Err(Error::Unreachable) => {}
            Err(e) => last_error = format!("That doesn't look like a RomM server ({e})."),
        }
    }
    Err(last_error)
}

async fn poll_until_done(client: Client, auth: DeviceAuth) {
    let mut poller = Poller::new(auth.interval, auth.expires_in, Instant::now());
    let mut wait = Duration::from_secs(auth.interval.max(1));
    loop {
        tokio::time::sleep(wait).await;
        let outcome = client
            .device_token(&auth.device_code)
            .await
            .unwrap_or(PollOutcome::Pending);
        match poller.next(&outcome, Instant::now()) {
            PollStep::Wait(next) => wait = next,
            PollStep::Done => {
                let outcome = match outcome {
                    PollOutcome::Pending | PollOutcome::SlowDown => PollOutcome::Expired,
                    o => o,
                };
                on_ui(move |c| c.pairing_finished(outcome));
                return;
            }
        }
    }
}
