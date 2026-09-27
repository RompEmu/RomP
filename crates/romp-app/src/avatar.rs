use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::path::{Path, PathBuf};

pub const SIZE: u32 = 128;

pub fn cache_path(data: &Path, server: &str) -> PathBuf {
    data.join(format!("avatar-{}", crate::paths::server_key(server)))
}

pub fn decode(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let img = image::load_from_memory(bytes).ok()?;
    let side = img.width().min(img.height());
    let square = img.crop_imm(
        (img.width() - side) / 2,
        (img.height() - side) / 2,
        side,
        side,
    );
    let small = square
        .resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        small.as_raw(),
        SIZE,
        SIZE,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, _| {
            if x < width / 2 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn pictures_become_small_centred_squares() {
        let buffer = decode(&png(300, 200)).unwrap();
        assert_eq!((buffer.width(), buffer.height()), (SIZE, SIZE));
        let left = buffer.as_slice()[(SIZE * SIZE / 2) as usize];
        let right = buffer.as_slice()[(SIZE * SIZE / 2 + SIZE - 1) as usize];
        assert_eq!((left.r, left.b), (255, 0));
        assert_eq!((right.r, right.b), (0, 255));
    }

    #[test]
    fn broken_files_are_ignored() {
        assert!(decode(b"not an image").is_none());
    }
}
