use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadState {
    pub buttons: u16,
    pub axes: [i16; 6],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AppMsg {
    Pad {
        port: u8,
        state: PadState,
    },
    Pointer {
        x: i16,
        y: i16,
        pressed: bool,
    },
    Mouse {
        dx: i16,
        dy: i16,
        buttons: u8,
    },
    PortDevice {
        port: u8,
        device: u32,
    },
    Key {
        code: u32,
        character: u32,
        modifiers: u16,
        down: bool,
    },
    Pause(bool),
    Volume(u8),
    SaveSlot(u8),
    LoadSlot(u8),
    Shutdown,
    Reset,
    /// Signs in to RetroAchievements and starts tracking the loaded game.
    Achievements {
        username: String,
        token: String,
        hardcore: bool,
        console_id: u32,
        /// RomM's RetroAchievements hash for the game, which spares hashing disc images.
        hash: Option<String>,
    },
    ListAchievements,
    /// The answer to an `AchievementsRequest`, fetched by the app.
    AchievementsResponse {
        id: u64,
        status: i32,
        body: Vec<u8>,
    },
}

/// One achievement as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AchievementInfo {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_url: String,
    pub badge_locked_url: String,
    pub unlocked: bool,
    pub progress: String,
}

/// Something that happened while tracking achievements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AchievementEvent {
    SignedIn,
    SignInFailed(String),
    GameLoaded {
        title: String,
        achievements: u32,
    },
    GameUnavailable(String),
    Unlocked {
        id: u32,
        title: String,
        description: String,
        points: u32,
        badge_url: String,
    },
    Mastered,
    LeaderboardStarted {
        title: String,
        description: String,
    },
    LeaderboardFailed {
        title: String,
    },
    LeaderboardSubmitted {
        title: String,
        score: String,
    },
    Progress {
        title: String,
        badge_url: String,
        progress: String,
    },
    ProgressHidden,
    Tracker {
        id: u32,
        display: String,
    },
    TrackerHidden {
        id: u32,
    },
    Challenge {
        id: u32,
        badge_url: String,
    },
    ChallengeHidden {
        id: u32,
    },
    Offline,
    Online,
    Reset,
    ServerError(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunnerMsg {
    Started {
        core_name: String,
        core_version: String,
        fps: f64,
        sample_rate: f64,
    },
    Controllers {
        ports: Vec<Vec<(String, u32)>>,
    },
    Rotation(u8),
    SramWritten,
    StateWritten {
        slot: u8,
        ok: bool,
    },
    StateLoaded {
        slot: u8,
        ok: bool,
    },
    Exited {
        error: Option<String>,
    },
    /// A request to RetroAchievements; the sandboxed runner has no network of its own.
    AchievementsRequest {
        id: u64,
        url: String,
        post: Option<String>,
        content_type: Option<String>,
        agent: String,
    },
    Achievement(AchievementEvent),
    AchievementList(Vec<AchievementInfo>),
}
