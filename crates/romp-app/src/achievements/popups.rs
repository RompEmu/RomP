use romp_proto::msg::{AchievementEvent, AppMsg};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// What a game needs to track achievements, gathered by the app before it starts.
pub struct Launch {
    pub username: String,
    pub token: String,
    pub hardcore: bool,
    pub console_id: u32,
    pub hash: Option<String>,
    pub http: reqwest::Client,
    pub rt: tokio::runtime::Handle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub title: String,
    pub detail: String,
    pub badge_url: Option<String>,
    pub seconds: u64,
}

fn toast(title: impl Into<String>, detail: impl Into<String>, seconds: u64) -> Toast {
    Toast {
        title: title.into(),
        detail: detail.into(),
        badge_url: None,
        seconds,
    }
}

fn points(n: u32) -> String {
    if n == 1 {
        "1 point".into()
    } else {
        format!("{n} points")
    }
}

/// The popup, if any, for something that happened while tracking achievements.
pub fn toast_for(event: &AchievementEvent) -> Option<Toast> {
    Some(match event {
        AchievementEvent::GameLoaded {
            title,
            achievements,
        } => toast(
            "RetroAchievements",
            match achievements {
                1 => format!("1 achievement for {title}"),
                n => format!("{n} achievements for {title}"),
            },
            4,
        ),
        AchievementEvent::SignInFailed(reason) => toast(
            "RetroAchievements sign-in failed",
            format!("{reason}. Sign in again in Settings."),
            6,
        ),
        AchievementEvent::Unlocked {
            title,
            description,
            points: p,
            badge_url,
            ..
        } => Toast {
            title: format!("Unlocked: {title}"),
            detail: format!("{description} · {}", points(*p)),
            badge_url: (!badge_url.is_empty()).then(|| badge_url.clone()),
            seconds: 5,
        },
        AchievementEvent::Mastered => toast("Game mastered", "Every achievement is yours.", 6),
        AchievementEvent::LeaderboardStarted { title, .. } => {
            toast("Leaderboard attempt started", title.clone(), 3)
        }
        AchievementEvent::LeaderboardFailed { title } => {
            toast("Leaderboard attempt failed", title.clone(), 3)
        }
        AchievementEvent::LeaderboardSubmitted { title, score } => toast(
            "Leaderboard score submitted",
            format!("{title}: {score}"),
            4,
        ),
        AchievementEvent::Offline => toast(
            "RetroAchievements is unreachable",
            "Unlocks will be sent when it's back.",
            5,
        ),
        AchievementEvent::Online => toast("RetroAchievements is back", "Unlocks were sent.", 3),
        AchievementEvent::HardcoreOff(reason) => toast(
            "Hardcore is off for this game",
            format!("Because {reason}."),
            6,
        ),
        AchievementEvent::ServerError(message) => {
            toast("RetroAchievements error", message.clone(), 5)
        }
        AchievementEvent::GameUnavailable(_)
        | AchievementEvent::SignedIn
        | AchievementEvent::Progress { .. }
        | AchievementEvent::ProgressHidden
        | AchievementEvent::Tracker { .. }
        | AchievementEvent::TrackerHidden { .. }
        | AchievementEvent::Challenge { .. }
        | AchievementEvent::ChallengeHidden { .. }
        | AchievementEvent::Reset => return None,
    })
}

enum Update {
    Reply {
        id: u64,
        status: i32,
        body: Vec<u8>,
    },
    Badge {
        url: String,
        pixels: SharedPixelBuffer<Rgba8Pixel>,
    },
}

pub struct View {
    pub title: String,
    pub detail: String,
    pub badge: Option<Image>,
}

/// Connects a running game to RetroAchievements through the app, and paces its popups.
pub struct Link {
    launch: Launch,
    updates: (Sender<Update>, Receiver<Update>),
    queue: VecDeque<Toast>,
    showing: Option<(Toast, Instant)>,
    badges: HashMap<String, Image>,
    fetching: HashSet<String>,
    trackers: BTreeMap<u32, String>,
    challenges: BTreeMap<u32, String>,
    progress: Option<(String, String, String, Instant)>,
    overlay_changed: bool,
}

const PROGRESS_SHOWN: Duration = Duration::from_secs(3);

/// The small indicators that stay on screen while playing.
#[derive(Default)]
pub struct Overlay {
    pub trackers: Vec<String>,
    pub challenges: Vec<Image>,
    pub progress: Option<View>,
}

impl Link {
    pub fn new(launch: Launch) -> Self {
        Self {
            launch,
            updates: channel(),
            queue: VecDeque::new(),
            showing: None,
            badges: HashMap::new(),
            fetching: HashSet::new(),
            trackers: BTreeMap::new(),
            challenges: BTreeMap::new(),
            progress: None,
            overlay_changed: false,
        }
    }

    pub fn start_message(&self) -> AppMsg {
        AppMsg::Achievements {
            username: self.launch.username.clone(),
            token: self.launch.token.clone(),
            hardcore: self.launch.hardcore,
            console_id: self.launch.console_id,
            hash: self.launch.hash.clone(),
        }
    }

    pub fn request(
        &self,
        id: u64,
        url: String,
        post: Option<String>,
        content_type: Option<String>,
        agent: String,
    ) {
        let http = self.launch.http.clone();
        let done = self.updates.0.clone();
        self.launch.rt.spawn(async move {
            let (status, body) = super::forward(
                &http,
                &url,
                post.as_deref(),
                content_type.as_deref(),
                &agent,
            )
            .await;
            let _ = done.send(Update::Reply { id, status, body });
        });
    }

    /// Starts fetching a badge image unless it is already here or on its way.
    pub fn want_badge(&mut self, url: &str) {
        if url.is_empty() || !super::allowed(url) || self.badges.contains_key(url) {
            return;
        }
        if !self.fetching.insert(url.to_string()) {
            return;
        }
        let http = self.launch.http.clone();
        let done = self.updates.0.clone();
        let url = url.to_string();
        self.launch.rt.spawn(async move {
            if let Some(pixels) = fetch_badge(&http, &url).await {
                let _ = done.send(Update::Badge { url, pixels });
            }
        });
    }

    pub fn badge(&self, url: &str) -> Option<Image> {
        self.badges.get(url).cloned()
    }

    fn indicate(&mut self, event: &AchievementEvent) {
        match event {
            AchievementEvent::Tracker { id, display } => {
                self.trackers.insert(*id, display.clone());
            }
            AchievementEvent::TrackerHidden { id } => {
                self.trackers.remove(id);
            }
            AchievementEvent::Challenge { id, badge_url } => {
                self.want_badge(badge_url);
                self.challenges.insert(*id, badge_url.clone());
            }
            AchievementEvent::ChallengeHidden { id } => {
                self.challenges.remove(id);
            }
            AchievementEvent::Progress {
                title,
                badge_url,
                progress,
            } => {
                self.want_badge(badge_url);
                self.progress = Some((
                    title.clone(),
                    progress.clone(),
                    badge_url.clone(),
                    Instant::now(),
                ));
            }
            AchievementEvent::ProgressHidden => self.progress = None,
            AchievementEvent::Reset => {
                self.trackers.clear();
                self.challenges.clear();
                self.progress = None;
            }
            _ => return,
        }
        self.overlay_changed = true;
    }

    /// The indicators to show, when they changed since the last call.
    pub fn overlay(&mut self, now: Instant) -> Option<Overlay> {
        if self
            .progress
            .as_ref()
            .is_some_and(|(.., since)| now.duration_since(*since) >= PROGRESS_SHOWN)
        {
            self.progress = None;
            self.overlay_changed = true;
        }
        if !std::mem::take(&mut self.overlay_changed) {
            return None;
        }
        Some(Overlay {
            trackers: self.trackers.values().cloned().collect(),
            challenges: self
                .challenges
                .values()
                .filter_map(|url| self.badge(url))
                .collect(),
            progress: self
                .progress
                .as_ref()
                .map(|(title, progress, url, _)| View {
                    title: title.clone(),
                    detail: progress.clone(),
                    badge: self.badge(url),
                }),
        })
    }

    pub fn event(&mut self, event: &AchievementEvent) {
        self.indicate(event);
        let Some(toast) = toast_for(event) else {
            return;
        };
        if let Some(url) = toast.badge_url.clone() {
            self.want_badge(&url);
        }
        self.queue.push_back(toast);
    }

    /// Replies for the runner, the popup to show now, and whether new badges arrived.
    pub fn poll(&mut self, now: Instant) -> Polled {
        let mut polled = Polled::default();
        while let Ok(update) = self.updates.1.try_recv() {
            match update {
                Update::Reply { id, status, body } => {
                    polled
                        .replies
                        .push(AppMsg::AchievementsResponse { id, status, body });
                }
                Update::Badge { url, pixels } => {
                    self.fetching.remove(&url);
                    let shown = self.challenges.values().any(|u| *u == url)
                        || self.progress.as_ref().is_some_and(|(_, _, u, _)| *u == url);
                    self.overlay_changed |= shown;
                    self.badges.insert(url, Image::from_rgba8(pixels));
                    polled.new_badges = true;
                }
            }
        }
        let expired = self
            .showing
            .as_ref()
            .is_some_and(|(t, since)| now.duration_since(*since) >= Duration::from_secs(t.seconds));
        if expired || self.showing.is_none() {
            self.showing = self.queue.pop_front().map(|t| (t, now));
        }
        polled.view = self.showing.as_ref().map(|(t, _)| View {
            title: t.title.clone(),
            detail: t.detail.clone(),
            badge: t.badge_url.as_ref().and_then(|url| self.badge(url)),
        });
        polled
    }
}

#[derive(Default)]
pub struct Polled {
    pub replies: Vec<AppMsg>,
    pub view: Option<View>,
    pub new_badges: bool,
}

async fn fetch_badge(http: &reqwest::Client, url: &str) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let bytes = http.get(url).send().await.ok()?.bytes().await.ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocks_show_their_badge_title_and_points() {
        let t = toast_for(&AchievementEvent::Unlocked {
            id: 1,
            title: "Ring Collector".into(),
            description: "Collect 100 rings".into(),
            points: 5,
            badge_url: "https://media.retroachievements.org/Badge/1.png".into(),
        })
        .unwrap();
        assert_eq!(t.title, "Unlocked: Ring Collector");
        assert_eq!(t.detail, "Collect 100 rings · 5 points");
        assert_eq!(
            t.badge_url.as_deref(),
            Some("https://media.retroachievements.org/Badge/1.png")
        );
    }

    #[test]
    fn quiet_events_show_nothing() {
        assert_eq!(toast_for(&AchievementEvent::SignedIn), None);
        assert_eq!(
            toast_for(&AchievementEvent::GameUnavailable("no set".into())),
            None
        );
        let loaded = toast_for(&AchievementEvent::GameLoaded {
            title: "Sonic".into(),
            achievements: 1,
        })
        .unwrap();
        assert_eq!(loaded.detail, "1 achievement for Sonic");
    }

    fn link() -> (Link, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let link = Link::new(Launch {
            username: "p".into(),
            token: "t".into(),
            hardcore: false,
            console_id: 3,
            hash: None,
            http: reqwest::Client::new(),
            rt: rt.handle().clone(),
        });
        (link, rt)
    }

    #[test]
    fn trackers_and_progress_come_and_go() {
        let (mut link, _rt) = link();
        let start = Instant::now();
        assert!(link.overlay(start).is_none(), "nothing changed yet");
        link.event(&AchievementEvent::Tracker {
            id: 2,
            display: "01:23".into(),
        });
        link.event(&AchievementEvent::Tracker {
            id: 1,
            display: "500".into(),
        });
        link.event(&AchievementEvent::Progress {
            title: "Collector".into(),
            badge_url: String::new(),
            progress: "37/50".into(),
        });
        let overlay = link.overlay(start).unwrap();
        assert_eq!(overlay.trackers, ["500", "01:23"]);
        assert_eq!(overlay.progress.unwrap().detail, "37/50");
        assert!(link.overlay(start).is_none(), "only changes are reported");
        link.event(&AchievementEvent::TrackerHidden { id: 1 });
        assert_eq!(link.overlay(start).unwrap().trackers, ["01:23"]);
        let later = Instant::now() + Duration::from_secs(4);
        assert!(link.overlay(later).unwrap().progress.is_none());
    }

    #[test]
    fn popups_take_turns() {
        let (mut link, _rt) = link();
        let start = Instant::now();
        link.event(&AchievementEvent::Mastered);
        link.event(&AchievementEvent::Online);
        let title = |p: Polled| p.view.map(|v| v.title);
        assert_eq!(title(link.poll(start)).as_deref(), Some("Game mastered"));
        assert_eq!(
            title(link.poll(start + Duration::from_secs(5))).as_deref(),
            Some("Game mastered")
        );
        assert_eq!(
            title(link.poll(start + Duration::from_secs(6))).as_deref(),
            Some("RetroAchievements is back")
        );
        assert!(title(link.poll(start + Duration::from_secs(10))).is_none());
    }
}
