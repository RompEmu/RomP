use crate::romm::types::{
    ClientSave, DeviceAuth, Firmware, Heartbeat, Negotiation, Platform, PollOutcome,
    RemoteCollection, RemoteSave, RemoteState, RomDetail, RomFile, RomPage, User,
};
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
    #[error("the server has a newer version")]
    Conflict,
    #[error("cancelled")]
    Cancelled,
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
            "This server runs RomM {version}. Romp needs RomM 5.0 or newer."
        )),
        None => Err(format!("Unrecognised RomM version \"{version}\".")),
    }
}

pub struct SaveUpload<'a> {
    pub rom_id: i64,
    pub slot: &'a str,
    pub emulator: &'a str,
    pub device_id: &'a str,
    pub session_id: Option<i64>,
    pub overwrite: bool,
    pub file_name: &'a str,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct Client {
    base: Url,
    http: reqwest::Client,
    transfer: reqwest::Client,
    token: Option<String>,
}

impl Client {
    pub fn new(base: Url) -> Self {
        Self::with_request_timeout(base, Duration::from_secs(30))
    }

    pub(crate) fn with_request_timeout(base: Url, timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(timeout)
            .build()
            .expect("http client");
        let transfer = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Self {
            base,
            http,
            transfer,
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

    pub async fn server_time(&self) -> Option<std::time::SystemTime> {
        let resp = self
            .send(self.request(reqwest::Method::GET, "/api/heartbeat"))
            .await
            .ok()?;
        let date = resp.headers().get(reqwest::header::DATE)?.to_str().ok()?;
        httpdate::parse_http_date(date).ok()
    }

    pub async fn device_init(&self, device_id: &str, name: &str) -> Result<DeviceAuth, Error> {
        let body = serde_json::json!({
            "client_device_identifier": device_id,
            "name": name,
            "client": "Romp",
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
            #[serde(default)]
            scopes: Vec<String>,
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
            200 => {
                let token = resp.json::<Token>().await.map_err(decode)?;
                Ok(PollOutcome::Approved {
                    token: token.access_token,
                    scopes: token.scopes,
                })
            }
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

    pub async fn played_roms(&self, offset: i64, limit: i64) -> Result<RomPage, Error> {
        let query = [
            ("last_played", "true".to_string()),
            ("offset", offset.to_string()),
            ("limit", limit.to_string()),
            ("order_by", "id".to_string()),
            ("with_char_index", "false".to_string()),
            ("with_filter_values", "false".to_string()),
            ("with_rom_id_index", "false".to_string()),
        ];
        self.get_json("/api/roms", &query).await
    }

    pub async fn mark_played(&self, id: i64) -> Result<(), Error> {
        self.send(
            self.request(reqwest::Method::PUT, &format!("/api/roms/{id}/props"))
                .query(&[("update_last_played", "true")])
                .json(&serde_json::json!({})),
        )
        .await
        .map(|_| ())
    }

    pub async fn similar(&self, id: i64, limit: u32) -> Result<Vec<i64>, Error> {
        #[derive(serde::Deserialize)]
        struct Similar {
            rom: RomRef,
        }
        #[derive(serde::Deserialize)]
        struct RomRef {
            id: i64,
        }
        let found: Vec<Similar> = self
            .get_json(
                &format!("/api/roms/{id}/similar"),
                &[("limit", limit.to_string())],
            )
            .await?;
        Ok(found.into_iter().map(|s| s.rom.id).collect())
    }

    pub async fn collections(&self) -> Result<Vec<RemoteCollection>, Error> {
        self.get_json("/api/collections", &[]).await
    }

    pub async fn smart_collections(&self) -> Result<Vec<RemoteCollection>, Error> {
        self.get_json("/api/collections/smart", &[]).await
    }

    pub async fn virtual_collections(&self, kind: &str) -> Result<Vec<RemoteCollection>, Error> {
        self.get_json("/api/collections/virtual", &[("type", kind.to_string())])
            .await
    }

    pub async fn create_collection(
        &self,
        name: &str,
        favorite: bool,
    ) -> Result<RemoteCollection, Error> {
        let form = reqwest::multipart::Form::new().text("name", name.to_string());
        let resp = self
            .send(
                self.request(reqwest::Method::POST, "/api/collections")
                    .query(&[
                        ("is_public", "false"),
                        ("is_favorite", if favorite { "true" } else { "false" }),
                    ])
                    .multipart(form),
            )
            .await?;
        resp.json().await.map_err(|e| Error::Decode(e.to_string()))
    }

    pub async fn set_collection_member(
        &self,
        id: i64,
        rom_id: i64,
        member: bool,
    ) -> Result<(), Error> {
        let method = if member {
            reqwest::Method::POST
        } else {
            reqwest::Method::DELETE
        };
        self.send(
            self.request(method, &format!("/api/collections/{id}/roms"))
                .json(&serde_json::json!({ "rom_ids": [rom_id] })),
        )
        .await
        .map(|_| ())
    }

    pub async fn rename_collection(&self, id: i64, name: &str) -> Result<(), Error> {
        let current: RemoteCollection = self
            .get_json(&format!("/api/collections/{id}"), &[])
            .await?;
        let rom_ids = serde_json::to_string(&current.rom_ids).expect("ids serialize");
        let form = reqwest::multipart::Form::new()
            .text("rom_ids", rom_ids)
            .text("name", name.to_string());
        self.send(
            self.request(reqwest::Method::PUT, &format!("/api/collections/{id}"))
                .multipart(form),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete_collection(&self, id: i64) -> Result<(), Error> {
        self.send(self.request(reqwest::Method::DELETE, &format!("/api/collections/{id}")))
            .await
            .map(|_| ())
    }

    pub async fn avatar(&self, user_id: i64) -> Result<Vec<u8>, Error> {
        self.fetch_bytes(&format!("/api/users/{user_id}/avatar"))
            .await
    }

    pub async fn rom_ids(&self) -> Result<Vec<i64>, Error> {
        self.get_json("/api/roms/identifiers", &[]).await
    }

    pub async fn fetch_bytes(&self, path: &str) -> Result<Vec<u8>, Error> {
        let resp = self.send(self.request(reqwest::Method::GET, path)).await?;
        Ok(resp.bytes().await.map_err(|_| Error::Unreachable)?.to_vec())
    }

    pub async fn rom_detail(&self, id: i64) -> Result<RomDetail, Error> {
        self.get_json(&format!("/api/roms/{id}"), &[]).await
    }

    pub async fn firmware(&self, platform_id: Option<i64>) -> Result<Vec<Firmware>, Error> {
        let query: Vec<_> = platform_id
            .map(|id| ("platform_id", id.to_string()))
            .into_iter()
            .collect();
        self.get_json("/api/firmware", &query).await
    }

    fn content_url(&self, prefix: &str, file_name: &str) -> Url {
        let mut url = self.url(prefix);
        url.path_segments_mut()
            .expect("http url")
            .pop_if_empty()
            .push(file_name);
        url
    }

    pub fn rom_file_url(&self, rom_id: i64, file: &RomFile) -> Url {
        let mut url = self.content_url(&format!("/api/roms/{rom_id}/content/"), &file.file_name);
        url.query_pairs_mut()
            .append_pair("file_ids", &file.id.to_string());
        url
    }

    pub fn firmware_url(&self, firmware: &Firmware) -> Url {
        self.content_url(
            &format!("/api/firmware/{}/content/", firmware.id),
            &firmware.file_name,
        )
    }

    pub async fn negotiate(
        &self,
        device_id: &str,
        saves: &[ClientSave],
        rom_ids: &[i64],
    ) -> Result<Negotiation, Error> {
        let body =
            serde_json::json!({ "device_id": device_id, "saves": saves, "rom_ids": rom_ids });
        let resp = self
            .send(
                self.request(reqwest::Method::POST, "/api/sync/negotiate")
                    .json(&body),
            )
            .await?;
        resp.json().await.map_err(|e| Error::Decode(e.to_string()))
    }

    pub async fn complete_session(
        &self,
        session_id: i64,
        completed: u32,
        failed: u32,
    ) -> Result<(), Error> {
        let body =
            serde_json::json!({ "operations_completed": completed, "operations_failed": failed });
        self.send(
            self.request(
                reqwest::Method::POST,
                &format!("/api/sync/sessions/{session_id}/complete"),
            )
            .json(&body),
        )
        .await
        .map(|_| ())
    }

    pub async fn list_saves(
        &self,
        rom_id: i64,
        slot: &str,
        device_id: &str,
    ) -> Result<Vec<RemoteSave>, Error> {
        self.get_json(
            "/api/saves",
            &[
                ("rom_id", rom_id.to_string()),
                ("slot", slot.to_string()),
                ("device_id", device_id.to_string()),
            ],
        )
        .await
    }

    pub async fn upload_save(&self, upload: SaveUpload<'_>) -> Result<RemoteSave, Error> {
        let mut query = vec![
            ("rom_id", upload.rom_id.to_string()),
            ("slot", upload.slot.to_string()),
            ("emulator", upload.emulator.to_string()),
            ("device_id", upload.device_id.to_string()),
            ("overwrite", upload.overwrite.to_string()),
        ];
        if let Some(session) = upload.session_id {
            query.push(("session_id", session.to_string()));
        }
        let part =
            reqwest::multipart::Part::bytes(upload.bytes).file_name(upload.file_name.to_string());
        let form = reqwest::multipart::Form::new().part("saveFile", part);
        let resp = self
            .send(
                self.request(reqwest::Method::POST, "/api/saves")
                    .query(&query)
                    .multipart(form),
            )
            .await?;
        resp.json().await.map_err(|e| Error::Decode(e.to_string()))
    }

    pub async fn download_save(
        &self,
        save_id: i64,
        device_id: &str,
        session_id: Option<i64>,
    ) -> Result<Vec<u8>, Error> {
        let mut query = vec![
            ("device_id", device_id.to_string()),
            ("optimistic", "true".to_string()),
        ];
        if let Some(session) = session_id {
            query.push(("session_id", session.to_string()));
        }
        let resp = self
            .send(
                self.request(
                    reqwest::Method::GET,
                    &format!("/api/saves/{save_id}/content"),
                )
                .query(&query),
            )
            .await?;
        Ok(resp.bytes().await.map_err(|_| Error::Unreachable)?.to_vec())
    }

    pub async fn states(&self, rom_id: i64) -> Result<Vec<RemoteState>, Error> {
        self.get_json("/api/states", &[("rom_id", rom_id.to_string())])
            .await
    }

    pub async fn upload_state(
        &self,
        rom_id: i64,
        emulator: &str,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<RemoteState, Error> {
        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name.to_string());
        let form = reqwest::multipart::Form::new().part("stateFile", part);
        let resp = self
            .send(
                self.request(reqwest::Method::POST, "/api/states")
                    .query(&[
                        ("rom_id", rom_id.to_string()),
                        ("emulator", emulator.to_string()),
                    ])
                    .multipart(form),
            )
            .await?;
        resp.json().await.map_err(|e| Error::Decode(e.to_string()))
    }

    pub async fn download_state(&self, id: i64) -> Result<Vec<u8>, Error> {
        self.fetch_bytes(&format!("/api/states/{id}/content")).await
    }

    pub async fn download(&self, url: Url, offset: u64) -> Result<reqwest::Response, Error> {
        let mut req = self.transfer.get(url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        if offset > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        self.send(req).await
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
            409 => Err(Error::Conflict),
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
        body_json, body_string_contains, header, header_regex, method, path, query_param,
        query_param_is_missing,
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
                "name": "Romp on test",
                "client": "Romp",
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
            .device_init("dev-1", "Romp on test")
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
                json!({"access_token": "rmm_abc", "device_id": "u", "scopes": ["assets.read"], "expires_at": null})
            )
            .await,
            PollOutcome::Approved {
                token: "rmm_abc".into(),
                scopes: vec!["assets.read".into()]
            }
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
    async fn played_roms_asks_for_games_with_a_last_played_time() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms"))
            .and(query_param("last_played", "true"))
            .and(query_param("offset", "0"))
            .and(query_param("limit", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": 7, "platform_id": 1, "name": "A", "fs_name": "a.md", "summary": null,
                    "updated_at": "t", "path_cover_small": null, "path_cover_large": null,
                    "fs_size_bytes": 1, "rom_user": {"last_played": "2026-09-01T10:00:00+00:00"}
                }]
            })))
            .mount(&server)
            .await;
        let page = authed(&server).played_roms(0, 100).await.unwrap();
        assert_eq!(
            page.items[0].last_played(),
            crate::sync::parse_iso("2026-09-01T10:00:00Z")
        );
    }

    #[tokio::test]
    async fn avatar_downloads_the_users_picture() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/users/3/avatar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"png".to_vec()))
            .mount(&server)
            .await;
        assert_eq!(authed(&server).avatar(3).await.unwrap(), b"png");
    }

    #[tokio::test]
    async fn mark_played_updates_last_played_on_the_server() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/api/roms/7/props"))
            .and(query_param("update_last_played", "true"))
            .and(body_json(serde_json::json!({})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        authed(&server).mark_played(7).await.unwrap();
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

    #[tokio::test]
    async fn similar_returns_library_rom_ids_in_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/7/similar"))
            .and(query_param("limit", "12"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"rom": {"id": 9, "name": "A"}, "score": 3.5, "reasons": [{"facet": "genre", "value": "Shooter"}]},
                {"rom": {"id": 4, "name": "B"}, "score": 1.0, "reasons": []}
            ])))
            .mount(&server)
            .await;
        assert_eq!(authed(&server).similar(7, 12).await.unwrap(), [9, 4]);
    }

    #[tokio::test]
    async fn collection_lists() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/collections/virtual"))
            .and(query_param("type", "genre"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": "Zz==", "name": "Shooter", "type": "genre", "rom_ids": [1]}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/collections/smart"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": 2, "name": "Unplayed", "rom_ids": [1, 2], "is_smart": true, "user_id": 1}
            ])))
            .mount(&server)
            .await;
        let client = authed(&server);
        assert_eq!(
            client.virtual_collections("genre").await.unwrap()[0].name,
            "Shooter"
        );
        assert!(client.smart_collections().await.unwrap()[0].is_smart);
    }

    #[tokio::test]
    async fn creating_and_editing_collections() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/collections"))
            .and(query_param("is_favorite", "true"))
            .and(query_param("is_public", "false"))
            .and(body_string_contains("name=\"name\"\r\n\r\nFavourites"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(
                {"id": 7, "name": "Favourites", "rom_ids": [], "is_favorite": true, "user_id": 1}
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/collections/7/roms"))
            .and(body_json(serde_json::json!({"rom_ids": [42]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/api/collections/7/roms"))
            .and(body_json(serde_json::json!({"rom_ids": [42]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/collections/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(
                {"id": 7, "name": "Old", "rom_ids": [3, 4]}
            )))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/api/collections/7"))
            .and(body_string_contains("[3,4]"))
            .and(body_string_contains("RPGs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/api/collections/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let client = authed(&server);
        let created = client.create_collection("Favourites", true).await.unwrap();
        assert_eq!(created.id.to_string(), "7");
        client.set_collection_member(7, 42, true).await.unwrap();
        client.set_collection_member(7, 42, false).await.unwrap();
        client.rename_collection(7, "RPGs").await.unwrap();
        client.delete_collection(7).await.unwrap();
    }

    #[test]
    fn content_urls_encode_names_as_one_segment() {
        let client = Client::new(Url::parse("https://host/romm/").unwrap());
        let f = RomFile {
            id: 3705,
            file_name: "Arc (Disc 1) #1?.chd".into(),
            file_path: String::new(),
            file_size_bytes: 0,
            sha1_hash: None,
        };
        assert_eq!(
            client.rom_file_url(3672, &f).as_str(),
            "https://host/romm/api/roms/3672/content/Arc%20(Disc%201)%20%231%3F.chd?file_ids=3705"
        );
        let fw = Firmware {
            id: 81,
            file_name: "scph5501.bin".into(),
            sha1_hash: None,
        };
        assert_eq!(
            client.firmware_url(&fw).as_str(),
            "https://host/romm/api/firmware/81/content/scph5501.bin"
        );
    }

    #[tokio::test]
    async fn rom_detail_and_firmware() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/roms/3672"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": 3672, "platform_id": 32, "platform_slug": "psx", "fs_name": "Arc III", "fs_path": "roms/psx",
                "has_multiple_files": true, "files": [{"id": 3705, "file_name": "Disc 1.chd", "file_path": "roms/psx/Arc III",
                "file_size_bytes": 10, "sha1_hash": "ab", "category": "game"}]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/firmware"))
            .and(query_param("platform_id", "32"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": 81, "file_name": "scph5501.bin", "file_path": "bios/psx", "file_size_bytes": 524288, "sha1_hash": null}])))
            .mount(&server)
            .await;
        let client = authed(&server);
        assert_eq!(client.rom_detail(3672).await.unwrap().files[0].id, 3705);
        assert_eq!(
            client.firmware(Some(32)).await.unwrap()[0].file_name,
            "scph5501.bin"
        );
    }

    #[tokio::test]
    async fn downloads_are_not_limited_by_the_request_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/big"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("data")
                    .set_delay(Duration::from_millis(1500)),
            )
            .mount(&server)
            .await;
        let client =
            Client::with_request_timeout(base_of(&server, "/"), Duration::from_millis(500));
        assert!(client
            .get_json::<serde_json::Value>("/big", &[])
            .await
            .is_err());
        let resp = client.download(client.url("/big"), 0).await.unwrap();
        assert_eq!(resp.text().await.unwrap(), "data");
    }

    fn save_json(id: i64) -> serde_json::Value {
        serde_json::json!({"id": id, "rom_id": 5, "file_name": "Zelda [2026-09-26_10-00-00].srm",
            "slot": "autosave", "updated_at": "2026-09-26T10:00:00+00:00", "content_hash": "abc",
            "emulator": "snes9x", "device_syncs": []})
    }

    #[tokio::test]
    async fn me_reads_current_device() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/users/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"id": 1, "username": "b", "current_device_id": "dev-uuid"}),
            ))
            .mount(&server)
            .await;
        assert_eq!(
            authed(&server)
                .me()
                .await
                .unwrap()
                .current_device_id
                .as_deref(),
            Some("dev-uuid")
        );
    }

    #[tokio::test]
    async fn negotiate_sends_device_and_saves() {
        let server = MockServer::start().await;
        let save = ClientSave {
            rom_id: 5,
            file_name: "Zelda.srm".into(),
            slot: Some("autosave".into()),
            emulator: Some("snes9x".into()),
            content_hash: Some("abc".into()),
            updated_at: "2026-09-26T10:00:00+00:00".into(),
            file_size_bytes: 8192,
        };
        Mock::given(method("POST"))
            .and(path("/api/sync/negotiate"))
            .and(body_json(serde_json::json!({
                "device_id": "dev", "rom_ids": [5],
                "saves": [{"rom_id": 5, "file_name": "Zelda.srm", "slot": "autosave", "emulator": "snes9x",
                           "content_hash": "abc", "updated_at": "2026-09-26T10:00:00+00:00", "file_size_bytes": 8192}]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": 9, "operations": [{"action": "upload", "rom_id": 5, "save_id": null,
                "file_name": "Zelda.srm", "slot": "autosave", "reason": "x"}], "total_upload": 1})))
            .mount(&server)
            .await;
        let n = authed(&server)
            .negotiate("dev", &[save], &[5])
            .await
            .unwrap();
        assert_eq!(n.session_id, 9);
        assert_eq!(n.operations[0].action, "upload");
        Mock::given(method("POST"))
            .and(path("/api/sync/sessions/9/complete"))
            .and(body_json(
                serde_json::json!({"operations_completed": 1, "operations_failed": 0}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(1)
            .mount(&server)
            .await;
        authed(&server).complete_session(9, 1, 0).await.unwrap();
    }

    #[tokio::test]
    async fn upload_save_puts_metadata_in_query_and_file_in_multipart() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/saves"))
            .and(query_param("rom_id", "5"))
            .and(query_param("slot", "autosave"))
            .and(query_param("emulator", "snes9x"))
            .and(query_param("device_id", "dev"))
            .and(query_param("session_id", "9"))
            .and(query_param("overwrite", "false"))
            .and(header_regex("content-type", "^multipart/form-data"))
            .and(body_string_contains(
                "name=\"saveFile\"; filename=\"Zelda.srm\"",
            ))
            .and(body_string_contains("SRAMDATA"))
            .respond_with(ResponseTemplate::new(200).set_body_json(save_json(1)))
            .mount(&server)
            .await;
        let saved = authed(&server)
            .upload_save(SaveUpload {
                rom_id: 5,
                slot: "autosave",
                emulator: "snes9x",
                device_id: "dev",
                session_id: Some(9),
                overwrite: false,
                file_name: "Zelda.srm",
                bytes: b"SRAMDATA".to_vec(),
            })
            .await
            .unwrap();
        assert_eq!(saved.id, 1);
    }

    #[tokio::test]
    async fn upload_409_is_conflict() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/saves"))
            .respond_with(ResponseTemplate::new(409).set_body_json(
                serde_json::json!({"detail": "Slot has a newer save since your last sync"}),
            ))
            .mount(&server)
            .await;
        let err = authed(&server)
            .upload_save(SaveUpload {
                rom_id: 5,
                slot: "autosave",
                emulator: "snes9x",
                device_id: "dev",
                session_id: None,
                overwrite: false,
                file_name: "Zelda.srm",
                bytes: vec![1],
            })
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Conflict));
    }

    #[tokio::test]
    async fn download_and_list_saves_pass_device() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/saves/1/content"))
            .and(query_param("device_id", "dev"))
            .and(query_param("optimistic", "true"))
            .and(query_param("session_id", "9"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"SRAM".to_vec()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/saves"))
            .and(query_param("rom_id", "5"))
            .and(query_param("slot", "autosave"))
            .and(query_param("device_id", "dev"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([save_json(1)])),
            )
            .mount(&server)
            .await;
        let client = authed(&server);
        assert_eq!(
            client.download_save(1, "dev", Some(9)).await.unwrap(),
            b"SRAM"
        );
        assert_eq!(
            client.list_saves(5, "autosave", "dev").await.unwrap()[0].id,
            1
        );
    }

    #[tokio::test]
    async fn states_list_upload_download() {
        let server = MockServer::start().await;
        let state = serde_json::json!({"id": 3, "rom_id": 5, "file_name": "slot-1.snes9x.1.63.state",
            "updated_at": "2026-09-26T10:00:00+00:00", "emulator": "snes9x"});
        Mock::given(method("GET"))
            .and(path("/api/states"))
            .and(query_param("rom_id", "5"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([state.clone()])),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/states"))
            .and(query_param("rom_id", "5"))
            .and(query_param("emulator", "snes9x"))
            .and(body_string_contains(
                "name=\"stateFile\"; filename=\"slot-1.snes9x.1.63.state\"",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(state))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/states/3/content"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"STATE".to_vec()))
            .mount(&server)
            .await;
        let client = authed(&server);
        assert_eq!(client.states(5).await.unwrap()[0].id, 3);
        assert_eq!(
            client
                .upload_state(5, "snes9x", "slot-1.snes9x.1.63.state", b"S".to_vec())
                .await
                .unwrap()
                .id,
            3
        );
        assert_eq!(client.download_state(3).await.unwrap(), b"STATE");
    }
}
