//! The sketch's **Insert DXF or DWG** (P3I.6; exercise E1, `reference/onshape/sheetmetal/`
//! `ex1-importing-dxf-bend/step-03.png`): a DXF's (or a converted DWG's) lines, arcs, circles,
//! polylines (with bulges) and splines become sketch curves, scaled from the **Units** the user
//! says the file is in, sharing their end points where they meet so they close regions. With
//! **Use file origin position** off, the geometry is centred on the sketch origin.
//!
//! [`InsertDxf`] adds them to a sketch as one undoable command ("Insert DXF/DWG"). Texts and
//! fills are left out (counted in the report).

use cadrs_drawing::dxf::DxfDrawing;
use cadrs_drawing::sheet_sketch::Entity;
use cadrs_sketch::{Curve, CurveKind, PointId, Sketch, SketchOp, Vec2};

use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::{ElementId, FeatureId};

/// The units a DXF's numbers are in (the dialog's Units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DxfUnits {
    Meter,
    Centimeter,
    #[default]
    Millimeter,
    Inch,
    Foot,
    Yard,
}

impl DxfUnits {
    pub const ALL: [DxfUnits; 6] = [DxfUnits::Meter, DxfUnits::Centimeter, DxfUnits::Millimeter, DxfUnits::Inch, DxfUnits::Foot, DxfUnits::Yard];

    pub fn label(self) -> &'static str {
        match self {
            DxfUnits::Meter => "Meter",
            DxfUnits::Centimeter => "Centimeter",
            DxfUnits::Millimeter => "Millimeter",
            DxfUnits::Inch => "Inch",
            DxfUnits::Foot => "Foot",
            DxfUnits::Yard => "Yard",
        }
    }

    /// Millimetres per unit.
    pub fn mm(self) -> f64 {
        match self {
            DxfUnits::Meter => 1000.0,
            DxfUnits::Centimeter => 10.0,
            DxfUnits::Millimeter => 1.0,
            DxfUnits::Inch => 25.4,
            DxfUnits::Foot => 304.8,
            DxfUnits::Yard => 914.4,
        }
    }

    /// The units a file says it is in (its `$INSUNITS`), millimetres when it says nothing we
    /// know.
    pub fn of_file(unit_mm: f64) -> DxfUnits {
        DxfUnits::ALL.into_iter().find(|u| (u.mm() - unit_mm).abs() < 1e-9 * unit_mm.max(1.0)).unwrap_or_default()
    }
}

/// What an import brought in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub lines: usize,
    pub arcs: usize,
    pub circles: usize,
    pub splines: usize,
    /// Texts, fills and degenerate curves left out.
    pub skipped: usize,
}

impl ImportReport {
    pub fn curves(&self) -> usize {
        self.lines + self.arcs + self.circles + self.splines
    }
}

/// How close two ends must be to be one sketch point (mm): loose enough for files whose
/// ends miss each other by rounding (CAM and older CAD exports write 4–6 decimals), far below
/// any real feature.
const JOIN: f64 = 5e-4;

struct Builder {
    s: Sketch,
    r: ImportReport,
}

impl Builder {
    fn pt(&mut self, p: Vec2) -> PointId {
        self.s.point_at(p, JOIN).unwrap_or_else(|| self.s.add_point(p))
    }

    fn line(&mut self, a: Vec2, b: Vec2) {
        if a.distance(b) <= JOIN {
            self.r.skipped += 1;
            return;
        }
        let (a, b) = (self.pt(a), self.pt(b));
        self.s.curves.insert(Curve { kind: CurveKind::Line { a, b }, construction: false });
        self.r.lines += 1;
    }

    /// Counter-clockwise about `c` from `a` to `b`.
    fn arc(&mut self, c: Vec2, a: Vec2, b: Vec2) {
        if a.distance(b) <= JOIN || c.distance(a) <= JOIN {
            self.r.skipped += 1;
            return;
        }
        let center = self.s.add_point(c);
        let (start, end) = (self.pt(a), self.pt(b));
        self.s.curves.insert(Curve { kind: CurveKind::Arc { center, start, end }, construction: false });
        self.r.arcs += 1;
    }

    fn circle(&mut self, c: Vec2, radius: f64) {
        if radius <= JOIN {
            self.r.skipped += 1;
            return;
        }
        let center = self.s.add_point(c);
        self.s.curves.insert(Curve { kind: CurveKind::Circle { center, radius }, construction: false });
        self.r.circles += 1;
    }

    /// A polyline segment from `a` to `b` bent by `bulge` (tan of a quarter of its signed sweep).
    fn bulged(&mut self, a: Vec2, b: Vec2, bulge: f64) {
        if bulge.abs() < 1e-12 {
            return self.line(a, b);
        }
        let sweep = 4.0 * bulge.atan();
        let chord = a.distance(b);
        let r = chord / (2.0 * (sweep / 2.0).sin()).abs();
        let mid = Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let d = Vec2::new((b.x - a.x) / chord, (b.y - a.y) / chord);
        // The centre is left of a→b for a counter-clockwise bulge less than a half turn.
        let h = (r * r - chord * chord / 4.0).max(0.0).sqrt();
        let side = if (bulge > 0.0) == (sweep.abs() < std::f64::consts::PI) { 1.0 } else { -1.0 };
        let c = Vec2::new(mid.x - d.y * h * side, mid.y + d.x * h * side);
        if bulge > 0.0 { self.arc(c, a, b) } else { self.arc(c, b, a) }
    }
}

/// A read DXF as sketch geometry: numbers in `units`, at the file's own origin or centred on
/// the sketch origin.
pub fn sketch_of(d: &DxfDrawing, units: DxfUnits, file_origin: bool) -> (Sketch, ImportReport) {
    // The reader keeps the file's numbers (its `unit_mm` only says what the file claims).
    let k = units.mm();
    let mut ents: Vec<Entity> = d.entities.clone();
    let mut b = Builder { s: Sketch::new(), r: ImportReport::default() };
    // Where the geometry goes: its bounds' centre onto the origin unless the file's origin is kept.
    let mut shift = [0.0, 0.0];
    if !file_origin {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let mut grow = |p: [f64; 2]| {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        };
        for e in &ents {
            match e {
                Entity::Line { a, b } => [*a, *b].into_iter().for_each(&mut grow),
                Entity::Arc { center, radius, .. } | Entity::Circle { center, radius } => {
                    grow([center[0] - radius, center[1] - radius]);
                    grow([center[0] + radius, center[1] + radius]);
                }
                Entity::Polyline { points, .. } => points.iter().copied().for_each(&mut grow),
                Entity::Spline { control, fit, .. } => control.iter().chain(fit).copied().for_each(&mut grow),
                _ => {}
            }
        }
        if lo[0] <= hi[0] {
            shift = [-(lo[0] + hi[0]) / 2.0, -(lo[1] + hi[1]) / 2.0];
        }
    }
    let v = |p: [f64; 2]| Vec2::new((p[0] + shift[0]) * k, (p[1] + shift[1]) * k);
    for e in ents.drain(..) {
        match e {
            Entity::Line { a, b: e2 } => b.line(v(a), v(e2)),
            Entity::Arc { center, radius, start, end } => {
                let at = |deg: f64| [center[0] + radius * deg.to_radians().cos(), center[1] + radius * deg.to_radians().sin()];
                b.arc(v(center), v(at(start)), v(at(end)));
            }
            Entity::Circle { center, radius } => b.circle(v(center), radius * k),
            Entity::Polyline { points, bulges, closed } => {
                let n = points.len();
                let segs = if closed { n } else { n.saturating_sub(1) };
                for i in 0..segs {
                    let (p, q) = (points[i], points[(i + 1) % n]);
                    b.bulged(v(p), v(q), bulges.get(i).copied().unwrap_or(0.0));
                }
            }
            e @ Entity::Spline { .. } => {
                let Entity::Spline { fit, .. } = &e else { unreachable!() };
                let pts: Vec<Vec2> = if fit.len() >= 2 { fit.iter().map(|p| v(*p)).collect() } else { cadrs_drawing::dxf::spline_points(&e).into_iter().map(v).collect() };
                let closed = pts.len() > 3 && pts[0].distance(pts[pts.len() - 1]) <= JOIN;
                let pts = if closed { &pts[..pts.len() - 1] } else { &pts[..] };
                match b.s.add_spline(pts, closed, None, None, false) {
                    Some(_) => b.r.splines += 1,
                    None => b.r.skipped += 1,
                }
            }
            _ => b.r.skipped += 1,
        }
    }
    (b.s, b.r)
}

/// Reads a DXF or DWG file (a DWG through the external converter).
pub fn read_file(path: &std::path::Path) -> Result<DxfDrawing, String> {
    let dwg = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
    let text = if dwg {
        let conv = cadrs_drawing::dwg::find_converter().ok_or(cadrs_drawing::dwg::INSTALL_HINT)?;
        cadrs_drawing::dwg::dwg_to_dxf(&conv, path)?
    } else {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        String::from_utf8_lossy(&bytes).into_owned()
    };
    cadrs_drawing::dxf::read_dxf(&text)
}

/// Adds imported geometry to a sketch (one undo step, "Insert DXF/DWG").
#[derive(Debug, Clone)]
pub struct InsertDxf {
    pub element: ElementId,
    pub feature: FeatureId,
    pub geometry: Sketch,
}

impl Command for InsertDxf {
    fn label(&self) -> String {
        "Insert DXF/DWG".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let op = SketchOp::Paste { sketch: Box::new(self.geometry.clone()), offset: Vec2::ZERO };
        crate::commands::EditSketch { element: self.element, feature: self.feature, op }.apply(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulges_lines_and_units() {
        // A 20 × 10 slot (two lines and two half-circle bulges), in inches, a 1 mm hole.
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n4\n70\n1\n10\n0.0\n20\n0.0\n10\n20.0\n20\n0.0\n42\n1.0\n10\n20.0\n20\n10.0\n10\n0.0\n20\n10.0\n42\n1.0\n0\nCIRCLE\n10\n10.0\n20\n5.0\n40\n1.0\n0\nENDSEC\n0\nEOF\n";
        let d = cadrs_drawing::dxf::read_dxf(dxf).unwrap();
        assert_eq!(DxfUnits::of_file(d.unit_mm), DxfUnits::Inch);
        // Read as millimetres: the numbers as they are (the file's own claim overridden).
        let (s, r) = sketch_of(&d, DxfUnits::Millimeter, true);
        assert_eq!((r.lines, r.arcs, r.circles), (2, 2, 1));
        // 4 slot corners shared + 2 arc centres + 1 circle centre.
        assert_eq!(s.points.len(), 7);
        let regions = cadrs_sketch::region::regions(&s);
        let areas: Vec<f64> = regions.iter().map(|g| g.area()).collect();
        let slot = 20.0 * 10.0 + std::f64::consts::PI * 25.0 - std::f64::consts::PI;
        assert!(areas.iter().any(|a| (a - slot).abs() < 1e-6), "{areas:?}");
        // In inches: 25.4 times larger.
        let (s, _) = sketch_of(&d, DxfUnits::Inch, true);
        let x: f64 = s.points.values().map(|p| p.pos.x).fold(f64::MIN, f64::max);
        assert!((x - 20.0 * 25.4).abs() < 1e-9, "{x}");
        // Centred.
        let (s, _) = sketch_of(&d, DxfUnits::Millimeter, false);
        let (lo, hi) = s.points.values().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.pos.x), hi.max(p.pos.x)));
        assert!((lo + 10.0).abs() < 1e-9 && (hi - 10.0).abs() < 1e-9, "{lo} {hi}");
    }

    #[test]
    fn slightly_gapped_ends_still_close_a_region() {
        // A 30 × 20 rectangle of four LINEs whose ends miss each other by up to 0.0003 mm
        // (rounded coordinates), and a hole: one region with a hole, its area the rectangle's.
        let lines = [
            ([0.0, 0.0], [30.0003, 0.0]),
            ([30.0, 0.0002], [30.0, 20.0]),
            ([29.9998, 20.0001], [0.0, 20.0]),
            ([0.0, 19.9997], [0.0001, 0.0]),
        ];
        let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
        for (a, b) in lines {
            dxf += &format!("0\nLINE\n8\n0\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n", a[0], a[1], b[0], b[1]);
        }
        dxf += "0\nCIRCLE\n8\n0\n10\n15.0\n20\n10.0\n40\n4.0\n0\nENDSEC\n0\nEOF\n";
        let d = cadrs_drawing::dxf::read_dxf(&dxf).unwrap();
        let (s, r) = sketch_of(&d, DxfUnits::Millimeter, true);
        assert_eq!((r.lines, r.circles, r.skipped), (4, 1, 0));
        // The four corners shared (plus the circle's centre).
        assert_eq!(s.points.len(), 5);
        let regions = cadrs_sketch::region::regions(&s);
        let plate = regions.iter().find(|g| !g.holes.is_empty()).expect("the plate closes round its hole");
        let want = 30.0 * 20.0 - std::f64::consts::PI * 16.0;
        assert!((plate.area() - want).abs() < 0.05, "{} vs {want}", plate.area());
    }
}
