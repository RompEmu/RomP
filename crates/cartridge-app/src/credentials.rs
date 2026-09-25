pub trait TokenStore: Send + Sync {
    fn load(&self, server: &str) -> Option<String>;
    fn save(&self, server: &str, token: &str) -> Result<(), String>;
    fn delete(&self, server: &str);
}

const SERVICE: &str = "Cartridge";

pub struct Keychain;

impl TokenStore for Keychain {
    fn load(&self, server: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, server)
            .ok()?
            .get_password()
            .ok()
    }

    fn save(&self, server: &str, token: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, server)
            .and_then(|e| e.set_password(token))
            .map_err(|e| format!("Could not store the sign-in in the keychain: {e}"))
    }

    fn delete(&self, server: &str) {
        if let Ok(entry) = keyring::Entry::new(SERVICE, server) {
            let _ = entry.delete_credential();
        }
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
