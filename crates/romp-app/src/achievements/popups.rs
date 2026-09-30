use romp_proto::msg::{AchievementEvent, AppMsg};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, VecDeque};
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
}

impl Link {
    pub fn new(launch: Launch) -> Self {
        Self {
            launch,
            updates: channel(),
            queue: VecDeque::new(),
            showing: None,
            badges: HashMap::new(),
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

    pub fn event(&mut self, event: &AchievementEvent) {
        let Some(toast) = toast_for(event) else {
            return;
        };
        if let Some(url) = &toast.badge_url {
            if super::allowed(url) && !self.badges.contains_key(url) {
                let http = self.launch.http.clone();
                let done = self.updates.0.clone();
                let url = url.clone();
                self.launch.rt.spawn(async move {
                    let Ok(response) = http.get(&url).send().await else {
                        return;
                    };
                    let Ok(bytes) = response.bytes().await else {
                        return;
                    };
                    let Ok(picture) = image::load_from_memory(&bytes) else {
                        return;
                    };
                    let rgba = picture.to_rgba8();
                    let pixels = SharedPixelBuffer::clone_from_slice(
                        rgba.as_raw(),
                        rgba.width(),
                        rgba.height(),
                    );
                    let _ = done.send(Update::Badge { url, pixels });
                });
            }
        }
        self.queue.push_back(toast);
    }

    /// Replies for the runner, and the popup to show now, or None to hide it.
    pub fn poll(&mut self, now: Instant) -> (Vec<AppMsg>, Option<View>) {
        let mut replies = Vec::new();
        while let Ok(update) = self.updates.1.try_recv() {
            match update {
                Update::Reply { id, status, body } => {
                    replies.push(AppMsg::AchievementsResponse { id, status, body });
                }
                Update::Badge { url, pixels } => {
                    self.badges.insert(url, Image::from_rgba8(pixels));
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
        let view = self.showing.as_ref().map(|(t, _)| View {
            title: t.title.clone(),
            detail: t.detail.clone(),
            badge: t
                .badge_url
                .as_ref()
                .and_then(|url| self.badges.get(url).cloned()),
        });
        (replies, view)
    }
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
    fn popups_take_turns() {
        let (mut link, _rt) = link();
        let start = Instant::now();
        link.event(&AchievementEvent::Mastered);
        link.event(&AchievementEvent::Online);
        let (_, first) = link.poll(start);
        assert_eq!(first.unwrap().title, "Game mastered");
        let (_, still) = link.poll(start + Duration::from_secs(5));
        assert_eq!(still.unwrap().title, "Game mastered");
        let (_, second) = link.poll(start + Duration::from_secs(6));
        assert_eq!(second.unwrap().title, "RetroAchievements is back");
        let (_, gone) = link.poll(start + Duration::from_secs(10));
        assert!(gone.is_none());
    }
}
