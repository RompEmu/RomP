use crate::romm::client::{Client, Error};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::AsyncWriteExt;
use tr::tr;
use url::Url;

#[derive(Debug)]
pub enum DownloadError {
    Network(Error),
    Io(String),
    HashMismatch,
    Cancelled,
    UnsafePath,
    Unpack(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(Error::Unreachable) => f.write_str(&tr!("Could not reach the server")),
            Self::Network(e) => f.write_str(&tr!("Download failed: {e}", e)),
            Self::Io(e) => f.write_str(&tr!("Could not save the file: {e}", e)),
            Self::HashMismatch => f.write_str(&tr!("The downloaded file is corrupted; try again")),
            Self::Cancelled => f.write_str(&tr!("Download cancelled")),
            Self::UnsafePath => f.write_str(&tr!("The server sent an invalid file path")),
            Self::Unpack(e) => write!(f, "{e}"),
        }
    }
}

fn io(e: std::io::Error) -> DownloadError {
    DownloadError::Io(e.to_string())
}

pub fn sha1_file(path: &Path) -> std::io::Result<String> {
    use sha1::Digest;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha1::Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

async fn matches(path: &Path, sha1: Option<&str>, size: Option<u64>) -> bool {
    let Some(expected) = sha1.filter(|h| !h.is_empty()) else {
        let len = tokio::fs::metadata(path).await.map(|m| m.len()).ok();
        return size.is_none() || len == size;
    };
    let path = path.to_path_buf();
    let expected = expected.to_ascii_lowercase();
    tokio::task::spawn_blocking(move || sha1_file(&path).is_ok_and(|h| h == expected))
        .await
        .unwrap_or(false)
}

pub async fn download_file(
    client: &Client,
    url: Url,
    dest: &Path,
    sha1: Option<&str>,
    size: Option<u64>,
    progress: &(dyn Fn(u64) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    if let Ok(meta) = tokio::fs::metadata(dest).await {
        if matches(dest, sha1, size).await {
            progress(meta.len());
            return Ok(());
        }
    }
    let dir = dest.parent().ok_or(DownloadError::UnsafePath)?;
    tokio::fs::create_dir_all(dir).await.map_err(io)?;
    let name = dest
        .file_name()
        .ok_or(DownloadError::UnsafePath)?
        .to_string_lossy();
    let part = dir.join(format!("{name}.part"));
    let mut offset = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if cancel.load(Ordering::SeqCst) {
        return Err(DownloadError::Cancelled);
    }
    let mut resp = match client.download(url.clone(), offset).await {
        Err(Error::Status(416)) if offset > 0 => {
            if matches(&part, sha1, size).await {
                progress(offset);
                return tokio::fs::rename(&part, dest).await.map_err(io);
            }
            let _ = tokio::fs::remove_file(&part).await;
            offset = 0;
            client
                .download(url, 0)
                .await
                .map_err(DownloadError::Network)?
        }
        other => other.map_err(DownloadError::Network)?,
    };
    let mut file = if resp.status().as_u16() == 206 && offset > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .await
            .map_err(io)?
    } else {
        offset = 0;
        tokio::fs::File::create(&part).await.map_err(io)?
    };
    progress(offset);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|_| DownloadError::Network(Error::Unreachable))?
    {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await.map_err(io)?;
            return Err(DownloadError::Cancelled);
        }
        file.write_all(&chunk).await.map_err(io)?;
        offset += chunk.len() as u64;
        progress(offset);
    }
    file.flush().await.map_err(io)?;
    drop(file);
    if !matches(&part, sha1, size).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(DownloadError::HashMismatch);
    }
    tokio::fs::rename(&part, dest).await.map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::client::tests::base_of;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const BODY: &[u8] = b"0123456789abcdefghij";

    fn sha1_hex(bytes: &[u8]) -> String {
        use sha1::Digest;
        sha1::Sha1::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    async fn serve(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/f"))
            .and(header("range", "bytes=10-"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(&BODY[10..]))
            .with_priority(1)
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/f"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .with_priority(2)
            .mount(server)
            .await;
    }

    fn setup(server: &MockServer) -> (Client, Url, tempfile::TempDir) {
        let client = Client::new(base_of(server, "/"));
        let url = client.url("/f");
        (client, url, tempfile::tempdir().unwrap())
    }

    #[tokio::test]
    async fn downloads_and_verifies() {
        let server = MockServer::start().await;
        serve(&server).await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("a/b/game.bin");
        let seen = std::sync::Mutex::new(0u64);
        download_file(
            &client,
            url,
            &dest,
            Some(&sha1_hex(BODY)),
            None,
            &|n| *seen.lock().unwrap() = n,
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
        assert_eq!(*seen.lock().unwrap(), BODY.len() as u64);
        assert!(!dest.with_file_name("game.bin.part").exists());
    }

    #[tokio::test]
    async fn files_the_server_did_not_hash_are_checked_by_size() {
        let server = MockServer::start().await;
        serve(&server).await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.iso");
        download_file(
            &client,
            url,
            &dest,
            Some(""),
            Some(BODY.len() as u64),
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    }

    #[tokio::test]
    async fn resumes_from_part_file() {
        let server = MockServer::start().await;
        serve(&server).await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.bin");
        std::fs::write(dir.path().join("game.bin.part"), &BODY[..10]).unwrap();
        download_file(
            &client,
            url,
            &dest,
            Some(&sha1_hex(BODY)),
            None,
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    }

    #[tokio::test]
    async fn hash_mismatch_deletes_part() {
        let server = MockServer::start().await;
        serve(&server).await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.bin");
        let err = download_file(
            &client,
            url,
            &dest,
            Some("00"),
            None,
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::HashMismatch));
        assert!(!dest.exists());
        assert!(!dir.path().join("game.bin.part").exists());
    }

    #[tokio::test]
    async fn cancel_keeps_part() {
        let server = MockServer::start().await;
        serve(&server).await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.bin");
        let err = download_file(
            &client,
            url,
            &dest,
            None,
            None,
            &|_| {},
            &AtomicBool::new(true),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::Cancelled));
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn existing_verified_file_is_not_downloaded_again() {
        let server = MockServer::start().await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.bin");
        std::fs::write(&dest, BODY).unwrap();
        download_file(
            &client,
            url,
            &dest,
            Some(&sha1_hex(BODY)),
            None,
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn complete_part_answered_with_416_is_finished() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/f"))
            .respond_with(ResponseTemplate::new(416))
            .mount(&server)
            .await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.bin");
        std::fs::write(dir.path().join("game.bin.part"), BODY).unwrap();
        download_file(
            &client,
            url,
            &dest,
            Some(&sha1_hex(BODY)),
            None,
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    }

    #[tokio::test]
    async fn bad_part_answered_with_416_restarts_from_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/f"))
            .and(header("range", "bytes=20-"))
            .respond_with(ResponseTemplate::new(416))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/f"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .with_priority(2)
            .mount(&server)
            .await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.bin");
        std::fs::write(dir.path().join("game.bin.part"), b"XXXXXXXXXXXXXXXXXXXX").unwrap();
        download_file(
            &client,
            url,
            &dest,
            Some(&sha1_hex(BODY)),
            None,
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
    }

    #[tokio::test]
    async fn without_a_hash_the_size_must_match() {
        let server = MockServer::start().await;
        serve(&server).await;
        let (client, url, dir) = setup(&server);
        let dest = dir.path().join("game.bin");
        std::fs::write(&dest, b"truncated").unwrap();
        download_file(
            &client,
            url.clone(),
            &dest,
            None,
            Some(BODY.len() as u64),
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), BODY);
        let err = download_file(
            &client,
            url,
            &dir.path().join("other.bin"),
            None,
            Some(999),
            &|_| {},
            &AtomicBool::new(false),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DownloadError::HashMismatch));
    }
}
