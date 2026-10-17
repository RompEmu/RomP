//! Games sent from RomM's "Install on device": heard over the device socket, claimed, and
//! downloaded one at a time through the same path as the game page's Download.
use super::game::Origin;
use super::{on_ui, with_controller, Controller};
use crate::installs::{card, Cancelled, Stage};
use crate::romm::client::InstallOutcome;
use crate::romm::device_socket::{self, DeviceEvent};
use crate::romm::types::InstallRequest;
use slint::{Timer, TimerMode};
use std::time::Duration;
use tr::tr;

/// How long the card says a game is ready before it goes.
const READY_FOR: Duration = Duration::from_secs(10);

/// RomP takes games sent from RomM only when all of these hold.
#[derive(Debug, Clone, Copy)]
struct InstallGate {
    signed_in: bool,
    online: bool,
    setting: bool,
    server_allows: bool,
}

impl InstallGate {
    fn open(self) -> bool {
        self.signed_in && self.online && self.setting && self.server_allows
    }
}

impl Controller {
    /// Tells RomM whether this device takes installs, and opens or closes the device socket.
    pub(super) fn refresh_installs(&self) {
        let (Some(client), Some(device)) = (self.client.borrow().clone(), self.sync_device())
        else {
            return self.close_installs();
        };
        if self.offline.get() {
            return self.close_installs();
        }
        let setting = self.prefs.get().accept_installs;
        self.shared.rt.spawn(async move {
            let allows = client
                .heartbeat()
                .await
                .ok()
                .and_then(|h| h.device_install)
                .is_some_and(|d| d.enabled);
            if allows {
                if let Err(e) = client.set_remote_install(&device, setting).await {
                    tracing::warn!("offering this device for installs: {e}");
                }
            }
            on_ui(move |c| c.open_installs(allows));
        });
    }

    fn open_installs(&self, server_allows: bool) {
        let gate = InstallGate {
            signed_in: self.client.borrow().is_some() && self.sync_device().is_some(),
            online: !self.offline.get(),
            setting: self.prefs.get().accept_installs,
            server_allows,
        };
        if !gate.open() {
            return self.close_installs();
        }
        if self.device_socket.borrow().is_some() {
            return;
        }
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        let Some(token) = client.token().map(str::to_string) else {
            return;
        };
        let socket = device_socket::spawn(
            self.shared.rt.handle(),
            client.base().clone(),
            token,
            |event| on_ui(move |c| c.device_event(event)),
        );
        *self.device_socket.borrow_mut() = Some(socket);
    }

    /// Stops listening; an install already downloading finishes.
    pub(super) fn close_installs(&self) {
        self.device_socket.borrow_mut().take();
        if self.install_downloading().is_some() {
            self.installs.borrow_mut().clear();
        } else {
            self.installs.borrow_mut().abandon();
        }
    }

    /// The request whose download is running, if the running download is an install.
    fn install_downloading(&self) -> Option<String> {
        match self.downloading.borrow().as_ref() {
            Some((_, _, Origin::Install(id))) => Some(id.clone()),
            _ => None,
        }
    }

    fn device_event(&self, event: DeviceEvent) {
        match event {
            DeviceEvent::Connected | DeviceEvent::InstallQueued { .. } => self.claim_installs(),
            DeviceEvent::InstallCancelled { id } => {
                if self.installs.borrow_mut().cancel(&id) != Cancelled::Running {
                    return;
                }
                // Only the install's own download stops; one started from a game page doesn't.
                if self.install_downloading().as_deref() == Some(id.as_str()) {
                    self.cancel_download();
                } else {
                    self.installs.borrow_mut().finish();
                    self.pump_installs();
                }
            }
            DeviceEvent::Refused(reason) => {
                tracing::info!("RomM refused the device socket: {reason}");
                self.device_socket.borrow_mut().take();
            }
            DeviceEvent::Disconnected => {}
        }
    }

    fn claim_installs(&self) {
        let (Some(client), Some(device)) = (self.client.borrow().clone(), self.sync_device())
        else {
            return;
        };
        self.shared.rt.spawn(async move {
            match client.claim_installs(&device).await {
                Ok(requests) => on_ui(move |c| {
                    c.installs.borrow_mut().add(requests);
                    c.pump_installs();
                    c.show_waiting_install();
                }),
                Err(e) => tracing::warn!("claiming installs: {e}"),
            }
        });
    }

    /// Starts the next install, unless a game or another download is running.
    pub(super) fn pump_installs(&self) {
        if self.running.borrow().is_some() || self.downloading_id().is_some() {
            return;
        }
        if self.device_socket.borrow().is_none() || !self.prefs.get().accept_installs {
            return;
        }
        let request = {
            let mut queue = self.installs.borrow_mut();
            queue.running().cloned().or_else(|| queue.next())
        };
        let Some(request) = request else {
            return;
        };
        let detail = self.shared.store.lock().unwrap().game(request.rom_id);
        let Some(detail) = detail else {
            // Added to RomM since the last sync: sync once, then try again.
            let waited = self.install_awaiting_sync.borrow().as_deref() == Some(&request.id);
            if !waited {
                *self.install_awaiting_sync.borrow_mut() = Some(request.id.clone());
                return self.sync();
            }
            let reason = tr!("This game isn't in RomP's library.");
            return self.install_finished(request.id, Err(Some(reason)));
        };
        self.install_awaiting_sync.borrow_mut().take();
        // Files already here aren't fetched again; nothing fetched reports as already installed.
        self.show_install_card(&detail.title, request.rom_id, &Stage::Downloading(0.0));
        let only = (!request.file_ids.is_empty()).then(|| request.file_ids.clone());
        self.start_download(detail, Origin::Install(request.id), only);
    }

    pub(super) fn install_progress(&self, fraction: f32) {
        let rom_id = self.installs.borrow().running().map(|r| r.rom_id);
        let title = rom_id.and_then(|id| self.shared.store.lock().unwrap().game(id));
        if let (Some(detail), Some(rom_id)) = (title, rom_id) {
            self.show_install_card(&detail.title, rom_id, &Stage::Downloading(fraction));
        }
    }

    /// While a game runs or a page download goes on, the next install says it's waiting.
    fn show_waiting_install(&self) {
        if self.install_downloading().is_some() {
            return;
        }
        let next = {
            let queue = self.installs.borrow();
            queue.running().or(queue.first_waiting()).cloned()
        };
        let Some(next) = next else { return };
        let title = self.install_title(next.rom_id);
        self.show_install_card(&title, next.rom_id, &Stage::Waiting);
    }

    fn install_title(&self, rom_id: i64) -> String {
        self.shared
            .store
            .lock()
            .unwrap()
            .game(rom_id)
            .map_or_else(|| tr!("A game"), |g| g.title)
    }

    fn show_install_card(&self, title: &str, rom_id: i64, stage: &Stage) {
        let Some(ui) = self.ui() else { return };
        let playing = self.running.borrow().is_some();
        let shown = card(title, stage, playing, self.installs.borrow().waiting());
        ui.set_install_card_title(shown.title.into());
        ui.set_install_card_detail(shown.detail.into());
        ui.set_install_card_progress(shown.progress);
        ui.set_install_card_done(shown.done);
        ui.set_install_card_failed(shown.failed);
        ui.set_install_card_shown(true);
        self.install_card_rom.set(Some(rom_id));
        let timer = shown.done.then(|| {
            let timer = Timer::default();
            timer.start(TimerMode::SingleShot, READY_FOR, || {
                with_controller(|c| c.hide_install_card());
            });
            timer
        });
        *self.install_card_timer.borrow_mut() = timer;
    }

    pub(super) fn hide_install_card(&self) {
        self.install_card_timer.borrow_mut().take();
        self.install_card_rom.set(None);
        if let Some(ui) = self.ui() {
            ui.set_install_card_shown(false);
        }
    }

    pub(super) fn install_card_open(&self) {
        if let Some(rom_id) = self.install_card_rom.get() {
            self.open_game(rom_id);
        }
    }

    pub(super) fn install_card_play(&self) {
        if let Some(rom_id) = self.install_card_rom.get() {
            self.hide_install_card();
            self.open_game(rom_id);
            self.play_game();
        }
    }

    /// `Ok(fetched)`: whether anything had to be downloaded. `Err(None)`: the download stopped.
    pub(super) fn install_finished(
        &self,
        request_id: String,
        outcome: Result<bool, Option<String>>,
    ) {
        let request = self
            .installs
            .borrow()
            .running()
            .filter(|r| r.id == request_id)
            .cloned();
        let romm_cancelled = self.installs.borrow().cancelled_by_romm(&request_id);
        self.installs.borrow_mut().finish();
        let title = request
            .as_ref()
            .map(|r| (self.install_title(r.rom_id), r.rom_id));
        match (&outcome, &request) {
            (Ok(fetched), Some(request)) => {
                let outcome = if *fetched {
                    InstallOutcome::Done
                } else {
                    InstallOutcome::AlreadyInstalled
                };
                self.report_install(request, outcome, None);
                if let Some((title, rom_id)) = &title {
                    self.show_install_card(title, *rom_id, &Stage::Done);
                }
            }
            (Err(Some(reason)), Some(request)) => {
                self.report_install(request, InstallOutcome::Failed, Some(reason));
                if let Some((title, rom_id)) = &title {
                    self.show_install_card(title, *rom_id, &Stage::Failed(reason.clone()));
                }
            }
            // Paused here rather than withdrawn in RomM: RomM would otherwise keep it as taken
            // and send it again on the next connection.
            (Err(None), Some(request)) if !romm_cancelled => {
                let reason = tr!("Paused on this device");
                self.report_install(request, InstallOutcome::Failed, Some(&reason));
                self.hide_install_card();
            }
            _ => self.hide_install_card(),
        }
        self.reload_games();
        self.pump_installs();
        self.show_waiting_install();
    }

    fn report_install(
        &self,
        request: &InstallRequest,
        outcome: InstallOutcome,
        reason: Option<&str>,
    ) {
        let (Some(client), Some(device)) = (self.client.borrow().clone(), self.sync_device())
        else {
            return;
        };
        let id = request.id.clone();
        let reason = reason.map(str::to_string);
        self.shared.rt.spawn(async move {
            if let Err(e) = client
                .report_install(&device, &id, outcome, reason.as_deref())
                .await
            {
                tracing::warn!("reporting install {id}: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_run_only_when_everything_allows_it() {
        let on = InstallGate {
            signed_in: true,
            online: true,
            setting: true,
            server_allows: true,
        };
        assert!(on.open());
        assert!(!InstallGate {
            online: false,
            ..on
        }
        .open());
        assert!(!InstallGate {
            setting: false,
            ..on
        }
        .open());
        assert!(!InstallGate {
            server_allows: false,
            ..on
        }
        .open());
        assert!(!InstallGate {
            signed_in: false,
            ..on
        }
        .open());
    }
}
