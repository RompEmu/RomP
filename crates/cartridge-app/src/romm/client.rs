use crate::romm::types::{DeviceAuth, Heartbeat, Platform, PollOutcome, RomPage, User};
use serde::de::DeserializeOwned;
use std::time::Duration;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach the server")]
    Unreachable,
    #[error("this device is not signed in to the server")]
    Unauthorized,
    #[error("the server answered with status {0}")]
    Status(u16),
    #[error("unexpected response from the server: {0}")]
    Decode(String),
}

pub fn server_candidates(input: &str) -> Result<Vec<Url>, String> {
    let input = input.trim().trim_end_matches('/');
    if input.is_empty() {
        return Err("Enter your RomM server address.".into());
    }
    let raw: Vec<String> = if input.contains("://") {
        vec![input.to_string()]
    } else {
        vec![format!("https://{input}"), format!("http://{input}")]
    };
    raw.iter()
        .map(|s| {
            let invalid = || format!("\"{input}\" is not a valid address.");
            let mut url = Url::parse(s).map_err(|_| invalid())?;
            if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
                return Err(invalid());
            }
            if !url.path().ends_with('/') {
                let path = format!("{}/", url.path());
                url.set_path(&path);
            }
            Ok(url)
        })
        .collect()
}

pub fn check_version(version: &str) -> Result<(), String> {
    if version == "development" {
        return Ok(());
    }
    match version
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok())
    {
        Some(major) if major >= 5 => Ok(()),
        Some(_) => Err(format!(
            "This server runs RomM {version}. Cartridge needs RomM 5.0 or newer."
        )),
        None => Err(format!("Unrecognised RomM version \"{version}\".")),
    }
}

#[derive(Clone)]
pub struct Client {
    base: Url,
    http: reqwest::Client,
    token: Option<String>,
}

impl Client {
    pub fn new(base: Url) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Self {
            base,
            http,
            token: None,
        }
    }

    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    pub fn url(&self, path: &str) -> Url {
        self.base
            .join(path.trim_start_matches('/'))
            .expect("relative url")
    }

    pub async fn heartbeat(&self) -> Result<Heartbeat, Error> {
        self.get_json("/api/heartbeat", &[]).await
    }

    pub async fn device_init(&self, device_id: &str, name: &str) -> Result<DeviceAuth, Error> {
        let body = serde_json::json!({
            "client_device_identifier": device_id,
            "name": name,
            "client": "Cartridge",
            "platform": std::env::consts::OS,
            "client_version": env!("CARGO_PKG_VERSION"),
            "requested_scopes": crate::romm::pairing::SCOPES,
        });
        let resp = self
            .send(
                self.request(reqwest::Method::POST, "/api/auth/device/init")
                    .json(&body),
            )
            .await?;
        resp.json().await.map_err(|e| Error::Decode(e.to_string()))
    }

    pub async fn device_token(&self, device_code: &str) -> Result<PollOutcome, Error> {
        #[derive(serde::Deserialize)]
        struct Token {
            access_token: String,
        }
        #[derive(serde::Deserialize)]
        struct Detail {
            detail: String,
        }
        let resp = self
            .request(reqwest::Method::POST, "/api/auth/device/token")
            .json(&serde_json::json!({ "device_code": device_code }))
            .send()
            .await
            .map_err(|_| Error::Unreachable)?;
        let decode = |e: reqwest::Error| Error::Decode(e.to_string());
        match resp.status().as_u16() {
            200 => Ok(PollOutcome::Approved(
                resp.json::<Token>().await.map_err(decode)?.access_token,
            )),
            429 => Ok(PollOutcome::SlowDown),
            400 => Ok(
                match resp.json::<Detail>().await.map_err(decode)?.detail.as_str() {
                    "authorization_pending" => PollOutcome::Pending,
                    "slow_down" => PollOutcome::SlowDown,
                    "access_denied" => PollOutcome::Denied,
                    _ => PollOutcome::Expired,
                },
            ),
            status => Err(Error::Status(status)),
        }
    }

    pub async fn me(&self) -> Result<User, Error> {
        self.get_json("/api/users/me", &[]).await
    }

    pub async fn platforms(&self) -> Result<Vec<Platform>, Error> {
        self.get_json("/api/platforms", &[]).await
    }

    pub async fn roms_page(
        &self,
        offset: i64,
        limit: i64,
        updated_after: Option<&str>,
    ) -> Result<RomPage, Error> {
        let mut query = vec![
            ("offset", offset.to_string()),
            ("limit", limit.to_string()),
            ("order_by", "id".to_string()),
            ("order_dir", "asc".to_string()),
            ("with_char_index", "false".to_string()),
            ("with_filter_values", "false".to_string()),
            ("with_rom_id_index", "false".to_string()),
        ];
        if let Some(since) = updated_after {
            query.push(("updated_after", since.to_string()));
        }
        self.get_json("/api/roms", &query).await
    }

    pub async fn rom_ids(&self) -> Result<Vec<i64>, Error> {
        self.get_json("/api/roms/identifiers", &[]).await
    }

    pub async fn fetch_bytes(&self, path: &str) -> Result<Vec<u8>, Error> {
        let resp = self.send(self.request(reqwest::Method::GET, path)).await?;
        Ok(resp.bytes().await.map_err(|_| Error::Unreachable)?.to_vec())
    }

    pub(crate) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.http.request(method, self.url(path));
        match &self.token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }

    pub(crate) async fn send(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, Error> {
        let resp = req.send().await.map_err(|e| {
            tracing::debug!("request failed: {e:?}");
            Error::Unreachable
        })?;
        match resp.status().as_u16() {
            200..=299 => Ok(resp),
            401 => Err(Error::Unauthorized),
            status => Err(Error::Status(status)),
        }
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, Error> {
        let resp = self
            .send(self.request(reqwest::Method::GET, path).query(query))
            .await?;
        resp.json().await.map_err(|e| Error::Decode(e.to_string()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use wiremock::matchers::{
        body_json, header, method, path, query_param, query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) fn base_of(server: &MockServer, sub: &str) -> Url {
        Url::parse(&format!("{}{sub}", server.uri())).unwrap()
    }

    #[test]
    fn server_candidates_add_schemes_and_trailing_slash() {
        let c = server_candidates(" romm.tvpc.home ").unwrap();
        let c: Vec<_> = c.iter().map(Url::as_str).collect();
        assert_eq!(c, ["https://romm.tvpc.home/", "http://romm.tvpc.home/"]);
        let c = server_candidates("http://host:8080/romm").unwrap();
        assert_eq!(c[0].as_str(), "http://host:8080/romm/");
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn server_candidates_reject_garbage() {
        assert!(server_candidates("").is_err());
        assert!(server_candidates("ftp://host").is_err());
    }

    #[test]
    fn version_check() {
        assert!(check_version("5.3.1").is_ok());
        assert!(check_version("5.0.0-beta.1").is_ok());
        assert!(check_version("development").is_ok());
        assert!(check_version("4.9.2").is_err());
        assert!(check_version("nonsense").is_err());
    }

    #[test]
    fn urls_resolve_under_a_sub_path() {
        let client = Client::new(Url::parse("https://host/romm/").unwrap());
        assert_eq!(
            client.url("/api/heartbeat").as_str(),
            "https://host/romm/api/heartbeat"
        );
        assert_eq!(
            client
                .url("/assets/romm/resources/roms/1/2/cover/small.png?ts=2026-09-25 09:38:20")
                .as_str(),
            "https://host/romm/assets/romm/resources/roms/1/2/cover/small.png?ts=2026-09-25%2009:38:20"
        );
    }

    #[tokio::test]
    async fn heartbeat_reads_version() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/heartbeat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "SYSTEM": {"VERSION": "5.3.1", "GIT_BRANCH": null, "SHOW_SETUP_WIZARD": false},
                "FRONTEND": {"DISABLE_USERPASS_LOGIN": false}
            })))
            .mount(&server)
            .await;
        let hb = Client::new(base_of(&server, "/"))
            .heartbeat()
            .await
            .unwrap();
        assert_eq!(hb.system.version, "5.3.1");
    }

    #[tokio::test]
    async fn unreachable_server_is_reported() {
        let client = Client::new(Url::parse("http://127.0.0.1:9/").unwrap());
        assert!(matches!(client.heartbeat().await, Err(Error::Unreachable)));
    }

    #[tokio::test]
    async fn device_init_sends_identity_and_scopes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth/device/init"))
            .and(body_json(serde_json::json!({
                "client_device_identifier": "dev-1",
                "name": "Cartridge on test",
                "client": "Cartridge",
                "platform": std::env::consts::OS,
                "client_version": env!("CARGO_PKG_VERSION"),
                "requested_scopes": crate::romm::pairing::SCOPES,
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "device_code": "d".repeat(64), "user_code": "FDF64KC5",
                "verification_path": "/pair/device",
                "verification_path_complete": "/pair/device?user_code=FDF64KC5",
                "expires_in": 600, "interval": 5
            })))
            .mount(&server)
            .await;
        let auth = Client::new(base_of(&server, "/"))
            .device_init("dev-1", "Cartridge on test")
            .await
            .unwrap();
        assert_eq!(auth.user_code, "FDF64KC5");
        assert_eq!((auth.expires_in, auth.interval), (600, 5));
    }

    async fn poll_with(status: u16, body: serde_json::Value) -> PollOutcome {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/auth/device/token"))
            .and(body_json(serde_json::json!({"device_code": "abc"})))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        Client::new(base_of(&server, "/"))
            .device_token("abc")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn device_token_outcomes() {
        use serde_json::json;
        assert_eq!(
            poll_with(400, json!({"detail": "authorization_pending"})).await,
            PollOutcome::Pending
        );
        assert_eq!(
            poll_with(400, json!({"detail": "slow_down"})).await,
            PollOutcome::SlowDown
        );
        assert_eq!(
            poll_with(429, json!({"detail": "Too many polling attempts."})).await,
            PollOutcome::SlowDown
        );
        assert_eq!(
            poll_with(400, json!({"detail": "access_denied"})).await,
            PollOutcome::Denied
        );
        assert_eq!(
            poll_with(400, json!({"detail": "expired_token"})).await,
            PollOutcome::Expired
        );
        assert_eq!(
            poll_with(
                200,
                json!({"access_token": "rmm_abc", "device_id": "u", "scopes": [], "expires_at": null})
            )
            .await,
            PollOutcome::Approved("rmm_abc".into())
        );
    }

    fn authed(server: &MockServer) -> Client {
        Client::new(base_of(server, "/")).with_token("rmm_test".into())
    }

    #[tokio::test]
    async fn me_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/users/me"))
            .and(header("authorization", "Bearer rmm_test"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"id": 1, "username": "beshr", "role": "admin"}),
                ),
            )
            .mount(&server)
            .await;
        assert_eq!(authed(&server).me().await.unwrap().username, "beshr");
    }

    #[tokio::test]
    async fn unauthorized_maps_to_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/users/me"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({"detail": "Not authenticated"})),
            )
            .mount(&server)
            .await;
        assert!(matches!(
            authed(&server).me().await,
            Err(Error::Unauthorized)
        ));
    }

    #[tokio::test]
    async fn roms_page_requests_light_pages_in_id_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param("offset", "500"))
            .and(query_param("limit", "500"))
            .and(query_param("order_by", "id"))
            .and(query_param("order_dir", "asc"))
            .and(query_param("with_char_index", "false"))
            .and(query_param("with_filter_values", "false"))
            .and(query_param("with_rom_id_index", "false"))
            .and(query_param("updated_after", "2026-09-25T09:38:20+00:00"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": 4596, "platform_id": 47, "name": "Tir Na Nog", "fs_name": "Tir Na Nog.tzx",
                    "summary": "s", "updated_at": "2026-09-25T09:38:20+00:00",
                    "path_cover_small": "/assets/romm/resources/roms/47/4596/cover/small.png?ts=2026-09-25 09:38:20",
                    "path_cover_large": "", "fs_size_bytes": 46857, "files": []
                }],
                "total": 4596, "limit": 500, "offset": 500, "char_index": {}, "rom_id_index": [], "filter_values": {}
            })))
            .mount(&server)
            .await;
        let page = authed(&server)
            .roms_page(500, 500, Some("2026-09-25T09:38:20+00:00"))
            .await
            .unwrap();
        assert_eq!(page.items[0].id, 4596);
    }

    #[tokio::test]
    async fn roms_page_without_since_omits_the_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param_is_missing("updated_after"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"items": [], "total": 0})),
            )
            .mount(&server)
            .await;
        assert!(authed(&server)
            .roms_page(0, 500, None)
            .await
            .unwrap()
            .items
            .is_empty());
    }

    #[tokio::test]
    async fn platforms_and_ids() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/platforms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": 47, "slug": "zxs", "display_name": "ZX Spectrum", "rom_count": 3, "name": "ZX Spectrum"}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/roms/identifiers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([1, 2, 3])))
            .mount(&server)
            .await;
        let client = authed(&server);
        assert_eq!(
            client.platforms().await.unwrap()[0].display_name,
            "ZX Spectrum"
        );
        assert_eq!(client.rom_ids().await.unwrap(), vec![1, 2, 3]);
    }
}
