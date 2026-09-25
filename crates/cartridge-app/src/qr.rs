use qrcode::{Color, QrCode};

const QUIET: u32 = 4;

pub fn qr_pixels(text: &str, scale: u32) -> Result<(Vec<u8>, u32), String> {
    let code = QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let modules = code.width() as u32;
    let colors = code.to_colors();
    let side = (modules + 2 * QUIET) * scale;
    let mut buf = vec![255u8; (side * side * 4) as usize];
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = (x / scale, y / scale);
            let dark = mx >= QUIET
                && my >= QUIET
                && mx < QUIET + modules
                && my < QUIET + modules
                && colors[((my - QUIET) * modules + (mx - QUIET)) as usize] == Color::Dark;
            if dark {
                let i = ((y * side + x) * 4) as usize;
                buf[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
    }
    Ok((buf, side))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(buf: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * side + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn qr_has_quiet_zone_and_finder_pattern() {
        let (buf, side) =
            qr_pixels("http://romm.tvpc.home/pair/device?user_code=FDF64KC5", 4).unwrap();
        assert_eq!(buf.len(), (side * side * 4) as usize);
        assert_eq!(side % 4, 0);
        assert_eq!(pixel(&buf, side, 0, 0), [255, 255, 255, 255]);
        assert_eq!(pixel(&buf, side, 4 * 4, 4 * 4), [0, 0, 0, 255]);
    }
}
