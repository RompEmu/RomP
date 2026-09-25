use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    std::env::var_os("CARTRIDGE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("Cartridge")
        })
}

pub fn db_path() -> PathBuf {
    data_dir().join("cartridge.db")
}

pub fn runner_exe() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("cartridge-runner"))
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

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
