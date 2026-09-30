use super::{with_controller, Controller};
use crate::romm::client::{Client, Error};
use slint::{Timer, TimerMode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NOW_PLAYING_EVERY: Duration = Duration::from_secs(30);
const SHORTEST_SESSION: Duration = Duration::from_secs(30);

pub(super) struct Session {
    rom_id: i64,
    started: SystemTime,
    clock: Instant,
    _now_playing: Timer,
}

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// A session worth recording, as its start and end in milliseconds since 1970.
fn recorded(started: SystemTime, played: Duration) -> Option<(i64, i64)> {
    let start = millis(started);
    (played >= SHORTEST_SESSION).then(|| (start, start + played.as_millis() as i64))
}

impl Controller {
    fn activity_client(&self) -> Option<Client> {
        self.client
            .borrow()
            .clone()
            .filter(|_| !self.offline.get() && self.has_scope("roms.user.write"))
    }

    fn send_now_playing(&self, rom_id: i64) {
        let (Some(client), Some(device)) = (self.activity_client(), self.sync_device()) else {
            return;
        };
        self.shared.rt.spawn(async move {
            if let Err(e) = client.now_playing(rom_id, &device).await {
                tracing::debug!("now playing {rom_id}: {e}");
            }
        });
    }

    /// Tells RomM a game is running, until `end_session`.
    pub(super) fn start_session(&self, rom_id: i64) {
        self.send_now_playing(rom_id);
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, NOW_PLAYING_EVERY, move || {
            with_controller(|c| c.send_now_playing(rom_id));
        });
        *self.session.borrow_mut() = Some(Session {
            rom_id,
            started: SystemTime::now(),
            clock: Instant::now(),
            _now_playing: timer,
        });
    }

    pub(super) fn end_session(&self) {
        let Some(session) = self.session.borrow_mut().take() else {
            return;
        };
        if let (Some(client), Some(device)) = (self.activity_client(), self.sync_device()) {
            self.shared.rt.spawn(async move {
                let _ = client.clear_activity(&device).await;
            });
        }
        if let Some((start, end)) = recorded(session.started, session.clock.elapsed()) {
            self.shared
                .store
                .lock()
                .unwrap()
                .add_play_session(session.rom_id, start, end);
        }
        self.upload_play_sessions();
    }

    pub(super) fn upload_play_sessions(&self) {
        let Some(client) = self.activity_client() else {
            return;
        };
        let sessions = self.shared.store.lock().unwrap().play_sessions();
        if sessions.is_empty() {
            return;
        }
        let device = self.sync_device();
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            match client
                .upload_play_sessions(device.as_deref(), &sessions)
                .await
            {
                Err(Error::Unreachable) => return,
                Err(e) => tracing::warn!("uploading play time: {e}"),
                Ok(()) => {}
            }
            let ids: Vec<i64> = sessions.iter().map(|s| s.id).collect();
            store.lock().unwrap().remove_play_sessions(&ids);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_sessions_are_not_recorded() {
        let started = UNIX_EPOCH + Duration::from_secs(1_790_762_400);
        assert_eq!(recorded(started, Duration::from_secs(29)), None);
        assert_eq!(
            recorded(started, Duration::from_secs(90)),
            Some((1_790_762_400_000, 1_790_762_490_000))
        );
    }
}
