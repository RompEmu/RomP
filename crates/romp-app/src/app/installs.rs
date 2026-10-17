//! Games sent from RomM's "Install on device": heard over the device socket, claimed, and
//! downloaded one at a time through the same path as the game page's Download.
use super::game::{downloaded_path, Origin};
use super::{on_ui, Controller};
use crate::installs::Cancelled;
use crate::romm::client::InstallOutcome;
use crate::romm::device_socket::{self, DeviceEvent};
use crate::romm::types::InstallRequest;
use tr::tr;

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
        self.installs.borrow_mut().clear();
    }

    fn device_event(&self, event: DeviceEvent) {
        match event {
            DeviceEvent::Connected | DeviceEvent::InstallQueued { .. } => self.claim_installs(),
            DeviceEvent::InstallCancelled { id } => {
                if self.installs.borrow_mut().cancel(&id) == Cancelled::Running {
                    self.cancel_download();
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
        if downloaded_path(&detail).is_some() {
            self.report_install(&request, InstallOutcome::AlreadyInstalled, None);
            self.installs.borrow_mut().finish();
            return self.pump_installs();
        }
        self.show_install_status(&detail.title, 0);
        let only = (!request.file_ids.is_empty()).then(|| request.file_ids.clone());
        self.start_download(detail, Origin::Install(request.id), only);
    }

    pub(super) fn install_progress(&self, fraction: f32) {
        let rom_id = self.installs.borrow().running().map(|r| r.rom_id);
        let title = rom_id.and_then(|id| self.shared.store.lock().unwrap().game(id));
        if let Some(detail) = title {
            self.show_install_status(&detail.title, (fraction * 100.0).round() as u32);
        }
    }

    fn show_install_status(&self, title: &str, percent: u32) {
        if let Some(ui) = self.ui() {
            ui.set_sync_status(
                tr!("Downloading {0}, sent from RomM · {1}%", title, percent).into(),
            );
        }
    }

    /// `Err(None)` is a cancelled download, which RomM already ended.
    pub(super) fn install_finished(&self, request_id: String, outcome: Result<(), Option<String>>) {
        let request = self
            .installs
            .borrow()
            .running()
            .filter(|r| r.id == request_id)
            .cloned();
        self.installs.borrow_mut().finish();
        let status = match (&outcome, &request) {
            (Ok(()), Some(request)) => {
                self.report_install(request, InstallOutcome::Done, None);
                String::new()
            }
            (Err(Some(reason)), Some(request)) => {
                self.report_install(request, InstallOutcome::Failed, Some(reason));
                tr!("A game sent from RomM didn't download: {0}", reason)
            }
            _ => String::new(),
        };
        if let Some(ui) = self.ui() {
            ui.set_sync_status(status.into());
        }
        self.reload_games();
        self.pump_installs();
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
