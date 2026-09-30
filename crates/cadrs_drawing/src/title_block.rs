//! The parametric title block (D1.9, X3).
//!
//! cadrs's own layout, drawn from the fields ASME Y14.1 and ISO 7200 call for: a tolerance
//! note, material, "DO NOT SCALE DRAWING" and the projection symbol on the left; drawn,
//! checked and approved names and dates in the middle; title, size, drawing number, revision,
//! scale and "sheet n of m" on the right. Fields are filled from the sheet (scale, size,
//! projection, sheet number), the drawing's own properties (drawn/checked/approved) and the
//! referenced part or assembly (name, description, part number, revision, material). An empty
//! field shows [`DASHES`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graphics::{Align, GText, Graphics, Weight};
use crate::standard::{Projection, Rect, Scale, SheetSize};
use crate::template::DrawingUnits;

/// What an empty field shows.
pub const DASHES: &str = "--";

/// A title-block field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TitleField {
    Title,
    Description,
    Material,
    DrawnBy,
    DrawnDate,
    CheckedBy,
    CheckedDate,
    ApprovedBy,
    ApprovedDate,
    Company,
    Size,
    Number,
    Revision,
    Scale,
    Sheet,
    Projection,
}

/// The referenced part's or assembly's properties (the property model of P3B.6 fills the ones
/// cadrs does not have yet).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReferenceProps {
    pub name: Option<String>,
    pub description: Option<String>,
    pub part_number: Option<String>,
    pub revision: Option<String>,
    pub material: Option<String>,
    /// P3C.5: read from the property model too.
    pub vendor: Option<String>,
}

/// The drawing's own title-block properties (who drew, checked and approved it, and when).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TitleProps {
    #[serde(default)]
    pub drawn_by: Option<String>,
    #[serde(default)]
    pub drawn_date: Option<String>,
    #[serde(default)]
    pub checked_by: Option<String>,
    #[serde(default)]
    pub checked_date: Option<String>,
    #[serde(default)]
    pub approved_by: Option<String>,
    #[serde(default)]
    pub approved_date: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
}

/// Everything a title block reads.
#[derive(Debug, Clone)]
pub struct TitleContext<'a> {
    pub reference: &'a ReferenceProps,
    pub props: &'a TitleProps,
    pub size: SheetSize,
    pub scale: Scale,
    pub projection: Projection,
    /// 0-based.
    pub sheet_index: usize,
    pub sheet_count: usize,
}

/// The value each field shows.
pub fn resolve(ctx: &TitleContext) -> BTreeMap<TitleField, String> {
    let val = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| DASHES.to_string())
    };
    let r = ctx.reference;
    let p = ctx.props;
    use TitleField as F;
    BTreeMap::from([
        (F::Title, val(&r.name)),
        (F::Description, val(&r.description)),
        (F::Material, val(&r.material)),
        (F::Number, val(&r.part_number)),
        (F::Revision, val(&r.revision)),
        (F::DrawnBy, val(&p.drawn_by)),
        (F::DrawnDate, val(&p.drawn_date)),
        (F::CheckedBy, val(&p.checked_by)),
        (F::CheckedDate, val(&p.checked_date)),
        (F::ApprovedBy, val(&p.approved_by)),
        (F::ApprovedDate, val(&p.approved_date)),
        (F::Company, val(&p.company)),
        (F::Size, ctx.size.letter().to_string()),
        (F::Scale, ctx.scale.label()),
        (
            F::Sheet,
            format!("{} OF {}", ctx.sheet_index + 1, ctx.sheet_count.max(1)),
        ),
        (F::Projection, ctx.projection.label().to_uppercase()),
    ])
}

/// The title block's size in mm: 6.25 × 1.75 in on ANSI A and B (about 60 % of an A frame's
/// width and about as tall as the course's, 1.6 in, so the Ex1 flange's front and top views fit
/// above each other at 1:2 as in `ex1-drawing.png`), 7.5 × 2.5 in on C–E; 150 × 45 mm on ISO A4
/// and A3 and 180 × 55 mm (ISO 7200's full width) on A2–A0.
pub fn size(sheet: SheetSize) -> (f64, f64) {
    const IN: f64 = 25.4;
    match sheet {
        SheetSize::AnsiA | SheetSize::AnsiB => (6.25 * IN, 1.75 * IN),
        SheetSize::AnsiC | SheetSize::AnsiD | SheetSize::AnsiE => (7.5 * IN, 2.5 * IN),
        SheetSize::IsoA4 | SheetSize::IsoA3 => (150.0, 45.0),
        SheetSize::IsoA2 | SheetSize::IsoA1 | SheetSize::IsoA0 => (180.0, 55.0),
    }
}

/// Where the title block goes: the bottom-right corner of the frame.
pub fn placement(inner: Rect, sheet: SheetSize) -> Rect {
    let (w, h) = size(sheet);
    let w = w.min(inner.width());
    Rect::new(inner.max[0] - w, inner.min[1], inner.max[0], inner.min[1] + h)
}

/// Draws the title block into `rect`.
pub fn draw(g: &mut Graphics, rect: Rect, ctx: &TitleContext, units: DrawingUnits) {
    let v = resolve(ctx);
    let (x0, y0) = (rect.min[0], rect.min[1]);
    let w = rect.width();
    let h = rect.height();
    let r = h / 5.0;
    let x = |f: f64| x0 + w * f;
    let y = |k: f64| y0 + r * k;
    // Column splits.
    let (c1, c2) = (0.33, 0.60);
    let cn = 0.475; // name | date
    // Outline and main columns.
    g.rect(rect, Weight::Medium);
    g.line([x(c1), y(0.0)], [x(c1), y(5.0)], Weight::Medium);
    g.line([x(c2), y(0.0)], [x(c2), y(5.0)], Weight::Medium);
    // Left column rows: note (3..5), material (2..3), do not scale (1..2), projection (0..1).
    for k in [1.0, 2.0, 3.0] {
        g.line([x(0.0), y(k)], [x(c1), y(k)], Weight::Thin);
    }
    // Middle column: header (4..5), drawn (3..4), checked (2..3), approved (1..2),
    // company (0..1).
    for k in [1.0, 2.0, 3.0, 4.0] {
        g.line([x(c1), y(k)], [x(c2), y(k)], Weight::Thin);
    }
    g.line([x(cn), y(1.0)], [x(cn), y(5.0)], Weight::Thin);
    // Right column: title (2..5), size | number | rev (1..2), scale | sheet (0..1).
    g.line([x(c2), y(2.0)], [x(1.0), y(2.0)], Weight::Medium);
    g.line([x(c2), y(1.0)], [x(1.0), y(1.0)], Weight::Thin);
    let (cs, cr) = (0.67, 0.90);
    g.line([x(cs), y(1.0)], [x(cs), y(2.0)], Weight::Thin);
    g.line([x(cr), y(1.0)], [x(cr), y(2.0)], Weight::Thin);
    let csh = 0.76;
    g.line([x(csh), y(0.0)], [x(csh), y(1.0)], Weight::Thin);

    let cap = 1.4; // caption height
    let pad = 0.8;
    let caption = |g: &mut Graphics, fx: f64, k: f64, s: &str| {
        g.text(GText::new([x(fx) + pad, y(k) - pad], cap, s).align(Align::TopLeft));
    };
    let value = |g: &mut Graphics, fx0: f64, fx1: f64, k0: f64, k1: f64, h: f64, s: &str| {
        let cx = (x(fx0) + x(fx1)) / 2.0;
        let cy = (y(k0) + y(k1)) / 2.0 - cap * 0.3;
        g.text(GText::new([cx, cy], h, s).align(Align::Center));
    };
    let val = |f: TitleField| v[&f].clone();

    // Left column.
    let unit_line = match units {
        DrawingUnits::Inch => "DIMENSIONS ARE IN INCHES",
        DrawingUnits::Millimeter => "DIMENSIONS ARE IN MILLIMETERS",
    };
    let tol: [&str; 3] = match units {
        DrawingUnits::Inch => ["TOLERANCES:", "X.XX ±.01  X.XXX ±.005", "ANGLES ±0.5°"],
        DrawingUnits::Millimeter => ["GENERAL TOLERANCES:", "ISO 2768-m", "ANGLES ±0.5°"],
    };
    let lines = ["UNLESS OTHERWISE SPECIFIED:", unit_line, tol[0], tol[1], tol[2]];
    let lh = (2.0 * r - 2.0 * pad) / lines.len() as f64;
    for (i, s) in lines.iter().enumerate() {
        g.text(
            GText::new([x(0.0) + pad, y(5.0) - pad - lh * i as f64], cap.min(lh * 0.8), *s)
                .align(Align::TopLeft),
        );
    }
    caption(g, 0.0, 3.0, "MATERIAL");
    value(g, 0.0, c1, 2.0, 3.0, 2.2, &val(TitleField::Material));
    value(g, 0.0, c1, 1.0, 2.0, 2.2, "DO NOT SCALE DRAWING");
    caption(g, 0.0, 1.0, "PROJECTION");
    projection_symbol(
        g,
        [x(c1 * 0.5), y(0.42)],
        r * 0.55,
        ctx.projection,
    );
    g.text(
        GText::new([x(c1) - pad, y(0.0) + pad], cap, val(TitleField::Projection))
            .align(Align::BottomRight),
    );

    // Middle column.
    let mid = |f0: f64, f1: f64| (f0 + f1) / 2.0;
    g.text(GText::new([x(mid(c1, cn)), y(4.5)], cap, "NAME").align(Align::Center));
    g.text(GText::new([x(mid(cn, c2)), y(4.5)], cap, "DATE").align(Align::Center));
    for (k, label, name, date) in [
        (3.0, "DRAWN", TitleField::DrawnBy, TitleField::DrawnDate),
        (2.0, "CHECKED", TitleField::CheckedBy, TitleField::CheckedDate),
        (1.0, "APPROVED", TitleField::ApprovedBy, TitleField::ApprovedDate),
    ] {
        caption(g, c1, k + 1.0, label);
        value(g, c1, cn, k, k + 1.0, 1.9, &val(name));
        value(g, cn, c2, k, k + 1.0, 1.9, &val(date));
    }
    caption(g, c1, 1.0, "COMPANY");
    value(g, c1, c2, 0.0, 1.0, 2.2, &val(TitleField::Company));

    // Right column.
    caption(g, c2, 5.0, "TITLE");
    // Long titles and descriptions shrink to fit the cell.
    let fit = |s: &str, h: f64, bold: bool| {
        let mut style = cadrs_sketch::text::TextStyle::new(s);
        style.bold = bold;
        let w = cadrs_sketch::text::layout(&style).width;
        let room = x(1.0) - x(c2) - 4.0 * pad;
        if w * h > room && w > 0.0 { room / w } else { h }
    };
    let title = val(TitleField::Title);
    let description = val(TitleField::Description);
    g.text(
        GText::new([x(mid(c2, 1.0)), y(3.75)], fit(&title, 3.6, true), title)
            .align(Align::Center)
            .bold(),
    );
    g.text(
        GText::new([x(mid(c2, 1.0)), y(2.7)], fit(&description, 2.5, false), description)
            .align(Align::Center),
    );
    caption(g, c2, 2.0, "SIZE");
    value(g, c2, cs, 1.0, 2.0, 4.0, &val(TitleField::Size));
    caption(g, cs, 2.0, "DWG. NO.");
    value(g, cs, cr, 1.0, 2.0, 2.5, &val(TitleField::Number));
    caption(g, cr, 2.0, "REV");
    value(g, cr, 1.0, 1.0, 2.0, 2.5, &val(TitleField::Revision));
    caption(g, c2, 1.0, "SCALE");
    value(g, c2, csh, 0.0, 1.0, 2.5, &val(TitleField::Scale));
    caption(g, csh, 1.0, "SHEET");
    value(g, csh, 1.0, 0.0, 1.0, 2.5, &val(TitleField::Sheet));
}

/// The projection symbol (ISO 5456-2 / ASME Y14.3): a truncated cone's side view (a
/// trapezoid, large end left) and its end view (two concentric circles). Third angle puts the
/// end view right of the side view, first angle left of it. `h` is the large diameter.
pub fn projection_symbol(g: &mut Graphics, center: [f64; 2], h: f64, projection: Projection) {
    let big = h / 2.0;
    let small = h / 4.0;
    let len = h * 1.1;
    let gap = h * 0.35;
    let total = len + gap + h;
    let left = center[0] - total / 2.0;
    let (trap_x, circ_cx) = match projection {
        Projection::Third => (left, left + len + gap + big),
        Projection::First => (left + h + gap, left + big),
    };
    let cy = center[1];
    // Trapezoid: large end on the left.
    let a = [trap_x, cy - big];
    let b = [trap_x + len, cy - small];
    let c = [trap_x + len, cy + small];
    let d = [trap_x, cy + big];
    g.line(a, b, Weight::Thin);
    g.line(b, c, Weight::Thin);
    g.line(c, d, Weight::Thin);
    g.line(d, a, Weight::Thin);
    g.circle([circ_cx, cy], big, Weight::Thin);
    g.circle([circ_cx, cy], small, Weight::Thin);
    // Centre lines through both views.
    let ext = h * 0.12;
    g.line(
        [left - ext, cy],
        [left + total + ext, cy],
        Weight::Center,
    );
    g.line(
        [circ_cx, cy - big - ext],
        [circ_cx, cy + big + ext],
        Weight::Center,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(r: &'a ReferenceProps, p: &'a TitleProps) -> TitleContext<'a> {
        TitleContext {
            reference: r,
            props: p,
            size: SheetSize::AnsiA,
            scale: Scale::new(1, 2),
            projection: Projection::Third,
            sheet_index: 1,
            sheet_count: 3,
        }
    }

    #[test]
    fn fields_resolve_from_properties() {
        let r = ReferenceProps {
            name: Some("Universal Joint Flange".into()),
            description: Some("Made by cadrs".into()),
            part_number: Some("PRT-316".into()),
            revision: Some("B".into()),
            material: Some("Steel".into()),
            vendor: None,
        };
        let p = TitleProps {
            drawn_by: Some("K. Owner".into()),
            drawn_date: Some("2026-09-28".into()),
            approved_by: Some("  ".into()),
            ..Default::default()
        };
        let v = resolve(&ctx(&r, &p));
        assert_eq!(v[&TitleField::Title], "Universal Joint Flange");
        assert_eq!(v[&TitleField::Description], "Made by cadrs");
        assert_eq!(v[&TitleField::Number], "PRT-316");
        assert_eq!(v[&TitleField::Revision], "B");
        assert_eq!(v[&TitleField::Material], "Steel");
        assert_eq!(v[&TitleField::DrawnBy], "K. Owner");
        assert_eq!(v[&TitleField::DrawnDate], "2026-09-28");
        assert_eq!(v[&TitleField::Scale], "1:2");
        assert_eq!(v[&TitleField::Size], "A");
        assert_eq!(v[&TitleField::Sheet], "2 OF 3");
        assert_eq!(v[&TitleField::Projection], "THIRD ANGLE");
        // Blank and missing properties fall back to dashes.
        assert_eq!(v[&TitleField::ApprovedBy], DASHES);
        assert_eq!(v[&TitleField::CheckedBy], DASHES);
        assert_eq!(v[&TitleField::Company], DASHES);
    }

    #[test]
    fn empty_properties_show_dashes() {
        let r = ReferenceProps::default();
        let p = TitleProps::default();
        let v = resolve(&ctx(&r, &p));
        for f in [
            TitleField::Title,
            TitleField::Description,
            TitleField::Number,
            TitleField::Revision,
            TitleField::Material,
            TitleField::DrawnBy,
            TitleField::ApprovedDate,
        ] {
            assert_eq!(v[&f], DASHES, "{f:?}");
        }
        // Sheet-derived fields are always filled.
        assert_eq!(v[&TitleField::Scale], "1:2");
        assert_eq!(v[&TitleField::Sheet], "2 OF 3");
    }

    #[test]
    fn title_block_fits_its_frame() {
        use crate::standard::{Orientation, SheetFormat, frame};
        // ANSI A: 6.25 in of the 10 in frame (the course's sheets: about 57–62 %).
        let (w, h) = size(SheetSize::AnsiA);
        assert_eq!((w / 25.4, h / 25.4), (6.25, 1.75));
        for s in SheetSize::ALL {
            for o in Orientation::ALL {
                let inner = frame(SheetFormat::new(s, o)).inner;
                let r = placement(inner, s);
                assert!(r.width() <= inner.width() && r.height() < inner.height() / 2.0, "{s:?} {o:?}");
            }
        }
    }

    #[test]
    fn projection_symbol_sides() {
        let side = |p| {
            let mut g = Graphics::default();
            projection_symbol(&mut g, [0.0, 0.0], 10.0, p);
            g.circles[0].center[0]
        };
        assert!(side(Projection::Third) > 0.0, "third angle: circles on the right");
        assert!(side(Projection::First) < 0.0, "first angle: circles on the left");
    }
}
