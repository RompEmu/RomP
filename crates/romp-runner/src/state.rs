use crate::frontend::Frontend;
use romp_libretro as lr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{info, warn};

const SRAM_CHECK: Duration = Duration::from_secs(2);

pub fn slot_file_name(slot: u8) -> String {
    if (1..=5).contains(&slot) {
        format!("slot-{slot}.state")
    } else {
        "auto.state".to_string()
    }
}

pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

pub struct StateManager {
    dir: PathBuf,
    last_saved_sram: Option<Vec<u8>>,
    last_check: Instant,
}

impl StateManager {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            last_saved_sram: None,
            last_check: Instant::now(),
        }
    }

    fn sram_path(&self) -> PathBuf {
        self.dir.join("game.srm")
    }

    pub fn load_initial_sram(&mut self, core: &mut lr::Core, frontend: &mut Frontend) {
        let Some(region) = (unsafe { core.memory_region(lr::RETRO_MEMORY_SAVE_RAM, frontend) })
        else {
            return;
        };
        match std::fs::read(self.sram_path()) {
            Ok(bytes) if bytes.len() == region.len() => {
                region.copy_from_slice(&bytes);
                info!(len = bytes.len(), "SRAM loaded");
            }
            Ok(bytes) => warn!(
                file = bytes.len(),
                core = region.len(),
                "SRAM size mismatch; ignoring file"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warn!("read SRAM: {e}"),
        }
        self.last_saved_sram = Some(region.to_vec());
    }

    pub fn tick_sram(&mut self, core: &mut lr::Core, frontend: &mut Frontend) -> bool {
        if self.last_check.elapsed() < SRAM_CHECK {
            return false;
        }
        self.last_check = Instant::now();
        self.flush_sram(core, frontend)
    }

    pub fn flush_sram(&mut self, core: &mut lr::Core, frontend: &mut Frontend) -> bool {
        let Some(snapshot) = (unsafe { core.memory_region(lr::RETRO_MEMORY_SAVE_RAM, frontend) })
            .map(|r| r.to_vec())
        else {
            return false;
        };
        if self.last_saved_sram.as_deref() == Some(&snapshot[..]) {
            return false;
        }
        match write_atomic(&self.sram_path(), &snapshot) {
            Ok(()) => {
                self.last_saved_sram = Some(snapshot);
                true
            }
            Err(e) => {
                warn!("write SRAM: {e}");
                false
            }
        }
    }

    pub fn save_state(&self, slot: u8, core: &mut lr::Core, frontend: &mut Frontend) -> bool {
        let data = match core.serialize(frontend) {
            Ok(d) => d,
            Err(e) => {
                warn!("serialize: {e}");
                return false;
            }
        };
        match write_atomic(&self.dir.join(slot_file_name(slot)), trimmed(&data)) {
            Ok(()) => true,
            Err(e) => {
                warn!("write state: {e}");
                false
            }
        }
    }

    pub fn load_state(&self, slot: u8, core: &mut lr::Core, frontend: &mut Frontend) -> bool {
        match self.try_load_state(slot, core, frontend) {
            Ok(()) => true,
            Err(e) => {
                warn!("{e}");
                false
            }
        }
    }

    pub fn try_load_state(
        &self,
        slot: u8,
        core: &mut lr::Core,
        frontend: &mut Frontend,
    ) -> Result<(), String> {
        let data = std::fs::read(self.dir.join(slot_file_name(slot)))
            .map_err(|e| format!("read state: {e}"))?;
        let data = padded(data, core.serialize_size(frontend));
        core.unserialize(&data, frontend)
            .map_err(|e| format!("unserialize: {e}"))
    }

    pub fn save_on_shutdown(
        &mut self,
        core: &mut lr::Core,
        frontend: &mut Frontend,
        auto_state: bool,
    ) {
        self.flush_sram(core, frontend);
        if auto_state {
            self.save_state(0, core, frontend);
        }
    }
}

// Some cores report a fixed maximum size and leave the rest zeroed; the zeros are
// not stored, and come back as padding when the state is loaded.
fn trimmed(data: &[u8]) -> &[u8] {
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    &data[..end.max(1).min(data.len())]
}

fn padded(mut data: Vec<u8>, size: usize) -> Vec<u8> {
    if data.len() < size {
        data.resize(size, 0);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_zeros_are_not_stored_and_come_back_on_load() {
        let state = [7, 0, 3, 0, 0, 0, 0, 0];
        let stored = trimmed(&state);
        assert_eq!(stored, [7, 0, 3]);
        assert_eq!(padded(stored.to_vec(), state.len()), state);
        assert_eq!(padded(state.to_vec(), 4), state);
        assert_eq!(trimmed(&[0, 0]), [0]);
    }

    #[test]
    fn slot_names() {
        assert_eq!(slot_file_name(0), "auto.state");
        assert_eq!(slot_file_name(1), "slot-1.state");
        assert_eq!(slot_file_name(5), "slot-5.state");
        assert_eq!(slot_file_name(9), "auto.state");
    }

    #[test]
    fn write_atomic_replaces_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.srm");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
