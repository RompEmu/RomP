pub fn rotate(rgba: &[u8], width: u32, height: u32, quarter_turns: u8) -> (Vec<u8>, u32, u32) {
    let (w, h) = (width as usize, height as usize);
    let turns = quarter_turns % 4;
    if turns == 0 || rgba.len() < w * h * 4 {
        return (rgba.to_vec(), width, height);
    }
    let (nw, nh) = if turns == 2 { (w, h) } else { (h, w) };
    let mut out = vec![0u8; w * h * 4];
    for ny in 0..nh {
        for nx in 0..nw {
            let (x, y) = match turns {
                1 => (w - 1 - ny, nx),
                2 => (w - 1 - nx, h - 1 - ny),
                _ => (ny, h - 1 - nx),
            };
            let src = (y * w + x) * 4;
            let dst = (ny * nw + nx) * 4;
            out[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    (out, nw as u32, nh as u32)
}

pub fn unrotate_point(u: f32, v: f32, quarter_turns: u8) -> (f32, f32) {
    match quarter_turns % 4 {
        1 => (1.0 - v, u),
        2 => (1.0 - u, 1.0 - v),
        3 => (v, 1.0 - u),
        _ => (u, v),
    }
}

pub fn unrotate_delta(dx: i16, dy: i16, quarter_turns: u8) -> (i16, i16) {
    let neg = |v: i16| v.saturating_neg();
    match quarter_turns % 4 {
        1 => (neg(dy), dx),
        2 => (neg(dx), neg(dy)),
        3 => (dy, neg(dx)),
        _ => (dx, dy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(values: &[u8]) -> Vec<u8> {
        values.iter().flat_map(|v| [*v, 0, 0, 255]).collect()
    }

    fn reds(rgba: &[u8]) -> Vec<u8> {
        rgba.chunks(4).map(|p| p[0]).collect()
    }

    #[test]
    fn quarter_turns_rotate_counterclockwise() {
        let image = pixels(&[1, 2, 3, 4, 5, 6]);
        let (same, w, h) = rotate(&image, 3, 2, 0);
        assert_eq!((reds(&same), w, h), (vec![1, 2, 3, 4, 5, 6], 3, 2));
        let (ccw, w, h) = rotate(&image, 3, 2, 1);
        assert_eq!((reds(&ccw), w, h), (vec![3, 6, 2, 5, 1, 4], 2, 3));
        let (half, w, h) = rotate(&image, 3, 2, 2);
        assert_eq!((reds(&half), w, h), (vec![6, 5, 4, 3, 2, 1], 3, 2));
        let (cw, w, h) = rotate(&image, 3, 2, 3);
        assert_eq!((reds(&cw), w, h), (vec![4, 1, 5, 2, 6, 3], 2, 3));
    }

    #[test]
    fn mouse_movement_turns_back_with_the_picture() {
        assert_eq!(unrotate_delta(3, 5, 0), (3, 5));
        assert_eq!(unrotate_delta(3, 5, 1), (-5, 3));
        assert_eq!(unrotate_delta(3, 5, 2), (-3, -5));
        assert_eq!(unrotate_delta(3, 5, 3), (5, -3));
        assert_eq!(unrotate_delta(i16::MIN, 0, 2), (i16::MAX, 0));
    }

    #[test]
    fn pointer_positions_map_back_to_the_unrotated_picture() {
        assert_eq!(unrotate_point(0.25, 0.75, 0), (0.25, 0.75));
        assert_eq!(unrotate_point(0.25, 0.75, 1), (0.25, 0.25));
        assert_eq!(unrotate_point(0.25, 0.75, 2), (0.75, 0.25));
        assert_eq!(unrotate_point(0.25, 0.75, 3), (0.75, 0.75));
    }
}
