//! Drawing standards: sheet sizes (ANSI/ASME Y14.1 and ISO 5457 / ISO 216), orientations,
//! projection methods and the border and zone layout of a sheet (D1.6, D1.8).
//!
//! Everything is in millimetres on the sheet, origin at the bottom-left corner of the trimmed
//! sheet, y up.

use serde::{Deserialize, Serialize};

/// A drafting standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Standard {
    Ansi,
    Iso,
}

impl Standard {
    pub const ALL: [Standard; 2] = [Standard::Ansi, Standard::Iso];

    pub fn label(self) -> &'static str {
        match self {
            Standard::Ansi => "ANSI",
            Standard::Iso => "ISO",
        }
    }

    /// The sheet sizes of this standard, smallest first.
    pub fn sizes(self) -> &'static [SheetSize] {
        match self {
            Standard::Ansi => &SheetSize::ANSI,
            Standard::Iso => &SheetSize::ISO,
        }
    }

    /// The projection method the standard's templates use: third angle in ANSI (ASME Y14.3),
    /// first angle in ISO (ISO 5456-2).
    pub fn default_projection(self) -> Projection {
        match self {
            Standard::Ansi => Projection::Third,
            Standard::Iso => Projection::First,
        }
    }
}

/// A standard sheet size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SheetSize {
    AnsiA,
    AnsiB,
    AnsiC,
    AnsiD,
    AnsiE,
    IsoA4,
    IsoA3,
    IsoA2,
    IsoA1,
    IsoA0,
}

impl SheetSize {
    pub const ANSI: [SheetSize; 5] = [
        SheetSize::AnsiA,
        SheetSize::AnsiB,
        SheetSize::AnsiC,
        SheetSize::AnsiD,
        SheetSize::AnsiE,
    ];
    pub const ISO: [SheetSize; 5] = [
        SheetSize::IsoA4,
        SheetSize::IsoA3,
        SheetSize::IsoA2,
        SheetSize::IsoA1,
        SheetSize::IsoA0,
    ];
    /// Every size, ANSI then ISO.
    pub const ALL: [SheetSize; 10] = [
        SheetSize::AnsiA,
        SheetSize::AnsiB,
        SheetSize::AnsiC,
        SheetSize::AnsiD,
        SheetSize::AnsiE,
        SheetSize::IsoA4,
        SheetSize::IsoA3,
        SheetSize::IsoA2,
        SheetSize::IsoA1,
        SheetSize::IsoA0,
    ];

    pub fn standard(self) -> Standard {
        match self {
            SheetSize::AnsiA
            | SheetSize::AnsiB
            | SheetSize::AnsiC
            | SheetSize::AnsiD
            | SheetSize::AnsiE => Standard::Ansi,
            _ => Standard::Iso,
        }
    }

    /// The size letter as printed in the title block: "A", "B" … or "A4", "A3" ….
    pub fn letter(self) -> &'static str {
        match self {
            SheetSize::AnsiA => "A",
            SheetSize::AnsiB => "B",
            SheetSize::AnsiC => "C",
            SheetSize::AnsiD => "D",
            SheetSize::AnsiE => "E",
            SheetSize::IsoA4 => "A4",
            SheetSize::IsoA3 => "A3",
            SheetSize::IsoA2 => "A2",
            SheetSize::IsoA1 => "A1",
            SheetSize::IsoA0 => "A0",
        }
    }

    /// "ANSI A", "ISO A3".
    pub fn label(self) -> String {
        format!("{} {}", self.standard().label(), self.letter())
    }

    /// "ANSI A (8.5 × 11 in)", "ISO A3 (297 × 420 mm)".
    pub fn long_label(self) -> String {
        let (s, l) = self.short_long_mm();
        match self.standard() {
            Standard::Ansi => format!("{} ({} × {} in)", self.label(), fmt_in(s / 25.4), fmt_in(l / 25.4)),
            Standard::Iso => format!("{} ({} × {} mm)", self.label(), s, l),
        }
    }

    /// The size with its width × height in `orientation`: "ANSI A (11 × 8.5 in)" landscape,
    /// "ANSI A (8.5 × 11 in)" portrait.
    pub fn oriented_label(self, orientation: Orientation) -> String {
        let (s, l) = self.short_long_mm();
        let (w, h) = match orientation {
            Orientation::Landscape => (l, s),
            Orientation::Portrait => (s, l),
        };
        match self.standard() {
            Standard::Ansi => format!("{} ({} × {} in)", self.label(), fmt_in(w / 25.4), fmt_in(h / 25.4)),
            Standard::Iso => format!("{} ({} × {} mm)", self.label(), w, h),
        }
    }

    /// The short and long side of the trimmed sheet, in mm. ANSI sizes are exact inch values
    /// (ASME Y14.1); ISO sizes are ISO 216 A-series values (ISO 5457).
    pub fn short_long_mm(self) -> (f64, f64) {
        const IN: f64 = 25.4;
        match self {
            SheetSize::AnsiA => (8.5 * IN, 11.0 * IN),
            SheetSize::AnsiB => (11.0 * IN, 17.0 * IN),
            SheetSize::AnsiC => (17.0 * IN, 22.0 * IN),
            SheetSize::AnsiD => (22.0 * IN, 34.0 * IN),
            SheetSize::AnsiE => (34.0 * IN, 44.0 * IN),
            SheetSize::IsoA4 => (210.0, 297.0),
            SheetSize::IsoA3 => (297.0, 420.0),
            SheetSize::IsoA2 => (420.0, 594.0),
            SheetSize::IsoA1 => (594.0, 841.0),
            SheetSize::IsoA0 => (841.0, 1189.0),
        }
    }

    /// How many zones run along the long and the short side. ISO 5457 fixes these (fields of
    /// about 50 mm); for ANSI we use zones of about 4¼–5½ in, as ASME Y14.1 recommends.
    pub fn zone_counts(self) -> (usize, usize) {
        match self {
            SheetSize::AnsiA => (2, 2),
            SheetSize::AnsiB => (4, 2),
            SheetSize::AnsiC => (4, 4),
            SheetSize::AnsiD => (8, 4),
            SheetSize::AnsiE => (8, 6),
            SheetSize::IsoA4 => (6, 4),
            SheetSize::IsoA3 => (8, 6),
            SheetSize::IsoA2 => (12, 8),
            SheetSize::IsoA1 => (16, 12),
            SheetSize::IsoA0 => (24, 16),
        }
    }
}

fn fmt_in(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Which way up a sheet is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

impl Orientation {
    pub const ALL: [Orientation; 2] = [Orientation::Landscape, Orientation::Portrait];

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Landscape => "Landscape",
            Orientation::Portrait => "Portrait",
        }
    }
}

/// The projection method (D1.5): where projected views go relative to their parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Projection {
    /// ISO: the view from the left is placed on the right, the view from above below.
    First,
    /// ANSI: the view from the right is placed on the right, the view from above above.
    Third,
}

impl Projection {
    pub const ALL: [Projection; 2] = [Projection::First, Projection::Third];

    pub fn label(self) -> &'static str {
        match self {
            Projection::First => "First angle",
            Projection::Third => "Third angle",
        }
    }
}

/// A sheet's size and orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SheetFormat {
    pub size: SheetSize,
    #[serde(default)]
    pub orientation: Orientation,
}

impl SheetFormat {
    pub fn new(size: SheetSize, orientation: Orientation) -> Self {
        Self { size, orientation }
    }

    /// Width and height of the trimmed sheet, in mm.
    pub fn size_mm(self) -> (f64, f64) {
        let (s, l) = self.size.short_long_mm();
        match self.orientation {
            Orientation::Landscape => (l, s),
            Orientation::Portrait => (s, l),
        }
    }

    /// Zones across (columns) and up (rows).
    pub fn zone_grid(self) -> (usize, usize) {
        let (l, s) = self.size.zone_counts();
        match self.orientation {
            Orientation::Landscape => (l, s),
            Orientation::Portrait => (s, l),
        }
    }
}

/// An axis-aligned rectangle in sheet millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl Rect {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            min: [x0.min(x1), y0.min(y1)],
            max: [x0.max(x1), y0.max(y1)],
        }
    }

    pub fn width(&self) -> f64 {
        self.max[0] - self.min[0]
    }

    pub fn height(&self) -> f64 {
        self.max[1] - self.min[1]
    }

    pub fn center(&self) -> [f64; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }
}

/// One zone band division: its label and where it starts and ends along its axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub label: String,
    pub from: f64,
    pub to: f64,
}

/// The border of a sheet: the outer line, the drawing frame inside it, and the zones labelled
/// in the band between them.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The outer border line (zone labels are between it and `inner`).
    pub outer: Rect,
    /// The drawing frame: views and the title block go inside it.
    pub inner: Rect,
    /// Columns, left to right, with their labels.
    pub columns: Vec<Zone>,
    /// Rows, bottom to top, with their labels.
    pub rows: Vec<Zone>,
}

/// The border and zones of a sheet of `format` under `standard`'s conventions:
/// - ANSI (ASME Y14.1): the border 0.25 in in from the trimmed edge and the frame 0.25 in
///   inside it; columns numbered from the right (…, 2, 1), rows lettered from the bottom
///   (A, B, …), so zone A1 is at the title block.
/// - ISO 5457: the frame 20 mm in on the left (filing margin) and 10 mm elsewhere, the border
///   line halfway out; columns numbered from the left (1, 2, …), rows lettered from the top
///   (A, B, …).
pub fn frame(format: SheetFormat) -> Frame {
    let (w, h) = format.size_mm();
    let standard = format.size.standard();
    let (outer, inner) = match standard {
        Standard::Ansi => {
            let a = 0.25 * 25.4;
            let b = 0.5 * 25.4;
            (Rect::new(a, a, w - a, h - a), Rect::new(b, b, w - b, h - b))
        }
        Standard::Iso => (
            Rect::new(5.0, 5.0, w - 5.0, h - 5.0),
            Rect::new(20.0, 10.0, w - 10.0, h - 10.0),
        ),
    };
    let (nx, ny) = format.zone_grid();
    let split = |from: f64, to: f64, n: usize| -> Vec<(f64, f64)> {
        let step = (to - from) / n as f64;
        (0..n)
            .map(|i| (from + step * i as f64, from + step * (i + 1) as f64))
            .collect()
    };
    let letter = |i: usize| -> String {
        // A, B, … skipping I and O (ASME Y14.1 and ISO 5457 both omit them).
        let letters: Vec<char> = ('A'..='Z').filter(|c| *c != 'I' && *c != 'O').collect();
        let c = letters[i % letters.len()];
        if i >= letters.len() {
            format!("{c}{c}")
        } else {
            c.to_string()
        }
    };
    let columns = split(inner.min[0], inner.max[0], nx)
        .into_iter()
        .enumerate()
        .map(|(i, (from, to))| Zone {
            label: match standard {
                Standard::Ansi => (nx - i).to_string(),
                Standard::Iso => (i + 1).to_string(),
            },
            from,
            to,
        })
        .collect();
    let rows = split(inner.min[1], inner.max[1], ny)
        .into_iter()
        .enumerate()
        .map(|(i, (from, to))| Zone {
            label: match standard {
                Standard::Ansi => letter(i),
                Standard::Iso => letter(ny - 1 - i),
            },
            from,
            to,
        })
        .collect();
    Frame {
        outer,
        inner,
        columns,
        rows,
    }
}

/// A drawing scale, `num:den` (1:2 halves lengths on the sheet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scale {
    pub num: u32,
    pub den: u32,
}

impl Default for Scale {
    fn default() -> Self {
        Self { num: 1, den: 1 }
    }
}

impl Scale {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    /// Sheet length per model length.
    pub fn factor(self) -> f64 {
        self.num as f64 / self.den.max(1) as f64
    }

    /// "1:2".
    pub fn label(self) -> String {
        format!("{}:{}", self.num, self.den)
    }

    /// Reads "1:2", "2 : 1" or "0.5".
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some((a, b)) = s.split_once(':') {
            let num = a.trim().parse::<u32>().ok()?;
            let den = b.trim().parse::<u32>().ok()?;
            (num > 0 && den > 0).then_some(Self { num, den })
        } else {
            let v = s.parse::<f64>().ok()?;
            if v <= 0.0 || !v.is_finite() {
                return None;
            }
            if v >= 1.0 {
                Some(Self::new(v.round() as u32, 1))
            } else {
                Some(Self::new(1, (1.0 / v).round() as u32))
            }
        }
    }

    /// The scales offered in the Sheet properties dialog (ISO 5455 and the usual inch ones).
    pub const COMMON: [Scale; 14] = [
        Scale::new(1, 1),
        Scale::new(1, 2),
        Scale::new(1, 4),
        Scale::new(1, 5),
        Scale::new(1, 8),
        Scale::new(1, 10),
        Scale::new(1, 20),
        Scale::new(1, 50),
        Scale::new(1, 100),
        Scale::new(2, 1),
        Scale::new(4, 1),
        Scale::new(5, 1),
        Scale::new(10, 1),
        Scale::new(20, 1),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_a_zones_read_like_the_course() {
        // ex1-drawing.png: columns 2 | 1 left to right, rows B over A.
        let f = frame(SheetFormat::new(SheetSize::AnsiA, Orientation::Landscape));
        let cols: Vec<&str> = f.columns.iter().map(|z| z.label.as_str()).collect();
        let rows: Vec<&str> = f.rows.iter().map(|z| z.label.as_str()).collect();
        assert_eq!(cols, ["2", "1"]);
        assert_eq!(rows, ["A", "B"]);
        assert!((f.inner.width() - 10.0 * 25.4).abs() < 1e-9);
    }

    #[test]
    fn iso_zones_count_from_the_top_left() {
        let f = frame(SheetFormat::new(SheetSize::IsoA3, Orientation::Landscape));
        assert_eq!(f.columns.len(), 8);
        assert_eq!(f.rows.len(), 6);
        assert_eq!(f.columns[0].label, "1");
        assert_eq!(f.rows.last().unwrap().label, "A");
        assert_eq!(f.rows[0].label, "F");
        // Filing margin on the left.
        assert_eq!(f.inner.min[0], 20.0);
    }

    #[test]
    fn scales_parse() {
        assert_eq!(Scale::parse("1:2"), Some(Scale::new(1, 2)));
        assert_eq!(Scale::parse(" 2 : 1 "), Some(Scale::new(2, 1)));
        assert_eq!(Scale::parse("0.25"), Some(Scale::new(1, 4)));
        assert_eq!(Scale::parse("0:1"), None);
        assert_eq!(Scale::new(1, 2).label(), "1:2");
    }

    #[test]
    fn size_labels_follow_the_orientation() {
        assert_eq!(SheetSize::AnsiA.oriented_label(Orientation::Landscape), "ANSI A (11 × 8.5 in)");
        assert_eq!(SheetSize::AnsiA.oriented_label(Orientation::Portrait), "ANSI A (8.5 × 11 in)");
        assert_eq!(SheetSize::IsoA3.oriented_label(Orientation::Landscape), "ISO A3 (420 × 297 mm)");
    }
}
