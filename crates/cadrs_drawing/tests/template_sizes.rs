//! Every built-in template's sheet is the size the standards give (P3C.1 unit test).
//!
//! The expected values are an independent table: ASME Y14.1 (ANSI) sizes in inches and
//! ISO 216 / ISO 5457 A-series sizes in millimetres, short side × long side.

use cadrs_drawing::{Orientation, builtin_templates};

/// (name part, short side, long side, unit mm per unit).
const EXPECTED: &[(&str, f64, f64, f64)] = &[
    ("ANSI_A_", 8.5, 11.0, 25.4),
    ("ANSI_B_", 11.0, 17.0, 25.4),
    ("ANSI_C_", 17.0, 22.0, 25.4),
    ("ANSI_D_", 22.0, 34.0, 25.4),
    ("ANSI_E_", 34.0, 44.0, 25.4),
    ("ISO_A4_", 210.0, 297.0, 1.0),
    ("ISO_A3_", 297.0, 420.0, 1.0),
    ("ISO_A2_", 420.0, 594.0, 1.0),
    ("ISO_A1_", 594.0, 841.0, 1.0),
    ("ISO_A0_", 841.0, 1189.0, 1.0),
];

#[test]
fn every_template_has_its_standard_sheet_size() {
    let templates = builtin_templates();
    // ANSI A–E and ISO A4–A0, landscape and portrait, INCH and MM.
    assert_eq!(templates.len(), EXPECTED.len() * 4);
    for t in &templates {
        let (prefix, short, long, unit) = EXPECTED
            .iter()
            .copied()
            .find(|(p, ..)| t.name.starts_with(p))
            .unwrap_or_else(|| panic!("{} is not a standard size", t.name));
        let (short, long) = (short * unit, long * unit);
        let (w, h) = t.format.size_mm();
        let portrait = t.name.contains("_Portrait_");
        assert_eq!(t.format.orientation == Orientation::Portrait, portrait, "{}", t.name);
        let (ew, eh) = if portrait { (short, long) } else { (long, short) };
        assert!(
            (w - ew).abs() < 1e-9 && (h - eh).abs() < 1e-9,
            "{}: {w} × {h} mm, expected {ew} × {eh} ({prefix})",
            t.name
        );
        assert!(t.name.ends_with("_INCH.dwt") || t.name.ends_with("_MM.dwt"), "{}", t.name);
    }
    // The course's template: ANSI_A_INCH is 11 × 8.5 in, landscape.
    let a = templates.iter().find(|t| t.name == "ANSI_A_INCH.dwt").unwrap();
    let (w, h) = a.format.size_mm();
    assert_eq!((w / 25.4, h / 25.4), (11.0, 8.5));
}
