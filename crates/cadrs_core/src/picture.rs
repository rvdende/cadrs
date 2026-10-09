//! The **Image** feature: a picture file (PNG, JPEG or GIF) dropped into a Part Studio, made into
//! a flat surface part showing it, which moves like any other part.
//!
//! - The file's bytes are a blob ([`crate::blobs`]) named by their content hash, as an Import's
//!   are; the feature keeps the hash, the file name, the plane it lies on, where its centre is on
//!   that plane and its size. [`ImageFeature::from_file`] (or the [`AddImage`] command) stores
//!   the bytes and sizes the picture: its longer side [`DEFAULT_SIZE`] mm, keeping its aspect.
//! - **Rebuild** (`rebuild/kernel_ops/picture.rs`): the rectangle is filled with its plane into a
//!   surface part named after the file, which carries the picture as a
//!   [`crate::solid::SolidImage`]: the rectangle's corner and its width and height vectors.
//!   Moving or copying the part (Transform) maps those with it, so the picture stays on it.
//! - The picture's top is the plane's +v side and its left the −u side, as seen from the side the
//!   plane's normal points to.

use std::sync::Arc;

use cadrs_sketch::PlaneRef;
use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, FeatureKind};
use crate::ids::{ElementId, FeatureId};

/// The longer side of a newly inserted picture (mm).
pub const DEFAULT_SIZE: f64 = 100.0;

/// The picture formats an Image reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
}

impl ImageFormat {
    /// The format of a file with this name (by its extension, in any case), if Image reads it.
    pub fn of_file_name(name: &str) -> Option<Self> {
        let ext = std::path::Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "gif" => Some(Self::Gif),
            _ => None,
        }
    }

    /// [`Self::of_file_name`] of a path's file name.
    pub fn of_path(path: &std::path::Path) -> Option<Self> {
        Self::of_file_name(path.file_name()?.to_str()?)
    }

    fn image(self) -> image::ImageFormat {
        match self {
            Self::Png => image::ImageFormat::Png,
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::Gif => image::ImageFormat::Gif,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
        }
    }
}

/// An Image feature: a picture stored with the document, shown on a flat surface part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageFeature {
    /// The file's content hash ([`crate::blobs::hash_of`]).
    pub blob: String,
    /// The file's name as inserted ("logo.png"); the part is named after its stem.
    pub file_name: String,
    pub format: ImageFormat,
    /// The plane it lies on (its frame as of when it was placed).
    pub plane: PlaneRef,
    /// Where its centre is, in the plane's coordinates (mm).
    pub center: [f64; 2],
    /// Its size along the plane's u (width) and v (height) axes (mm).
    pub width: f64,
    pub height: f64,
}

impl ImageFeature {
    /// An image of the file `file_name` (its bytes stored as a blob), centred at `center` on
    /// `plane`, its longer side [`DEFAULT_SIZE`] mm.
    pub fn from_file(file_name: &str, bytes: Vec<u8>, plane: PlaneRef, center: [f64; 2]) -> Result<Self, String> {
        let format = ImageFormat::of_file_name(file_name)
            .or_else(|| sniff(&bytes))
            .ok_or_else(|| format!("{file_name}: not a PNG, JPEG or GIF file"))?;
        let (w, h) = pixel_size(&bytes, format).map_err(|e| format!("{file_name}: {e}"))?;
        let k = DEFAULT_SIZE / w.max(h) as f64;
        let blob = crate::blobs::insert(bytes);
        Ok(Self { blob, file_name: file_name.to_string(), format, plane, center, width: w as f64 * k, height: h as f64 * k })
    }

    /// Why it can't be built, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        if self.blob.is_empty() {
            Some("Select a picture")
        } else if !(self.width > 0.0 && self.height > 0.0) {
            Some("The picture has no size")
        } else {
            None
        }
    }

    /// The features it depends on: the plane's.
    pub fn parents(&self) -> Vec<FeatureId> {
        match self.plane {
            PlaneRef::Feature(fp) => vec![FeatureId(fp.feature)],
            PlaneRef::Face(fp) => vec![FeatureId(fp.feature)],
            _ => Vec::new(),
        }
    }

    /// The file name without its extension ("logo"), for the part's name.
    pub fn stem(&self) -> String {
        crate::import::stem(&self.file_name)
    }

    /// The extension its blob file gets.
    pub fn extension(&self) -> String {
        std::path::Path::new(&self.file_name)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| self.format.extension().to_string())
    }

    /// Where the picture lies in space: its lower-left corner and its width and height vectors.
    pub fn placement(&self) -> crate::solid::SolidImage {
        let f = self.plane.frame();
        let at = |x: f64, y: f64| f.to_world(cadrs_sketch::Vec2::new(x, y));
        let corner = at(self.center[0] - self.width / 2.0, self.center[1] - self.height / 2.0);
        let s = |a: [f64; 3], k: f64| [a[0] * k, a[1] * k, a[2] * k];
        crate::solid::SolidImage { blob: self.blob.clone(), corner, u: s(unit(f.u), self.width), v: s(unit(f.v), self.height) }
    }
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-15 { a } else { [a[0] / l, a[1] / l, a[2] / l] }
}

/// The format of a picture by its first bytes.
fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some(ImageFormat::Png),
        image::ImageFormat::Jpeg => Some(ImageFormat::Jpeg),
        image::ImageFormat::Gif => Some(ImageFormat::Gif),
        _ => None,
    }
}

/// A picture's width and height in pixels.
fn pixel_size(bytes: &[u8], format: ImageFormat) -> Result<(u32, u32), String> {
    // The bytes say what they are when the extension is wrong (a PNG saved as .jpg).
    let format = sniff(bytes).unwrap_or(format);
    let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(bytes), format.image())
        .into_dimensions()
        .map_err(|e| format!("cannot read the picture: {e}"))?;
    if w == 0 || h == 0 {
        return Err("the picture is empty".into());
    }
    Ok((w, h))
}

/// Inserts a picture as an Image feature at the end of a Part Studio: "Image N". The bytes are
/// kept in the blob cache ([`crate::blobs`]); the Store writes them next to the document when it
/// is saved.
#[derive(Debug, Clone)]
pub struct AddImage {
    pub element: ElementId,
    pub feature: FeatureId,
    pub file_name: String,
    pub bytes: Arc<Vec<u8>>,
    pub plane: PlaneRef,
    pub center: [f64; 2],
}

impl Command for AddImage {
    fn label(&self) -> String {
        "Insert image".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let x = ImageFeature::from_file(&self.file_name, self.bytes.to_vec(), self.plane, self.center).map_err(CommandError::Invalid)?;
        crate::commands::AddFeature {
            element: self.element,
            feature: self.feature,
            base_name: "Image".into(),
            kind: FeatureKind::Image(x),
        }
        .apply(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny PNG, `w` × `h` pixels.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn formats_by_extension_in_any_case() {
        assert_eq!(ImageFormat::of_file_name("a.JPG"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::of_file_name("a.jpeg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::of_file_name("a.png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::of_file_name("a.GIF"), Some(ImageFormat::Gif));
        assert_eq!(ImageFormat::of_file_name("a.step"), None);
    }

    #[test]
    fn a_picture_keeps_its_aspect_with_its_longer_side_the_default_size() {
        let x = ImageFeature::from_file("logo.png", png(40, 20), PlaneRef::Front, [5.0, 0.0]).unwrap();
        assert_eq!((x.width, x.height), (DEFAULT_SIZE, DEFAULT_SIZE / 2.0));
        assert_eq!(x.stem(), "logo");
        assert!(crate::blobs::contains(&x.blob));
        // On the Front plane (u = +X, v = +Z), centred at (5, 0).
        let p = x.placement();
        assert_eq!(p.corner, [5.0 - 50.0, 0.0, -25.0]);
        assert_eq!(p.u, [100.0, 0.0, 0.0]);
        assert_eq!(p.v, [0.0, 0.0, 50.0]);
        // The bytes say what a misnamed file is.
        assert!(ImageFeature::from_file("logo.jpg", png(4, 4), PlaneRef::Top, [0.0, 0.0]).is_ok());
        assert!(ImageFeature::from_file("notes.png", b"not a picture".to_vec(), PlaneRef::Top, [0.0, 0.0]).is_err());
    }
}
