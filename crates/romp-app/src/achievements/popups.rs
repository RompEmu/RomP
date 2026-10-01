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
    pub store: std::sync::Arc<std::sync::Mutex<crate::store::Store>>,
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
            id,
            title,
            description,
            ..
        } if super::is_notice(*id) => toast(title.clone(), description.clone(), 6),
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

/// How much RetroAchievements may show over the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    All,
    Quiet,
    Off,
}

pub const LEVELS: [&str; 3] = ["All", "Quiet", "Off"];
pub const CORNERS: [&str; 4] = ["Top right", "Top left", "Bottom right", "Bottom left"];

impl Level {
    pub fn from_index(index: u8) -> Self {
        match index {
            0 => Self::All,
            1 => Self::Quiet,
            _ => Self::Off,
        }
    }
}

/// How something that happened is shown: as a popup, or only as a brief note in the corner.
#[derive(Debug, Clone, PartialEq)]
enum Shown {
    Popup(Toast),
    Chip(String),
}

/// Whether an event is worth interrupting the game for even when popups are quiet.
fn matters(event: &AchievementEvent) -> bool {
    matches!(
        event,
        AchievementEvent::Unlocked { .. }
            | AchievementEvent::Mastered
            | AchievementEvent::SignInFailed(_)
            | AchievementEvent::HardcoreOff(_)
    )
}

fn shown(event: &AchievementEvent, level: Level) -> Option<Shown> {
    let toast = toast_for(event)?;
    Some(match level {
        Level::All => Shown::Popup(toast),
        Level::Quiet if matters(event) => Shown::Popup(toast),
        _ => Shown::Chip(toast.title),
    })
}

/// Which quarter of the way an achievement's progress is, from texts like "34/100" or "34%".
fn quarter(progress: &str) -> Option<u8> {
    let text = progress.replace(',', "");
    let text = text.trim();
    let (done, total) = match text.strip_suffix('%') {
        Some(percent) => (percent.trim().parse::<f64>().ok()?, 100.0),
        None => {
            let (a, b) = text.split_once('/')?;
            (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?)
        }
    };
    (total > 0.0).then(|| ((done / total * 4.0).floor() as i64).clamp(0, 4) as u8)
}

/// How often quiet popups may show the same achievement's progress.
const PROGRESS_EVERY: Duration = Duration::from_secs(60);

/// Whether progress is shown, given when and at which quarter it last was.
fn progress_due(
    level: Level,
    last: Option<(Option<u8>, Instant)>,
    progress: &str,
    now: Instant,
) -> bool {
    if level == Level::All {
        return true;
    }
    let rested = last.is_none_or(|(_, at)| now.duration_since(at) >= PROGRESS_EVERY);
    match quarter(progress) {
        Some(q) => {
            let passed = last.and_then(|(q, _)| q).unwrap_or(0);
            (1..=3).contains(&q) && q > passed && rested
        }
        None => rested,
    }
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
    level: Level,
    progress_shown: HashMap<String, (Option<u8>, Instant)>,
    chip: Option<(String, Instant)>,
    session: HashMap<String, String>,
}

const CHIP_SHOWN: Duration = Duration::from_millis(2500);
const UNLOCKED: &str = "Unlocked while playing";

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
            level: Level::Quiet,
            progress_shown: HashMap::new(),
            chip: None,
            session: HashMap::new(),
        }
    }

    pub fn set_level(&mut self, level: Level) {
        self.level = level;
        self.overlay_changed = true;
    }

    /// What happened to an achievement while playing, by its title: unlocked, or how far along.
    pub fn session_note(&self, title: &str) -> Option<&str> {
        self.session.get(title).map(String::as_str)
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
        let store = self.launch.store.clone();
        let username = self.launch.username.clone();
        self.launch.rt.spawn(async move {
            let unlock = post.as_deref().and_then(romp_cheevos::Unlock::from_request);
            let (status, body) = super::forward(
                &http,
                &url,
                post.as_deref(),
                content_type.as_deref(),
                &agent,
            )
            .await;
            if let Some(unlock) = unlock {
                super::note_unlock(&store, &username, &unlock, status, &body);
            }
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
                if !self.session.get(title).is_some_and(|n| n == UNLOCKED) {
                    self.session.insert(title.clone(), progress.clone());
                }
                let now = Instant::now();
                let last = self.progress_shown.get(title).copied();
                if !progress_due(self.level, last, progress, now) {
                    return;
                }
                self.progress_shown
                    .insert(title.clone(), (quarter(progress), now));
                if self.level == Level::Off {
                    self.chip = Some((format!("{progress} · {title}"), now));
                    return;
                }
                self.want_badge(badge_url);
                self.progress = Some((title.clone(), progress.clone(), badge_url.clone(), now));
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
        if self.level == Level::Off {
            return Some(Overlay::default());
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
        if let AchievementEvent::Unlocked { id, title, .. } = event {
            if !super::is_notice(*id) {
                self.session.insert(title.clone(), UNLOCKED.into());
            }
        }
        match shown(event, self.level) {
            Some(Shown::Popup(toast)) => {
                if let Some(url) = toast.badge_url.clone() {
                    self.want_badge(&url);
                }
                self.queue.push_back(toast);
            }
            Some(Shown::Chip(text)) => self.chip = Some((text, Instant::now())),
            None => {}
        }
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
        if self
            .chip
            .as_ref()
            .is_some_and(|(_, since)| now.duration_since(*since) >= CHIP_SHOWN)
        {
            self.chip = None;
        }
        polled.chip = self.chip.as_ref().map(|(text, _)| text.clone());
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
    /// A brief note to show in the corner instead of a popup.
    pub chip: Option<String>,
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
    fn retroachievements_notices_are_not_shown_as_unlocks() {
        let t = toast_for(&AchievementEvent::Unlocked {
            id: 101_000_001,
            title: "Warning: Unknown Emulator".into(),
            description: "Hardcore unlocks cannot be earned using this emulator.".into(),
            points: 0,
            badge_url: "https://media.retroachievements.org/Badge/00000.png".into(),
        })
        .unwrap();
        assert_eq!(t.title, "Warning: Unknown Emulator");
        assert_eq!(
            t.detail,
            "Hardcore unlocks cannot be earned using this emulator."
        );
        assert_eq!(t.badge_url, None);
    }

    fn unlock() -> AchievementEvent {
        AchievementEvent::Unlocked {
            id: 1,
            title: "Ring Collector".into(),
            description: "Collect 100 rings".into(),
            points: 5,
            badge_url: String::new(),
        }
    }

    #[test]
    fn quiet_popups_keep_unlocks_and_shrink_the_rest() {
        let loaded = AchievementEvent::GameLoaded {
            title: "Sonic".into(),
            achievements: 3,
        };
        assert!(matches!(
            shown(&unlock(), Level::Quiet),
            Some(Shown::Popup(_))
        ));
        assert!(matches!(shown(&loaded, Level::All), Some(Shown::Popup(_))));
        assert_eq!(
            shown(&loaded, Level::Quiet),
            Some(Shown::Chip("RetroAchievements".into()))
        );
        assert_eq!(
            shown(&unlock(), Level::Off),
            Some(Shown::Chip("Unlocked: Ring Collector".into()))
        );
        assert_eq!(shown(&AchievementEvent::SignedIn, Level::All), None);
    }

    #[test]
    fn progress_reads_as_quarters() {
        assert_eq!(quarter("34/100"), Some(1));
        assert_eq!(quarter("1,500/2,000"), Some(3));
        assert_eq!(quarter("50%"), Some(2));
        assert_eq!(quarter("0/10"), Some(0));
        assert_eq!(quarter("10/10"), Some(4));
        assert_eq!(quarter("Level 3"), None);
    }

    #[test]
    fn quiet_progress_shows_only_new_milestones_at_most_once_a_minute() {
        let start = Instant::now();
        let later = |secs| start + Duration::from_secs(secs);
        assert!(progress_due(
            Level::All,
            Some((Some(1), start)),
            "26/100",
            start
        ));
        assert!(
            !progress_due(Level::Quiet, None, "3/100", start),
            "not a milestone yet"
        );
        assert!(progress_due(Level::Quiet, None, "25/100", start));
        let shown = Some((Some(1), start));
        assert!(
            !progress_due(Level::Quiet, shown, "30/100", later(120)),
            "same quarter"
        );
        assert!(
            !progress_due(Level::Quiet, shown, "50/100", later(30)),
            "too soon"
        );
        assert!(progress_due(Level::Quiet, shown, "50/100", later(61)));
        assert!(
            !progress_due(Level::Quiet, shown, "100/100", later(61)),
            "the unlock says it"
        );
        assert!(progress_due(Level::Off, None, "Level 3", start));
        assert!(!progress_due(
            Level::Off,
            Some((None, start)),
            "Level 4",
            later(10)
        ));
    }

    #[test]
    fn quiet_links_note_minor_events_in_the_corner_and_remember_the_session() {
        let (mut link, _rt) = link();
        let start = Instant::now();
        link.event(&AchievementEvent::Online);
        link.event(&unlock());
        let polled = link.poll(start);
        assert_eq!(
            polled.view.map(|v| v.title).as_deref(),
            Some("Unlocked: Ring Collector")
        );
        assert_eq!(polled.chip.as_deref(), Some("RetroAchievements is back"));
        assert_eq!(link.poll(start + Duration::from_secs(3)).chip, None);
        assert_eq!(link.session_note("Ring Collector"), Some(UNLOCKED));
        link.set_level(Level::Off);
        link.event(&AchievementEvent::Tracker {
            id: 1,
            display: "0:42".into(),
        });
        assert!(link.overlay(start).is_some_and(|o| o.trackers.is_empty()));
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
            store: std::sync::Arc::new(std::sync::Mutex::new(
                crate::store::Store::open_in_memory().unwrap(),
            )),
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
        link.set_level(Level::All);
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
