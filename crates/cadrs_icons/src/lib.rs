//! cadrs's SVG icons, embedded at compile time (vendored from icon-rs; drawn by cadrs_icons_gen).
//!
//! Every icon uses a 24x24 view box and is one of four [`Kind`]s: shaded isometric solids for
//! features, flat sketch tools, and ink-drawn glyphs and UI icons that an app can tint. Each
//! icon ships a light-theme and a dark-theme SVG; for any other theme, [`Icon::recolor`] swaps in
//! your own [`Palette`], and [`Icon::tinted`] paints a glyph or UI icon in any colour.

mod palette;

pub use palette::{Palette, Rgb};

/// What an icon depicts, which decides how it is drawn and whether it can be tinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Isometric CAD geometry with shaded faces: features, mates, analysis tools.
    Solid,
    /// Flat sketch geometry, with the points the user picks in the accent colour.
    Sketch,
    /// Small markers such as sketch constraints, legible at 10-16 px.
    Glyph,
    /// Interface icons such as undo, folder and search.
    Line,
}

impl Kind {
    /// Glyph and line icons are drawn in ink with at most a small accent detail, so they can be
    /// tinted freely with [`Icon::tinted`].
    pub fn is_tintable(self) -> bool {
        matches!(self, Kind::Glyph | Kind::Line)
    }
}

/// One icon: its name, kind, title and SVG source for light and dark backgrounds.
#[derive(Clone, Copy, Debug)]
pub struct Icon {
    /// Kebab-case file stem, such as `"linear-pattern"`.
    pub name: &'static str,
    pub kind: Kind,
    /// Human-readable name, such as `"Linear pattern"`.
    pub title: &'static str,
    /// For light backgrounds: dark outline, white to mid-grey faces.
    pub svg: &'static str,
    /// For dark backgrounds: light outline, darker faces, same lighting direction.
    pub svg_dark: &'static str,
}

impl Icon {
    /// The SVG for a light or dark background.
    pub fn themed(&self, dark: bool) -> &'static str {
        if dark { self.svg_dark } else { self.svg }
    }

    /// The light SVG with every palette colour replaced by the matching slot in `palette`.
    pub fn recolor(&self, palette: &Palette) -> String {
        // Two passes through placeholders, so a new colour that equals a later light slot is
        // not replaced again.
        let placeholder = |slot: usize| format!("#\0{slot}");
        let mut svg = self.svg.to_owned();
        for (slot, from) in Palette::LIGHT.slots().into_iter().enumerate() {
            svg = svg.replace(&from.to_hex(), &placeholder(slot));
        }
        for (slot, to) in palette.slots().into_iter().enumerate() {
            svg = svg.replace(&placeholder(slot), &to.to_hex());
        }
        svg
    }

    /// The light SVG with its ink painted `color`; any accent detail keeps the light accent.
    /// Meant for [`Kind::is_tintable`] icons; for other kinds only the outlines change.
    pub fn tinted(&self, color: impl Into<Rgb>) -> String {
        self.tinted_with(color, Palette::LIGHT.accent)
    }

    /// The light SVG with its ink painted `color` and its accent detail painted `accent`.
    pub fn tinted_with(&self, color: impl Into<Rgb>, accent: impl Into<Rgb>) -> String {
        self.recolor(&Palette {
            ink: color.into(),
            accent: accent.into(),
            ..Palette::LIGHT
        })
    }

    /// The light SVG with its ink set to `currentColor`, for HTML: the icon then takes the CSS
    /// `color` of the element it sits in, in any theme. Any accent detail keeps the light accent.
    /// Meant for [`Kind::is_tintable`] icons.
    pub fn current_color(&self) -> String {
        self.svg
            .replace(&Palette::LIGHT.ink.to_hex(), "currentColor")
    }

    /// Whether the icon has any accent-coloured detail. Apps that tint by multiplying a
    /// single-colour raster need a second layer (or a pre-coloured raster) for these.
    pub fn has_accent(&self) -> bool {
        self.svg.contains(&Palette::LIGHT.accent.to_hex())
    }
}

macro_rules! icons {
    ($($konst:ident => ($file:literal, $kind:ident, $title:literal)),* $(,)?) => {
        $(
            #[doc = concat!("![", $title, "](https://rvdende.github.io/icon-rs/icons/", $file, ".svg) ", $title)]
            pub const $konst: Icon = Icon {
                name: $file,
                kind: Kind::$kind,
                title: $title,
                svg: include_str!(concat!("../icons/", $file, ".svg")),
                svg_dark: include_str!(concat!("../icons/dark/", $file, ".svg")),
            };
        )*

        /// Every icon, grouped by kind in toolbar order.
        pub const ALL: &[Icon] = &[$($konst),*];
    };
}

include!("generated.rs");

/// Looks an icon up by its file stem, such as `"extrude"`.
pub fn get(name: &str) -> Option<Icon> {
    ALL.iter().copied().find(|icon| icon.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_a_24px_svg() {
        for icon in ALL {
            for svg in [icon.svg, icon.svg_dark] {
                assert!(svg.starts_with("<svg"), "{} is not an svg", icon.name);
                assert!(
                    svg.contains(r#"width="24" height="24""#),
                    "{} is not 24x24",
                    icon.name
                );
            }
        }
    }

    #[test]
    fn every_svg_file_is_registered() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                continue;
            }
            let stem = path.file_stem().unwrap().to_str().unwrap();
            assert!(
                get(stem).is_some(),
                "icons/{stem}.svg is missing from the icons! list"
            );
        }
    }

    #[test]
    fn dark_variant_is_the_light_svg_recoloured() {
        for icon in ALL {
            assert_eq!(icon.recolor(&Palette::DARK), icon.svg_dark, "{}", icon.name);
            assert_eq!(icon.recolor(&Palette::LIGHT), icon.svg, "{}", icon.name);
        }
    }

    #[test]
    fn light_svgs_only_use_palette_colours() {
        for icon in ALL {
            let mut rest = icon.svg;
            while let Some(at) = rest.find("=\"#") {
                let hex = Rgb::from_hex(&rest[at + 2..at + 9]).unwrap();
                assert!(
                    Palette::LIGHT.slots().contains(&hex),
                    "{} uses {hex:?}",
                    icon.name
                );
                rest = &rest[at + 9..];
            }
        }
    }

    #[test]
    fn recolor_does_not_chain_replacements() {
        // The new ink equals the light palette's top colour; outlines must stay white.
        let palette = Palette {
            ink: Palette::LIGHT.top,
            top: Rgb(1, 2, 3),
            ..Palette::LIGHT
        };
        let svg = SHELL.recolor(&palette);
        assert!(svg.contains(r##"stroke="#ffffff""##));
        assert!(svg.contains(r##"fill="#010203""##));
    }

    #[test]
    fn tintable_icons_use_only_ink_and_accent() {
        let accent = Palette::LIGHT.accent.to_hex();
        for icon in ALL.iter().filter(|i| i.kind.is_tintable()) {
            let white = icon.tinted(Rgb::WHITE);
            let colours = white.matches("=\"#").count();
            let allowed = white.matches("=\"#ffffff").count()
                + white.matches(&format!("=\"{accent}")).count();
            assert_eq!(
                colours, allowed,
                "{} uses a colour other than ink and accent",
                icon.name
            );
        }
    }

    #[test]
    fn current_color_paints_only_the_ink() {
        let svg = UNDO.current_color();
        assert!(svg.contains(r#"stroke="currentColor""#));
        assert!(!svg.contains(&Palette::LIGHT.ink.to_hex()));
        assert!(svg.contains(&Palette::LIGHT.accent.to_hex()));
    }

    #[test]
    fn tinting_keeps_the_accent() {
        let icon = ALL
            .iter()
            .find(|i| i.has_accent())
            .expect("an accented icon");
        let svg = icon.tinted_with(Rgb::WHITE, Rgb(1, 2, 3));
        assert!(svg.contains("#010203") && !svg.contains(&Palette::LIGHT.accent.to_hex()));
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(Rgb::from_hex("#38bdf8"), Some(Rgb(0x38, 0xbd, 0xf8)));
        assert_eq!(Rgb::from_hex("38BDF8"), Some(Rgb(0x38, 0xbd, 0xf8)));
        assert_eq!(Rgb(0x38, 0xbd, 0xf8).to_hex(), "#38bdf8");
        assert_eq!(Rgb::from_hex("#38bdf"), None);
        assert_eq!(Rgb::from_hex("#38bdfg"), None);
    }

    #[test]
    fn light_and_dark_are_new_of_their_line_and_accent() {
        // The site's JavaScript port of Palette::new must give these same values.
        assert_eq!(
            Palette::new(Palette::LIGHT.ink, Palette::LIGHT.accent),
            Palette::LIGHT
        );
        assert_eq!(
            Palette::new(Palette::DARK.ink, Palette::DARK.accent),
            Palette::DARK
        );
    }

    #[test]
    fn new_keeps_faces_neutral() {
        let p = Palette::new(Rgb(0x0c, 0x4a, 0x6e), Rgb(0xf5, 0x9e, 0x0b));
        assert_eq!(p.accent, Rgb(0xf5, 0x9e, 0x0b));
        assert_eq!(p.top, Rgb::WHITE);
        assert!(p.soft.luma() > p.mid.luma() && p.mid.luma() > p.shade.luma());
    }

    #[test]
    fn lookup_by_name() {
        assert_eq!(get("loft").unwrap().name, "loft");
        assert!(get("nope").is_none());
    }
}
