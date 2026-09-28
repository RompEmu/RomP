use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::info;

pub struct LoadedRom {
    pub effective_path: PathBuf,
    pub bytes: Option<Vec<u8>>,
}

pub fn open_rom(
    rom_path: &Path,
    session_id: &str,
    need_fullpath: bool,
    valid_extensions: &str,
) -> Result<LoadedRom> {
    let ext = rom_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    if let Some(ext_str) = ext.as_deref() {
        // .7z is never passed through: cores that list it often cannot decode it.
        if ext_str == "zip" && core_accepts_extension(valid_extensions, ext_str) {
            info!(
                archive = %rom_path.display(),
                ext = %ext_str,
                "core consumes archive natively; passing through without extraction"
            );
            let bytes = if !need_fullpath {
                Some(
                    std::fs::read(rom_path)
                        .with_context(|| format!("read {}", rom_path.display()))?,
                )
            } else {
                None
            };
            return Ok(LoadedRom {
                effective_path: rom_path.to_path_buf(),
                bytes,
            });
        }
        if let Some(disc) = detect_disc_image_kind(rom_path, ext_str)? {
            return extract_disc_image_archive(rom_path, ext_str, session_id, disc);
        }
        match ext_str {
            "zip" => return open_zip(rom_path, session_id, need_fullpath, valid_extensions),
            "7z" => return open_7z(rom_path, session_id, need_fullpath, valid_extensions),
            _ => {}
        }
    }
    let bytes = if !need_fullpath {
        Some(std::fs::read(rom_path).with_context(|| format!("read {}", rom_path.display()))?)
    } else {
        None
    };
    Ok(LoadedRom {
        effective_path: rom_path.to_path_buf(),
        bytes,
    })
}

fn core_accepts_extension(valid_extensions: &str, ext: &str) -> bool {
    valid_extensions
        .split('|')
        .any(|e| e.trim().eq_ignore_ascii_case(ext))
}

fn open_zip(
    zip_path: &Path,
    session_id: &str,
    need_fullpath: bool,
    valid_extensions: &str,
) -> Result<LoadedRom> {
    let f = std::fs::File::open(zip_path)
        .with_context(|| format!("open archive {}", zip_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(f).with_context(|| format!("read zip {}", zip_path.display()))?;

    let mut candidates: Vec<(usize, String, u64)> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index_raw(i)
            .with_context(|| format!("scan zip entry {i}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let Some(entry_ext) = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
        else {
            continue;
        };
        if core_accepts_extension(valid_extensions, &entry_ext) {
            candidates.push((i, name, entry.size()));
        }
    }
    if candidates.is_empty() {
        return Err(anyhow!(
            "no recognised ROM entries in archive {}",
            zip_path.display()
        ));
    }
    candidates.sort_by_key(|c| std::cmp::Reverse(c.2));
    let (idx, name, size) = candidates[0].clone();
    if candidates.len() > 1 {
        let list: Vec<String> = candidates
            .iter()
            .map(|(_, n, s)| format!("{n} ({s} B)"))
            .collect();
        info!(
            archive = %zip_path.display(),
            chosen = %name,
            chosen_size = size,
            candidates = %list.join(", "),
            "multi-rom archive; picked largest"
        );
    } else {
        info!(archive = %zip_path.display(), entry = %name, size, "extracted ROM from archive");
    }

    let mut entry = archive
        .by_index(idx)
        .with_context(|| format!("decompress zip entry {name}"))?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut bytes)
        .with_context(|| format!("decompress zip entry {name}"))?;

    if !need_fullpath {
        let file_name = Path::new(&name)
            .file_name()
            .ok_or_else(|| anyhow!("invalid entry name in archive: {name}"))?;
        let effective_path = zip_path
            .parent()
            .map(|p| p.join(file_name))
            .unwrap_or_else(|| PathBuf::from(file_name));
        return Ok(LoadedRom {
            effective_path,
            bytes: Some(bytes),
        });
    }

    let base = scratch_dir(session_id);
    std::fs::create_dir_all(&base).with_context(|| format!("mkdir {}", base.display()))?;
    let file_name = Path::new(&name)
        .file_name()
        .ok_or_else(|| anyhow!("invalid entry name in archive: {name}"))?;
    let out_path = base.join(file_name);
    std::fs::write(&out_path, &bytes).with_context(|| format!("write {}", out_path.display()))?;
    let canon = out_path.canonicalize().unwrap_or(out_path);
    Ok(LoadedRom {
        effective_path: canon,
        bytes: None,
    })
}

fn open_7z(
    archive_path: &Path,
    session_id: &str,
    need_fullpath: bool,
    valid_extensions: &str,
) -> Result<LoadedRom> {
    let mut reader = sevenz_rust::SevenZReader::open(archive_path, sevenz_rust::Password::empty())
        .with_context(|| format!("read 7z {}", archive_path.display()))?;

    let mut candidates: Vec<(String, u64)> = Vec::new();
    for entry in &reader.archive().files {
        if entry.is_directory() {
            continue;
        }
        let Some(entry_ext) = Path::new(entry.name())
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
        else {
            continue;
        };
        if core_accepts_extension(valid_extensions, &entry_ext) {
            candidates.push((entry.name().to_string(), entry.size()));
        }
    }
    if candidates.is_empty() {
        return Err(anyhow!(
            "no recognised ROM entries in archive {}",
            archive_path.display()
        ));
    }
    candidates.sort_by_key(|c| std::cmp::Reverse(c.1));
    let (name, size) = candidates[0].clone();
    if candidates.len() > 1 {
        let list: Vec<String> = candidates
            .iter()
            .map(|(n, s)| format!("{n} ({s} B)"))
            .collect();
        info!(
            archive = %archive_path.display(),
            chosen = %name,
            chosen_size = size,
            candidates = %list.join(", "),
            "multi-rom 7z archive; picked largest"
        );
    } else {
        info!(archive = %archive_path.display(), entry = %name, size, "extracted ROM from 7z archive");
    }

    let mut bytes: Vec<u8> = Vec::with_capacity(size as usize);
    let chosen_name = name.clone();
    reader
        .for_each_entries(|entry, reader| {
            if entry.name() == chosen_name {
                std::io::copy(reader, &mut bytes)?;
                return Ok(false);
            }
            Ok(true)
        })
        .with_context(|| format!("decompress 7z entry {name}"))?;

    if !need_fullpath {
        let file_name = Path::new(&name)
            .file_name()
            .ok_or_else(|| anyhow!("invalid entry name in archive: {name}"))?;
        let effective_path = archive_path
            .parent()
            .map(|p| p.join(file_name))
            .unwrap_or_else(|| PathBuf::from(file_name));
        return Ok(LoadedRom {
            effective_path,
            bytes: Some(bytes),
        });
    }

    let base = scratch_dir(session_id);
    std::fs::create_dir_all(&base).with_context(|| format!("mkdir {}", base.display()))?;
    let file_name = Path::new(&name)
        .file_name()
        .ok_or_else(|| anyhow!("invalid entry name in archive: {name}"))?;
    let out_path = base.join(file_name);
    std::fs::write(&out_path, &bytes).with_context(|| format!("write {}", out_path.display()))?;
    let canon = out_path.canonicalize().unwrap_or(out_path);
    Ok(LoadedRom {
        effective_path: canon,
        bytes: None,
    })
}

enum DiscImageKind {
    Cue(String),
    Gdi(String),
    Chd(String),
    Iso(String),
}

fn detect_disc_image_kind(archive_path: &Path, ext: &str) -> Result<Option<DiscImageKind>> {
    match ext {
        "zip" => detect_disc_image_in_zip(archive_path),
        "7z" => detect_disc_image_in_7z(archive_path),
        _ => Ok(None),
    }
}

fn detect_disc_image_in_zip(archive_path: &Path) -> Result<Option<DiscImageKind>> {
    let f = std::fs::File::open(archive_path)
        .with_context(|| format!("open archive {}", archive_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(f).with_context(|| format!("read zip {}", archive_path.display()))?;
    let mut index_kind: Option<DiscImageKind> = None;
    let mut data_kind: Option<DiscImageKind> = None;
    for i in 0..archive.len() {
        let entry = archive
            .by_index_raw(i)
            .with_context(|| format!("scan zip entry {i}"))?;
        if entry.is_dir() {
            continue;
        }
        record_disc_entry(entry.name(), &mut index_kind, &mut data_kind);
    }
    Ok(index_kind.or(data_kind))
}

fn detect_disc_image_in_7z(archive_path: &Path) -> Result<Option<DiscImageKind>> {
    let reader = sevenz_rust::SevenZReader::open(archive_path, sevenz_rust::Password::empty())
        .with_context(|| format!("read 7z {}", archive_path.display()))?;
    let mut index_kind: Option<DiscImageKind> = None;
    let mut data_kind: Option<DiscImageKind> = None;
    for entry in &reader.archive().files {
        if entry.is_directory() {
            continue;
        }
        record_disc_entry(entry.name(), &mut index_kind, &mut data_kind);
    }
    Ok(index_kind.or(data_kind))
}

fn record_disc_entry(
    name: &str,
    index_kind: &mut Option<DiscImageKind>,
    data_kind: &mut Option<DiscImageKind>,
) {
    let Some(ext) = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
    else {
        return;
    };
    match ext.as_str() {
        "cue" if index_kind.is_none() => *index_kind = Some(DiscImageKind::Cue(name.to_string())),
        "gdi" if index_kind.is_none() => *index_kind = Some(DiscImageKind::Gdi(name.to_string())),
        "chd" if data_kind.is_none() => *data_kind = Some(DiscImageKind::Chd(name.to_string())),
        "iso" if data_kind.is_none() => *data_kind = Some(DiscImageKind::Iso(name.to_string())),
        _ => {}
    }
}

fn extract_disc_image_archive(
    archive_path: &Path,
    archive_ext: &str,
    session_id: &str,
    kind: DiscImageKind,
) -> Result<LoadedRom> {
    let base = scratch_dir(session_id);
    std::fs::create_dir_all(&base).with_context(|| format!("mkdir {}", base.display()))?;

    let primary_entry_name = match &kind {
        DiscImageKind::Cue(n)
        | DiscImageKind::Gdi(n)
        | DiscImageKind::Chd(n)
        | DiscImageKind::Iso(n) => n.clone(),
    };
    let primary_basename = Path::new(&primary_entry_name)
        .file_name()
        .ok_or_else(|| anyhow!("invalid entry name in archive: {primary_entry_name}"))?
        .to_owned();
    let primary_out_path = base.join(&primary_basename);

    let extracted =
        extract_disc_entries_single_pass(archive_path, archive_ext, &base, &primary_entry_name)?;

    let renamed = match &kind {
        DiscImageKind::Cue(_) => {
            let cue_bytes = std::fs::read(&primary_out_path)
                .with_context(|| format!("read extracted cue {}", primary_out_path.display()))?;
            let refs = parse_cue_referenced_files(&cue_bytes);
            rename_data_files_to_index_refs(&base, &extracted, &refs, &primary_entry_name)?
        }
        DiscImageKind::Gdi(_) => {
            let gdi_bytes = std::fs::read(&primary_out_path)
                .with_context(|| format!("read extracted gdi {}", primary_out_path.display()))?;
            let refs = parse_gdi_referenced_files(&gdi_bytes);
            rename_data_files_to_index_refs(&base, &extracted, &refs, &primary_entry_name)?
        }
        DiscImageKind::Chd(_) | DiscImageKind::Iso(_) => 0,
    };

    info!(
        archive = %archive_path.display(),
        primary = %primary_entry_name,
        extracted_files = extracted.len(),
        renamed = renamed,
        "extracted disc image from archive"
    );

    let canon = primary_out_path.canonicalize().unwrap_or(primary_out_path);
    Ok(LoadedRom {
        effective_path: canon,
        bytes: None,
    })
}

// Solid 7z streams must be decoded in one pass, so every entry is extracted or drained in order.
fn extract_disc_entries_single_pass(
    archive_path: &Path,
    archive_ext: &str,
    base: &Path,
    primary_entry: &str,
) -> Result<Vec<(String, PathBuf)>> {
    let mut extracted: Vec<(String, PathBuf)> = Vec::new();
    match archive_ext {
        "zip" => {
            let f = std::fs::File::open(archive_path)
                .with_context(|| format!("open archive {}", archive_path.display()))?;
            let mut archive = zip::ZipArchive::new(f)
                .with_context(|| format!("read zip {}", archive_path.display()))?;
            for i in 0..archive.len() {
                let mut entry = archive
                    .by_index(i)
                    .with_context(|| format!("read zip entry {i}"))?;
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name().to_string();
                if !is_disc_relevant_entry(&name, primary_entry) {
                    continue;
                }
                let basename = Path::new(&name)
                    .file_name()
                    .ok_or_else(|| anyhow!("invalid entry name: {name}"))?
                    .to_owned();
                let out_path = base.join(&basename);
                let mut out = std::fs::File::create(&out_path)
                    .with_context(|| format!("create {}", out_path.display()))?;
                std::io::copy(&mut entry, &mut out)
                    .with_context(|| format!("decompress zip entry {name}"))?;
                extracted.push((name, out_path));
            }
        }
        "7z" => {
            let mut reader =
                sevenz_rust::SevenZReader::open(archive_path, sevenz_rust::Password::empty())
                    .with_context(|| format!("read 7z {}", archive_path.display()))?;
            reader
                .for_each_entries(|entry, r| {
                    let name = entry.name().to_string();
                    if !is_disc_relevant_entry(&name, primary_entry) {
                        std::io::copy(r, &mut std::io::sink())?;
                        return Ok(true);
                    }
                    let basename = Path::new(&name)
                        .file_name()
                        .ok_or_else(|| {
                            std::io::Error::other(format!("invalid entry name: {name}"))
                        })?
                        .to_owned();
                    let out_path = base.join(&basename);
                    let mut out = std::fs::File::create(&out_path).map_err(|e| {
                        std::io::Error::other(format!("create {}: {e}", out_path.display()))
                    })?;
                    std::io::copy(r, &mut out)?;
                    extracted.push((name, out_path));
                    Ok(true)
                })
                .with_context(|| format!("walk 7z {}", archive_path.display()))?;
        }
        _ => return Err(anyhow!("unsupported archive ext: {archive_ext}")),
    }
    Ok(extracted)
}

fn is_disc_relevant_entry(entry_name: &str, primary_entry: &str) -> bool {
    if entry_name == primary_entry {
        return true;
    }
    let ext = Path::new(entry_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "cue" | "gdi" | "bin" | "iso" | "raw" | "img" | "chd"
    )
}

fn rename_data_files_to_index_refs(
    base: &Path,
    extracted: &[(String, PathBuf)],
    references: &[String],
    primary_entry: &str,
) -> Result<usize> {
    let primary_basename = Path::new(primary_entry)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let mut consumed: Vec<usize> = Vec::new();
    let mut renamed_count = 0usize;
    for ref_name in references {
        let ref_basename = Path::new(ref_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(ref_name);
        let target_path = base.join(ref_basename);
        if target_path.exists() {
            if let Some(idx) = extracted.iter().position(|(_, p)| p == &target_path) {
                consumed.push(idx);
            }
            continue;
        }
        let ref_ext = Path::new(ref_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let chosen = extracted
            .iter()
            .enumerate()
            .find(|(idx, (arch_name, on_disk))| {
                if consumed.contains(idx) {
                    return false;
                }
                let arch_basename = Path::new(arch_name)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if arch_basename == primary_basename {
                    return false;
                }
                let on_disk_ext = on_disk
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                on_disk_ext == ref_ext
            });
        let Some((idx, (arch_name, on_disk_path))) = chosen else {
            return Err(anyhow!(
                "cue/gdi references {ref_name} but no extracted file with matching extension"
            ));
        };
        consumed.push(idx);
        std::fs::rename(on_disk_path, &target_path).with_context(|| {
            format!(
                "rename {} → {}",
                on_disk_path.display(),
                target_path.display()
            )
        })?;
        info!(
            cue_ref = %ref_name,
            archive_entry = %arch_name,
            renamed_to = %target_path.display(),
            "cue/gdi reference resolved by extension rename"
        );
        renamed_count += 1;
    }
    Ok(renamed_count)
}

fn parse_cue_referenced_files(cue_bytes: &[u8]) -> Vec<String> {
    let text = std::str::from_utf8(cue_bytes).unwrap_or("");
    let mut files = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let rest = trimmed
            .strip_prefix("FILE ")
            .or_else(|| trimmed.strip_prefix("file "))
            .or_else(|| trimmed.strip_prefix("File "));
        let Some(rest) = rest else { continue };
        let rest = rest.trim();
        let name = if let Some(start) = rest.strip_prefix('"') {
            start.split('"').next().unwrap_or("").to_string()
        } else {
            rest.split_whitespace().next().unwrap_or("").to_string()
        };
        if !name.is_empty() {
            files.push(name);
        }
    }
    files
}

fn parse_gdi_referenced_files(gdi_bytes: &[u8]) -> Vec<String> {
    let text = std::str::from_utf8(gdi_bytes).unwrap_or("");
    let mut files = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let with_quotes = trimmed.find('"');
        let name = if let Some(q) = with_quotes {
            let after = &trimmed[q + 1..];
            after.split('"').next().unwrap_or("").to_string()
        } else {
            trimmed.split_whitespace().nth(4).unwrap_or("").to_string()
        };
        if !name.is_empty() {
            files.push(name);
        }
    }
    files
}

pub fn scratch_dir(session_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("romp-rom-{session_id}"))
}

pub fn remove_stale_scratch(tmp: &Path) {
    let Ok(entries) = std::fs::read_dir(tmp) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix("romp-rom-"))
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if !is_running(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(unix)]
fn is_running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    alive || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(windows)]
fn is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
    }
    let mut code = 0;
    let queried = unsafe { GetExitCodeProcess(process, &mut code) } != 0;
    unsafe { CloseHandle(process) };
    !queried || code == STILL_ACTIVE as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_scratch_dirs_of_dead_runners_are_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join(format!("romp-rom-{}", std::process::id()));
        let dead = tmp.path().join("romp-rom-2147483000");
        let other = tmp.path().join("unrelated");
        for dir in [&live, &dead, &other] {
            std::fs::create_dir_all(dir.join("disc")).unwrap();
        }
        remove_stale_scratch(tmp.path());
        assert!(live.exists());
        assert!(!dead.exists());
        assert!(other.exists());
    }
}
