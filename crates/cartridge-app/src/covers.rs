use crate::romm::client::Client;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub const THUMB_WIDTH: u32 = 200;

pub fn make_thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let img = if img.width() > THUMB_WIDTH {
        let height = (img.height() as u64 * THUMB_WIDTH as u64 / img.width() as u64).max(1) as u32;
        img.resize_exact(THUMB_WIDTH, height, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.to_rgba8()
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "webp", "gif"];

async fn write_atomic(dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    tokio::fs::write(&tmp, bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| e.to_string())
}

pub struct Covers {
    dir: PathBuf,
}

impl Covers {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn path_for(&self, game_id: i64, cover: &str) -> PathBuf {
        self.dir.join(format!(
            "{game_id}-{:016x}.png",
            crate::paths::fnv1a(cover.as_bytes())
        ))
    }

    fn large_base(&self, game_id: i64, cover: &str) -> PathBuf {
        self.dir.join(format!(
            "large-{game_id}-{:016x}",
            crate::paths::fnv1a(cover.as_bytes())
        ))
    }

    pub fn icon_path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("platform-{slug}.svg"))
    }

    pub async fn ensure_icon(&self, client: &Client, slug: &str) -> Result<PathBuf, String> {
        if !crate::layout::safe_component(slug) {
            return Err(format!("invalid platform slug: {slug}"));
        }
        let path = self.icon_path(slug);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(path);
        }
        let bytes = client
            .fetch_bytes(&format!("/assets/platforms/{slug}.svg"))
            .await
            .map_err(|e| e.to_string())?;
        write_atomic(&self.dir, &path, &bytes).await?;
        Ok(path)
    }

    pub fn cached_large(&self, game_id: i64, cover: &str) -> Option<PathBuf> {
        let base = self.large_base(game_id, cover);
        IMAGE_EXTENSIONS
            .iter()
            .map(|ext| base.with_extension(ext))
            .find(|p| p.exists())
    }

    pub async fn ensure_large(
        &self,
        client: &Client,
        game_id: i64,
        cover: &str,
    ) -> Result<PathBuf, String> {
        if let Some(path) = self.cached_large(game_id, cover) {
            return Ok(path);
        }
        let bytes = client.fetch_bytes(cover).await.map_err(|e| e.to_string())?;
        let ext = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Jpeg) => "jpg",
            Ok(image::ImageFormat::WebP) => "webp",
            Ok(image::ImageFormat::Gif) => "gif",
            _ => "png",
        };
        let path = self.large_base(game_id, cover).with_extension(ext);
        write_atomic(&self.dir, &path, &bytes).await?;
        Ok(path)
    }

    pub async fn ensure(
        &self,
        client: &Client,
        game_id: i64,
        cover: &str,
    ) -> Result<PathBuf, String> {
        let path = self.path_for(game_id, cover);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(path);
        }
        let bytes = client.fetch_bytes(cover).await.map_err(|e| e.to_string())?;
        let thumb = tokio::task::spawn_blocking(move || make_thumbnail(&bytes))
            .await
            .map_err(|e| e.to_string())??;
        write_atomic(&self.dir, &path, &thumb).await?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::client::tests::base_of;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 10, 10, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn thumbnails_are_scaled_to_width_keeping_aspect() {
        let thumb = image::load_from_memory(&make_thumbnail(&png(400, 600)).unwrap()).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (THUMB_WIDTH, 300));
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let thumb = image::load_from_memory(&make_thumbnail(&png(100, 140)).unwrap()).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (100, 140));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(make_thumbnail(b"not an image").is_err());
    }

    #[test]
    fn cache_key_changes_with_cover_path() {
        let covers = Covers::new("/c".into());
        let a = covers.path_for(7, "/assets/x.png?ts=1");
        assert_ne!(a, covers.path_for(7, "/assets/x.png?ts=2"));
        assert!(a.file_name().unwrap().to_str().unwrap().starts_with("7-"));
    }

    #[tokio::test]
    async fn ensure_downloads_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/assets/romm/resources/roms/1/7/cover/small.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png(400, 560)))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let covers = Covers::new(dir.path().to_path_buf());
        let client = Client::new(base_of(&server, "/"));
        let cover = "/assets/romm/resources/roms/1/7/cover/small.png?ts=2026-09-25 09:38:20";
        let first = covers.ensure(&client, 7, cover).await.unwrap();
        let second = covers.ensure(&client, 7, cover).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(image::open(&first).unwrap().width(), THUMB_WIDTH);
    }

    #[tokio::test]
    async fn ensure_large_keeps_original_bytes_once() {
        let server = MockServer::start().await;
        let original = png(600, 800);
        Mock::given(method("GET"))
            .and(path("/assets/romm/resources/roms/1/7/cover/big.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(original.clone()))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let covers = Covers::new(dir.path().to_path_buf());
        let client = Client::new(base_of(&server, "/"));
        let cover = "/assets/romm/resources/roms/1/7/cover/big.png?ts=1";
        let first = covers.ensure_large(&client, 7, cover).await.unwrap();
        assert_eq!(covers.ensure_large(&client, 7, cover).await.unwrap(), first);
        assert_eq!(std::fs::read(&first).unwrap(), original);
        assert!(first
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("large-7-"));
        assert_eq!(first.extension().unwrap(), "png");
    }

    fn jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[tokio::test]
    async fn large_cover_extension_follows_the_image_and_parallel_calls_agree() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cover"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(jpeg()))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let covers = Covers::new(dir.path().to_path_buf());
        let client = Client::new(base_of(&server, "/"));
        let (a, b) = tokio::join!(
            covers.ensure_large(&client, 9, "/cover"),
            covers.ensure_large(&client, 9, "/cover")
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a, b);
        assert_eq!(a.extension().unwrap(), "jpg");
        assert_eq!(covers.cached_large(9, "/cover"), Some(a));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn platform_icons_are_fetched_once_and_slugs_are_checked() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/assets/platforms/snes.svg"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<svg/>"))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let covers = Covers::new(dir.path().to_path_buf());
        let client = Client::new(base_of(&server, "/"));
        let icon = covers.ensure_icon(&client, "snes").await.unwrap();
        assert_eq!(covers.ensure_icon(&client, "snes").await.unwrap(), icon);
        assert_eq!(std::fs::read_to_string(icon).unwrap(), "<svg/>");
        assert!(covers.ensure_icon(&client, "../x").await.is_err());
    }

    #[test]
    fn svg_icons_with_css_classes_load() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("platform-x.svg");
        std::fs::write(
            &icon,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs><style>.a{fill:#a7a1cf;}</style></defs><rect class="a" width="10" height="10"/></svg>"#,
        )
        .unwrap();
        let image = slint::Image::load_from_path(&icon).unwrap();
        assert_eq!(image.size().width, 10);
    }
}
