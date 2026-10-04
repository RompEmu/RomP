pub mod popups;

use crate::romm::types::{RaEarned, RomDetail, User};
use romp_proto::msg::AchievementInfo;
use serde::Deserialize;

const HOST: &str = "retroachievements.org";
const DOREQUEST: &str = "https://retroachievements.org/dorequest.php";
/// rcheevos retries requests that fail with this status, as when the server can't be reached.
const RETRYABLE: i32 = -2;
const REFUSED: i32 = -1;

const CONSOLES: &[(&[&str], u32)] = &[
    (&["genesis"], 1),
    (&["n64"], 2),
    (&["snes", "sfam"], 3),
    (&["gb"], 4),
    (&["gba"], 5),
    (&["gbc"], 6),
    (&["nes", "famicom"], 7),
    (&["tg16", "supergrafx"], 8),
    (&["segacd"], 9),
    (&["sega32"], 10),
    (&["sms"], 11),
    (&["psx"], 12),
    (&["lynx"], 13),
    (&["neo-geo-pocket", "neo-geo-pocket-color"], 14),
    (&["gamegear"], 15),
    (&["ngc"], 16),
    (&["jaguar"], 17),
    (&["nds"], 18),
    (&["wii"], 19),
    (&["ps2"], 21),
    (&["odyssey-2"], 23),
    (&["atari2600"], 25),
    (&["dos"], 26),
    (
        &["arcade", "neogeoaes", "neogeomvs", "cps1", "cps2", "cps3"],
        27,
    ),
    (&["virtualboy"], 28),
    (&["msx", "msx2", "msx2plus"], 29),
    (&["c64"], 30),
    (&["amiga"], 35),
    (&["saturn"], 39),
    (&["dc"], 40),
    (&["psp"], 41),
    (&["philips-cd-i"], 42),
    (&["3do"], 43),
    (&["colecovision"], 44),
    (&["intellivision"], 45),
    (&["vectrex"], 46),
    (&["wonderswan", "wonderswan-color"], 53),
    (&["zxs"], 59),
    (&["turbografx-cd"], 76),
    (&["fds"], 81),
];

/// RetroAchievements' number for the platform, or 0 to let it tell from the file.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Keeps an unlock that didn't reach RetroAchievements, and forgets it once one attempt gets through.
/// RetroAchievements only accepts hardcore unlocks from emulators on its approved list, which it
/// considers once an emulator has been public for six months. Until Romp is on it, hardcore is off.
pub const HARDCORE_ALLOWED: bool = false;

pub fn note_unlock(
    store: &std::sync::Mutex<crate::store::Store>,
    username: &str,
    unlock: &romp_cheevos::Unlock,
    status: i32,
    body: &[u8],
) {
    let store = store.lock().unwrap();
    match romp_cheevos::award_outcome(status, body) {
        romp_cheevos::Award::Retry => store.add_pending_unlock(username, unlock, now_millis()),
        romp_cheevos::Award::Accepted | romp_cheevos::Award::Refused => {
            store.remove_pending_unlock(username, unlock);
        }
    }
}

/// Sends unlocks earned while RetroAchievements was out of reach, oldest first, until one fails again.
pub async fn send_pending_unlocks(
    http: &reqwest::Client,
    store: &std::sync::Mutex<crate::store::Store>,
    account: &Account,
) -> usize {
    send_pending_unlocks_with(store, account, |request| async move {
        forward(
            http,
            &request.url,
            request.post.as_deref(),
            request.content_type.as_deref(),
            "",
        )
        .await
    })
    .await
}

async fn send_pending_unlocks_with<F, Fut>(
    store: &std::sync::Mutex<crate::store::Store>,
    account: &Account,
    send: F,
) -> usize
where
    F: Fn(romp_cheevos::Request) -> Fut,
    Fut: std::future::Future<Output = (i32, Vec<u8>)>,
{
    let pending = store.lock().unwrap().pending_unlocks(&account.username);
    let mut sent = 0;
    for (unlock, unlocked_at) in pending {
        let seconds = u32::try_from((now_millis() - unlocked_at).max(0) / 1000).unwrap_or(u32::MAX);
        let Some(request) =
            romp_cheevos::award_request(&account.username, &account.token, &unlock, seconds)
        else {
            store
                .lock()
                .unwrap()
                .remove_pending_unlock(&account.username, &unlock);
            continue;
        };
        let (status, body) = send(request).await;
        let outcome = romp_cheevos::award_outcome(status, &body);
        if outcome == romp_cheevos::Award::Retry {
            break;
        }
        store
            .lock()
            .unwrap()
            .remove_pending_unlock(&account.username, &unlock);
        if outcome == romp_cheevos::Award::Accepted {
            sent += 1;
        }
    }
    sent
}

/// RetroAchievements adds warnings, such as for an emulator it doesn't recognise yet, as achievements.
pub fn is_notice(id: u32) -> bool {
    id >= 101_000_000
}

/// The real achievements in a list, without RetroAchievements' notices.
pub fn without_notices(list: Vec<AchievementInfo>) -> Vec<AchievementInfo> {
    list.into_iter().filter(|a| !is_notice(a.id)).collect()
}

/// The badge to show: the coloured one once earned, the grey one before.
pub fn badge_url(a: &AchievementInfo) -> &str {
    if a.unlocked || a.badge_locked_url.is_empty() {
        &a.badge_url
    } else {
        &a.badge_locked_url
    }
}

pub fn row_detail(a: &AchievementInfo) -> String {
    let points = tr::tr!("{n} point" | "{n} points" % a.points);
    let mut parts = vec![a.description.clone(), points];
    if !a.unlocked && !a.progress.is_empty() {
        parts.push(a.progress.clone());
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

pub fn summary(list: &[AchievementInfo]) -> String {
    if list.is_empty() {
        return tr::tr!("No achievements for this game");
    }
    let earned = list.iter().filter(|a| a.unlocked).count();
    tr::tr!("{earned} of {total} earned", earned, total = list.len())
}

/// A game's achievements as RomM stores them, marked with what RomM knows the player earned.
pub fn from_romm(rom: &RomDetail, user: Option<&User>) -> Vec<AchievementInfo> {
    let earned: Vec<u32> = user
        .and_then(|u| u.ra_progression.as_ref())
        .and_then(|p| {
            p.results
                .iter()
                .find(|g| g.rom_ra_id.is_some() && g.rom_ra_id == rom.ra_id)
        })
        .map(|g| {
            g.earned_achievements
                .iter()
                .filter_map(RaEarned::achievement_id)
                .collect()
        })
        .unwrap_or_default();
    rom.merged_ra_metadata
        .as_ref()
        .map(|m| m.achievements.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            let id = u32::try_from(a.ra_id?).ok()?;
            Some(AchievementInfo {
                id,
                title: a.title.clone().unwrap_or_default(),
                description: a.description.clone().unwrap_or_default(),
                points: a.points.and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
                badge_url: a.badge_url.clone().unwrap_or_default(),
                badge_locked_url: a.badge_url_lock.clone().unwrap_or_default(),
                unlocked: earned.contains(&id),
                progress: String::new(),
            })
        })
        .collect()
}

/// A game's achievements and the player's unlocks, straight from RetroAchievements.
pub async fn from_retroachievements(
    http: &reqwest::Client,
    account: &Account,
    game_id: u32,
    hash: Option<&str>,
) -> Option<Vec<AchievementInfo>> {
    let send = |request: romp_cheevos::Request| async move {
        forward(
            http,
            &request.url,
            request.post.as_deref(),
            request.content_type.as_deref(),
            "",
        )
        .await
    };
    let request = romp_cheevos::catalog_request(&account.username, &account.token, game_id, hash)?;
    let (status, body) = send(request).await;
    let (game_id, list) = romp_cheevos::parse_catalog(status, &body)?;
    let mut list = without_notices(list);
    let request = romp_cheevos::unlocks_request(&account.username, &account.token, game_id, false)?;
    let (status, body) = send(request).await;
    let earned = romp_cheevos::parse_unlocks(status, &body).unwrap_or_default();
    for a in &mut list {
        a.unlocked = earned.contains(&a.id);
    }
    Some(list)
}

pub fn console_id(platform_slug: &str) -> u32 {
    CONSOLES
        .iter()
        .find(|(slugs, _)| slugs.contains(&platform_slug))
        .map_or(0, |(_, id)| *id)
}

fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    }
}

/// `Romp/x.y.z (OS)` followed by what the runner adds: the core and rcheevos versions.
pub fn user_agent(runner_part: &str) -> String {
    format!(
        "Romp/{} ({}) {runner_part}",
        env!("CARGO_PKG_VERSION"),
        os()
    )
    .trim()
    .to_string()
}

/// Only RetroAchievements itself is reachable through the runner's requests.
pub fn allowed(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str()
                .is_some_and(|h| h == HOST || h.ends_with(&format!(".{HOST}")))
    })
}

/// Sends a request from the runner and returns the HTTP status and body for rcheevos.
pub async fn forward(
    http: &reqwest::Client,
    url: &str,
    post: Option<&str>,
    content_type: Option<&str>,
    agent: &str,
) -> (i32, Vec<u8>) {
    if !allowed(url) {
        tracing::warn!("refused an achievements request to {url}");
        return (REFUSED, Vec::new());
    }
    let request = match post {
        Some(body) => http
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                content_type.unwrap_or("application/x-www-form-urlencoded"),
            )
            .body(body.to_string()),
        None => http.get(url),
    };
    match request
        .header(reqwest::header::USER_AGENT, user_agent(agent))
        .send()
        .await
    {
        Ok(response) => {
            let status = i32::from(response.status().as_u16());
            let body = response
                .bytes()
                .await
                .map(|b| b.to_vec())
                .unwrap_or_default();
            (status, body)
        }
        Err(e) => {
            tracing::debug!("achievements request failed: {e}");
            (RETRYABLE, Vec::new())
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Account {
    pub username: String,
    pub token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LoginReply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    user: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    error: String,
}

/// Trades a password for RetroAchievements' own sign-in token; the password is not kept.
pub async fn sign_in(
    http: &reqwest::Client,
    endpoint: &str,
    username: &str,
    password: &str,
) -> Result<Account, String> {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("r", "login2")
        .append_pair("u", username)
        .append_pair("p", password)
        .finish();
    let reply: LoginReply = http
        .post(endpoint)
        .header(reqwest::header::USER_AGENT, user_agent(""))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form)
        .send()
        .await
        .map_err(|_| tr::tr!("Could not reach RetroAchievements"))?
        .json()
        .await
        .map_err(|_| tr::tr!("RetroAchievements sent an answer RomP did not understand"))?;
    if !reply.success || reply.token.is_empty() {
        return Err(if reply.error.is_empty() {
            tr::tr!("RetroAchievements did not accept the sign-in")
        } else {
            reply.error
        });
    }
    Ok(Account {
        username: if reply.user.is_empty() {
            username.to_string()
        } else {
            reply.user
        },
        token: reply.token,
    })
}

pub fn sign_in_endpoint() -> &'static str {
    DOREQUEST
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header_regex, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn romm_lists_achievements_with_what_the_player_earned() {
        let rom: RomDetail = serde_json::from_value(serde_json::json!({
            "id": 1, "platform_slug": "snes", "fs_name": "g", "fs_path": "roms/snes",
            "has_multiple_files": false, "files": [], "ra_id": 228,
            "merged_ra_metadata": {"achievements": [
                {"ra_id": 7, "title": "Missile", "description": "Find one", "points": 5,
                 "badge_url": "https://media.retroachievements.org/Badge/1.png",
                 "badge_url_lock": "https://media.retroachievements.org/Badge/1_lock.png"},
                {"ra_id": 8, "title": "Bombs", "points": 10}
            ]}
        }))
        .unwrap();
        let user: User = serde_json::from_value(serde_json::json!({
            "username": "p", "current_device_id": null,
            "ra_progression": {"total": 1, "results": [
                {"rom_ra_id": 228, "earned_achievements": [{"id": "7", "date": "x"}]},
                {"rom_ra_id": 999, "earned_achievements": [{"id": 8}]}
            ]}
        }))
        .unwrap();
        let list = from_romm(&rom, Some(&user));
        assert_eq!(list.len(), 2);
        assert!(list[0].unlocked && !list[1].unlocked);
        assert_eq!(
            badge_url(&list[0]),
            "https://media.retroachievements.org/Badge/1.png"
        );
        assert_eq!(row_detail(&list[1]), "10 points");
        assert_eq!(summary(&list), "1 of 2 earned");
        assert!(from_romm(&rom, None).iter().all(|a| !a.unlocked));
    }

    #[tokio::test]
    async fn unlocks_made_offline_are_sent_later_with_their_age() {
        use wiremock::matchers::{body_string_contains, method};
        let store = std::sync::Mutex::new(crate::store::Store::open_in_memory().unwrap());
        let unlock = romp_cheevos::Unlock {
            achievement_id: 39673,
            hardcore: false,
            hash: "abc".into(),
        };
        note_unlock(&store, "player", &unlock, -2, b"");
        assert_eq!(store.lock().unwrap().pending_unlocks("player").len(), 1);
        let an_hour_ago = now_millis() - 3_600_000;
        store
            .lock()
            .unwrap()
            .remove_pending_unlock("player", &unlock);
        store
            .lock()
            .unwrap()
            .add_pending_unlock("player", &unlock, an_hour_ago);

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains("r=awardachievement"))
            .and(body_string_contains("a=39673"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "Success": true, "Score": 10, "SoftcoreScore": 10,
                "AchievementID": 39673, "AchievementsRemaining": 2
            })))
            .expect(1)
            .mount(&server)
            .await;
        let account = Account {
            username: "player".into(),
            token: "tok".into(),
        };
        let endpoint = format!("{}/dorequest.php", server.uri());
        let http = reqwest::Client::new();
        let sent = send_pending_unlocks_with(&store, &account, |request| {
            let (http, endpoint) = (&http, &endpoint);
            async move {
                assert!(
                    request.post.as_deref().unwrap_or_default().contains("o="),
                    "the unlock's age is sent"
                );
                let response = http
                    .post(endpoint)
                    .body(request.post.unwrap_or_default())
                    .send()
                    .await
                    .unwrap();
                let status = i32::from(response.status().as_u16());
                (status, response.bytes().await.unwrap().to_vec())
            }
        })
        .await;
        assert_eq!(sent, 1);
        assert!(store.lock().unwrap().pending_unlocks("player").is_empty());
    }

    #[test]
    fn notices_are_not_achievements() {
        let entry = |id| AchievementInfo {
            id,
            title: String::new(),
            description: String::new(),
            points: 0,
            badge_url: String::new(),
            badge_locked_url: String::new(),
            unlocked: true,
            progress: String::new(),
        };
        let list = without_notices(vec![entry(39673), entry(101_000_001)]);
        assert_eq!(list.len(), 1);
        assert_eq!(summary(&list), "1 of 1 earned");
    }

    #[test]
    fn platforms_map_to_retroachievements_consoles() {
        assert_eq!(console_id("snes"), 3);
        assert_eq!(console_id("psx"), 12);
        assert_eq!(console_id("neogeomvs"), 27);
        assert_eq!(console_id("xbox"), 0);
    }

    #[test]
    fn only_retroachievements_is_reachable() {
        assert!(allowed("https://retroachievements.org/dorequest.php"));
        assert!(allowed("https://media.retroachievements.org/Badge/1.png"));
        assert!(!allowed("http://retroachievements.org/dorequest.php"));
        assert!(!allowed("https://retroachievements.org.evil.com/x"));
        assert!(!allowed("https://example.com/?retroachievements.org"));
        assert!(!allowed("not a url"));
    }

    #[test]
    fn the_user_agent_names_romp_then_the_core() {
        let agent = user_agent("Snes9x/1.63 rcheevos/12.5");
        assert!(agent.starts_with(&format!("Romp/{} (", env!("CARGO_PKG_VERSION"))));
        assert!(agent.ends_with(") Snes9x/1.63 rcheevos/12.5"));
    }

    #[tokio::test]
    async fn signing_in_keeps_the_token_not_the_password() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains("r=login2"))
            .and(body_string_contains("u=Player"))
            .and(body_string_contains("p=hunter2"))
            .and(header_regex("user-agent", "^Romp/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "Success": true, "User": "player", "Token": "tok", "Score": 1
            })))
            .mount(&server)
            .await;
        let account = sign_in(&reqwest::Client::new(), &server.uri(), "Player", "hunter2")
            .await
            .unwrap();
        assert_eq!(
            account,
            Account {
                username: "player".into(),
                token: "tok".into()
            }
        );
    }

    #[tokio::test]
    async fn a_wrong_password_is_explained() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "Success": false, "Error": "Invalid User/Password combination. Please try again"
            })))
            .mount(&server)
            .await;
        let err = sign_in(&reqwest::Client::new(), &server.uri(), "p", "x")
            .await
            .unwrap_err();
        assert!(err.starts_with("Invalid User/Password"), "{err}");
    }
}
