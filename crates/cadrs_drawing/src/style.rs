//! Drawing properties (D2.5, X5): the defaults every sheet of a drawing uses for units and
//! precision, dimensions, annotations, views, construction geometry, formats and tables.
//!
//! The Drawing properties panel is generic: [`FIELDS`] lists every setting with its section,
//! group and label, and [`DrawingStyle::get`], [`DrawingStyle::set`] and
//! [`DrawingStyle::options`] read and write them by key. Later milestones read the typed fields
//! (for example P3C.3 formats dimension values with [`DrawingStyle::format_length`]).

use cadrs_sketch::units::LengthUnit;
use serde::{Deserialize, Serialize};

use crate::standard::Standard;
use crate::template::DrawingUnits;

/// The decimal separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DecimalSeparator {
    #[default]
    Period,
    Comma,
}

/// Where a dual dimension's second value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DualLocation {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

/// Arrowhead shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ArrowStyle {
    #[default]
    Filled,
    Open,
    Closed,
    Dot,
    Slash,
}

/// How tangent edges are drawn in views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TangentEdges {
    Hidden,
    #[default]
    Solid,
    Phantom,
}

/// How a virtual sharp is drawn (D5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VirtualSharp {
    #[default]
    Mark,
    Extension,
}

/// Date format in title blocks and notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DateFormat {
    #[default]
    Iso,
    Us,
    European,
}

/// The drawing properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingStyle {
    // Units and precision: Primary
    pub units: LengthUnit,
    pub decimal_separator: DecimalSeparator,
    /// Decimals of lengths (3 = `0.123`).
    pub precision: u8,
    pub tolerance_precision: u8,
    pub angular_precision: u8,
    // Dual
    pub show_dual: bool,
    pub show_dual_unit: bool,
    pub dual_location: DualLocation,
    pub dual_units: LengthUnit,
    pub dual_precision: u8,
    pub dual_tolerance_precision: u8,
    // Leading and trailing zeros
    pub length_leading_zeros: bool,
    pub length_trailing_zeros: bool,
    pub angle_leading_zeros: bool,
    pub angle_trailing_zeros: bool,
    pub tolerance_leading_zeros: bool,
    pub tolerance_trailing_zeros: bool,
    // Dimensions (lengths in sheet mm)
    pub dim_text_height: f64,
    pub dim_arrow: ArrowStyle,
    pub dim_arrow_length: f64,
    pub extension_gap: f64,
    pub extension_beyond: f64,
    // Annotations
    pub note_text_height: f64,
    pub leader_arrow: ArrowStyle,
    // Views
    pub hidden_lines: bool,
    pub tangent_edges: TangentEdges,
    pub show_threads: bool,
    // Construction geometry
    pub centermark_size: f64,
    pub centerline_extension: f64,
    pub virtual_sharp: VirtualSharp,
    // Formats
    pub date_format: DateFormat,
    // Tables
    pub table_text_height: f64,
    pub table_row_height: f64,
}

impl Default for DrawingStyle {
    fn default() -> Self {
        Self::for_units(DrawingUnits::Millimeter, Standard::Iso)
    }
}

impl DrawingStyle {
    /// The defaults of a template: inch drawings show 3 decimals without leading zeros
    /// (`.250`, ASME Y14.5) with 2-decimal millimetre duals; millimetre drawings show 2
    /// decimals with leading zeros and 3-decimal inch duals.
    pub fn for_units(units: DrawingUnits, standard: Standard) -> Self {
        let inch = units == DrawingUnits::Inch;
        Self {
            units: if inch {
                LengthUnit::Inch
            } else {
                LengthUnit::Millimeter
            },
            decimal_separator: DecimalSeparator::Period,
            precision: if inch { 3 } else { 2 },
            tolerance_precision: if inch { 3 } else { 2 },
            angular_precision: 1,
            show_dual: false,
            show_dual_unit: false,
            dual_location: DualLocation::Top,
            dual_units: if inch {
                LengthUnit::Millimeter
            } else {
                LengthUnit::Inch
            },
            dual_precision: if inch { 2 } else { 3 },
            dual_tolerance_precision: if inch { 2 } else { 3 },
            length_leading_zeros: !inch,
            length_trailing_zeros: true,
            angle_leading_zeros: true,
            angle_trailing_zeros: true,
            tolerance_leading_zeros: !inch,
            tolerance_trailing_zeros: true,
            dim_text_height: if inch { 0.12 * 25.4 } else { 3.5 },
            dim_arrow: ArrowStyle::Filled,
            dim_arrow_length: if inch { 0.125 * 25.4 } else { 3.0 },
            extension_gap: if inch { 0.0625 * 25.4 } else { 1.0 },
            extension_beyond: if inch { 0.125 * 25.4 } else { 2.0 },
            note_text_height: if inch { 0.12 * 25.4 } else { 3.5 },
            leader_arrow: ArrowStyle::Filled,
            hidden_lines: false,
            tangent_edges: TangentEdges::Solid,
            show_threads: true,
            centermark_size: if inch { 0.125 * 25.4 } else { 3.0 },
            centerline_extension: if inch { 0.125 * 25.4 } else { 3.0 },
            virtual_sharp: VirtualSharp::Mark,
            date_format: match standard {
                Standard::Ansi => DateFormat::Us,
                Standard::Iso => DateFormat::Iso,
            },
            table_text_height: if inch { 0.12 * 25.4 } else { 3.5 },
            table_row_height: if inch { 0.25 * 25.4 } else { 7.0 },
        }
    }

    /// A length in model mm as a dimension shows it, e.g. `.266` (inch, no leading zero) or
    /// `8.25` (mm), following units, precision, separator and zeros.
    pub fn format_length(&self, mm: f64) -> String {
        let v = mm / self.units.mm();
        format_number(
            v,
            self.precision,
            self.length_leading_zeros,
            self.length_trailing_zeros,
            self.decimal_separator,
        )
    }

    /// An angle in degrees as a dimension shows it (`43.0°`).
    pub fn format_angle(&self, deg: f64) -> String {
        format!(
            "{}°",
            format_number(
                deg,
                self.angular_precision,
                self.angle_leading_zeros,
                self.angle_trailing_zeros,
                self.decimal_separator
            )
        )
    }

    /// A value's current setting.
    pub fn get(&self, key: &str) -> Option<FieldValue> {
        use FieldValue::{Bool, Choice};
        let unit = |u: LengthUnit| UNIT_CHOICES.iter().position(|x| *x == u).unwrap_or(0);
        let len = |key: &str, v: f64| {
            let opts = length_presets(key);
            opts.iter()
                .enumerate()
                .min_by(|a, b| (a.1 - v).abs().total_cmp(&(b.1 - v).abs()))
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        Some(match key {
            "units" => Choice(unit(self.units)),
            "decimal_separator" => Choice(self.decimal_separator as usize),
            "precision" => Choice(self.precision as usize),
            "tolerance_precision" => Choice(self.tolerance_precision as usize),
            "angular_precision" => Choice(self.angular_precision as usize),
            "show_dual" => Bool(self.show_dual),
            "show_dual_unit" => Bool(self.show_dual_unit),
            "dual_location" => Choice(self.dual_location as usize),
            "dual_units" => Choice(unit(self.dual_units)),
            "dual_precision" => Choice(self.dual_precision as usize),
            "dual_tolerance_precision" => Choice(self.dual_tolerance_precision as usize),
            "length_leading_zeros" => Bool(self.length_leading_zeros),
            "length_trailing_zeros" => Bool(self.length_trailing_zeros),
            "angle_leading_zeros" => Bool(self.angle_leading_zeros),
            "angle_trailing_zeros" => Bool(self.angle_trailing_zeros),
            "tolerance_leading_zeros" => Bool(self.tolerance_leading_zeros),
            "tolerance_trailing_zeros" => Bool(self.tolerance_trailing_zeros),
            "dim_text_height" => Choice(len(key, self.dim_text_height)),
            "dim_arrow" => Choice(self.dim_arrow as usize),
            "dim_arrow_length" => Choice(len(key, self.dim_arrow_length)),
            "extension_gap" => Choice(len(key, self.extension_gap)),
            "extension_beyond" => Choice(len(key, self.extension_beyond)),
            "note_text_height" => Choice(len(key, self.note_text_height)),
            "leader_arrow" => Choice(self.leader_arrow as usize),
            "hidden_lines" => Bool(self.hidden_lines),
            "tangent_edges" => Choice(self.tangent_edges as usize),
            "show_threads" => Bool(self.show_threads),
            "centermark_size" => Choice(len(key, self.centermark_size)),
            "centerline_extension" => Choice(len(key, self.centerline_extension)),
            "virtual_sharp" => Choice(self.virtual_sharp as usize),
            "date_format" => Choice(self.date_format as usize),
            "table_text_height" => Choice(len(key, self.table_text_height)),
            "table_row_height" => Choice(len(key, self.table_row_height)),
            _ => return None,
        })
    }

    /// Changes a setting. Fails on an unknown key, a value of the wrong kind or an index out
    /// of range.
    pub fn set(&mut self, key: &str, value: FieldValue) -> Result<(), String> {
        let n = self.options(key).len();
        let bad = || format!("bad value {value:?} for {key}");
        match value {
            FieldValue::Bool(b) => {
                let slot = match key {
                    "show_dual" => &mut self.show_dual,
                    "show_dual_unit" => &mut self.show_dual_unit,
                    "length_leading_zeros" => &mut self.length_leading_zeros,
                    "length_trailing_zeros" => &mut self.length_trailing_zeros,
                    "angle_leading_zeros" => &mut self.angle_leading_zeros,
                    "angle_trailing_zeros" => &mut self.angle_trailing_zeros,
                    "tolerance_leading_zeros" => &mut self.tolerance_leading_zeros,
                    "tolerance_trailing_zeros" => &mut self.tolerance_trailing_zeros,
                    "hidden_lines" => &mut self.hidden_lines,
                    "show_threads" => &mut self.show_threads,
                    _ => return Err(bad()),
                };
                *slot = b;
            }
            FieldValue::Choice(i) => {
                if i >= n {
                    return Err(bad());
                }
                let preset = |key: &str| length_presets(key)[i];
                match key {
                    "units" => self.units = UNIT_CHOICES[i],
                    "decimal_separator" => {
                        self.decimal_separator = [DecimalSeparator::Period, DecimalSeparator::Comma][i]
                    }
                    "precision" => self.precision = i as u8,
                    "tolerance_precision" => self.tolerance_precision = i as u8,
                    "angular_precision" => self.angular_precision = i as u8,
                    "dual_location" => {
                        self.dual_location = [
                            DualLocation::Top,
                            DualLocation::Bottom,
                            DualLocation::Left,
                            DualLocation::Right,
                        ][i]
                    }
                    "dual_units" => self.dual_units = UNIT_CHOICES[i],
                    "dual_precision" => self.dual_precision = i as u8,
                    "dual_tolerance_precision" => self.dual_tolerance_precision = i as u8,
                    "dim_text_height" => self.dim_text_height = preset(key),
                    "dim_arrow" => self.dim_arrow = ARROWS[i],
                    "dim_arrow_length" => self.dim_arrow_length = preset(key),
                    "extension_gap" => self.extension_gap = preset(key),
                    "extension_beyond" => self.extension_beyond = preset(key),
                    "note_text_height" => self.note_text_height = preset(key),
                    "leader_arrow" => self.leader_arrow = ARROWS[i],
                    "tangent_edges" => {
                        self.tangent_edges =
                            [TangentEdges::Hidden, TangentEdges::Solid, TangentEdges::Phantom][i]
                    }
                    "centermark_size" => self.centermark_size = preset(key),
                    "centerline_extension" => self.centerline_extension = preset(key),
                    "virtual_sharp" => {
                        self.virtual_sharp = [VirtualSharp::Mark, VirtualSharp::Extension][i]
                    }
                    "date_format" => {
                        self.date_format = [DateFormat::Iso, DateFormat::Us, DateFormat::European][i]
                    }
                    "table_text_height" => self.table_text_height = preset(key),
                    "table_row_height" => self.table_row_height = preset(key),
                    _ => return Err(bad()),
                }
            }
        }
        Ok(())
    }

    /// The choices of a choice setting, as the panel lists them (lengths in the drawing's
    /// units). Empty for a checkbox or an unknown key.
    pub fn options(&self, key: &str) -> Vec<String> {
        let decimals = || (0..=6u8).map(decimals_label).collect::<Vec<_>>();
        let angles = || (0..=4u8).map(decimals_label).collect::<Vec<_>>();
        let units = || UNIT_CHOICES.iter().map(|u| unit_plural(*u).to_string()).collect();
        let arrows = || {
            ["Filled", "Open", "Closed", "Dot", "Slash"]
                .map(String::from)
                .to_vec()
        };
        match key {
            "units" | "dual_units" => units(),
            "decimal_separator" => vec!["Period".into(), "Comma".into()],
            "precision" | "tolerance_precision" | "dual_precision" | "dual_tolerance_precision" => {
                decimals()
            }
            "angular_precision" => angles(),
            "dual_location" => ["Top", "Bottom", "Left", "Right"].map(String::from).to_vec(),
            "dim_arrow" | "leader_arrow" => arrows(),
            "tangent_edges" => ["Hidden", "Solid", "Phantom"].map(String::from).to_vec(),
            "virtual_sharp" => ["Mark", "Extension lines"].map(String::from).to_vec(),
            "date_format" => ["YYYY-MM-DD", "MM/DD/YYYY", "DD.MM.YYYY"]
                .map(String::from)
                .to_vec(),
            k if !length_presets(k).is_empty() => length_presets(k)
                .iter()
                .map(|mm| self.length_label(*mm))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A sheet length (text height, arrow size) in the drawing's units: `0.12 in`, `3.5 mm`.
    fn length_label(&self, mm: f64) -> String {
        if self.units == LengthUnit::Inch {
            let v = mm / 25.4;
            let s = format!("{v:.4}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            format!("{s} in")
        } else {
            let s = format!("{mm:.2}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            format!("{s} mm")
        }
    }
}

const UNIT_CHOICES: [LengthUnit; 5] = [
    LengthUnit::Inch,
    LengthUnit::Millimeter,
    LengthUnit::Centimeter,
    LengthUnit::Meter,
    LengthUnit::Foot,
];

const ARROWS: [ArrowStyle; 5] = [
    ArrowStyle::Filled,
    ArrowStyle::Open,
    ArrowStyle::Closed,
    ArrowStyle::Dot,
    ArrowStyle::Slash,
];

fn unit_plural(u: LengthUnit) -> &'static str {
    match u {
        LengthUnit::Millimeter => "Millimeters",
        LengthUnit::Centimeter => "Centimeters",
        LengthUnit::Meter => "Meters",
        LengthUnit::Inch => "Inches",
        LengthUnit::Foot => "Feet",
        LengthUnit::Yard => "Yards",
    }
}

/// The preset sheet lengths (mm) of a length setting: the usual inch and millimetre values.
fn length_presets(key: &str) -> &'static [f64] {
    const TEXT: &[f64] = &[1.8, 2.5, 3.048, 3.5, 5.0, 7.0];
    const SMALL: &[f64] = &[0.5, 1.0, 1.5875, 2.0, 3.0, 3.175, 5.0];
    const ROW: &[f64] = &[5.0, 6.35, 7.0, 8.0, 10.0];
    match key {
        "dim_text_height" | "note_text_height" | "table_text_height" => TEXT,
        "dim_arrow_length" | "extension_gap" | "extension_beyond" | "centermark_size"
        | "centerline_extension" => SMALL,
        "table_row_height" => ROW,
        _ => &[],
    }
}

/// "0.123" for 3 decimals, "0" for none.
pub fn decimals_label(d: u8) -> String {
    if d == 0 {
        "0".into()
    } else {
        format!("0.{}", &"123456789"[..d as usize])
    }
}

/// A number with `decimals` decimals, dropping the leading zero before the separator
/// (`.250`) and/or trailing zeros (`0.25`) as asked.
pub fn format_number(
    v: f64,
    decimals: u8,
    leading_zeros: bool,
    trailing_zeros: bool,
    sep: DecimalSeparator,
) -> String {
    let mut s = format!("{:.*}", decimals as usize, v);
    if s.starts_with("-") && s.trim_start_matches(['-', '0', '.']).is_empty() {
        s.remove(0); // no "-0.00"
    }
    if !trailing_zeros && s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if !leading_zeros {
        if let Some(rest) = s.strip_prefix("0.") {
            s = format!(".{rest}");
        } else if let Some(rest) = s.strip_prefix("-0.") {
            s = format!("-.{rest}");
        }
    }
    if sep == DecimalSeparator::Comma {
        s = s.replace('.', ",");
    }
    s
}

/// A setting's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldValue {
    Bool(bool),
    Choice(usize),
}

/// The Drawing properties panel's icon tabs (D2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleSection {
    UnitsPrecision,
    Dimensions,
    Annotations,
    Views,
    Construction,
    Formats,
    Tables,
}

impl StyleSection {
    pub const ALL: [StyleSection; 7] = [
        StyleSection::UnitsPrecision,
        StyleSection::Dimensions,
        StyleSection::Annotations,
        StyleSection::Views,
        StyleSection::Construction,
        StyleSection::Formats,
        StyleSection::Tables,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StyleSection::UnitsPrecision => "Units and precision",
            StyleSection::Dimensions => "Dimensions",
            StyleSection::Annotations => "Annotations",
            StyleSection::Views => "Views",
            StyleSection::Construction => "Construction geometry",
            StyleSection::Formats => "Formats",
            StyleSection::Tables => "Tables",
        }
    }

    /// A short key for UI names: `units`, `dimensions`, ….
    pub fn key(self) -> &'static str {
        match self {
            StyleSection::UnitsPrecision => "units",
            StyleSection::Dimensions => "dimensions",
            StyleSection::Annotations => "annotations",
            StyleSection::Views => "views",
            StyleSection::Construction => "construction",
            StyleSection::Formats => "formats",
            StyleSection::Tables => "tables",
        }
    }
}

/// One setting of the Drawing properties panel.
#[derive(Debug, Clone, Copy)]
pub struct StyleField {
    pub key: &'static str,
    pub section: StyleSection,
    /// The group header it is under ("Primary", "Dual", …).
    pub group: &'static str,
    pub label: &'static str,
}

const fn f(
    key: &'static str,
    section: StyleSection,
    group: &'static str,
    label: &'static str,
) -> StyleField {
    StyleField {
        key,
        section,
        group,
        label,
    }
}

use StyleSection as S;

/// Every setting, in panel order.
pub const FIELDS: &[StyleField] = &[
    f("units", S::UnitsPrecision, "Primary", "Units"),
    f("decimal_separator", S::UnitsPrecision, "Primary", "Decimal separator"),
    f("precision", S::UnitsPrecision, "Primary", "Precision"),
    f("tolerance_precision", S::UnitsPrecision, "Primary", "Tolerance precision"),
    f("angular_precision", S::UnitsPrecision, "Primary", "Angular precision"),
    f("show_dual", S::UnitsPrecision, "Dual", "Show dual dimensions"),
    f("show_dual_unit", S::UnitsPrecision, "Dual", "Show dual unit"),
    f("dual_location", S::UnitsPrecision, "Dual", "Dimension location"),
    f("dual_units", S::UnitsPrecision, "Dual", "Units"),
    f("dual_precision", S::UnitsPrecision, "Dual", "Precision"),
    f("dual_tolerance_precision", S::UnitsPrecision, "Dual", "Tolerance precision"),
    f("length_leading_zeros", S::UnitsPrecision, "Leading and trailing zeros", "Length leading zeros"),
    f("length_trailing_zeros", S::UnitsPrecision, "Leading and trailing zeros", "Length trailing zeros"),
    f("angle_leading_zeros", S::UnitsPrecision, "Leading and trailing zeros", "Angle leading zeros"),
    f("angle_trailing_zeros", S::UnitsPrecision, "Leading and trailing zeros", "Angle trailing zeros"),
    f("tolerance_leading_zeros", S::UnitsPrecision, "Leading and trailing zeros", "Tolerance leading zeros"),
    f("tolerance_trailing_zeros", S::UnitsPrecision, "Leading and trailing zeros", "Tolerance trailing zeros"),
    f("dim_text_height", S::Dimensions, "Text", "Text height"),
    f("dim_arrow", S::Dimensions, "Arrows", "Arrow style"),
    f("dim_arrow_length", S::Dimensions, "Arrows", "Arrow length"),
    f("extension_gap", S::Dimensions, "Extension lines", "Gap from geometry"),
    f("extension_beyond", S::Dimensions, "Extension lines", "Extension beyond"),
    f("note_text_height", S::Annotations, "Notes", "Text height"),
    f("leader_arrow", S::Annotations, "Leaders", "Arrow style"),
    f("hidden_lines", S::Views, "Display", "Show hidden lines"),
    f("tangent_edges", S::Views, "Display", "Tangent edges"),
    f("show_threads", S::Views, "Display", "Show threads"),
    f("centermark_size", S::Construction, "Centermarks", "Mark size"),
    f("centerline_extension", S::Construction, "Centerlines", "Extension"),
    f("virtual_sharp", S::Construction, "Virtual sharps", "Display"),
    f("date_format", S::Formats, "Dates", "Date format"),
    f("table_text_height", S::Tables, "Text", "Text height"),
    f("table_row_height", S::Tables, "Rows", "Row height"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_round_trips() {
        let mut s = DrawingStyle::for_units(DrawingUnits::Inch, Standard::Ansi);
        for fld in FIELDS {
            let v = s.get(fld.key).unwrap_or_else(|| panic!("{}", fld.key));
            match v {
                FieldValue::Bool(b) => {
                    assert!(s.options(fld.key).is_empty());
                    s.set(fld.key, FieldValue::Bool(!b)).unwrap();
                    assert_eq!(s.get(fld.key), Some(FieldValue::Bool(!b)), "{}", fld.key);
                }
                FieldValue::Choice(_) => {
                    let n = s.options(fld.key).len();
                    assert!(n >= 2, "{}", fld.key);
                    s.set(fld.key, FieldValue::Choice(n - 1)).unwrap();
                    assert_eq!(s.get(fld.key), Some(FieldValue::Choice(n - 1)), "{}", fld.key);
                    assert!(s.set(fld.key, FieldValue::Choice(n)).is_err());
                }
            }
        }
    }

    #[test]
    fn inch_defaults_read_like_the_course() {
        // lesson-drawing-properties-sheets-flyout.png: Inches, Period, 0.123, 0.123, 0.1;
        // dual Millimeters 0.12; no length leading zeros, trailing zeros on.
        let s = DrawingStyle::for_units(DrawingUnits::Inch, Standard::Ansi);
        let opt = |k: &str| match s.get(k).unwrap() {
            FieldValue::Choice(i) => s.options(k)[i].clone(),
            FieldValue::Bool(b) => b.to_string(),
        };
        assert_eq!(opt("units"), "Inches");
        assert_eq!(opt("decimal_separator"), "Period");
        assert_eq!(opt("precision"), "0.123");
        assert_eq!(opt("angular_precision"), "0.1");
        assert_eq!(opt("dual_units"), "Millimeters");
        assert_eq!(opt("dual_precision"), "0.12");
        assert_eq!(opt("length_leading_zeros"), "false");
        assert_eq!(opt("length_trailing_zeros"), "true");
        assert_eq!(s.format_length(0.266 * 25.4), ".266");
        assert_eq!(s.format_angle(43.0), "43.0°");
    }

    #[test]
    fn numbers_follow_zero_rules() {
        let p = DecimalSeparator::Period;
        assert_eq!(format_number(0.25, 3, false, true, p), ".250");
        assert_eq!(format_number(0.25, 3, true, false, p), "0.25");
        assert_eq!(format_number(8.25, 2, true, true, DecimalSeparator::Comma), "8,25");
        assert_eq!(format_number(-0.0001, 2, true, true, p), "0.00");
        assert_eq!(format_number(12.0, 2, true, false, p), "12");
    }
}
