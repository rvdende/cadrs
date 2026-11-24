//! Organizations' logos: reading the picture a person picks, and showing logos.
//!
//! A logo is square and kept at 512 px a side ([`cadrs_org::profile::LOGO_SIDE`]), with a
//! 128 px copy for lists and cards. [`process`] takes whatever image file the person picked
//! (PNG, JPEG or GIF), refuses one that is too small or far from square, and makes the two
//! pictures; both are kept as blobs of the organization ([`cadrs_org::blob`]).
//!
//! [`LogoImages`] turns a logo's bytes into a picture to draw, once for each; [`logo`] draws
//! one, or the organization's initials on a colour of its own while there is none (or it
//! hasn't arrived yet).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::FontWeight;
use cadrs_org::profile::{LOGO_SIDE, LOGO_THUMB_SIDE, MAX_LOGO_BYTES, MAX_LOGO_THUMB_BYTES};
use cadrs_ui::theme::Theme;
use image::{DynamicImage, ImageFormat, RgbaImage, imageops::FilterType};

/// The largest file read as a logo.
pub const MAX_FILE: u64 = 20 * 1024 * 1024;
/// The largest side read (a guard against a file that claims to be enormous).
const MAX_SIDE: u32 = 10_000;
/// How far from square a picture may be and still be cropped to one: the longer side at most
/// this many times the shorter.
const MAX_ASPECT: f64 = 1.25;

/// Why a picked file can't be a logo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogoError {
    /// Not a PNG, JPEG or GIF picture (or a damaged one).
    NotAPicture,
    /// Smaller than 512 px on a side: (width, height).
    TooSmall(u32, u32),
    /// Too far from square: (width, height).
    NotSquare(u32, u32),
    /// The file is larger than [`MAX_FILE`].
    FileTooLarge,
}

impl LogoError {
    /// What to tell the person, from the catalogs.
    pub fn message(&self) -> String {
        use cadrs_ui::i18n::{tr, tr_with};
        let size = |w: &u32, h: &u32| [("width", w.to_string()), ("height", h.to_string())];
        match self {
            LogoError::NotAPicture => tr("org-logo-not-a-picture"),
            LogoError::FileTooLarge => tr("org-logo-file-too-large"),
            LogoError::TooSmall(w, h) => {
                let s = size(w, h);
                tr_with(
                    "org-logo-too-small",
                    &[(s[0].0, &s[0].1), (s[1].0, &s[1].1)],
                )
            }
            LogoError::NotSquare(w, h) => {
                let s = size(w, h);
                tr_with(
                    "org-logo-not-square",
                    &[(s[0].0, &s[0].1), (s[1].0, &s[1].1)],
                )
            }
        }
    }
}

/// A logo ready to keep: the 512 px picture and the 128 px one, each an image file's bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Processed {
    pub full: Vec<u8>,
    pub thumb: Vec<u8>,
}

/// Makes a logo from a picked image file's bytes.
pub fn process(bytes: &[u8]) -> Result<Processed, LogoError> {
    if bytes.len() as u64 > MAX_FILE {
        return Err(LogoError::FileTooLarge);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| LogoError::NotAPicture)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    let picture = reader.decode().map_err(|_| LogoError::NotAPicture)?;
    let (w, h) = (picture.width(), picture.height());
    if w.min(h) < LOGO_SIDE {
        return Err(LogoError::TooSmall(w, h));
    }
    if f64::from(w.max(h)) > f64::from(w.min(h)) * MAX_ASPECT {
        return Err(LogoError::NotSquare(w, h));
    }
    // The middle square.
    let side = w.min(h);
    let square = picture.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let full = square
        .resize_exact(LOGO_SIDE, LOGO_SIDE, FilterType::Lanczos3)
        .to_rgba8();
    let thumb = image::imageops::resize(
        &full,
        LOGO_THUMB_SIDE,
        LOGO_THUMB_SIDE,
        FilterType::Lanczos3,
    );
    Ok(Processed {
        full: encode(&full, MAX_LOGO_BYTES),
        thumb: encode(&thumb, MAX_LOGO_THUMB_BYTES),
    })
}

/// The picture as a PNG, or when that is over `max` bytes (a photograph) as a JPEG on white,
/// at the best quality that fits.
fn encode(picture: &RgbaImage, max: u64) -> Vec<u8> {
    let mut png = Vec::new();
    let written = DynamicImage::ImageRgba8(picture.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png);
    if written.is_ok() && png.len() as u64 <= max {
        return png;
    }
    // JPEG has no transparency: over white, as a logo is shown.
    let mut flat = image::RgbImage::new(picture.width(), picture.height());
    for (to, from) in flat.pixels_mut().zip(picture.pixels()) {
        let a = u32::from(from.0[3]);
        let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        to.0 = [over(from.0[0]), over(from.0[1]), over(from.0[2])];
    }
    let mut smallest = Vec::new();
    for quality in [92, 85, 75, 60, 45, 30] {
        let mut jpeg = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
        if flat.write_with_encoder(encoder).is_err() {
            continue;
        }
        if jpeg.len() as u64 <= max {
            return jpeg;
        }
        smallest = jpeg;
    }
    smallest
}

/// Whether `bytes` are a logo as cadrs makes one: a PNG or JPEG of exactly 128 × 128 or
/// 512 × 512 pixels, no larger a file than a logo of that side may be. Read from the file's
/// header, without decoding it.
///
/// Logos come from other people's devices. One that isn't this is never kept, decoded or drawn:
/// a "logo" of 11 000 × 11 000 pixels of one colour is a small file and half a gigabyte of
/// picture.
pub fn is_logo(bytes: &[u8]) -> bool {
    let Ok(reader) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()
    else {
        return false;
    };
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return false;
    }
    let len = bytes.len() as u64;
    match reader.into_dimensions() {
        Ok((w, h)) if w == h && w == LOGO_SIDE => len <= MAX_LOGO_BYTES,
        Ok((w, h)) if w == h && w == LOGO_THUMB_SIDE => len <= MAX_LOGO_THUMB_BYTES,
        _ => false,
    }
}

/// A logo's pixels, when `bytes` are one ([`is_logo`]): at most 512 × 512, so a bounded few
/// milliseconds of work.
fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    if !is_logo(bytes) {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(LOGO_SIDE);
    limits.max_image_height = Some(LOGO_SIDE);
    limits.max_alloc = Some(16 << 20);
    reader.limits(limits);
    Some(reader.decode().ok()?.to_rgba8())
}

/// Logos as pictures to draw, by their bytes' hash; `None` for bytes that turned out not to be
/// a logo (so they aren't looked at again).
#[derive(Resource, Default)]
pub struct LogoImages(HashMap<[u8; 32], Option<Handle<Image>>>);

impl LogoImages {
    /// The picture for the logo with this hash, made from `bytes` the first time. `None` when
    /// the bytes aren't a logo.
    pub fn get(
        &mut self,
        images: &mut Assets<Image>,
        hash: [u8; 32],
        bytes: &[u8],
    ) -> Option<Handle<Image>> {
        self.0
            .entry(hash)
            .or_insert_with(|| {
                let picture = decode(bytes)?;
                Some(images.add(Image::new(
                    Extent3d {
                        width: picture.width(),
                        height: picture.height(),
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    picture.into_raw(),
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                )))
            })
            .clone()
    }

    /// Whether the logo with this hash was looked at already (a picture or not).
    pub fn seen(&self, hash: &[u8; 32]) -> bool {
        self.0.contains_key(hash)
    }
}

/// The picture of a logo blob of one of this device's organizations or of a listed shop, when
/// its bytes are here ([`crate::social::logo_bytes`]).
pub fn handle(world: &mut World, blob: &cadrs_org::BlobRef) -> Option<Handle<Image>> {
    // Made before: no need to read its file again.
    if let Some(made) = world.resource::<LogoImages>().0.get(&blob.hash) {
        return made.clone();
    }
    let bytes = crate::social::logo_bytes(world, blob)?;
    world.resource_scope(|world, mut logos: Mut<LogoImages>| {
        logos.get(
            &mut world.resource_mut::<Assets<Image>>(),
            blob.hash,
            &bytes,
        )
    })
}

/// A logo `size` px a side named `name`: the picture, or while there is none the first letters
/// of `org` on a colour of its own.
pub fn logo(
    name: impl Into<String>,
    theme: &Theme,
    picture: Option<Handle<Image>>,
    org: &str,
    size: f32,
) -> impl Bundle {
    let t = theme.clone();
    let initials = cadrs_ui::Avatar::initials(org);
    let color = cadrs_ui::Avatar::color_for(org);
    let radius = (size * 0.18).max(3.0);
    (
        Name::new(name.into()),
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(Val::Px(radius)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(if picture.is_some() {
            Color::WHITE
        } else {
            color
        }),
        Children::spawn(bevy::ecs::spawn::SpawnWith(
            move |p: &mut ChildSpawner| match picture {
                Some(picture) => {
                    p.spawn((
                        ImageNode::new(picture),
                        Node {
                            width: Val::Px(size),
                            height: Val::Px(size),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                }
                None => {
                    p.spawn((
                        t.text(initials, size * 0.4, FontWeight::BOLD, Color::WHITE),
                        Pickable::IGNORE,
                    ));
                }
            },
        )),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG `w` × `h`: a flat colour, or noise (which a PNG can't make small).
    pub(crate) fn picture(w: u32, h: u32, noisy: bool) -> Vec<u8> {
        let mut seed = 0x2545_f491_u32;
        let image = RgbaImage::from_fn(w, h, |x, y| {
            if noisy {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                image::Rgba([seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 255])
            } else {
                image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 0x80, 255])
            }
        });
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn size_of(bytes: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory(bytes).unwrap();
        (image.width(), image.height())
    }

    #[test]
    fn a_square_picture_becomes_the_two_logos() {
        let logo = process(&picture(1024, 1024, false)).unwrap();
        assert_eq!(size_of(&logo.full), (LOGO_SIDE, LOGO_SIDE));
        assert_eq!(size_of(&logo.thumb), (LOGO_THUMB_SIDE, LOGO_THUMB_SIDE));
        assert!(logo.full.len() as u64 <= MAX_LOGO_BYTES);
        assert!(logo.thumb.len() as u64 <= MAX_LOGO_THUMB_BYTES);
        // Exactly 512 is enough, and a little off square is cropped to its middle.
        assert!(process(&picture(512, 512, false)).is_ok());
        let cropped = process(&picture(640, 512, false)).unwrap();
        assert_eq!(size_of(&cropped.full), (LOGO_SIDE, LOGO_SIDE));
        // The same picture gives the same bytes: every device names the same blobs.
        assert_eq!(
            process(&picture(512, 512, false)),
            process(&picture(512, 512, false))
        );
    }

    /// What other people's devices send as a logo is looked at before anything is made of it:
    /// only the two pictures [`process`] makes pass, by their header.
    #[test]
    fn only_a_logo_is_taken_for_one() {
        let made = process(&picture(700, 700, false)).unwrap();
        assert!(is_logo(&made.full) && is_logo(&made.thumb));
        let noisy = process(&picture(512, 512, true)).unwrap();
        assert!(is_logo(&noisy.full), "a JPEG logo is one too");
        // A picture larger than a logo (a flat one of any size is a small file, so the file's
        // size gives nothing away), an oblong one, another size, another format, and no
        // picture at all.
        for (what, bytes) in [
            ("large", picture(2048, 2048, false)),
            ("oblong", picture(4096, 1, false)),
            ("256", picture(256, 256, false)),
            (
                "not a picture",
                b"\x89PNG\r\n\x1a\n and then nothing".to_vec(),
            ),
            ("empty", Vec::new()),
        ] {
            assert!(!is_logo(&bytes), "{what}");
            assert!(decode(&bytes).is_none(), "{what}");
        }
        let mut gif = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::new(128, 128))
            .write_to(&mut std::io::Cursor::new(&mut gif), ImageFormat::Gif)
            .unwrap();
        assert!(!is_logo(&gif));
        // One that isn't a logo is remembered as such, and not looked at twice.
        let mut images = Assets::<Image>::default();
        let mut logos = LogoImages::default();
        assert!(
            logos
                .get(&mut images, [1; 32], &picture(256, 256, false))
                .is_none()
        );
        assert!(logos.seen(&[1; 32]));
        assert!(logos.get(&mut images, [2; 32], &made.thumb).is_some());
    }

    #[test]
    fn what_cant_be_a_logo_says_why() {
        assert_eq!(
            process(&picture(511, 600, false)),
            Err(LogoError::TooSmall(511, 600))
        );
        assert_eq!(
            process(&picture(1024, 512, false)),
            Err(LogoError::NotSquare(1024, 512))
        );
        assert_eq!(process(b"not a picture"), Err(LogoError::NotAPicture));
        assert_eq!(process(&[]), Err(LogoError::NotAPicture));
    }

    /// A photograph doesn't fit as a PNG: it is kept as a JPEG that does.
    #[test]
    fn a_noisy_picture_still_fits() {
        let logo = process(&picture(600, 600, true)).unwrap();
        assert!(
            logo.full.len() as u64 <= MAX_LOGO_BYTES,
            "{}",
            logo.full.len()
        );
        assert!(
            logo.thumb.len() as u64 <= MAX_LOGO_THUMB_BYTES,
            "{}",
            logo.thumb.len()
        );
        assert_eq!(size_of(&logo.full), (LOGO_SIDE, LOGO_SIDE));
        assert_eq!(size_of(&logo.thumb), (LOGO_THUMB_SIDE, LOGO_THUMB_SIDE));
        assert_eq!(
            image::guess_format(&logo.thumb).unwrap(),
            ImageFormat::Jpeg,
            "noise at 128 px is over the limit as a PNG"
        );
    }
}
