use super::{on_ui, Controller};
use crate::achievements::{self, popups::Launch, Account};
use crate::credentials::{Keychain, TokenStore};
use crate::store::GameDetail;

const USER_KEY: &str = "ra_username";

impl Controller {
    pub(super) fn ra_account(&self) -> Option<Account> {
        let username = self.shared.store.lock().unwrap().get(USER_KEY)?;
        let token = Keychain::retro_achievements().load(&username)?;
        Some(Account { username, token })
    }

    /// RetroAchievements for this game, when the player is signed in and the console has achievements.
    pub(super) fn achievements_launch(&self, detail: &GameDetail) -> Option<Launch> {
        let account = self.ra_account()?;
        let hash = self
            .ra_hash
            .borrow_mut()
            .take()
            .filter(|(id, _)| *id == detail.id)
            .map(|(_, hash)| hash);
        Some(Launch {
            username: account.username,
            token: account.token,
            hardcore: false,
            console_id: achievements::console_id(&detail.platform_slug),
            hash,
            http: self.shared.http.clone(),
            rt: self.shared.rt.handle().clone(),
        })
    }

    pub(super) fn show_ra_account(&self) {
        let Some(ui) = self.ui() else { return };
        let signed_in = self.ra_account().map(|a| a.username);
        ui.set_ra_signed_in(signed_in.is_some());
        ui.set_ra_user(signed_in.unwrap_or_default().into());
        if ui.get_ra_username_input().is_empty() {
            let suggested = self.shared.store.lock().unwrap().get("romm_ra_username");
            ui.set_ra_username_input(suggested.unwrap_or_default().into());
        }
    }

    pub(super) fn ra_sign_in(&self, username: String, password: String) {
        let Some(ui) = self.ui() else { return };
        let username = username.trim().to_string();
        if username.is_empty() || password.is_empty() {
            ui.set_ra_status("Enter your RetroAchievements username and password.".into());
            return;
        }
        ui.set_ra_busy(true);
        ui.set_ra_status("Signing in…".into());
        let http = self.shared.http.clone();
        self.shared.rt.spawn(async move {
            let result = achievements::sign_in(
                &http,
                achievements::sign_in_endpoint(),
                &username,
                &password,
            )
            .await;
            on_ui(move |c| c.ra_signed_in(result));
        });
    }

    fn ra_signed_in(&self, result: Result<Account, String>) {
        let Some(ui) = self.ui() else { return };
        ui.set_ra_busy(false);
        ui.set_ra_password_input("".into());
        match result.and_then(|account| {
            Keychain::retro_achievements().save(&account.username, &account.token)?;
            self.shared
                .store
                .lock()
                .unwrap()
                .set(USER_KEY, &account.username);
            Ok(account)
        }) {
            Ok(_) => ui.set_ra_status("".into()),
            Err(e) => ui.set_ra_status(e.into()),
        }
        self.show_ra_account();
    }

    pub(super) fn ra_sign_out(&self) {
        if let Some(account) = self.ra_account() {
            Keychain::retro_achievements().delete(&account.username);
        }
        self.shared.store.lock().unwrap().remove(USER_KEY);
        if let Some(ui) = self.ui() {
            ui.set_ra_status("".into());
        }
        self.show_ra_account();
    }
}
