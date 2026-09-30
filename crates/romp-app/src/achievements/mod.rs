pub mod popups;

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
        .map_err(|_| "Could not reach RetroAchievements".to_string())?
        .json()
        .await
        .map_err(|_| "RetroAchievements sent an answer Romp did not understand".to_string())?;
    if !reply.success || reply.token.is_empty() {
        return Err(if reply.error.is_empty() {
            "RetroAchievements did not accept the sign-in".into()
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
