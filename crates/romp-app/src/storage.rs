use crate::paths;
use std::path::{Path, PathBuf};

pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            Ok(t) if t.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

pub fn categories() -> Vec<(&'static str, PathBuf)> {
    vec![
        ("Games", paths::roms_dir()),
        ("Saves and states", paths::data_dir().join("saves")),
        ("BIOS files", paths::system_dir()),
        ("Emulators", paths::cores_dir()),
        ("Images", paths::covers_dir()),
    ]
}

pub fn clear_dir(path: &Path) {
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_add_up_nested_files_and_missing_dirs_are_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), [0u8; 10]).unwrap();
        std::fs::create_dir_all(dir.path().join("x/y")).unwrap();
        std::fs::write(dir.path().join("x/y/b"), [0u8; 5]).unwrap();
        assert_eq!(dir_size(dir.path()), 15);
        assert_eq!(dir_size(&dir.path().join("missing")), 0);
    }

    #[test]
    fn clearing_keeps_the_folder_but_empties_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b"), b"y").unwrap();
        clear_dir(dir.path());
        assert!(dir.path().exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
