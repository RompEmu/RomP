//! RetroAchievements through rcheevos' rc_client, with its network requests handed to the caller.

use romp_proto::msg::{AchievementEvent, AchievementInfo};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::path::Path;
use std::ptr::NonNull;

#[repr(C)]
struct RawEvent {
    kind: u32,
    id: u32,
    points: u32,
    title: *const c_char,
    description: *const c_char,
    badge_url: *const c_char,
    progress: *const c_char,
    tracker: *const c_char,
    error: *const c_char,
}

#[repr(C)]
struct RawAchievement {
    id: u32,
    points: u32,
    title: *const c_char,
    description: *const c_char,
    badge_url: *const c_char,
    badge_locked_url: *const c_char,
    progress: *const c_char,
    unlocked: u32,
}

type RequestOut = extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char);
type AchievementOut = extern "C" fn(*mut c_void, *const RawAchievement);
type IdOut = extern "C" fn(*mut c_void, u32);

pub type MemoryData = unsafe extern "C" fn(c_uint) -> *mut c_void;
pub type MemorySize = unsafe extern "C" fn(c_uint) -> usize;

unsafe extern "C" {
    fn romp_rc_create(host: *mut c_void, hardcore: c_int) -> *mut c_void;
    fn romp_rc_destroy(client: *mut c_void);
    fn romp_rc_login(client: *mut c_void, username: *const c_char, token: *const c_char);
    fn romp_rc_load_game(
        client: *mut c_void,
        console_id: u32,
        path: *const c_char,
        data: *const u8,
        size: usize,
    );
    fn romp_rc_load_hash(client: *mut c_void, hash: *const c_char);
    fn romp_rc_respond(
        callback: *const c_void,
        callback_data: *mut c_void,
        body: *const c_char,
        body_length: usize,
        status: c_int,
    );
    fn romp_rc_game(client: *mut c_void, title: *mut *const c_char, achievements: *mut u32) -> u32;
    fn romp_rc_user_agent_clause(client: *mut c_void, buffer: *mut c_char, size: usize) -> usize;
    fn romp_rc_set_memory_map(map: *const c_void);
    fn romp_rc_set_core_memory(data: Option<MemoryData>, size: Option<MemorySize>);
    fn romp_rc_catalog_request(
        username: *const c_char,
        token: *const c_char,
        game_id: u32,
        hash: *const c_char,
        ctx: *mut c_void,
        out: RequestOut,
    ) -> c_int;
    fn romp_rc_unlocks_request(
        username: *const c_char,
        token: *const c_char,
        game_id: u32,
        hardcore: c_int,
        ctx: *mut c_void,
        out: RequestOut,
    ) -> c_int;
    fn romp_rc_parse_catalog(
        body: *const c_char,
        length: usize,
        status: c_int,
        ctx: *mut c_void,
        out: AchievementOut,
    ) -> u32;
    fn romp_rc_parse_unlocks(
        body: *const c_char,
        length: usize,
        status: c_int,
        ctx: *mut c_void,
        out: IdOut,
    ) -> c_int;
    fn romp_rc_list_achievements(client: *mut c_void, ctx: *mut c_void, out: AchievementOut);
    fn romp_rc_hardcore(client: *mut c_void) -> c_int;
    fn romp_rc_set_hardcore(client: *mut c_void, enabled: c_int);
    fn romp_rc_console(client: *mut c_void) -> u32;
    fn romp_rc_progress_size(client: *mut c_void) -> usize;
    fn romp_rc_serialize_progress(client: *mut c_void, buffer: *mut u8, size: usize) -> c_int;
    fn romp_rc_deserialize_progress(client: *mut c_void, buffer: *const u8, size: usize) -> c_int;
    fn romp_rc_setting_allowed(
        library_name: *const c_char,
        key: *const c_char,
        value: *const c_char,
    ) -> c_int;
    fn romp_rc_system_allowed(library_name: *const c_char, console_id: u32) -> c_int;
    fn romp_rc_award_request(
        username: *const c_char,
        token: *const c_char,
        achievement_id: u32,
        hardcore: c_int,
        hash: *const c_char,
        seconds_since_unlock: u32,
        ctx: *mut c_void,
        out: RequestOut,
    ) -> c_int;
    fn romp_rc_award_accepted(body: *const c_char, length: usize, status: c_int) -> c_int;
    fn rc_client_do_frame(client: *mut c_void);
    fn rc_client_idle(client: *mut c_void);
    fn rc_client_reset(client: *mut c_void);
}

/// Keeps a copy of the memory map a core announces, for reading the memory achievements watch.
///
/// # Safety
/// `map` is null or points to a valid `retro_memory_map` for the duration of the call.
pub unsafe fn set_memory_map(map: *const c_void) {
    unsafe { romp_rc_set_memory_map(map) }
}

/// The core's `retro_get_memory_data` and `retro_get_memory_size`, used when it has no memory map.
pub fn set_core_memory(data: MemoryData, size: MemorySize) {
    unsafe { romp_rc_set_core_memory(Some(data), Some(size)) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub url: String,
    pub post: Option<String>,
    pub content_type: Option<String>,
}

struct Pending {
    callback: *const c_void,
    data: *mut c_void,
}

#[derive(Default)]
struct Host {
    client: Option<NonNull<c_void>>,
    next_id: u64,
    requests: Vec<Request>,
    pending: HashMap<u64, Pending>,
    events: Vec<AchievementEvent>,
}

fn text(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: rcheevos hands over nul-terminated strings that live through the callback.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

fn host<'a>(ptr: *mut c_void) -> &'a mut Host {
    // SAFETY: `ptr` is the boxed Host a Session registered, alive for as long as its client.
    unsafe { &mut *(ptr as *mut Host) }
}

#[unsafe(no_mangle)]
extern "C" fn romp_rc_on_request(
    host_ptr: *mut c_void,
    url: *const c_char,
    post: *const c_char,
    content_type: *const c_char,
    callback: *const c_void,
    callback_data: *mut c_void,
) {
    let host = host(host_ptr);
    host.next_id += 1;
    let id = host.next_id;
    let optional = |p: *const c_char| (!p.is_null()).then(|| text(p));
    host.requests.push(Request {
        id,
        url: text(url),
        post: optional(post),
        content_type: optional(content_type),
    });
    host.pending.insert(
        id,
        Pending {
            callback,
            data: callback_data,
        },
    );
}

#[unsafe(no_mangle)]
extern "C" fn romp_rc_on_event(host_ptr: *mut c_void, event: *const RawEvent) {
    // SAFETY: the shim passes a pointer to an event on its stack for the length of the call.
    let e = unsafe { &*event };
    let event = match e.kind {
        1 => AchievementEvent::Unlocked {
            id: e.id,
            title: text(e.title),
            description: text(e.description),
            points: e.points,
            badge_url: text(e.badge_url),
        },
        2 => AchievementEvent::LeaderboardStarted {
            title: text(e.title),
            description: text(e.description),
        },
        3 => AchievementEvent::LeaderboardFailed {
            title: text(e.title),
        },
        4 => AchievementEvent::LeaderboardSubmitted {
            title: text(e.title),
            score: text(e.tracker),
        },
        5 => AchievementEvent::Challenge {
            id: e.id,
            badge_url: text(e.badge_url),
        },
        6 => AchievementEvent::ChallengeHidden { id: e.id },
        7 | 9 => AchievementEvent::Progress {
            title: text(e.title),
            badge_url: text(e.badge_url),
            progress: text(e.progress),
        },
        8 => AchievementEvent::ProgressHidden,
        10 | 12 => AchievementEvent::Tracker {
            id: e.id,
            display: text(e.tracker),
        },
        11 => AchievementEvent::TrackerHidden { id: e.id },
        14 => AchievementEvent::Reset,
        15 => AchievementEvent::Mastered,
        16 => AchievementEvent::ServerError(text(e.error)),
        17 => AchievementEvent::Offline,
        18 => AchievementEvent::Online,
        _ => return,
    };
    host(host_ptr).events.push(event);
}

const DONE_LOGIN: c_int = 0;

#[unsafe(no_mangle)]
extern "C" fn romp_rc_on_done(
    host_ptr: *mut c_void,
    what: c_int,
    result: c_int,
    error: *const c_char,
) {
    let host = host(host_ptr);
    let error = || {
        let message = text(error);
        if message.is_empty() {
            format!("error {result}")
        } else {
            message
        }
    };
    let event = match (what, result) {
        (DONE_LOGIN, 0) => AchievementEvent::SignedIn,
        (DONE_LOGIN, _) => AchievementEvent::SignInFailed(error()),
        (_, 0) => {
            let mut title = std::ptr::null();
            let mut achievements = 0;
            let id = match host.client {
                // SAFETY: the client is alive while it reports on itself.
                Some(client) => unsafe {
                    romp_rc_game(client.as_ptr(), &mut title, &mut achievements)
                },
                None => 0,
            };
            if id == 0 {
                AchievementEvent::GameUnavailable("This game has no achievements".into())
            } else {
                AchievementEvent::GameLoaded {
                    title: text(title),
                    achievements,
                }
            }
        }
        _ => AchievementEvent::GameUnavailable(error()),
    };
    host.events.push(event);
}

extern "C" fn collect_request(
    ctx: *mut c_void,
    url: *const c_char,
    post: *const c_char,
    content_type: *const c_char,
) {
    // SAFETY: ctx is the Option<Request> the caller passed for this call only.
    let slot = unsafe { &mut *(ctx as *mut Option<Request>) };
    let optional = |p: *const c_char| (!p.is_null()).then(|| text(p));
    *slot = Some(Request {
        id: 0,
        url: text(url),
        post: optional(post),
        content_type: optional(content_type),
    });
}

extern "C" fn collect_achievement(ctx: *mut c_void, raw: *const RawAchievement) {
    // SAFETY: ctx is the Vec the caller passed for this call only, and raw lives through it.
    let list = unsafe { &mut *(ctx as *mut Vec<AchievementInfo>) };
    let a = unsafe { &*raw };
    list.push(AchievementInfo {
        id: a.id,
        title: text(a.title),
        description: text(a.description),
        points: a.points,
        badge_url: text(a.badge_url),
        badge_locked_url: text(a.badge_locked_url),
        unlocked: a.unlocked != 0,
        progress: text(a.progress),
    });
}

extern "C" fn collect_id(ctx: *mut c_void, id: u32) {
    // SAFETY: ctx is the Vec the caller passed for this call only.
    unsafe { &mut *(ctx as *mut Vec<u32>) }.push(id);
}

fn c(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}

/// The request for a game's achievement list, by RetroAchievements' game id or else by hash.
pub fn catalog_request(
    username: &str,
    token: &str,
    game_id: u32,
    hash: Option<&str>,
) -> Option<Request> {
    let mut out: Option<Request> = None;
    let (username, token, hash) = (c(username), c(token), hash.map(c));
    unsafe {
        romp_rc_catalog_request(
            username.as_ptr(),
            token.as_ptr(),
            game_id,
            hash.as_ref().map_or(std::ptr::null(), |h| h.as_ptr()),
            std::ptr::from_mut(&mut out).cast(),
            collect_request,
        );
    }
    out
}

/// The game's id and its official achievements, or None when the answer is not usable.
pub fn parse_catalog(status: i32, body: &[u8]) -> Option<(u32, Vec<AchievementInfo>)> {
    let mut list = Vec::new();
    let game_id = unsafe {
        romp_rc_parse_catalog(
            body.as_ptr().cast(),
            body.len(),
            status,
            std::ptr::from_mut(&mut list).cast(),
            collect_achievement,
        )
    };
    (game_id != 0).then_some((game_id, list))
}

/// The request for the achievements a player has earned in a game.
pub fn unlocks_request(
    username: &str,
    token: &str,
    game_id: u32,
    hardcore: bool,
) -> Option<Request> {
    let mut out: Option<Request> = None;
    let (username, token) = (c(username), c(token));
    unsafe {
        romp_rc_unlocks_request(
            username.as_ptr(),
            token.as_ptr(),
            game_id,
            c_int::from(hardcore),
            std::ptr::from_mut(&mut out).cast(),
            collect_request,
        );
    }
    out
}

pub fn parse_unlocks(status: i32, body: &[u8]) -> Option<Vec<u32>> {
    let mut ids = Vec::new();
    let result = unsafe {
        romp_rc_parse_unlocks(
            body.as_ptr().cast(),
            body.len(),
            status,
            std::ptr::from_mut(&mut ids).cast(),
            collect_id,
        )
    };
    (result == 0).then_some(ids)
}

/// An achievement earned in play, as sent to RetroAchievements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unlock {
    pub achievement_id: u32,
    pub hardcore: bool,
    pub hash: String,
}

impl Unlock {
    /// The unlock an `awardachievement` request is sending, read from its form fields.
    pub fn from_request(post: &str) -> Option<Unlock> {
        let field = |name: &str| {
            post.split('&')
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
        };
        if field("r")? != "awardachievement" {
            return None;
        }
        Some(Unlock {
            achievement_id: field("a")?.parse().ok()?,
            hardcore: field("h") == Some("1"),
            hash: field("m").unwrap_or_default().to_string(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Award {
    Accepted,
    /// RetroAchievements answered and turned it down; sending it again won't help.
    Refused,
    /// RetroAchievements could not be reached, or had trouble; worth sending again later.
    Retry,
}

/// What became of an unlock, from the server's answer to its request.
pub fn award_outcome(status: i32, body: &[u8]) -> Award {
    if status <= 0 || status >= 500 || status == 429 {
        return Award::Retry;
    }
    let accepted = unsafe { romp_rc_award_accepted(body.as_ptr().cast(), body.len(), status) != 0 };
    if accepted {
        Award::Accepted
    } else {
        Award::Refused
    }
}

/// The request that sends an unlock earned `seconds_since_unlock` seconds ago.
pub fn award_request(
    username: &str,
    token: &str,
    unlock: &Unlock,
    seconds_since_unlock: u32,
) -> Option<Request> {
    let mut out: Option<Request> = None;
    let (username, token, hash) = (c(username), c(token), c(&unlock.hash));
    unsafe {
        romp_rc_award_request(
            username.as_ptr(),
            token.as_ptr(),
            unlock.achievement_id,
            c_int::from(unlock.hardcore),
            hash.as_ptr(),
            seconds_since_unlock,
            std::ptr::from_mut(&mut out).cast(),
            collect_request,
        );
    }
    out
}

/// Whether RetroAchievements allows a core setting while playing in hardcore.
pub fn setting_allowed(core_library: &str, key: &str, value: &str) -> bool {
    let (core_library, key, value) = (c(core_library), c(key), c(value));
    unsafe { romp_rc_setting_allowed(core_library.as_ptr(), key.as_ptr(), value.as_ptr()) != 0 }
}

/// Whether RetroAchievements allows this core for this console in hardcore.
pub fn system_allowed(core_library: &str, console_id: u32) -> bool {
    let core_library = c(core_library);
    unsafe { romp_rc_system_allowed(core_library.as_ptr(), console_id) != 0 }
}

/// One player's achievement session for one game.
pub struct Session {
    client: NonNull<c_void>,
    host: Box<Host>,
}

impl Session {
    pub fn new(hardcore: bool) -> Option<Self> {
        let mut host = Box::<Host>::default();
        let host_ptr = std::ptr::from_mut(host.as_mut()).cast::<c_void>();
        // SAFETY: the boxed host outlives the client, which is destroyed in Drop.
        let client = NonNull::new(unsafe { romp_rc_create(host_ptr, c_int::from(hardcore)) })?;
        host.client = Some(client);
        Some(Self { client, host })
    }

    pub fn sign_in(&mut self, username: &str, token: &str) {
        let (Ok(username), Ok(token)) = (CString::new(username), CString::new(token)) else {
            return self
                .host
                .events
                .push(AchievementEvent::SignInFailed("invalid sign-in".into()));
        };
        unsafe { romp_rc_login(self.client.as_ptr(), username.as_ptr(), token.as_ptr()) }
    }

    /// Identifies the game from its file, or from its contents when the core loaded it into memory.
    pub fn load_game(&mut self, console_id: u32, path: &Path, data: Option<&[u8]>) {
        let path = CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        let (ptr, len) = data.map_or((std::ptr::null(), 0), |d| (d.as_ptr(), d.len()));
        unsafe { romp_rc_load_game(self.client.as_ptr(), console_id, path.as_ptr(), ptr, len) }
    }

    /// Loads the game by the hash RetroAchievements knows it by, as RomM computes it.
    pub fn load_hash(&mut self, hash: &str) {
        let Ok(hash) = CString::new(hash) else { return };
        unsafe { romp_rc_load_hash(self.client.as_ptr(), hash.as_ptr()) }
    }

    pub fn hardcore(&self) -> bool {
        unsafe { romp_rc_hardcore(self.client.as_ptr()) != 0 }
    }

    pub fn set_hardcore(&mut self, enabled: bool) {
        unsafe { romp_rc_set_hardcore(self.client.as_ptr(), c_int::from(enabled)) }
    }

    /// RetroAchievements' number for the loaded game's console.
    pub fn console_id(&self) -> u32 {
        unsafe { romp_rc_console(self.client.as_ptr()) }
    }

    /// Where every achievement stands, to keep beside a save state.
    pub fn progress(&mut self) -> Vec<u8> {
        let size = unsafe { romp_rc_progress_size(self.client.as_ptr()) };
        let mut buffer = vec![0u8; size];
        let ok = size > 0
            && unsafe {
                romp_rc_serialize_progress(self.client.as_ptr(), buffer.as_mut_ptr(), size)
            } == 0;
        if ok {
            buffer
        } else {
            Vec::new()
        }
    }

    /// Puts achievements back where a save state left them; with no saved progress, they start over.
    pub fn restore_progress(&mut self, saved: Option<&[u8]>) {
        let (ptr, len) = saved.map_or((std::ptr::null(), 0), |s| (s.as_ptr(), s.len()));
        unsafe { romp_rc_deserialize_progress(self.client.as_ptr(), ptr, len) };
    }

    /// The game's achievements with what has been earned so far.
    pub fn achievements(&mut self) -> Vec<AchievementInfo> {
        let mut list = Vec::new();
        unsafe {
            romp_rc_list_achievements(
                self.client.as_ptr(),
                std::ptr::from_mut(&mut list).cast(),
                collect_achievement,
            );
        }
        list
    }

    pub fn do_frame(&mut self) {
        unsafe { rc_client_do_frame(self.client.as_ptr()) }
    }

    /// Keeps the session alive while the game is paused; call at least once a second.
    pub fn idle(&mut self) {
        unsafe { rc_client_idle(self.client.as_ptr()) }
    }

    pub fn reset(&mut self) {
        unsafe { rc_client_reset(self.client.as_ptr()) }
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.host.requests)
    }

    pub fn take_events(&mut self) -> Vec<AchievementEvent> {
        std::mem::take(&mut self.host.events)
    }

    /// Hands the server's answer to request `id` back to rcheevos; `status` is the HTTP status.
    pub fn respond(&mut self, id: u64, status: i32, body: &[u8]) {
        let Some(pending) = self.host.pending.remove(&id) else {
            return;
        };
        unsafe {
            romp_rc_respond(
                pending.callback,
                pending.data,
                body.as_ptr().cast(),
                body.len(),
                status,
            );
        }
    }

    /// The `rcheevos/x.y` part of the user agent, which the app adds to its own.
    pub fn user_agent_clause(&mut self) -> String {
        let mut buffer = [0 as c_char; 64];
        let len = unsafe {
            romp_rc_user_agent_clause(self.client.as_ptr(), buffer.as_mut_ptr(), buffer.len())
        };
        let bytes: Vec<u8> = buffer[..len.min(buffer.len())]
            .iter()
            .map(|c| *c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { romp_rc_destroy(self.client.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(request: &Request) -> &str {
        request.post.as_deref().unwrap_or_default()
    }

    fn sign_in() -> Session {
        let mut session = Session::new(false).unwrap();
        session.sign_in("player", "secret-token");
        let requests = session.take_requests();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].url.ends_with("/dorequest.php"),
            "{}",
            requests[0].url
        );
        assert!(
            post(&requests[0]).contains("r=login2"),
            "{}",
            post(&requests[0])
        );
        assert!(post(&requests[0]).contains("u=player"));
        assert!(post(&requests[0]).contains("t=secret-token"));
        session.respond(
            requests[0].id,
            200,
            br#"{"Success":true,"User":"player","DisplayName":"player","Token":"secret-token","Score":10,"SoftcoreScore":5,"Messages":0,"Permissions":1,"AccountType":"Registered"}"#,
        );
        assert_eq!(session.take_events(), [AchievementEvent::SignedIn]);
        session
    }

    #[test]
    fn signing_in_goes_through_the_callers_requests() {
        sign_in();
    }

    #[test]
    fn a_rejected_token_is_reported() {
        let mut session = Session::new(false).unwrap();
        session.sign_in("player", "stale");
        let id = session.take_requests()[0].id;
        session.respond(
            id,
            401,
            br#"{"Success":false,"Error":"Invalid token","Code":"invalid_credentials"}"#,
        );
        let events = session.take_events();
        assert!(
            matches!(&events[..], [AchievementEvent::SignInFailed(m)] if m.contains("Invalid token")),
            "{events:?}"
        );
    }

    #[test]
    fn achievement_lists_come_from_rcheevos_own_parser() {
        let request = catalog_request("player", "tok", 228, None).unwrap();
        assert!(
            post(&request).contains("r=achievementsets"),
            "{}",
            post(&request)
        );
        assert!(post(&request).contains("g=228"));
        let body = br#"{"Success":true,"GameId":228,"Title":"Super Metroid","ConsoleId":3,
            "ImageIconUrl":"https://media.retroachievements.org/Images/1.png","RichPresencePatch":"",
            "Sets":[{"AchievementSetId":1,"GameId":228,"Title":null,"Type":"core",
              "ImageIconUrl":"https://media.retroachievements.org/Images/1.png",
              "Achievements":[
                {"ID":7,"Title":"Missile","Description":"Find a missile","Flags":3,"Points":5,
                 "MemAddr":"0xH0000=1","Author":"a","BadgeName":"00001","Created":0,"Modified":0,
                 "Type":"","Rarity":50.0,"RarityHardcore":25.0,
                 "BadgeURL":"https://media.retroachievements.org/Badge/00001.png",
                 "BadgeLockedURL":"https://media.retroachievements.org/Badge/00001_lock.png"},
                {"ID":8,"Title":"Draft","Description":"Unofficial","Flags":5,"Points":1,
                 "MemAddr":"0xH0000=2","Author":"a","BadgeName":"00002","Created":0,"Modified":0}],
              "Leaderboards":[]}]}"#;
        let (game_id, list) = parse_catalog(200, body).unwrap();
        assert_eq!(game_id, 228);
        assert_eq!(list.len(), 1, "only official achievements are listed");
        assert_eq!(list[0].title, "Missile");
        assert_eq!(list[0].points, 5);
        assert_eq!(
            list[0].badge_url,
            "https://media.retroachievements.org/Badge/00001.png"
        );
        assert!(parse_catalog(200, br#"{"Success":false,"Error":"Unknown game"}"#).is_none());

        let request = unlocks_request("player", "tok", 228, false).unwrap();
        assert!(post(&request).contains("r=unlocks"));
        assert_eq!(
            parse_unlocks(
                200,
                br#"{"Success":true,"UserUnlocks":[7,9],"GameID":228,"HardcoreMode":false}"#
            ),
            Some(vec![7, 9])
        );
        assert_eq!(
            parse_unlocks(401, br#"{"Success":false,"Error":"bad token"}"#),
            None
        );
    }

    #[test]
    fn hardcore_follows_retroachievements_rules_for_cores() {
        let mut session = Session::new(true).unwrap();
        assert!(session.hardcore());
        session.set_hardcore(false);
        assert!(!session.hardcore());
        assert!(!setting_allowed("Snes9x", "snes9x_layer_1", "disabled"));
        assert!(setting_allowed("Snes9x", "snes9x_region", "auto"));
        assert!(setting_allowed("Some Core", "anything", "on"));
        assert!(system_allowed("Snes9x", 3));
    }

    static mut RAM: [u8; 0x800] = [0; 0x800];

    unsafe extern "C" fn ram_data(id: c_uint) -> *mut c_void {
        if id == 2 {
            std::ptr::addr_of_mut!(RAM).cast()
        } else {
            std::ptr::null_mut()
        }
    }

    unsafe extern "C" fn ram_size(id: c_uint) -> usize {
        if id == 2 {
            0x800
        } else {
            0
        }
    }

    fn answer(session: &mut Session, game: &[u8]) {
        for request in session.take_requests() {
            let body: &[u8] = if post(&request).contains("r=achievementsets") {
                game
            } else {
                br#"{"Success":true,"Unlocks":[],"HardcoreUnlocks":[],"ServerNow":1790850000}"#
            };
            session.respond(request.id, 200, body);
        }
    }

    #[test]
    fn achievements_read_the_game_memory_from_the_start() {
        set_core_memory(ram_data, ram_size);
        let mut session = sign_in();
        session.load_hash("0123456789abcdef0123456789abcdef");
        let game = br#"{"Success":true,"GameId":5,"Title":"Test","ConsoleId":7,
            "ImageIconUrl":"https://media.retroachievements.org/Images/1.png","RichPresencePatch":"",
            "Sets":[{"AchievementSetId":1,"GameId":5,"Title":null,"Type":"core",
              "ImageIconUrl":"https://media.retroachievements.org/Images/1.png",
              "Achievements":[{"ID":9,"Title":"Poke","Description":"RAM byte 0x10 becomes 1","Flags":3,
                "Points":1,"MemAddr":"0xH0010=1","Author":"a","BadgeName":"1","Created":0,"Modified":0}],
              "Leaderboards":[]}]}"#;
        answer(&mut session, game);
        answer(&mut session, game);
        assert!(matches!(
            &session.take_events()[..],
            [AchievementEvent::GameLoaded {
                achievements: 1,
                ..
            }]
        ),);
        session.do_frame();
        unsafe { RAM[0x10] = 1 };
        session.do_frame();
        let events = session.take_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AchievementEvent::Unlocked { id: 9, .. })),
            "an achievement checked while the game loaded must not be switched off: {events:?}"
        );
    }

    #[test]
    fn late_unlocks_are_sent_with_how_long_ago_they_happened() {
        let unlock = Unlock {
            achievement_id: 39673,
            hardcore: false,
            hash: "811b027eaf99c2def7b933c5208636de".into(),
        };
        let request = award_request("player", "tok", &unlock, 3600).unwrap();
        let body = post(&request);
        assert!(body.contains("r=awardachievement"), "{body}");
        assert!(body.contains("a=39673") && body.contains("h=0") && body.contains("o=3600"));
        assert_eq!(Unlock::from_request(body), Some(unlock));
        assert_eq!(Unlock::from_request("r=ping&u=player"), None);

        assert_eq!(award_outcome(-2, b""), Award::Retry);
        assert_eq!(award_outcome(503, b"busy"), Award::Retry);
        assert_eq!(
            award_outcome(200, br#"{"Success":true,"Score":10,"SoftcoreScore":10,"AchievementID":39673,"AchievementsRemaining":3}"#),
            Award::Accepted
        );
        assert_eq!(
            award_outcome(200, br#"{"Success":false,"Error":"User already has this achievement awarded.","Score":10,"SoftcoreScore":10,"AchievementID":39673,"AchievementsRemaining":3}"#),
            Award::Accepted
        );
        assert_eq!(
            award_outcome(200, br#"{"Success":false,"Error":"Achievement not found"}"#),
            Award::Refused
        );
    }

    #[test]
    fn games_are_identified_by_the_hash_of_their_contents() {
        use md5::Digest;
        let mut session = sign_in();
        let rom = vec![0x42u8; 32 * 1024];
        session.load_game(4, Path::new("Game.gb"), Some(&rom));
        let requests = session.take_requests();
        let hash: String = md5::Md5::digest(&rom)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(
            requests.iter().any(|r| post(r).contains(&hash)),
            "{requests:?}"
        );
        assert!(session.user_agent_clause().starts_with("rcheevos/"));
    }
}
