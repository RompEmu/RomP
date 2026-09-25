use crate::sandbox::SandboxParams;
use landlock::{
    Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus, ABI,
};
use std::path::PathBuf;
use tracing::{info, warn};

pub fn apply(params: &SandboxParams<'_>) -> anyhow::Result<()> {
    let abi = ABI::V2;
    let read = AccessFs::from_read(abi);
    let read_write = AccessFs::from_all(abi);

    let mut ro: Vec<PathBuf> = vec![
        PathBuf::from("/usr"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
        PathBuf::from("/bin"),
        PathBuf::from("/etc"),
        PathBuf::from("/proc"),
        PathBuf::from("/sys"),
        PathBuf::from("/dev/urandom"),
        PathBuf::from("/dev/random"),
        PathBuf::from("/dev/zero"),
        params.system_dir.to_path_buf(),
    ];
    if let Some(p) = params.core_path.parent() {
        ro.push(p.to_path_buf());
    }
    if let Some(p) = params.rom_path.parent() {
        ro.push(p.to_path_buf());
    }
    if params.permissive_read {
        ro.push(PathBuf::from("/"));
    }

    let mut rw: Vec<PathBuf> = vec![
        params.save_dir.to_path_buf(),
        std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp")),
        PathBuf::from("/dev/shm"),
        PathBuf::from("/dev/dri"),
        PathBuf::from("/dev/snd"),
        PathBuf::from("/dev/null"),
        params.home_dir.join(".cache"),
    ];
    if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
        rw.push(PathBuf::from(rt));
    }

    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(read_write)?
        .create()?;
    for path in &ro {
        if let Ok(fd) = PathFd::new(path) {
            ruleset = ruleset.add_rule(PathBeneath::new(fd, read))?;
        }
    }
    for path in &rw {
        if let Ok(fd) = PathFd::new(path) {
            ruleset = ruleset.add_rule(PathBeneath::new(fd, read_write))?;
        }
    }
    match ruleset.restrict_self()?.ruleset {
        RulesetStatus::FullyEnforced => info!("landlock sandbox enforced"),
        RulesetStatus::PartiallyEnforced => warn!("landlock sandbox only partially enforced"),
        RulesetStatus::NotEnforced => {
            warn!("landlock unsupported by this kernel; runner is unsandboxed")
        }
    }
    Ok(())
}
