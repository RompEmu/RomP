use super::{on_ui, Controller};
use crate::achievements::{self, popups::Launch, Account};
use crate::credentials::{Keychain, TokenStore};
use crate::store::GameDetail;
use crate::AchievementRow;
use romp_proto::msg::AchievementInfo;
use slint::{ModelRc, VecModel};

const USER_KEY: &str = "ra_username";
const HARDCORE_KEY: &str = "ra_hardcore";

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
            hardcore: self.ra_hardcore(),
            console_id: achievements::console_id(&detail.platform_slug),
            hash,
            http: self.shared.http.clone(),
            rt: self.shared.rt.handle().clone(),
        })
    }

    /// After playing, RomM fetches the player's new RetroAchievements progress.
    pub(super) fn refresh_romm_achievements(&self) {
        if self.ra_account().is_none() || !self.has_scope("me.write") || self.offline.get() {
            return;
        }
        let user_id = self
            .shared
            .store
            .lock()
            .unwrap()
            .get("romm_user_id")
            .and_then(|id| id.parse::<i64>().ok());
        let (Some(client), Some(user_id)) = (self.client.borrow().clone(), user_id) else {
            return;
        };
        self.shared.rt.spawn(async move {
            if let Err(e) = client.refresh_retro_achievements(user_id).await {
                tracing::debug!("RomM RetroAchievements refresh: {e}");
            }
        });
    }

    pub(super) fn ra_hardcore(&self) -> bool {
        self.shared
            .store
            .lock()
            .unwrap()
            .get(HARDCORE_KEY)
            .as_deref()
            == Some("1")
    }

    pub(super) fn ra_hardcore_changed(&self) {
        let Some(ui) = self.ui() else { return };
        let on = if ui.get_ra_hardcore() { "1" } else { "0" };
        self.shared.store.lock().unwrap().set(HARDCORE_KEY, on);
    }

    pub(super) fn show_ra_account(&self) {
        let Some(ui) = self.ui() else { return };
        ui.set_ra_hardcore(self.ra_hardcore());
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
        self.update_pairing_prompt();
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
        self.update_pairing_prompt();
    }
}

const SHOWN_AT_FIRST: usize = 8;

fn cache_key(rom_id: i64) -> String {
    format!("achievements:{rom_id}")
}

fn badge_file(url: &str) -> Option<std::path::PathBuf> {
    let name = url.rsplit('/').next()?;
    let safe = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        && !name.starts_with('.');
    safe.then(|| crate::paths::data_dir().join("badges").join(name))
}

async fn save_badges(http: &reqwest::Client, list: &[AchievementInfo]) {
    for a in list {
        let url = achievements::badge_url(a);
        let Some(path) = badge_file(url).filter(|p| !p.exists()) else {
            continue;
        };
        if !achievements::allowed(url) {
            continue;
        }
        let Ok(response) = http.get(url).send().await else {
            return;
        };
        if let Ok(bytes) = response.bytes().await {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, &bytes);
        }
    }
}

impl Controller {
    /// Shows the game's achievements from the cache, then from RetroAchievements or RomM.
    pub(super) fn load_game_achievements(&self, detail: &GameDetail) {
        let id = detail.id;
        let cached = self.shared.store.lock().unwrap().get(&cache_key(id));
        let list: Vec<AchievementInfo> = cached
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        self.show_game_achievements(id, &list, false);
        let client = self.client.borrow().clone();
        let Some(client) = client.filter(|_| !self.offline.get()) else {
            return;
        };
        let account = self.ra_account();
        let http = self.shared.http.clone();
        self.shared.rt.spawn(async move {
            let Ok(rom) = client.rom_detail(id).await else {
                return;
            };
            let game_id = rom.ra_id.and_then(|g| u32::try_from(g).ok()).unwrap_or(0);
            let hash = rom.ra_hash.clone().filter(|h| !h.is_empty());
            let from_ra = match &account {
                Some(account) if game_id != 0 || hash.is_some() => {
                    achievements::from_retroachievements(&http, account, game_id, hash.as_deref())
                        .await
                }
                _ => None,
            };
            let list = match from_ra {
                Some(list) => list,
                None => {
                    let me = client.me().await.ok();
                    achievements::from_romm(&rom, me.as_ref())
                }
            };
            save_badges(&http, &list).await;
            on_ui(move |c| {
                let json = serde_json::to_string(&list).expect("achievements serialize");
                c.shared.store.lock().unwrap().set(&cache_key(id), &json);
                c.show_game_achievements(id, &list, false);
            });
        });
    }

    pub(super) fn show_all_achievements(&self) {
        let Some(id) = self.current_game().map(|g| g.id) else {
            return;
        };
        let cached = self.shared.store.lock().unwrap().get(&cache_key(id));
        let list: Vec<AchievementInfo> = cached
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        self.show_game_achievements(id, &list, true);
    }

    fn show_game_achievements(&self, id: i64, list: &[AchievementInfo], all: bool) {
        if self.current_game().map(|g| g.id) != Some(id) {
            return;
        }
        let Some(ui) = self.ui() else { return };
        let shown = if all {
            list.len()
        } else {
            list.len().min(SHOWN_AT_FIRST)
        };
        let rows: Vec<AchievementRow> = list[..shown]
            .iter()
            .map(|a| {
                let badge = badge_file(achievements::badge_url(a))
                    .and_then(|p| slint::Image::load_from_path(&p).ok());
                AchievementRow {
                    title: a.title.clone().into(),
                    detail: achievements::row_detail(a).into(),
                    has_badge: badge.is_some(),
                    badge: badge.unwrap_or_default(),
                    unlocked: a.unlocked,
                }
            })
            .collect();
        ui.set_game_achievements(ModelRc::new(VecModel::from(rows)));
        ui.set_game_achievements_summary(achievements::summary(list).into());
        ui.set_game_achievements_hidden((list.len() - shown) as i32);
    }
}
