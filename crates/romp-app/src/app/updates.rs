use super::{on_ui, Controller};
use crate::updates::{self, Platform, State, STATE_KEY};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Controller {
    fn update_state(&self) -> State {
        State::from_json(self.shared.store.lock().unwrap().get(STATE_KEY).as_deref())
    }

    fn save_update_state(&self, state: &State) {
        self.shared
            .store
            .lock()
            .unwrap()
            .set(STATE_KEY, &state.to_json());
    }

    /// Asks GitHub for the newest release, at most once a day, and tells about it.
    pub(super) fn check_for_updates(&self) {
        self.show_update();
        let state = self.update_state();
        if !self.prefs.get().check_updates || !state.due(now()) {
            return;
        }
        let http = self.shared.http.clone();
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            match updates::latest(&http, updates::RELEASES, Platform::current()).await {
                Ok(latest) => {
                    tracing::info!(version = latest.version, "newest RomP release");
                    let state = State {
                        checked_at: now(),
                        latest: Some(latest),
                        ..state
                    };
                    store.lock().unwrap().set(STATE_KEY, &state.to_json());
                    on_ui(|c| c.show_update());
                }
                Err(e) => tracing::warn!("checking for a newer RomP: {e}"),
            }
        });
    }

    pub(super) fn show_update(&self) {
        let Some(ui) = self.ui() else { return };
        let state = self.update_state();
        let current = updates::current_version();
        let on = self.prefs.get().check_updates;
        let available = state.available(&current).filter(|_| on);
        ui.set_update_version(
            available
                .map(|r| r.version.clone())
                .unwrap_or_default()
                .into(),
        );
        ui.set_update_homebrew(updates::homebrew());
        let offered = state.offer(&current).is_some() && on;
        ui.set_update_shown(offered && !self.update_dismissed.get());
    }

    pub(super) fn download_update(&self) {
        let state = self.update_state();
        let Some(offer) = state.available(&updates::current_version()) else {
            return;
        };
        let url = offer.download.as_deref().unwrap_or(&offer.page);
        if let Err(e) = open::that_detached(url) {
            tracing::warn!("opening {url}: {e}");
        }
    }

    pub(super) fn open_update_notes(&self) {
        let state = self.update_state();
        if let Some(offer) = state.available(&updates::current_version()) {
            if let Err(e) = open::that_detached(&offer.page) {
                tracing::warn!("opening {}: {e}", offer.page);
            }
        }
    }

    pub(super) fn skip_update(&self) {
        let mut state = self.update_state();
        state.skipped = state.latest.as_ref().map(|r| r.version.clone());
        self.save_update_state(&state);
        self.show_update();
    }

    pub(super) fn dismiss_update(&self) {
        self.update_dismissed.set(true);
        self.show_update();
    }
}
