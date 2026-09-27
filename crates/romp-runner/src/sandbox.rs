use std::path::Path;

pub struct SandboxParams<'a> {
    pub rom_path: &'a Path,
    pub core_path: &'a Path,
    pub socket_path: &'a Path,
    pub system_dir: &'a Path,
    pub save_dir: &'a Path,
    pub home_dir: &'a Path,
    pub needs_jit: bool,
    pub permissive_mach: bool,
    pub permissive_read: bool,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn sandbox_init(
        profile: *const std::ffi::c_char,
        flags: u64,
        errorbuf: *mut *mut std::ffi::c_char,
    ) -> i32;
    fn sandbox_free_error(errorbuf: *mut std::ffi::c_char);
}

#[cfg(target_os = "macos")]
pub fn apply(params: &SandboxParams<'_>) -> anyhow::Result<()> {
    let profile = std::ffi::CString::new(build_profile(params))?;
    let mut err = std::ptr::null_mut();
    if unsafe { sandbox_init(profile.as_ptr(), 0, &mut err) } != 0 {
        let msg = unsafe { std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned() };
        unsafe { sandbox_free_error(err) };
        anyhow::bail!("sandbox_init failed: {msg}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn apply(params: &SandboxParams<'_>) -> anyhow::Result<()> {
    crate::sandbox_linux::apply(params)
}

#[cfg(target_os = "macos")]
fn build_profile(p: &SandboxParams<'_>) -> String {
    let socket = sbpl_string(p.socket_path);
    let save = sbpl_string(p.save_dir);
    let caches = sbpl_string(&p.home_dir.join("Library/Caches"));
    let jit = if p.needs_jit {
        "(allow dynamic-code-generation)\n"
    } else {
        ""
    };
    let mach = if p.permissive_mach {
        WILDCARD_MACH_LOOKUP
    } else {
        TIGHTENED_MACH_LOOKUP
    };
    let reads = if p.permissive_read {
        WILDCARD_FILE_READ.to_string()
    } else {
        tightened_file_read(p)
    };
    format!(
        r#"(version 1)
(deny default)

(allow network-outbound (literal {socket}))
(allow system-socket)

{reads}
(allow file-map-executable)

(allow file-write*
  (subpath {save})
  (subpath {caches})
  (subpath "/private/tmp")
  (subpath "/private/var/folders"))
(allow file-write-data
  (literal "/dev/null")
  (literal "/dev/dtracehelper")
  (literal "/dev/stderr")
  (literal "/dev/stdout"))

{mach}

(allow iokit-open)
(allow process-info* (target self))
(allow signal (target self))
(allow sysctl-read)
(allow ipc-posix-shm*)
(allow ipc-posix-sem*)
{jit}"#
    )
}

#[cfg(target_os = "macos")]
fn tightened_file_read(p: &SandboxParams<'_>) -> String {
    let parent = |path: &Path| sbpl_string(path.parent().unwrap_or(path));
    let home = p.home_dir;
    format!(
        r#"(allow file-read*
  (literal {rom})
  (subpath {rom_dir})
  (literal {core})
  (subpath {core_dir})
  (subpath {system})
  (subpath {save})
  (subpath "/System")
  (subpath "/usr/lib")
  (subpath "/usr/share")
  (subpath "/Library/Frameworks")
  (subpath "/Library/Preferences")
  (subpath "/Library/Fonts")
  (subpath "/Applications")
  (subpath "/private/etc")
  (subpath "/var/db/timezone")
  (subpath "/private/var/db/timezone")
  (subpath {prefs})
  (subpath {caches})
  (subpath {fonts})
  (literal {text_enc})
  (literal {home})
  (subpath "/private/tmp")
  (subpath "/private/var/folders")
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/urandom")
  (literal "/dev/random")
  (literal "/dev/autofs_nowait"))
(allow file-read-metadata)"#,
        rom = sbpl_string(p.rom_path),
        rom_dir = parent(p.rom_path),
        core = sbpl_string(p.core_path),
        core_dir = parent(p.core_path),
        system = sbpl_string(p.system_dir),
        save = sbpl_string(p.save_dir),
        prefs = sbpl_string(&home.join("Library/Preferences")),
        caches = sbpl_string(&home.join("Library/Caches")),
        fonts = sbpl_string(&home.join("Library/Fonts")),
        text_enc = sbpl_string(&home.join(".CFUserTextEncoding")),
        home = sbpl_string(home),
    )
}

#[cfg(target_os = "macos")]
const WILDCARD_FILE_READ: &str = "; ROMP_SANDBOX_PERMISSIVE_READ\n(allow file-read*)";

#[cfg(target_os = "macos")]
const WILDCARD_MACH_LOOKUP: &str =
    "; ROMP_SANDBOX_PERMISSIVE_MACH\n(allow mach-lookup)\n(allow mach-task-name)";

#[cfg(target_os = "macos")]
const TIGHTENED_MACH_LOOKUP: &str = r##"(allow mach-lookup
    ; window server
    (global-name "com.apple.windowserver.active")
    (global-name "com.apple.windowserver.session")
    (global-name "com.apple.CARenderServer")
    ; audio
    (global-name "com.apple.audio.coreaudiod")
    (global-name "com.apple.audio.audiohald")
    (global-name "com.apple.audio.systemsoundserverd")
    (global-name-regex #"^com\.apple\.audio\..*")
    ; GPU and shader compiler
    (global-name "com.apple.IOSurfaceRoot")
    (global-name-regex #"^com\.apple\.Metal.*")
    (global-name-regex #"^com\.apple\.MTL.*")
    ; preferences
    (global-name "com.apple.cfprefsd.agent")
    (global-name "com.apple.cfprefsd.daemon")
    ; launch services
    (global-name "com.apple.coreservices.launchservicesd")
    (global-name "com.apple.lsd.mapdb")
    (global-name "com.apple.lsd.modifydb")
    ; notifications
    (global-name "com.apple.system.notification_center")
    (global-name-regex #"^com\.apple\.distributed_notifications@.*")
    ; logging
    (global-name "com.apple.diagnosticd")
    (global-name "com.apple.logd")
    (global-name "com.apple.system.logger")
    ; security and system config
    (global-name "com.apple.SecurityServer")
    (global-name "com.apple.trustd.agent")
    (global-name "com.apple.SystemConfiguration.configd")
    ; fonts
    (global-name "com.apple.fonts"))
(allow mach-task-name)"##;

#[cfg(target_os = "macos")]
fn sbpl_string(path: &Path) -> String {
    let mut out = String::from('"');
    for ch in path.to_string_lossy().chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(all(target_os = "macos", test))]
mod tests {
    use super::*;

    fn with_profile(permissive_mach: bool, permissive_read: bool, jit: bool, rom: &str) -> String {
        let core = Path::new(
            "/Users/t/Library/Application Support/Romp/cores/snes9x/snes9x_libretro.dylib",
        );
        let system = Path::new("/Users/t/Library/Application Support/Romp/system");
        let save = Path::new("/Users/t/Library/Application Support/Romp/saves/42");
        build_profile(&SandboxParams {
            rom_path: Path::new(rom),
            core_path: core,
            socket_path: Path::new("/tmp/romp-1.sock"),
            system_dir: system,
            save_dir: save,
            home_dir: Path::new("/Users/t"),
            needs_jit: jit,
            permissive_mach,
            permissive_read,
        })
    }

    fn profile() -> String {
        with_profile(false, false, false, "/Users/t/roms/game.smc")
    }

    #[test]
    fn profile_denies_by_default_and_names_game_paths() {
        let p = profile();
        assert!(p.contains("(deny default)"));
        assert!(p.contains("\"/Users/t/roms/game.smc\""));
        assert!(p.contains("snes9x_libretro.dylib"));
        assert!(p.contains("Romp/system\""));
        assert!(p.contains("Romp/saves/42\""));
    }

    #[test]
    fn writes_are_limited_to_save_dir_and_temp() {
        let p = profile();
        let writes = &p[p.find("(allow file-write*").unwrap()..];
        let writes = &writes[..writes.find("))").unwrap()];
        assert!(writes.contains("saves/42"));
        assert!(!writes.contains("Romp/system"));
        assert!(!writes.contains("cores"));
    }

    #[test]
    fn tightened_mach_lookup_is_named_and_permissive_is_wildcard() {
        assert!(profile().contains("com.apple.audio.coreaudiod"));
        let permissive = with_profile(true, false, false, "/r/g.smc");
        assert!(permissive.contains("ROMP_SANDBOX_PERMISSIVE_MACH"));
        assert!(!permissive.contains("com.apple.audio.coreaudiod"));
    }

    #[test]
    fn permissive_read_is_wildcard() {
        let p = with_profile(false, true, false, "/r/g.smc");
        assert!(p.contains("ROMP_SANDBOX_PERMISSIVE_READ"));
        assert!(!p.contains("/dev/urandom"));
    }

    #[test]
    fn jit_only_when_requested() {
        assert!(!profile().contains("dynamic-code-generation"));
        assert!(with_profile(false, false, true, "/r/g.smc")
            .contains("(allow dynamic-code-generation)"));
    }

    #[test]
    fn allows_posix_semaphores() {
        assert!(profile().contains("(allow ipc-posix-sem*)"));
    }

    #[test]
    fn profile_escapes_quotes_in_paths() {
        let p = with_profile(
            false,
            false,
            false,
            "/Users/t/roms/Pokémon \"Blue\" (USA).gb",
        );
        assert!(p.contains(r#""/Users/t/roms/Pokémon \"Blue\" (USA).gb""#));
    }

    #[test]
    fn parentheses_are_balanced() {
        for p in [profile(), with_profile(true, true, true, "/r/g.smc")] {
            let opens = p.bytes().filter(|&b| b == b'(').count();
            let closes = p.bytes().filter(|&b| b == b')').count();
            assert_eq!(opens, closes);
        }
    }
}
