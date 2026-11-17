//! QR codes: a text (a contact card's link, a pairing code) as an image to show, for a phone's
//! camera to read.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use qrcode::{Color as Module, EcLevel, QrCode};

/// Pixels per module of the image (shown scaled; sampled nearest, so the edges stay sharp).
const SCALE: usize = 4;
/// The quiet zone around the code, in modules (the standard asks for 4).
const QUIET: usize = 4;

/// The modules of `text`'s QR code, row by row (`true` dark), and its width; `None` when the
/// text is too long for a QR code.
pub fn modules(text: &str) -> Option<(Vec<bool>, usize)> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
    let width = code.width();
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == Module::Dark)
        .collect();
    Some((dark, width))
}

/// `text`'s QR code as an image: black on white, with its quiet zone.
pub fn image(text: &str) -> Option<Image> {
    let (dark, width) = modules(text)?;
    let side = (width + 2 * QUIET) * SCALE;
    let mut pixels = vec![255u8; side * side * 4];
    for y in 0..width {
        for x in 0..width {
            if !dark[y * width + x] {
                continue;
            }
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    let px = (x + QUIET) * SCALE + dx;
                    let py = (y + QUIET) * SCALE + dy;
                    let i = (py * side + px) * 4;
                    pixels[i..i + 3].copy_from_slice(&[0, 0, 0]);
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_link_fits_a_qr_code() {
        let link = format!("https://cadrs.net/c/{}", "a".repeat(250));
        let (dark, width) = modules(&link).unwrap();
        assert_eq!(dark.len(), width * width);
        // The finder pattern's corner module is dark.
        assert!(dark[0]);
        let image = image(&link).unwrap();
        assert_eq!(image.width() as usize, (width + 2 * QUIET) * SCALE);
    }
}
