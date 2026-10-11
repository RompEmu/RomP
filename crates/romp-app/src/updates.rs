//! Whether a newer RomP is out, and which of its downloads suits this computer.
use crate::external::Asset;
use serde::{Deserialize, Serialize};

pub const RELEASES: &str = "https://api.github.com/repos/RompEmu/RomP/releases/latest";
pub const STATE_KEY: &str = "updates";
const CHECK_EVERY: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    AppImage,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else if std::env::var_os("APPIMAGE").is_some() {
            Self::AppImage
        } else {
            Self::Linux
        }
    }

    fn asset_name(self, version: &str) -> String {
        match self {
            Self::MacOs => format!("RomP-{version}-macos-arm64.zip"),
            Self::Windows => format!("RomP-{version}-windows-x86_64.zip"),
            Self::AppImage => format!("RomP-{version}-x86_64.AppImage"),
            Self::Linux => format!("RomP-{version}-linux-x86_64.tar.gz"),
        }
    }
}

/// The newest release, with the download for this computer when it has one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub download: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub checked_at: i64,
    pub latest: Option<Release>,
    pub skipped: Option<String>,
}

impl State {
    pub fn from_json(text: Option<&str>) -> Self {
        text.and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("update state serializes")
    }

    pub fn due(&self, now: i64) -> bool {
        now < self.checked_at || now - self.checked_at >= CHECK_EVERY
    }

    /// The newest release, when it's newer than this one.
    pub fn available(&self, current: &str) -> Option<&Release> {
        self.latest
            .as_ref()
            .filter(|latest| newer(&latest.version, current))
    }

    /// The release to tell about: newer than this one, and not skipped.
    pub fn offer(&self, current: &str) -> Option<&Release> {
        self.available(current)
            .filter(|latest| self.skipped.as_deref() != Some(&latest.version))
    }
}

/// `0.13.1` or `v0.13.1`. Pre-releases and anything else give `None`.
fn version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
    let mut next = || parts.next()?.parse().ok();
    let found = (next()?, next()?, next()?);
    parts.next().is_none().then_some(found)
}

fn newer(candidate: &str, current: &str) -> bool {
    matches!((version(candidate), version(current)), (Some(a), Some(b)) if a > b)
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

fn release_from(found: GitHubRelease, platform: Platform) -> Option<Release> {
    let (major, minor, patch) = version(&found.tag_name)?;
    let version = format!("{major}.{minor}.{patch}");
    let file = platform.asset_name(&version);
    let download = found
        .assets
        .into_iter()
        .find(|a| a.name == file)
        .map(|a| a.browser_download_url);
    Some(Release {
        version,
        page: found.html_url,
        download,
    })
}

pub async fn latest(
    http: &reqwest::Client,
    url: &str,
    platform: Platform,
) -> Result<Release, String> {
    let found: GitHubRelease = http
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let tag = found.tag_name.clone();
    release_from(found, platform).ok_or_else(|| format!("{tag} isn't a version"))
}

/// This build's version. Development builds can pretend to be another, to try the update check.
pub fn current_version() -> String {
    #[cfg(debug_assertions)]
    if let Ok(pretend) = std::env::var("ROMP_PRETEND_VERSION") {
        return pretend;
    }
    env!("CARGO_PKG_VERSION").to_string()
}

/// Whether Homebrew installed RomP, which then updates it with `brew upgrade`.
pub fn homebrew() -> bool {
    cfg!(target_os = "macos")
        && ["/opt/homebrew/Caskroom/romp", "/usr/local/Caskroom/romp"]
            .iter()
            .any(|dir| std::path::Path::new(dir).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60;

    fn github(tag: &str, names: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.into(),
            html_url: format!("https://github.com/RompEmu/RomP/releases/tag/{tag}"),
            assets: names
                .iter()
                .map(|name| Asset {
                    name: (*name).into(),
                    browser_download_url: format!("https://example.com/{name}"),
                    digest: None,
                })
                .collect(),
        }
    }

    const ALL_FILES: [&str; 8] = [
        "RomP-0.13.1-linux-x86_64.tar.gz",
        "RomP-0.13.1-linux-x86_64.tar.gz.sha512",
        "RomP-0.13.1-macos-arm64.zip",
        "RomP-0.13.1-macos-arm64.zip.sha512",
        "RomP-0.13.1-windows-x86_64.zip",
        "RomP-0.13.1-windows-x86_64.zip.sha512",
        "RomP-0.13.1-x86_64.AppImage",
        "RomP-0.13.1-x86_64.AppImage.sha512",
    ];

    fn release(version: &str) -> Release {
        Release {
            version: version.into(),
            page: format!("https://github.com/RompEmu/RomP/releases/tag/v{version}"),
            download: None,
        }
    }

    #[test]
    fn versions_parse_with_or_without_a_v() {
        assert_eq!(version("v0.13.1"), Some((0, 13, 1)));
        assert_eq!(version("0.13.1"), Some((0, 13, 1)));
        assert_eq!(version("v1.0.0-beta.1"), None);
        assert_eq!(version("v1.0"), None);
        assert_eq!(version("nightly"), None);
    }

    #[test]
    fn versions_compare_by_number_not_text() {
        assert!(newer("0.13.10", "0.13.9"));
        assert!(newer("v0.14.0", "0.13.1"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.13.1", "0.13.1"));
        assert!(!newer("0.13.0", "0.13.1"));
        assert!(!newer("garbage", "0.13.1"));
    }

    #[test]
    fn each_platform_gets_its_own_download() {
        let pick = |platform| {
            release_from(github("v0.13.1", &ALL_FILES), platform)
                .unwrap()
                .download
                .unwrap()
        };
        assert!(pick(Platform::MacOs).ends_with("/RomP-0.13.1-macos-arm64.zip"));
        assert!(pick(Platform::Windows).ends_with("/RomP-0.13.1-windows-x86_64.zip"));
        assert!(pick(Platform::AppImage).ends_with("/RomP-0.13.1-x86_64.AppImage"));
        assert!(pick(Platform::Linux).ends_with("/RomP-0.13.1-linux-x86_64.tar.gz"));
    }

    #[test]
    fn a_release_without_this_computers_file_still_links_its_page() {
        let found = release_from(github("v0.14.0", &[]), Platform::MacOs).unwrap();
        assert_eq!(found.version, "0.14.0");
        assert_eq!(
            found.page,
            "https://github.com/RompEmu/RomP/releases/tag/v0.14.0"
        );
        assert_eq!(found.download, None);
    }

    #[test]
    fn a_tag_that_isnt_a_version_is_no_release() {
        assert_eq!(
            release_from(github("nightly", &ALL_FILES), Platform::Linux),
            None
        );
    }

    #[test]
    fn checks_at_most_once_a_day() {
        let state = State {
            checked_at: 1_000_000,
            ..State::default()
        };
        assert!(State::default().due(1_000_000));
        assert!(!state.due(1_000_000 + DAY - 1));
        assert!(state.due(1_000_000 + DAY));
        assert!(state.due(1_000_000 - 1), "a clock set back checks again");
    }

    #[test]
    fn offers_only_a_newer_release_that_wasnt_skipped() {
        let mut state = State {
            latest: Some(release("0.14.0")),
            ..State::default()
        };
        assert_eq!(state.offer("0.13.1"), Some(&release("0.14.0")));
        assert_eq!(state.offer("0.14.0"), None);
        assert_eq!(state.offer("0.15.0"), None);
        state.skipped = Some("0.14.0".into());
        assert_eq!(state.offer("0.13.1"), None);
        assert_eq!(state.available("0.13.1"), Some(&release("0.14.0")));
        assert_eq!(state.available("0.14.0"), None);
        state.latest = Some(release("0.14.1"));
        assert_eq!(state.offer("0.13.1"), Some(&release("0.14.1")));
    }

    #[test]
    fn state_round_trips_and_bad_json_starts_over() {
        let state = State {
            checked_at: 42,
            latest: Some(release("0.14.0")),
            skipped: Some("0.13.9".into()),
        };
        assert_eq!(State::from_json(Some(&state.to_json())), state);
        assert_eq!(State::from_json(Some("{oops")), State::default());
        assert_eq!(State::from_json(None), State::default());
    }

    #[test]
    fn this_build_reports_its_own_version() {
        assert_eq!(current_version(), env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn reads_githubs_latest_release() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "tag_name": "v0.13.1",
            "html_url": "https://github.com/RompEmu/RomP/releases/tag/v0.13.1",
            "prerelease": false,
            "draft": false,
            "assets": ALL_FILES.iter().map(|name| serde_json::json!({
                "name": name,
                "browser_download_url": format!("https://github.com/RompEmu/RomP/releases/download/v0.13.1/{name}"),
                "digest": "sha256:00",
            })).collect::<Vec<_>>(),
        });
        Mock::given(method("GET"))
            .and(path("/latest"))
            .and(header("accept", "application/vnd.github+json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let found = latest(
            &reqwest::Client::new(),
            &format!("{}/latest", server.uri()),
            Platform::Windows,
        )
        .await
        .unwrap();
        assert_eq!(found.version, "0.13.1");
        assert_eq!(
            found.download.as_deref(),
            Some("https://github.com/RompEmu/RomP/releases/download/v0.13.1/RomP-0.13.1-windows-x86_64.zip")
        );
    }

    #[tokio::test]
    async fn a_failed_request_is_an_error() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let result = latest(&reqwest::Client::new(), &server.uri(), Platform::Linux).await;
        assert!(result.is_err());
    }
}
