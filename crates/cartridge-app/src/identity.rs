use crate::store::Store;
use std::io::Read;

pub fn device_id(store: &Store) -> String {
    if let Some(id) = store.get("device_id") {
        return id;
    }
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("read /dev/urandom");
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    store.set("device_id", &id);
    id
}

pub fn device_name() -> String {
    let mut buf = [0u8; 256];
    let host = if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end])
            .trim_end_matches(".local")
            .to_string()
    } else {
        String::new()
    };
    let host = if host.is_empty() {
        "this computer".to_string()
    } else {
        host
    };
    format!("Cartridge on {host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_created_once_and_reused() {
        let store = Store::open_in_memory().unwrap();
        let first = device_id(&store);
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(device_id(&store), first);
        assert_ne!(device_id(&Store::open_in_memory().unwrap()), first);
    }

    #[test]
    fn device_name_mentions_the_app() {
        let name = device_name();
        assert!(name.starts_with("Cartridge on "));
        assert!(name.len() > "Cartridge on ".len());
    }
}
