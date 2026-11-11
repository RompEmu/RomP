use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub trait TokenStore: Send + Sync {
    fn load(&self, account: &str) -> Option<String>;
    fn save(&self, account: &str, token: &str) -> Result<(), String>;
    fn delete(&self, account: &str);

    /// Whether sign-ins have a keychain to go in, rather than a file.
    fn keychain_available(&self) -> bool {
        true
    }
}

pub trait Secrets: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, service: &str, account: &str);

    fn keychain_available(&self) -> bool {
        true
    }
}

pub struct Keychain;

impl Secrets for Keychain {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(service, account).and_then(|e| e.get_password()) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(service, account)
            .and_then(|e| e.set_password(value))
            .map_err(|e| e.to_string())
    }

    fn delete(&self, service: &str, account: &str) {
        if let Ok(entry) = keyring::Entry::new(service, account) {
            let _ = entry.delete_credential();
        }
    }
}

/// Sign-ins kept in a file only its owner can read, for computers with no keychain to keep them in
/// (Steam Deck's game mode and other handhelds run no Secret Service).
pub struct FileSecrets {
    path: PathBuf,
}

impl FileSecrets {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn key(service: &str, account: &str) -> String {
        format!("{service}\n{account}")
    }

    fn read(&self) -> Result<BTreeMap<String, String>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn write(&self, items: &BTreeMap<String, String>) -> Result<(), String> {
        if items.is_empty() {
            let _ = std::fs::remove_file(&self.path);
            return Ok(());
        }
        let json = serde_json::to_string(items).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Written beside the real file and renamed over it, so a crash never leaves it half-written.
        let temp = self.path.with_extension("tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        std::io::Write::write_all(
            &mut options.open(&temp).map_err(|e| e.to_string())?,
            json.as_bytes(),
        )
        .and_then(|()| std::fs::rename(&temp, &self.path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            e.to_string()
        })
    }
}

impl Secrets for FileSecrets {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        Ok(self.read()?.get(&Self::key(service, account)).cloned())
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        let mut items = self.read()?;
        items.insert(Self::key(service, account), value.into());
        self.write(&items)
    }

    fn delete(&self, service: &str, account: &str) {
        if let Ok(mut items) = self.read() {
            if items.remove(&Self::key(service, account)).is_some() {
                let _ = self.write(&items);
            }
        }
    }
}

/// The keychain when there is one, and a file when it can't be used. Once the keychain has failed
/// it isn't written to again in this run, since a keychain that couldn't be read may still hold
/// sign-ins that a write would replace.
pub struct WithFileFallback<P: Secrets> {
    keychain: P,
    file: FileSecrets,
    keychain_failed: AtomicBool,
}

impl<P: Secrets> WithFileFallback<P> {
    pub fn new(keychain: P, file: FileSecrets) -> Self {
        Self {
            keychain,
            file,
            keychain_failed: AtomicBool::new(false),
        }
    }

    fn keychain_failed(&self, error: &str) {
        if !self.keychain_failed.swap(true, Ordering::SeqCst) {
            tracing::warn!("keychain unavailable, keeping sign-ins in a file instead: {error}");
        }
    }
}

impl<P: Secrets> Secrets for WithFileFallback<P> {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        if !self.keychain_failed.load(Ordering::SeqCst) {
            match self.keychain.get(service, account) {
                Ok(Some(value)) => return Ok(Some(value)),
                Ok(None) => {}
                Err(e) => self.keychain_failed(&e),
            }
        }
        self.file.get(service, account)
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        if !self.keychain_failed.load(Ordering::SeqCst) {
            match self.keychain.set(service, account, value) {
                Ok(()) => {
                    self.file.delete(service, account);
                    return Ok(());
                }
                Err(e) => self.keychain_failed(&e),
            }
        }
        self.file.set(service, account, value)
    }

    fn delete(&self, service: &str, account: &str) {
        self.keychain.delete(service, account);
        self.file.delete(service, account);
    }

    /// Asks the keychain for the sign-ins item, which is read at sign-in anyway, so a first run
    /// finds out before the player has typed an address.
    fn keychain_available(&self) -> bool {
        if !self.keychain_failed.load(Ordering::SeqCst) {
            if let Err(e) = self.keychain.get(SERVICE, ACCOUNT) {
                self.keychain_failed(&e);
            }
        }
        !self.keychain_failed.load(Ordering::SeqCst)
    }
}

const SERVICE: &str = "Romp";
const ACCOUNT: &str = "sign-ins";

type Tokens = BTreeMap<String, String>;

#[derive(Clone, Copy)]
enum Kind {
    RomM,
    RetroAchievements,
}

impl Kind {
    fn key(self, account: &str) -> String {
        match self {
            Kind::RomM => format!("romm:{account}"),
            Kind::RetroAchievements => format!("ra:{account}"),
        }
    }

    fn legacy_service(self) -> &'static str {
        match self {
            Kind::RomM => "Romp",
            Kind::RetroAchievements => "Romp RetroAchievements",
        }
    }
}

/// Every sign-in lives in one keychain item, read once, so a rebuilt app asks for the keychain password once.
pub struct SignIns {
    secrets: Box<dyn Secrets>,
    cache: Mutex<Option<Tokens>>,
}

impl SignIns {
    pub fn new(secrets: impl Secrets + 'static) -> Arc<SignIns> {
        Arc::new(SignIns {
            secrets: Box::new(secrets),
            cache: Mutex::new(None),
        })
    }

    pub fn keychain_available(&self) -> bool {
        self.secrets.keychain_available()
    }

    pub fn romm(self: &Arc<Self>) -> Arc<dyn TokenStore> {
        Arc::new(SignInsOf {
            sign_ins: self.clone(),
            kind: Kind::RomM,
        })
    }

    pub fn retro_achievements(self: &Arc<Self>) -> Arc<dyn TokenStore> {
        Arc::new(SignInsOf {
            sign_ins: self.clone(),
            kind: Kind::RetroAchievements,
        })
    }

    /// A keychain that can't be read is an error, never an empty list, so a save can't overwrite the other sign-ins.
    fn stored(&self, cache: &mut Option<Tokens>) -> Result<Tokens, String> {
        if let Some(tokens) = cache {
            return Ok(tokens.clone());
        }
        let tokens: Tokens = self
            .secrets
            .get(SERVICE, ACCOUNT)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        *cache = Some(tokens.clone());
        Ok(tokens)
    }

    fn store(&self, cache: &mut Option<Tokens>, tokens: Tokens) -> Result<(), String> {
        if tokens.is_empty() {
            self.secrets.delete(SERVICE, ACCOUNT);
        } else {
            let json = serde_json::to_string(&tokens).map_err(|e| e.to_string())?;
            self.secrets.set(SERVICE, ACCOUNT, &json)?;
        }
        *cache = Some(tokens);
        Ok(())
    }

    fn load(&self, kind: Kind, account: &str) -> Option<String> {
        let mut cache = self.cache.lock().unwrap();
        let mut tokens = self.stored(&mut cache).ok()?;
        let key = kind.key(account);
        if let Some(token) = tokens.get(&key) {
            return Some(token.clone());
        }
        let token = self.secrets.get(kind.legacy_service(), account).ok()??;
        tokens.insert(key, token.clone());
        if self.store(&mut cache, tokens).is_ok() {
            self.secrets.delete(kind.legacy_service(), account);
        }
        Some(token)
    }

    fn save(&self, kind: Kind, account: &str, token: &str) -> Result<(), String> {
        let mut cache = self.cache.lock().unwrap();
        self.stored(&mut cache)
            .and_then(|mut tokens| {
                tokens.insert(kind.key(account), token.into());
                self.store(&mut cache, tokens)
            })
            .map_err(|e| tr::tr!("Could not store the sign-in in the keychain: {e}", e))
    }

    fn delete(&self, kind: Kind, account: &str) {
        let mut cache = self.cache.lock().unwrap();
        if let Ok(mut tokens) = self.stored(&mut cache) {
            if tokens.remove(&kind.key(account)).is_some() {
                let _ = self.store(&mut cache, tokens);
            }
        }
    }
}

struct SignInsOf {
    sign_ins: Arc<SignIns>,
    kind: Kind,
}

impl TokenStore for SignInsOf {
    fn load(&self, account: &str) -> Option<String> {
        self.sign_ins.load(self.kind, account)
    }

    fn save(&self, account: &str, token: &str) -> Result<(), String> {
        self.sign_ins.save(self.kind, account, token)
    }

    fn delete(&self, account: &str) {
        self.sign_ins.delete(self.kind, account);
    }

    fn keychain_available(&self) -> bool {
        self.sign_ins.keychain_available()
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub struct MemorySecrets {
    items: Arc<Mutex<std::collections::HashMap<(String, String), String>>>,
    reads: Arc<std::sync::atomic::AtomicUsize>,
    locked: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(test)]
impl MemorySecrets {
    fn item(&self, service: &str, account: &str) -> Option<String> {
        let key = (service.to_string(), account.to_string());
        self.items.lock().unwrap().get(&key).cloned()
    }

    fn reads(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn lock(&self, locked: bool) {
        self.locked
            .store(locked, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
impl Secrets for MemorySecrets {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.locked.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("locked".into());
        }
        Ok(self.item(service, account))
    }
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        let key = (service.to_string(), account.to_string());
        self.items.lock().unwrap().insert(key, value.into());
        Ok(())
    }
    fn delete(&self, service: &str, account: &str) {
        let key = (service.to_string(), account.to_string());
        self.items.lock().unwrap().remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_ins_in_the_old_items_move_into_one() {
        let secrets = MemorySecrets::default();
        secrets.set("Romp", "http://a/", "romm").unwrap();
        secrets
            .set("Romp RetroAchievements", "beshr", "ra")
            .unwrap();
        let sign_ins = SignIns::new(secrets.clone());
        assert_eq!(sign_ins.romm().load("http://a/").as_deref(), Some("romm"));
        assert_eq!(
            sign_ins.retro_achievements().load("beshr").as_deref(),
            Some("ra")
        );
        assert_eq!(secrets.item("Romp", "http://a/"), None);
        assert_eq!(secrets.item("Romp RetroAchievements", "beshr"), None);

        let reopened = SignIns::new(secrets.clone());
        assert_eq!(reopened.romm().load("http://a/").as_deref(), Some("romm"));
        assert_eq!(
            reopened.retro_achievements().load("beshr").as_deref(),
            Some("ra")
        );
    }

    #[test]
    fn the_keychain_is_read_once() {
        let secrets = MemorySecrets::default();
        let sign_ins = SignIns::new(secrets.clone());
        sign_ins.romm().save("http://a/", "romm").unwrap();
        sign_ins.retro_achievements().save("beshr", "ra").unwrap();

        let reopened = SignIns::new(secrets.clone());
        let before = secrets.reads();
        for _ in 0..3 {
            assert_eq!(reopened.romm().load("http://a/").as_deref(), Some("romm"));
            assert_eq!(
                reopened.retro_achievements().load("beshr").as_deref(),
                Some("ra")
            );
        }
        assert_eq!(secrets.reads() - before, 1);
    }

    #[test]
    fn a_keychain_that_cant_be_read_is_left_alone() {
        let secrets = MemorySecrets::default();
        SignIns::new(secrets.clone())
            .romm()
            .save("http://a/", "romm")
            .unwrap();
        let stored = secrets.item(SERVICE, ACCOUNT);

        secrets.lock(true);
        let sign_ins = SignIns::new(secrets.clone());
        assert_eq!(sign_ins.romm().load("http://a/"), None);
        assert!(sign_ins.retro_achievements().save("beshr", "ra").is_err());
        sign_ins.romm().delete("http://a/");
        assert_eq!(secrets.item(SERVICE, ACCOUNT), stored);

        secrets.lock(false);
        assert_eq!(sign_ins.romm().load("http://a/").as_deref(), Some("romm"));
    }

    #[test]
    fn sign_ins_are_per_kind_and_account() {
        let secrets = MemorySecrets::default();
        let sign_ins = SignIns::new(secrets.clone());
        sign_ins.romm().save("same", "one").unwrap();
        sign_ins.romm().save("other", "two").unwrap();
        sign_ins.retro_achievements().save("same", "three").unwrap();
        sign_ins.romm().delete("same");
        assert_eq!(sign_ins.romm().load("same"), None);
        assert_eq!(sign_ins.romm().load("other").as_deref(), Some("two"));
        assert_eq!(
            sign_ins.retro_achievements().load("same").as_deref(),
            Some("three")
        );
    }

    #[test]
    fn signing_out_of_everything_removes_the_item() {
        let secrets = MemorySecrets::default();
        let sign_ins = SignIns::new(secrets.clone());
        sign_ins.romm().save("http://a/", "romm").unwrap();
        sign_ins.retro_achievements().save("beshr", "ra").unwrap();
        sign_ins.romm().delete("http://a/");
        assert!(secrets.item(SERVICE, ACCOUNT).is_some());
        sign_ins.retro_achievements().delete("beshr");
        assert_eq!(secrets.item(SERVICE, ACCOUNT), None);
    }

    struct NoKeychain;

    impl Secrets for NoKeychain {
        fn get(&self, _: &str, _: &str) -> Result<Option<String>, String> {
            Err("no secret service".into())
        }
        fn set(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Err("no secret service".into())
        }
        fn delete(&self, _: &str, _: &str) {}
    }

    fn in_file(dir: &tempfile::TempDir) -> FileSecrets {
        FileSecrets::new(dir.path().join("sign-ins.json"))
    }

    #[test]
    fn without_a_keychain_sign_ins_are_kept_in_a_file_across_runs() {
        let dir = tempfile::tempdir().unwrap();
        let first = SignIns::new(WithFileFallback::new(NoKeychain, in_file(&dir)));
        first.romm().save("http://a/", "romm").unwrap();
        first.retro_achievements().save("beshr", "ra").unwrap();

        let reopened = SignIns::new(WithFileFallback::new(NoKeychain, in_file(&dir)));
        assert_eq!(reopened.romm().load("http://a/").as_deref(), Some("romm"));
        assert_eq!(
            reopened.retro_achievements().load("beshr").as_deref(),
            Some("ra")
        );
        reopened.romm().delete("http://a/");
        reopened.retro_achievements().delete("beshr");
        assert!(!dir.path().join("sign-ins.json").exists());
    }

    #[test]
    fn a_working_keychain_is_used_and_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        let sign_ins = SignIns::new(WithFileFallback::new(secrets.clone(), in_file(&dir)));
        sign_ins.romm().save("http://a/", "romm").unwrap();
        assert!(secrets.item(SERVICE, ACCOUNT).is_some());
        assert!(!dir.path().join("sign-ins.json").exists());
    }

    #[test]
    fn a_keychain_that_cant_be_read_is_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        SignIns::new(secrets.clone())
            .romm()
            .save("http://a/", "romm")
            .unwrap();
        let stored = secrets.item(SERVICE, ACCOUNT);

        secrets.lock(true);
        let sign_ins = SignIns::new(WithFileFallback::new(secrets.clone(), in_file(&dir)));
        sign_ins.retro_achievements().save("beshr", "ra").unwrap();
        assert_eq!(secrets.item(SERVICE, ACCOUNT), stored);
        assert_eq!(
            sign_ins.retro_achievements().load("beshr").as_deref(),
            Some("ra")
        );
    }

    #[test]
    fn a_missing_keychain_is_told_before_anything_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let without = SignIns::new(WithFileFallback::new(NoKeychain, in_file(&dir)));
        assert!(!without.keychain_available());
        assert!(!dir.path().join("sign-ins.json").exists());
        let with = SignIns::new(WithFileFallback::new(
            MemorySecrets::default(),
            in_file(&dir),
        ));
        assert!(with.keychain_available());
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        in_file(&dir).set("Romp", "a", "token").unwrap();
        let mode = std::fs::metadata(dir.path().join("sign-ins.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
