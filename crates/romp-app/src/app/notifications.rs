use super::{on_ui, with_controller, Controller};
use crate::notifications::{rows, unread_ids};
use crate::romm::types::RemoteNotification;
use crate::NotificationRow;
use slint::{ModelRc, Timer, TimerMode, VecModel};
use std::time::{Duration, SystemTime};

/// RomM only pushes notifications to browser sessions, so RomP asks for them this often.
const POLL_EVERY: Duration = Duration::from_secs(60);

impl Controller {
    pub(super) fn start_notifications(&self) {
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, POLL_EVERY, || {
            with_controller(|c| c.refresh_notifications());
        });
        *self.notifications_timer.borrow_mut() = Some(timer);
    }

    pub(super) fn refresh_notifications(&self) {
        if self.offline.get() {
            return;
        }
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        self.shared.rt.spawn(async move {
            match client.notifications().await {
                Ok(list) => on_ui(move |c| c.show_notifications(list)),
                Err(e) => tracing::debug!("notifications: {e}"),
            }
        });
    }

    /// `None` is RomM before 5.4, which keeps no notifications, so the bell goes away.
    fn show_notifications(&self, list: Option<Vec<RemoteNotification>>) {
        let Some(ui) = self.ui() else { return };
        ui.set_notifications_shown(list.is_some());
        let list = list.unwrap_or_default();
        ui.set_notifications_unread(unread_ids(&list).len() as i32);
        let shown: Vec<NotificationRow> = rows(&list, SystemTime::now())
            .into_iter()
            .map(|r| NotificationRow {
                id: r.id as i32,
                title: r.title.into(),
                body: r.body.into(),
                level: r.level.into(),
                when: r.when.into(),
                unread: r.unread,
            })
            .collect();
        ui.set_notifications(ModelRc::new(VecModel::from(shown)));
        *self.notifications.borrow_mut() = list;
    }

    /// Opening the inbox reads what's in it.
    pub(super) fn open_notifications(&self) {
        let Some(ui) = self.ui() else { return };
        ui.set_notifications_open(true);
        let unread = unread_ids(&self.notifications.borrow());
        if unread.is_empty() {
            return;
        }
        ui.set_notifications_unread(0);
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        self.shared.rt.spawn(async move {
            if let Err(e) = client.mark_notifications_read(&unread).await {
                tracing::debug!("marking notifications read: {e}");
            }
            on_ui(|c| c.refresh_notifications());
        });
    }

    pub(super) fn dismiss_notification(&self, id: i64) {
        let remaining: Vec<RemoteNotification> = self
            .notifications
            .borrow()
            .iter()
            .filter(|n| n.id != id)
            .cloned()
            .collect();
        self.show_notifications(Some(remaining));
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        self.shared.rt.spawn(async move {
            if let Err(e) = client.dismiss_notification(id).await {
                tracing::debug!("dismissing notification {id}: {e}");
                on_ui(|c| c.refresh_notifications());
            }
        });
    }

    pub(super) fn dismiss_all_notifications(&self) {
        self.show_notifications(Some(Vec::new()));
        let Some(client) = self.client.borrow().clone() else {
            return;
        };
        self.shared.rt.spawn(async move {
            if let Err(e) = client.dismiss_all_notifications().await {
                tracing::debug!("dismissing notifications: {e}");
                on_ui(|c| c.refresh_notifications());
            }
        });
    }
}
