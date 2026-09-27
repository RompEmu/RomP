pub trait TokenStore: Send + Sync {
    fn load(&self, server: &str) -> Option<String>;
    fn save(&self, server: &str, token: &str) -> Result<(), String>;
    fn delete(&self, server: &str);
}

pub struct Keychain {
    service: &'static str,
}

impl Keychain {
    pub fn app() -> Migrating<Keychain, Keychain> {
        Migrating {
            current: Keychain { service: "Romp" },
            legacy: Keychain {
                service: "Cartridge",
            },
        }
    }
}

impl TokenStore for Keychain {
    fn load(&self, server: &str) -> Option<String> {
        keyring::Entry::new(self.service, server)
            .ok()?
            .get_password()
            .ok()
    }

    fn save(&self, server: &str, token: &str) -> Result<(), String> {
        keyring::Entry::new(self.service, server)
            .and_then(|e| e.set_password(token))
            .map_err(|e| format!("Could not store the sign-in in the keychain: {e}"))
    }

    fn delete(&self, server: &str) {
        if let Ok(entry) = keyring::Entry::new(self.service, server) {
            let _ = entry.delete_credential();
        }
    }
}

pub struct Migrating<C, L> {
    current: C,
    legacy: L,
}

impl<C: TokenStore, L: TokenStore> TokenStore for Migrating<C, L> {
    fn load(&self, server: &str) -> Option<String> {
        if let Some(token) = self.current.load(server) {
            return Some(token);
        }
        let token = self.legacy.load(server)?;
        if self.current.save(server, &token).is_ok() {
            self.legacy.delete(server);
        }
        Some(token)
    }

    fn save(&self, server: &str, token: &str) -> Result<(), String> {
        self.current.save(server, token)
    }

    fn delete(&self, server: &str) {
        self.current.delete(server);
        self.legacy.delete(server);
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryTokens(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl TokenStore for MemoryTokens {
    fn load(&self, server: &str) -> Option<String> {
        self.0.lock().unwrap().get(server).cloned()
    }
    fn save(&self, server: &str, token: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(server.into(), token.into());
        Ok(())
    }
    fn delete(&self, server: &str) {
        self.0.lock().unwrap().remove(server);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_saved_under_the_old_name_moves_to_the_new_one() {
        let tokens = Migrating {
            current: MemoryTokens::default(),
            legacy: MemoryTokens::default(),
        };
        tokens.legacy.save("http://a/", "old").unwrap();
        assert_eq!(tokens.load("http://a/").as_deref(), Some("old"));
        assert_eq!(tokens.current.load("http://a/").as_deref(), Some("old"));
        assert_eq!(tokens.legacy.load("http://a/"), None);
        tokens.save("http://a/", "new").unwrap();
        assert_eq!(tokens.load("http://a/").as_deref(), Some("new"));
        tokens.legacy.save("http://a/", "stale").unwrap();
        tokens.delete("http://a/");
        assert_eq!(tokens.load("http://a/"), None);
    }

    #[test]
    fn memory_tokens_are_per_server() {
        let t = MemoryTokens::default();
        t.save("http://a/", "one").unwrap();
        t.save("http://b/", "two").unwrap();
        assert_eq!(t.load("http://a/").as_deref(), Some("one"));
        t.delete("http://a/");
        assert_eq!(t.load("http://a/"), None);
        assert_eq!(t.load("http://b/").as_deref(), Some("two"));
    }
}
