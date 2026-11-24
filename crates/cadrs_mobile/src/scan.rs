//! Reading a QR code from the camera: a frame's pixels in, the code's text out.
//!
//! The page grabs a frame from the camera's video into a canvas a few times a second and hands
//! its pixels here; the first code found ends the scan. A card's link adds a contact; a pairing
//! code asks to join the account (the page decides).

/// The text of the first QR code in an RGBA frame `width` × `height`, if there is one.
pub fn read(rgba: &[u8], width: usize, height: usize) -> Option<String> {
    if rgba.len() < width * height * 4 {
        return None;
    }
    let mut image = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
        let i = (y * width + x) * 4;
        // Luma from the colour.
        ((rgba[i] as u32 * 299 + rgba[i + 1] as u32 * 587 + rgba[i + 2] as u32 * 114) / 1000) as u8
    });
    image
        .detect_grids()
        .into_iter()
        .find_map(|grid| grid.decode().ok().map(|(_, text)| text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A QR code drawn into an RGBA frame, as a camera might see it (grey background, offset).
    fn frame(text: &str) -> (Vec<u8>, usize, usize) {
        let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
        let modules = code.width();
        let colors = code.to_colors();
        let scale = 6;
        let (w, h) = (640usize, 480usize);
        let (ox, oy) = (120usize, 40usize);
        let quiet = 4 * scale;
        let mut rgba = vec![180u8; w * h * 4];
        // The white quiet zone and the code.
        let side = modules * scale + 2 * quiet;
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = (x as isize - quiet as isize, y as isize - quiet as isize);
                let dark = mx >= 0
                    && my >= 0
                    && (mx as usize) < modules * scale
                    && (my as usize) < modules * scale
                    && colors[(my as usize / scale) * modules + mx as usize / scale]
                        == qrcode::Color::Dark;
                let v = if dark { 20 } else { 245 };
                let i = ((oy + y) * w + ox + x) * 4;
                rgba[i..i + 3].copy_from_slice(&[v, v, v]);
            }
        }
        (rgba, w, h)
    }

    #[test]
    fn a_card_link_is_read_from_a_frame() {
        let link = format!("https://cadrs.net/c/{}", "a2b3c4d5".repeat(30));
        let (rgba, w, h) = frame(&link);
        assert_eq!(read(&rgba, w, h), Some(link));
    }

    #[test]
    fn nothing_in_a_blank_frame() {
        assert_eq!(read(&vec![128u8; 64 * 48 * 4], 64, 48), None);
        assert_eq!(read(&[0u8; 3], 64, 48), None);
    }
}
