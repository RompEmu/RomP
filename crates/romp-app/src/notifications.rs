//! The player's RomM notifications: how each reads, and which are still unread.
use crate::romm::types::RemoteNotification;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tr::tr;

/// A notification as the inbox shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub level: String,
    pub when: String,
    pub unread: bool,
}

/// RomM leaves the title of a kind it knows for the client to word; any other comes with its own.
pub fn title(n: &RemoteNotification) -> String {
    if let Some(title) = n.title.as_deref().filter(|t| !t.trim().is_empty()) {
        return title.to_string();
    }
    match n.kind.as_str() {
        "scan_completed" => tr!("Library scan finished"),
        "scan_failed" => tr!("Library scan failed"),
        "task_completed" => tr!("A task finished"),
        "task_failed" => tr!("A task failed"),
        "streaming_session_ended" => tr!("Your stream ended"),
        "role_changed" => tr!("Your role changed"),
        "channel_disabled" => tr!("A notification channel was turned off"),
        "device_install_completed" => tr!("A game was installed on a device"),
        "device_install_failed" => tr!("A game couldn't be installed on a device"),
        _ => tr!("Notification"),
    }
}

pub fn unread_ids(list: &[RemoteNotification]) -> Vec<i64> {
    list.iter()
        .filter(|n| n.read_at.is_none())
        .map(|n| n.id)
        .collect()
}

pub fn rows(list: &[RemoteNotification], now: SystemTime) -> Vec<Row> {
    list.iter()
        .map(|n| Row {
            id: n.id,
            title: title(n),
            body: n.body.clone().unwrap_or_default(),
            level: n.level.clone(),
            when: crate::sync::parse_iso(&n.created_at)
                .and_then(|ms| u64::try_from(ms).ok())
                .map(|ms| crate::slots::when(UNIX_EPOCH + Duration::from_millis(ms), now))
                .unwrap_or_default(),
            unread: n.read_at.is_none(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: i64, kind: &str, title: Option<&str>, read: bool) -> RemoteNotification {
        RemoteNotification {
            id,
            kind: kind.into(),
            level: "info".into(),
            title: title.map(Into::into),
            body: Some("Details".into()),
            read_at: read.then(|| "2026-10-17T09:00:00Z".into()),
            created_at: "2026-10-17T09:00:00Z".into(),
        }
    }

    #[test]
    fn known_kinds_are_worded_here_and_others_keep_their_title() {
        assert_eq!(
            title(&notification(1, "scan_completed", None, false)),
            "Library scan finished"
        );
        assert_eq!(
            title(&notification(
                2,
                "custom",
                Some("Server maintenance"),
                false
            )),
            "Server maintenance"
        );
        assert_eq!(
            title(&notification(3, "something_new", None, false)),
            "Notification"
        );
        assert_eq!(
            title(&notification(4, "scan_failed", Some("  "), false)),
            "Library scan failed"
        );
    }

    #[test]
    fn rows_say_when_and_which_are_unread() {
        let list = [
            notification(2, "custom", Some("New"), false),
            notification(1, "custom", Some("Old"), true),
        ];
        assert_eq!(unread_ids(&list), [2]);
        let now = UNIX_EPOCH + Duration::from_secs(1_792_227_600 + 5 * 60);
        let shown = rows(&list, now);
        assert_eq!(shown[0].title, "New");
        assert_eq!(shown[0].body, "Details");
        assert!(shown[0].unread && !shown[1].unread);
        assert_eq!(shown[0].when, "5 minutes ago");
    }
}
