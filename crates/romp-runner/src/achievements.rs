use crate::archive::LoadedRom;
use romp_cheevos::Session;
use romp_proto::msg::{AchievementEvent, AchievementInfo, RunnerMsg};

/// The runner's side of RetroAchievements: rcheevos watches the game while the app does the talking.
pub struct Achievements {
    session: Session,
    console_id: u32,
    hash: Option<String>,
    agent: String,
}

/// `core/version rcheevos/x.y`, the part of the user agent only the runner knows.
pub fn agent(core_name: &str, core_version: &str, clause: &str) -> String {
    let name: String = core_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let version = core_version.split_whitespace().next().unwrap_or("0");
    format!("{name}/{version} {clause}").trim().to_string()
}

pub struct Tick {
    pub messages: Vec<RunnerMsg>,
    pub reset_core: bool,
}

impl Achievements {
    pub fn start(
        username: &str,
        token: &str,
        hardcore: bool,
        console_id: u32,
        hash: Option<String>,
        core: (&str, &str),
    ) -> Option<Self> {
        let mut session = Session::new(hardcore)?;
        session.sign_in(username, token);
        let agent = agent(core.0, core.1, &session.user_agent_clause());
        Some(Self {
            session,
            console_id,
            hash,
            agent,
        })
    }

    pub fn respond(&mut self, id: u64, status: i32, body: &[u8]) {
        self.session.respond(id, status, body);
    }

    pub fn reset(&mut self) {
        self.session.reset();
    }

    pub fn list(&mut self) -> Vec<AchievementInfo> {
        self.session.achievements()
    }

    /// Runs after each emulated frame, or while paused, and collects what the app should hear.
    pub fn tick(&mut self, ran_frame: bool, rom: &LoadedRom) -> Tick {
        if ran_frame {
            self.session.do_frame();
        } else {
            self.session.idle();
        }
        let mut reset_core = false;
        let mut messages = Vec::new();
        for event in self.session.take_events() {
            match &event {
                AchievementEvent::SignedIn => match &self.hash {
                    Some(hash) => self.session.load_hash(hash),
                    None => self.session.load_game(
                        self.console_id,
                        &rom.effective_path,
                        rom.bytes.as_deref(),
                    ),
                },
                AchievementEvent::Reset => reset_core = true,
                _ => {}
            }
            messages.push(RunnerMsg::Achievement(event));
        }
        messages.extend(self.session.take_requests().into_iter().map(|r| {
            RunnerMsg::AchievementsRequest {
                id: r.id,
                url: r.url,
                post: r.post,
                content_type: r.content_type,
                agent: self.agent.clone(),
            }
        }));
        Tick {
            messages,
            reset_core,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_is_named_without_spaces_in_the_user_agent() {
        assert_eq!(
            agent("Beetle PSX HW", "0.9.44.1 4e608ad", "rcheevos/12.5"),
            "Beetle_PSX_HW/0.9.44.1 rcheevos/12.5"
        );
        assert_eq!(
            agent("Snes9x", "", "rcheevos/12.5"),
            "Snes9x/0 rcheevos/12.5"
        );
    }
}
