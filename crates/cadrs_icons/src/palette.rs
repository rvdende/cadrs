//! The colours icons are drawn with. Shared with the generator (`xtask`), which includes this
//! file by path, so it must not depend on the rest of the crate.

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const WHITE: Rgb = Rgb(0xff, 0xff, 0xff);
    pub const BLACK: Rgb = Rgb(0, 0, 0);

    /// Parses `#rrggbb` or `rrggbb`.
    pub fn from_hex(hex: &str) -> Option<Rgb> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Rgb(channel(0)?, channel(2)?, channel(4)?))
    }

    /// Lowercase `#rrggbb`, the form the SVGs use.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Moves `percent` of the way towards `other`, rounding to the nearest channel value.
    pub fn mix(self, other: Rgb, percent: u32) -> Rgb {
        let percent = percent.min(100);
        let channel =
            |a: u8, b: u8| ((a as u32 * (100 - percent) + b as u32 * percent + 50) / 100) as u8;
        Rgb(
            channel(self.0, other.0),
            channel(self.1, other.1),
            channel(self.2, other.2),
        )
    }

    /// Perceived brightness, 0 to 255 (Rec. 601 weights).
    pub fn luma(self) -> u32 {
        (self.0 as u32 * 299 + self.1 as u32 * 587 + self.2 as u32 * 114) / 1000
    }
}

/// The five colours an icon is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    /// Outlines and arrow heads.
    pub ink: Rgb,
    /// Context faces that point up.
    pub top: Rgb,
    /// Context bevels and secondary faces.
    pub soft: Rgb,
    /// Context faces on the right.
    pub mid: Rgb,
    /// Context faces on the left, in shadow.
    pub shade: Rgb,
    /// What the tool acts on or creates: the selected face, the new volume, a sketch tool's
    /// input points.
    pub accent: Rgb,
}

impl Palette {
    /// The colours `Icon::svg` is drawn with: `Palette::new` of a near-black line and blue accent.
    pub const LIGHT: Palette = Palette {
        ink: Rgb(0x26, 0x26, 0x26),
        top: Rgb(0xff, 0xff, 0xff),
        soft: Rgb(0xd4, 0xd4, 0xd4),
        mid: Rgb(0xa4, 0xa4, 0xa4),
        shade: Rgb(0x74, 0x74, 0x74),
        accent: Rgb(0x25, 0x63, 0xeb),
    };

    /// The colours `Icon::svg_dark` is drawn with: `Palette::new` of a near-white line and a
    /// lighter blue accent.
    pub const DARK: Palette = Palette {
        ink: Rgb(0xe4, 0xe4, 0xe7),
        top: Rgb(0xa0, 0xa0, 0xa2),
        soft: Rgb(0x89, 0x89, 0x8b),
        mid: Rgb(0x70, 0x70, 0x71),
        shade: Rgb(0x52, 0x52, 0x53),
        accent: Rgb(0x60, 0xa5, 0xfa),
    };

    /// Builds a palette from a line colour and an accent colour. The four face shades are
    /// neutral tints of the line colour, because faces are context: they show existing geometry.
    /// The accent marks what a tool acts on or creates.
    ///
    /// A dark line (a light theme) is tinted towards white, a light line (a dark theme) towards
    /// black, so faces stay lit from above in both. [`Palette::LIGHT`] and [`Palette::DARK`]
    /// are exactly `new` of their line and accent.
    ///
    /// Takes anything that converts to [`Rgb`].
    pub fn new(ink: impl Into<Rgb>, accent: impl Into<Rgb>) -> Palette {
        let (ink, accent) = (ink.into(), accent.into());
        let (towards, [top, soft, mid, shade]) = if ink.luma() > 127 {
            (Rgb::BLACK, Self::TINTS_LIGHT_INK)
        } else {
            (Rgb::WHITE, Self::TINTS_DARK_INK)
        };
        Palette {
            ink,
            top: ink.mix(towards, top),
            soft: ink.mix(towards, soft),
            mid: ink.mix(towards, mid),
            shade: ink.mix(towards, shade),
            accent,
        }
    }

    /// This palette with a different accent colour.
    pub fn with_accent(self, accent: impl Into<Rgb>) -> Palette {
        Palette {
            accent: accent.into(),
            ..self
        }
    }

    /// How far `new` mixes a light line towards black for top, soft, mid and shade, in percent.
    pub(crate) const TINTS_LIGHT_INK: [u32; 4] = [30, 40, 51, 64];
    /// How far `new` mixes a dark line towards white for top, soft, mid and shade, in percent.
    pub(crate) const TINTS_DARK_INK: [u32; 4] = [100, 80, 58, 36];

    /// Ink, top, soft, mid, shade, accent.
    pub(crate) fn slots(&self) -> [Rgb; 6] {
        [
            self.ink,
            self.top,
            self.soft,
            self.mid,
            self.shade,
            self.accent,
        ]
    }
}
