//! libretro's VFS: the frontend's file access, which some cores use instead of their own. It
//! goes through the same files the sandbox already allows.

use std::ffi::{c_char, c_int, CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

pub const VERSION: u32 = 3;

const ACCESS_READ: u32 = 1;
const ACCESS_WRITE: u32 = 2;
const ACCESS_UPDATE_EXISTING: u32 = 4;

const SEEK_START: c_int = 0;
const SEEK_CURRENT: c_int = 1;
const SEEK_END: c_int = 2;

const STAT_IS_VALID: c_int = 1;
const STAT_IS_DIRECTORY: c_int = 2;

pub struct FileHandle {
    file: File,
    path: CString,
}

pub struct DirHandle {
    entries: Vec<(CString, bool)>,
    current: Option<usize>,
}

#[repr(C)]
pub struct retro_vfs_interface {
    get_path: unsafe extern "C" fn(*mut FileHandle) -> *const c_char,
    open: unsafe extern "C" fn(*const c_char, u32, u32) -> *mut FileHandle,
    close: unsafe extern "C" fn(*mut FileHandle) -> c_int,
    size: unsafe extern "C" fn(*mut FileHandle) -> i64,
    tell: unsafe extern "C" fn(*mut FileHandle) -> i64,
    seek: unsafe extern "C" fn(*mut FileHandle, i64, c_int) -> i64,
    read: unsafe extern "C" fn(*mut FileHandle, *mut u8, u64) -> i64,
    write: unsafe extern "C" fn(*mut FileHandle, *const u8, u64) -> i64,
    flush: unsafe extern "C" fn(*mut FileHandle) -> c_int,
    remove: unsafe extern "C" fn(*const c_char) -> c_int,
    rename: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int,
    truncate: unsafe extern "C" fn(*mut FileHandle, i64) -> i64,
    stat: unsafe extern "C" fn(*const c_char, *mut i32) -> c_int,
    mkdir: unsafe extern "C" fn(*const c_char) -> c_int,
    opendir: unsafe extern "C" fn(*const c_char, bool) -> *mut DirHandle,
    readdir: unsafe extern "C" fn(*mut DirHandle) -> bool,
    dirent_get_name: unsafe extern "C" fn(*mut DirHandle) -> *const c_char,
    dirent_is_dir: unsafe extern "C" fn(*mut DirHandle) -> bool,
    closedir: unsafe extern "C" fn(*mut DirHandle) -> c_int,
}

#[repr(C)]
pub struct retro_vfs_interface_info {
    pub required_interface_version: u32,
    pub iface: *const retro_vfs_interface,
}

pub static INTERFACE: retro_vfs_interface = retro_vfs_interface {
    get_path,
    open,
    close,
    size,
    tell,
    seek,
    read,
    write,
    flush,
    remove,
    rename,
    truncate,
    stat,
    mkdir,
    opendir,
    readdir,
    dirent_get_name,
    dirent_is_dir,
    closedir,
};

unsafe fn path(raw: *const c_char) -> Option<PathBuf> {
    if raw.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(raw) }.to_str().ok()?;
    (!text.is_empty()).then(|| PathBuf::from(text))
}

unsafe fn file<'a>(handle: *mut FileHandle) -> Option<&'a mut File> {
    unsafe { handle.as_mut() }.map(|h| &mut h.file)
}

fn status<T, E>(result: Result<T, E>) -> c_int {
    if result.is_ok() {
        0
    } else {
        -1
    }
}

unsafe extern "C" fn get_path(handle: *mut FileHandle) -> *const c_char {
    unsafe { handle.as_ref() }.map_or(std::ptr::null(), |h| h.path.as_ptr())
}

/// Opens like RetroArch: writing alone starts the file afresh, and updating keeps what's there.
unsafe extern "C" fn open(raw: *const c_char, mode: u32, _hints: u32) -> *mut FileHandle {
    let Some(file_path) = (unsafe { path(raw) }) else {
        return std::ptr::null_mut();
    };
    let writes = mode & ACCESS_WRITE != 0;
    let mut options = OpenOptions::new();
    options.read(mode & ACCESS_READ != 0 || !writes);
    if writes {
        options.write(true);
        if mode & ACCESS_UPDATE_EXISTING == 0 {
            options.create(true).truncate(true);
        }
    }
    match options.open(&file_path) {
        Ok(file) => Box::into_raw(Box::new(FileHandle {
            file,
            path: unsafe { CStr::from_ptr(raw) }.to_owned(),
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn close(handle: *mut FileHandle) -> c_int {
    if handle.is_null() {
        return -1;
    }
    drop(unsafe { Box::from_raw(handle) });
    0
}

unsafe extern "C" fn size(handle: *mut FileHandle) -> i64 {
    unsafe { file(handle) }
        .and_then(|f| f.metadata().ok())
        .map_or(-1, |m| m.len() as i64)
}

unsafe extern "C" fn tell(handle: *mut FileHandle) -> i64 {
    unsafe { file(handle) }
        .and_then(|f| f.stream_position().ok())
        .map_or(-1, |p| p as i64)
}

unsafe extern "C" fn seek(handle: *mut FileHandle, offset: i64, whence: c_int) -> i64 {
    let to = match whence {
        SEEK_START => match u64::try_from(offset) {
            Ok(at) => SeekFrom::Start(at),
            Err(_) => return -1,
        },
        SEEK_CURRENT => SeekFrom::Current(offset),
        SEEK_END => SeekFrom::End(offset),
        _ => return -1,
    };
    unsafe { file(handle) }
        .and_then(|f| f.seek(to).ok())
        .map_or(-1, |p| p as i64)
}

unsafe extern "C" fn read(handle: *mut FileHandle, buffer: *mut u8, len: u64) -> i64 {
    let (Some(f), false) = (unsafe { file(handle) }, buffer.is_null()) else {
        return -1;
    };
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, len as usize) };
    let mut total = 0;
    while total < buffer.len() {
        match f.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) if total > 0 => break,
            Err(_) => return -1,
        }
    }
    total as i64
}

unsafe extern "C" fn write(handle: *mut FileHandle, buffer: *const u8, len: u64) -> i64 {
    let (Some(f), false) = (unsafe { file(handle) }, buffer.is_null()) else {
        return -1;
    };
    let buffer = unsafe { std::slice::from_raw_parts(buffer, len as usize) };
    f.write_all(buffer).map_or(-1, |()| len as i64)
}

unsafe extern "C" fn flush(handle: *mut FileHandle) -> c_int {
    unsafe { file(handle) }.map_or(-1, |f| status(f.flush()))
}

unsafe extern "C" fn remove(raw: *const c_char) -> c_int {
    let Some(target) = (unsafe { path(raw) }) else {
        return -1;
    };
    if target.is_dir() {
        status(std::fs::remove_dir(target))
    } else {
        status(std::fs::remove_file(target))
    }
}

unsafe extern "C" fn rename(from: *const c_char, to: *const c_char) -> c_int {
    match unsafe { (path(from), path(to)) } {
        (Some(from), Some(to)) => status(std::fs::rename(from, to)),
        _ => -1,
    }
}

unsafe extern "C" fn truncate(handle: *mut FileHandle, length: i64) -> i64 {
    let (Some(f), Ok(length)) = (unsafe { file(handle) }, u64::try_from(length)) else {
        return -1;
    };
    status(f.set_len(length)).into()
}

unsafe extern "C" fn stat(raw: *const c_char, size: *mut i32) -> c_int {
    let Some(metadata) = (unsafe { path(raw) }).and_then(|p| std::fs::metadata(p).ok()) else {
        return 0;
    };
    if let Some(size) = unsafe { size.as_mut() } {
        *size = i32::try_from(metadata.len()).unwrap_or(i32::MAX);
    }
    if metadata.is_dir() {
        STAT_IS_VALID | STAT_IS_DIRECTORY
    } else {
        STAT_IS_VALID
    }
}

/// 0 when made, -2 when it already exists, -1 when it can't be made.
unsafe extern "C" fn mkdir(raw: *const c_char) -> c_int {
    let Some(dir) = (unsafe { path(raw) }) else {
        return -1;
    };
    match std::fs::create_dir(&dir) {
        Ok(()) => 0,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => -2,
        Err(_) => -1,
    }
}

unsafe extern "C" fn opendir(raw: *const c_char, include_hidden: bool) -> *mut DirHandle {
    let Some(dir) = (unsafe { path(raw) }) else {
        return std::ptr::null_mut();
    };
    let Ok(listing) = std::fs::read_dir(dir) else {
        return std::ptr::null_mut();
    };
    let entries = listing
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !include_hidden && name.starts_with('.') {
                return None;
            }
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            Some((CString::new(name).ok()?, is_dir))
        })
        .collect();
    Box::into_raw(Box::new(DirHandle {
        entries,
        current: None,
    }))
}

unsafe extern "C" fn readdir(handle: *mut DirHandle) -> bool {
    let Some(dir) = (unsafe { handle.as_mut() }) else {
        return false;
    };
    let next = dir.current.map_or(0, |i| i + 1);
    dir.current = Some(next);
    next < dir.entries.len()
}

unsafe fn current<'a>(handle: *mut DirHandle) -> Option<&'a (CString, bool)> {
    let dir = unsafe { handle.as_ref() }?;
    dir.entries.get(dir.current?)
}

unsafe extern "C" fn dirent_get_name(handle: *mut DirHandle) -> *const c_char {
    unsafe { current(handle) }.map_or(std::ptr::null(), |(name, _)| name.as_ptr())
}

unsafe extern "C" fn dirent_is_dir(handle: *mut DirHandle) -> bool {
    unsafe { current(handle) }.is_some_and(|(_, is_dir)| *is_dir)
}

unsafe extern "C" fn closedir(handle: *mut DirHandle) -> c_int {
    if handle.is_null() {
        return -1;
    }
    drop(unsafe { Box::from_raw(handle) });
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(path: &std::path::Path) -> CString {
        CString::new(path.to_str().unwrap()).unwrap()
    }

    #[test]
    fn files_write_read_seek_and_resize() {
        let dir = tempfile::tempdir().unwrap();
        let name = c(&dir.path().join("save.srm"));
        unsafe {
            let f = open(name.as_ptr(), ACCESS_WRITE, 0);
            assert!(!f.is_null());
            assert_eq!(write(f, b"hello world".as_ptr(), 11), 11);
            assert_eq!(close(f), 0);

            let f = open(name.as_ptr(), ACCESS_READ, 0);
            assert_eq!(size(f), 11);
            assert_eq!(seek(f, 6, SEEK_START), 6);
            let mut buffer = [0u8; 16];
            assert_eq!(read(f, buffer.as_mut_ptr(), 16), 5);
            assert_eq!(&buffer[..5], b"world");
            assert_eq!(tell(f), 11);
            assert_eq!(seek(f, -5, SEEK_END), 6);
            assert_eq!(CStr::from_ptr(get_path(f)), name.as_c_str());
            close(f);

            let f = open(
                name.as_ptr(),
                ACCESS_READ | ACCESS_WRITE | ACCESS_UPDATE_EXISTING,
                0,
            );
            assert_eq!(write(f, b"J".as_ptr(), 1), 1);
            assert_eq!(truncate(f, 5), 0);
            close(f);
        }
        assert_eq!(
            std::fs::read(dir.path().join("save.srm")).unwrap(),
            b"Jello"
        );
    }

    #[test]
    fn writing_alone_starts_afresh_and_updating_needs_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let name = c(&dir.path().join("new.bin"));
        unsafe {
            assert!(open(name.as_ptr(), ACCESS_WRITE | ACCESS_UPDATE_EXISTING, 0).is_null());
            assert!(open(name.as_ptr(), ACCESS_READ, 0).is_null());
        }
        std::fs::write(dir.path().join("new.bin"), b"old contents").unwrap();
        unsafe {
            let f = open(name.as_ptr(), ACCESS_WRITE, 0);
            assert_eq!(size(f), 0);
            close(f);
        }
    }

    #[test]
    fn stat_tells_files_from_folders_and_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("game.a26"), [0u8; 4096]).unwrap();
        let mut size = 0;
        unsafe {
            assert_eq!(
                stat(c(&dir.path().join("game.a26")).as_ptr(), &mut size),
                STAT_IS_VALID
            );
            assert_eq!(size, 4096);
            assert_eq!(
                stat(c(dir.path()).as_ptr(), std::ptr::null_mut()),
                STAT_IS_VALID | STAT_IS_DIRECTORY
            );
            assert_eq!(stat(c(&dir.path().join("missing")).as_ptr(), &mut size), 0);
            assert_eq!(stat(std::ptr::null(), &mut size), 0);
        }
    }

    #[test]
    fn folders_are_made_listed_renamed_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let sub = c(&dir.path().join("saves"));
        unsafe {
            assert_eq!(mkdir(sub.as_ptr()), 0);
            assert_eq!(mkdir(sub.as_ptr()), -2);
        }
        std::fs::write(dir.path().join("saves/a.srm"), b"a").unwrap();
        std::fs::write(dir.path().join("saves/.hidden"), b"h").unwrap();
        std::fs::create_dir(dir.path().join("saves/states")).unwrap();
        let mut seen = Vec::new();
        unsafe {
            let d = opendir(sub.as_ptr(), false);
            assert!(!d.is_null());
            assert!(
                dirent_get_name(d).is_null(),
                "nothing until the first readdir"
            );
            while readdir(d) {
                let name = CStr::from_ptr(dirent_get_name(d))
                    .to_str()
                    .unwrap()
                    .to_string();
                seen.push((name, dirent_is_dir(d)));
            }
            assert_eq!(closedir(d), 0);
        }
        seen.sort();
        assert_eq!(
            seen,
            [("a.srm".to_string(), false), ("states".to_string(), true)]
        );
        let renamed = c(&dir.path().join("saves/b.srm"));
        unsafe {
            assert_eq!(
                rename(
                    c(&dir.path().join("saves/a.srm")).as_ptr(),
                    renamed.as_ptr()
                ),
                0
            );
            assert_eq!(remove(renamed.as_ptr()), 0);
            assert_eq!(remove(renamed.as_ptr()), -1);
        }
    }
}
