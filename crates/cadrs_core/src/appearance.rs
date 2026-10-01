//! Appearances (PS9, X9, P3.5): the colour and opacity of parts, faces, features and sketches.
//!
//! - New parts and surfaces take the colours of an 8-colour [`PALETTE`] in turn (PS9.1): a part
//!   keeps the colour of the number it was made with ([`crate::Part::palette`]), so deleting a
//!   part doesn't recolour the others.
//! - A part's own appearance (Edit appearance, PS9.2–9.3) replaces its palette colour; a
//!   feature's appearance colours the faces that feature made; a face's appearance colours that
//!   face (PS9.4). The most specific wins: face, then feature, then part.
//! - A sketch's appearance colours its curves (PS9.5).
//!
//! Appearances are stored in the Part Studio ([`crate::PartProps`], and the element's
//! feature appearances), outside the feature list, so changing one doesn't rebuild anything.

use cadrs_sketch::FaceName;
use serde::{Deserialize, Serialize};

use crate::ids::FeatureId;
use crate::parts::Part;
use crate::document::PartProps;

/// A colour (sRGB) and its opacity (255: opaque).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Appearance {
    pub rgb: [u8; 3],
    #[serde(default = "opaque")]
    pub alpha: u8,
}

fn opaque() -> u8 {
    255
}

impl Appearance {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { rgb: [r, g, b], alpha: 255 }
    }

    pub fn with_alpha(self, alpha: u8) -> Self {
        Self { alpha, ..self }
    }

    pub fn is_opaque(&self) -> bool {
        self.alpha == 255
    }

    /// "#9BC1D8".
    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.rgb[0], self.rgb[1], self.rgb[2])
    }

    /// Reads "#9bc1d8", "9BC1D8" or the short "#abc" (the opacity is kept at 255).
    pub fn from_hex(text: &str) -> Option<Self> {
        let h = text.trim().trim_start_matches('#');
        let digits: Vec<u8> = h
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<Vec<u8>>>()?;
        match digits.len() {
            6 => Some(Self::rgb(
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
            )),
            3 => Some(Self::rgb(digits[0] * 17, digits[1] * 17, digits[2] * 17)),
            _ => None,
        }
    }

    /// The opacity in percent (0–100), as the dialog shows it.
    pub fn opacity_percent(&self) -> u8 {
        ((self.alpha as f32) / 255.0 * 100.0).round() as u8
    }

    /// Hue (degrees, 0–360), saturation and value (0–1) of the colour (the mixer's coordinates).
    pub fn hsv(&self) -> [f32; 3] {
        let [r, g, b] = self.rgb.map(|c| c as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        let s = if max == 0.0 { 0.0 } else { d / max };
        [h, s, max]
    }

    /// The colour of hue `h` (degrees), saturation `s` and value `v` (0–1).
    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = h.rem_euclid(360.0);
        let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match (h / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let u = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Self::rgb(u(r), u(g), u(b))
    }
}

/// The 8 colours new parts and surfaces take in turn (PS9.1). The first is the light blue-grey
/// parts have always had (`screens/24`); the others are soft tints of distinct hues, light enough
/// for black edges and the selection's amber to read on them.
pub const PALETTE: [Appearance; 8] = [
    Appearance::rgb(155, 193, 216), // light blue
    Appearance::rgb(168, 207, 154), // green
    Appearance::rgb(216, 167, 167), // rose
    Appearance::rgb(198, 179, 222), // lavender
    Appearance::rgb(230, 211, 143), // sand
    Appearance::rgb(143, 207, 200), // teal
    Appearance::rgb(217, 179, 140), // tan
    Appearance::rgb(176, 184, 192), // grey
];

/// The preset swatches of the Edit appearance dialog (PS9.2): the palette, then a row of
/// stronger colours and a row of neutrals.
pub const SWATCHES: [Appearance; 24] = [
    PALETTE[0], PALETTE[1], PALETTE[2], PALETTE[3], PALETTE[4], PALETTE[5], PALETTE[6], PALETTE[7],
    Appearance::rgb(214, 48, 49),
    Appearance::rgb(230, 126, 34),
    Appearance::rgb(241, 196, 15),
    Appearance::rgb(39, 174, 96),
    Appearance::rgb(22, 160, 133),
    Appearance::rgb(41, 128, 185),
    Appearance::rgb(52, 73, 94),
    Appearance::rgb(142, 68, 173),
    Appearance::rgb(255, 255, 255),
    Appearance::rgb(224, 224, 224),
    Appearance::rgb(189, 189, 189),
    Appearance::rgb(158, 158, 158),
    Appearance::rgb(117, 117, 117),
    Appearance::rgb(84, 84, 84),
    Appearance::rgb(48, 48, 48),
    Appearance::rgb(0, 0, 0),
];

/// The palette colour of the `n`-th part or surface made (0-based), cycling through the 8.
pub fn palette(n: u32) -> Appearance {
    PALETTE[(n % PALETTE.len() as u32) as usize]
}

/// A part's appearance: its own, else (a pattern's or mirror's copy, P3.8, PS9.6) its seed
/// part's, else its palette colour (a copy has its seed's palette entry).
pub fn part_appearance(part: &Part, props: &[PartProps]) -> Appearance {
    let own = |id: crate::ids::PartId| props.iter().find(|p| p.part == id).and_then(|p| p.appearance);
    own(part.id)
        .or_else(|| own(part.source?))
        .or_else(|| part.derived.as_ref()?.appearance)
        .or_else(|| part.solid.looks.first().map(|(_, a)| *a))
        .unwrap_or_else(|| default_appearance(part, props))
}

/// A keep-out or keep-in part's default colour (PCB9.4; `ex3-step9-triad-move.png` shows the
/// Keep-out grey).
pub const KEEP_GREY: Appearance = Appearance::rgb(128, 128, 128);

/// A part's colour when nothing sets one: grey for a part named as a keep-out or keep-in
/// ([`crate::pcb::names::role_of`]), else its palette colour.
pub fn default_appearance(part: &Part, props: &[PartProps]) -> Appearance {
    match crate::pcb::names::role_of(crate::parts::display_name(part, props)) {
        Some(crate::pcb::names::PartRole::KeepOut | crate::pcb::names::PartRole::KeepIn) => KEEP_GREY,
        _ => palette(part.palette),
    }
}

/// Where a face's appearance comes from (the Appearances panel shows it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Part,
    Feature(FeatureId),
    Face,
}

/// A face's appearance and where it comes from: the face's own, else the feature's that made
/// the face, else the part's (PS9.4).
pub fn face_appearance(
    part: &Part,
    face: &FaceName,
    props: &[PartProps],
    features: &[(FeatureId, Appearance)],
) -> (Appearance, Source) {
    let prop = props.iter().find(|p| p.part == part.id);
    if let Some(a) = prop.and_then(|p| p.faces.iter().find(|(f, _)| f == face)).map(|(_, a)| *a) {
        return (a, Source::Face);
    }
    if let Some((f, a)) = features.iter().find(|(f, _)| f.0 == face.op) {
        return (*a, Source::Feature(*f));
    }
    // A pattern's copy of a face shows the appearance of the feature that made the seed face.
    if let cadrs_sketch::FaceOrigin::Instance { of, .. } = face.origin
        && let Some((f, a)) = features.iter().find(|(f, _)| f.0 == of)
    {
        return (*a, Source::Feature(*f));
    }
    // P3H.6: the rebuild's own colour for the face (a copied context part, a composite's member),
    // unless the part has its own appearance.
    if prop.and_then(|p| p.appearance).is_none()
        && let Some((_, a)) = part.solid.looks.iter().find(|(f, _)| f == face)
    {
        return (*a, Source::Part);
    }
    (part_appearance(part, props), Source::Part)
}

/// True if any face of the part has its own or a feature's appearance (it is drawn per face).
pub fn has_face_overrides(part: &Part, props: &[PartProps], features: &[(FeatureId, Appearance)]) -> bool {
    props.iter().any(|p| p.part == part.id && !p.faces.is_empty())
        || (part.solid.looks.len() > 1 && !props.iter().any(|p| p.part == part.id && p.appearance.is_some()))
        || features
            .iter()
            .any(|(f, _)| part.solid.faces.iter().any(|x| x.name.op == f.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let a = Appearance::rgb(155, 193, 216);
        assert_eq!(a.hex(), "#9BC1D8");
        assert_eq!(Appearance::from_hex("#9bc1d8"), Some(a));
        assert_eq!(Appearance::from_hex("9BC1D8"), Some(a));
        assert_eq!(Appearance::from_hex("#fff"), Some(Appearance::rgb(255, 255, 255)));
        assert_eq!(Appearance::from_hex("#12345"), None);
        assert_eq!(Appearance::from_hex("#zzzzzz"), None);
    }

    #[test]
    fn hsv_round_trip() {
        for a in SWATCHES {
            let [h, s, v] = a.hsv();
            assert_eq!(Appearance::from_hsv(h, s, v), a, "{a:?}");
        }
        assert_eq!(Appearance::from_hsv(0.0, 1.0, 1.0), Appearance::rgb(255, 0, 0));
        assert_eq!(Appearance::from_hsv(120.0, 1.0, 1.0), Appearance::rgb(0, 255, 0));
        assert_eq!(Appearance::from_hsv(240.0, 1.0, 0.5), Appearance::rgb(0, 0, 128));
    }

    #[test]
    fn the_palette_cycles() {
        assert_eq!(palette(0), PALETTE[0]);
        assert_eq!(palette(8), PALETTE[0]);
        assert_eq!(palette(9), PALETTE[1]);
        assert_eq!(Appearance::rgb(1, 2, 3).with_alpha(128).opacity_percent(), 50);
    }
}
