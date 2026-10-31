use super::Controller;
use crate::whats_new::{due, notes, CHANGELOG, VERSION};
use crate::NoteSection;
use slint::{ModelRc, SharedString, VecModel};
use tr::tr;

/// The version whose notes were last shown, so each version shows them once.
const SEEN_KEY: &str = "whats_new_seen";

fn heading(title: &str) -> String {
    match title {
        "Added" => tr!("New"),
        "Changed" => tr!("Changed"),
        "Fixed" => tr!("Fixed"),
        "Removed" => tr!("Removed"),
        "Security" => tr!("Security"),
        other => other.to_string(),
    }
}

impl Controller {
    /// Opens this version's notes, from the About window or by themselves after an update.
    pub(super) fn show_whats_new(&self) {
        let Some(ui) = self.ui() else { return };
        let sections: Vec<NoteSection> = notes(CHANGELOG, VERSION)
            .into_iter()
            .map(|section| NoteSection {
                title: heading(&section.title).into(),
                items: ModelRc::new(VecModel::from(
                    section
                        .items
                        .into_iter()
                        .map(SharedString::from)
                        .collect::<Vec<_>>(),
                )),
            })
            .collect();
        ui.set_whats_new_version(VERSION.into());
        ui.set_whats_new_notes(ModelRc::new(VecModel::from(sections)));
        ui.set_whats_new_after_updates(self.prefs.get().whats_new);
        ui.set_whats_new_open(true);
    }

    /// Shows the notes the first time this version reaches the library.
    pub(super) fn show_whats_new_if_due(&self) {
        let seen = self.shared.store.lock().unwrap().get(SEEN_KEY);
        if !due(seen.as_deref(), VERSION, self.prefs.get().whats_new) {
            return;
        }
        self.shared.store.lock().unwrap().set(SEEN_KEY, VERSION);
        if !notes(CHANGELOG, VERSION).is_empty() {
            self.show_whats_new();
        }
    }

    pub(super) fn set_whats_new_after_updates(&self, on: bool) {
        let mut prefs = self.prefs.get();
        prefs.whats_new = on;
        self.prefs.set(prefs);
        self.shared
            .store
            .lock()
            .unwrap()
            .set("prefs", &prefs.to_json());
    }
}
