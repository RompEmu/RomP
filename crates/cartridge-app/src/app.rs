use crate::credentials::{Keychain, TokenStore};
use crate::romm::client::{check_version, server_candidates, Client, Error};
use crate::romm::pairing::{PollStep, Poller};
use crate::romm::types::{DeviceAuth, PollOutcome, User};
use crate::store::Store;
use crate::{identity, paths, qr, AppWindow};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode, Weak};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::task::AbortHandle;

const SCREEN_CONNECT: i32 = 0;
const SCREEN_PAIRING: i32 = 1;
const SCREEN_LIBRARY: i32 = 2;

struct Shared {
    rt: Runtime,
    store: Arc<Mutex<Store>>,
    tokens: Arc<dyn TokenStore>,
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
    };
    let ui = AppWindow::new()?;
    let controller = Rc::new(Controller {
        shared,
        ui: ui.as_weak(),
        pairing: RefCell::default(),
        client: RefCell::new(None),
    });
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
            (PollOutcome::Approved(token), Some(client)) => {
                let server = client.base().to_string();
                if let Err(e) = self.shared.tokens.save(&server, &token) {
                    ui.set_pair_failed(true);
                    ui.set_pair_status(e.into());
                    return;
                }
                self.shared.store.lock().unwrap().set("server", &server);
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
        if let Some(ui) = self.ui() {
            ui.set_screen(SCREEN_LIBRARY);
        }
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

    fn sign_out(&self) {
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
        if let Some(ui) = self.ui() {
            ui.set_user_label("".into());
            ui.set_connect_error("".into());
            ui.set_screen(SCREEN_CONNECT);
        }
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
