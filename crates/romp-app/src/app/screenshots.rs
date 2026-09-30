use super::Controller;
use crate::romm::client::Error;
use crate::Shot;
use slint::Model;
use std::path::{Path, PathBuf};

impl Controller {
    pub(super) fn screenshot_taken(&self, rom_id: i64, path: &Path) {
        self.shared
            .store
            .lock()
            .unwrap()
            .add_pending_screenshot(rom_id, &path.to_string_lossy());
        self.upload_screenshots();
        let Some(image) = slint::Image::load_from_path(path).ok() else {
            return;
        };
        self.with_game(rom_id, |g| {
            g.shots.insert(
                0,
                Shot {
                    image,
                    loaded: true,
                },
            );
        });
    }

    pub(super) fn upload_screenshots(&self) {
        let client = self.client.borrow().clone();
        let Some(client) = client.filter(|_| !self.offline.get() && self.has_scope("assets.write"))
        else {
            return;
        };
        let pending = self.shared.store.lock().unwrap().pending_screenshots();
        if pending.is_empty() {
            return;
        }
        let store = self.shared.store.clone();
        self.shared.rt.spawn(async move {
            for (rom_id, path) in pending {
                let file = PathBuf::from(&path);
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let result = match tokio::fs::read(&file).await {
                    Ok(bytes) => client.upload_screenshot(rom_id, &name, bytes).await,
                    Err(_) => Ok(()),
                };
                match result {
                    Err(Error::Unreachable) => return,
                    Err(e) => tracing::warn!("uploading screenshot {path}: {e}"),
                    Ok(()) => {}
                }
                store.lock().unwrap().remove_pending_screenshot(&path);
            }
        });
    }

    /// Screenshots taken on this computer, newest first, to show before RomM's.
    pub(super) fn local_shots(&self, rom_id: i64) -> Vec<Shot> {
        let dir = crate::paths::game_save_dir(&crate::paths::data_dir(), &self.server(), rom_id)
            .join(crate::screenshot::FOLDER);
        crate::screenshot::local(&dir)
            .iter()
            .filter_map(|p| slint::Image::load_from_path(p).ok())
            .map(|image| Shot {
                image,
                loaded: true,
            })
            .collect()
    }

    pub(super) fn shot_count(&self, rom_id: i64) -> usize {
        self.with_game(rom_id, |g| g.shots.row_count()).unwrap_or(0)
    }
}
