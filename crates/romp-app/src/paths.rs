use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    std::env::var_os("ROMP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("Romp")
        })
}

const LEGACY_DIR: &str = "Cartridge";
const LEGACY_DB: &str = "cartridge.db";
const DB_FILE: &str = "romp.db";

pub fn legacy_data_dir(data: &std::path::Path) -> PathBuf {
    data.with_file_name(LEGACY_DIR)
}

pub fn adopt_legacy_data(data: &std::path::Path) -> std::io::Result<Option<PathBuf>> {
    let legacy = legacy_data_dir(data);
    let moved = !data.exists() && legacy.is_dir();
    if moved {
        std::fs::rename(&legacy, data)?;
    }
    if !data.join(DB_FILE).exists() {
        for suffix in ["", "-wal", "-shm"] {
            let old = data.join(format!("{LEGACY_DB}{suffix}"));
            if old.exists() {
                std::fs::rename(old, data.join(format!("{DB_FILE}{suffix}")))?;
            }
        }
    }
    Ok(moved.then_some(legacy))
}

pub fn cores_dir() -> PathBuf {
    data_dir().join("cores")
}

pub fn system_dir() -> PathBuf {
    data_dir().join("system")
}

pub fn roms_dir() -> PathBuf {
    data_dir().join("roms")
}

pub fn covers_dir() -> PathBuf {
    data_dir().join("covers")
}

pub fn db_path() -> PathBuf {
    data_dir().join(DB_FILE)
}

pub fn runner_exe() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?
        .with_file_name(format!("romp-runner{}", std::env::consts::EXE_SUFFIX)))
}

pub fn local_save_dir_name(rom: &std::path::Path) -> String {
    let stem = rom
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    format!(
        "local-{stem}-{:016x}",
        fnv1a(rom.as_os_str().as_encoded_bytes())
    )
}

pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn server_key(server: &str) -> String {
    format!(
        "server-{:08x}",
        fnv1a(server.trim_end_matches('/').as_bytes()) as u32
    )
}

pub fn server_roms_dir(server: &str) -> PathBuf {
    roms_dir().join(server_key(server))
}

pub fn game_save_dir(data: &std::path::Path, server: &str, rom_id: i64) -> PathBuf {
    let saves = data.join("saves");
    let dir = saves.join(server_key(server)).join(rom_id.to_string());
    let legacy = saves.join(rom_id.to_string());
    if !dir.exists() && legacy.is_dir() {
        if let Some(parent) = dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&legacy, &dir);
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_data_folder_and_database_move_to_the_new_names() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("Cartridge");
        std::fs::create_dir_all(old.join("saves/7")).unwrap();
        std::fs::write(old.join("cartridge.db"), b"db").unwrap();
        std::fs::write(old.join("cartridge.db-wal"), b"wal").unwrap();
        let data = root.path().join("Romp");
        assert_eq!(adopt_legacy_data(&data).unwrap(), Some(old.clone()));
        assert!(!old.exists());
        assert!(data.join("saves/7").is_dir());
        assert_eq!(std::fs::read(data.join("romp.db")).unwrap(), b"db");
        assert_eq!(std::fs::read(data.join("romp.db-wal")).unwrap(), b"wal");
        assert_eq!(
            adopt_legacy_data(&data).unwrap(),
            None,
            "nothing left to move"
        );
    }

    #[test]
    fn existing_new_data_is_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("Cartridge")).unwrap();
        let data = root.path().join("Romp");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("romp.db"), b"new").unwrap();
        assert_eq!(adopt_legacy_data(&data).unwrap(), None);
        assert!(root.path().join("Cartridge").exists());
        assert_eq!(std::fs::read(data.join("romp.db")).unwrap(), b"new");
    }
    use std::path::Path;

    #[test]
    fn same_stem_different_files_get_different_dirs() {
        let gb = local_save_dir_name(Path::new("/roms/Tetris.gb"));
        let gbc = local_save_dir_name(Path::new("/roms/Tetris.gbc"));
        assert_ne!(gb, gbc);
        assert!(gb.starts_with("local-Tetris-"), "{gb}");
    }

    #[test]
    fn name_is_stable_for_the_same_path() {
        let a = local_save_dir_name(Path::new("/roms/Zelda.sfc"));
        assert_eq!(a, local_save_dir_name(Path::new("/roms/Zelda.sfc")));
        assert_eq!(a, "local-Zelda-2de6076057fa8b32");
    }

    #[test]
    fn missing_stem_still_gives_a_usable_name() {
        assert!(local_save_dir_name(Path::new("/")).starts_with("local-game-"));
    }

    #[test]
    fn server_keys_are_stable_and_ignore_trailing_slash() {
        let key = server_key("http://romm.tvpc.home/");
        assert_eq!(key, server_key("http://romm.tvpc.home"));
        assert!(key.starts_with("server-") && key.len() == "server-".len() + 8);
        assert_ne!(key, server_key("http://other/"));
    }

    #[test]
    fn legacy_saves_move_into_the_server_folder() {
        let data = tempfile::tempdir().unwrap();
        let legacy = data.path().join("saves/7");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("game.srm"), b"s").unwrap();
        let dir = game_save_dir(data.path(), "http://a/", 7);
        assert_eq!(
            dir,
            data.path()
                .join("saves")
                .join(server_key("http://a/"))
                .join("7")
        );
        assert_eq!(std::fs::read(dir.join("game.srm")).unwrap(), b"s");
        assert!(!legacy.exists());
        assert_eq!(
            game_save_dir(data.path(), "http://b/", 7),
            data.path()
                .join("saves")
                .join(server_key("http://b/"))
                .join("7")
        );
    }
}
