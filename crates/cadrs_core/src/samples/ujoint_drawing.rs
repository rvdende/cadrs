//! The finished Ex1 drawing of the Universal Joint Flange stand-in (P3C.7: the drawing the
//! export scenario and tests export; `ex1-drawing.png`), as `course_drw_ex1_ujoint` builds it
//! through the UI: ANSI A in inches, third angle, one sheet at 1:2.
//!
//! - Front at (66, 65.85), Top above it at (66, 170.8) and Right beside it at (180, 65.85) (the
//!   model origin's sheet points, mm), with hidden lines and phantom tangent edges; a shaded
//!   Isometric view at 1:4 up and right.
//! - Top: Ø4.750, 3.282 and 2.061; centermarks on the rim and the four counterbores; the
//!   callout `4x Ø.266 THRU ⌴Ø.438 ↧.250`.
//! - Front: 6.000 and 2.600; Right: 2.500, 43.0°, 120.0°, Ø1.750, Ø1.250, a centermark on the
//!   cross hole and the callout `8x Ø.266 THRU`.
//!
//! Everything is attached through persistent names, as the drawing tools attach it.

use std::sync::Arc;

use cadrs_drawing::annotation::{
    Annotation, AnnotationKind, Centerline, CenterlineKind, CircleCenterline, DimTool, EdgeRef, HoleCallout, Pick, PointOf, PointRef, Shape,
    propose, resolve,
};
use cadrs_drawing::style::TangentEdges;
use cadrs_drawing::view::{Placement, projected_view};
use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Projection, Scale, SheetId, View, ViewId, template};

use super::ujoint as uj;
use crate::command::CommandError;
use crate::document::Document;
use crate::drawing_source::{StudioState, part_key, source_of};
use crate::ids::ElementId;
use crate::views::ViewGeometry;

/// The drawing tab's name (as the course renames it, D8.6).
pub const DRAWING_NAME: &str = "Universal Joint Flange Drawing";

pub const SHEET: SheetId = SheetId::from_u128(0x0a1f_1a00_5000_0000_0000_0000_0000_0001);
pub const FRONT: ViewId = ViewId::from_u128(0x0a1f_1a00_7000_0000_0000_0000_0000_0001);
pub const TOP: ViewId = ViewId::from_u128(0x0a1f_1a00_7000_0000_0000_0000_0000_0002);
pub const RIGHT: ViewId = ViewId::from_u128(0x0a1f_1a00_7000_0000_0000_0000_0000_0003);
pub const ISO: ViewId = ViewId::from_u128(0x0a1f_1a00_7000_0000_0000_0000_0000_0004);

const IN: f64 = uj::IN;

/// The title block's Drawn row (the scenarios' user and date).
pub const DRAWN_BY: &str = "Alex Designer";
pub const DRAWN_DATE: &str = "2026-09-24";

fn bad(what: &str) -> CommandError {
    CommandError::Invalid(format!("Ex1 drawing: {what}"))
}

struct Vg {
    v: View,
    g: Arc<ViewGeometry>,
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

/// Model mm → inches.
fn inches(s: Shape) -> Shape {
    let k = |p: [f64; 2]| [p[0] / IN, p[1] / IN];
    match s {
        Shape::Point(p) => Shape::Point(k(p)),
        Shape::Line { a, b } => Shape::Line { a: k(a), b: k(b) },
        Shape::Circle { center, radius, arc } => Shape::Circle { center: k(center), radius: radius / IN, arc: arc.map(|a| a.map(k)) },
        Shape::Curve { a, b } => Shape::Curve { a: k(a), b: k(b) },
    }
}

impl Vg {
    /// The first model edge whose shape (inches) passes `f`.
    fn find(&self, what: &str, f: impl Fn(&Shape) -> bool) -> Result<EdgeRef, CommandError> {
        // Model edges first; else a named outline (a hole's silhouette).
        let mut best: Option<(bool, EdgeRef)> = None;
        for e in &self.g.projection.edges {
            let r = EdgeRef::of(e);
            if r.edge.is_none() && r.face.is_none() {
                continue;
            }
            let res = resolve(&self.v, &*self.g, &r);
            if res.found && f(&inches(res.shape)) && best.as_ref().is_none_or(|(m, _)| !m && res.from_model) {
                best = Some((res.from_model, EdgeRef { shape: res.shape, ..r }));
            }
        }
        best.map(|(_, r)| r).ok_or_else(|| bad(&format!("no {what} in the {} view", self.v.name)))
    }

    fn circle(&self, what: &str, c: [f64; 2], r: f64) -> Result<EdgeRef, CommandError> {
        self.find(what, |s| matches!(*s, Shape::Circle { center, radius, .. } if near(radius, r) && near(center[0], c[0]) && near(center[1], c[1])))
    }

    fn hline(&self, what: &str, y: f64, x: f64) -> Result<EdgeRef, CommandError> {
        self.find(what, |s| matches!(*s, Shape::Line { a, b } if near(a[1], y) && near(b[1], y) && a[0].min(b[0]) - 1e-4 <= x && x <= a[0].max(b[0]) + 1e-4))
    }

    fn vline(&self, what: &str, x: f64, y: f64) -> Result<EdgeRef, CommandError> {
        self.find(what, |s| matches!(*s, Shape::Line { a, b } if near(a[0], x) && near(b[0], x) && a[1].min(b[1]) - 1e-4 <= y && y <= a[1].max(b[1]) + 1e-4))
    }

    /// A dimension from the tool's picks, its text at `text` (inches).
    fn dim(&self, tool: DimTool, picks: &[Pick], text: [f64; 2]) -> Result<Annotation, CommandError> {
        let d = propose(tool, &self.v, &*self.g, picks, [text[0] * IN, text[1] * IN]).ok_or_else(|| bad("a dimension doesn't measure"))?;
        Ok(Annotation::new(AnnotationKind::Dimension(d)))
    }
}

/// The model origins of Front, Top and Right on the sheet (mm, 1:2).
const FRONT_AT: [f64; 2] = [66.0, 65.85];
const TOP_AT: [f64; 2] = [66.0, 170.8];
const RIGHT_AT: [f64; 2] = [180.0, 65.85];

/// A sheet point (mm) of a 1:2 view whose origin is at `origin`, in the view's inches (where
/// `course_drw_ex1_ujoint` clicks to place each annotation).
fn sheet_at(origin: [f64; 2], p: [f64; 2]) -> [f64; 2] {
    [(p[0] - origin[0]) * 2.0 / IN, (p[1] - origin[1]) * 2.0 / IN]
}

fn lines_centerline(a: EdgeRef, b: EdgeRef) -> Annotation {
    Annotation::new(AnnotationKind::Centerline(Centerline { kind: CenterlineKind::Lines { a, b }, extend: [0.0, 0.0] }))
}

fn point(e: EdgeRef) -> PointRef {
    let c = e.shape.circle().map(|(c, _)| c).unwrap_or([0.0, 0.0]);
    PointRef { edge: e, of: PointOf::Center, hint: c }
}

fn centre(e: EdgeRef) -> Pick {
    let c = e.shape.circle().map(|(c, _)| c).unwrap_or([0.0, 0.0]);
    Pick::Point(PointRef { edge: e, of: PointOf::Center, hint: c })
}

fn project(features: &[crate::Feature], v: &View) -> Result<Arc<ViewGeometry>, CommandError> {
    crate::views::project(
        features,
        crate::views::ViewRequest {
            part: Some(uj::PART),
            frame: v.frame.view_frame(),
            options: cadrs_kernel::ProjectOptions { tolerance: 0.01, hidden: true },
            shaded: false,
            props: Vec::new(),
            appearances: Vec::new(),
            cut: None,
            intersections: false,
            flat: false,
        },
    )
    .map_err(|e| bad(&e))
}

fn place(d: &mut Drawing, mut v: View, id: ViewId, hash: Option<u64>) -> Result<View, CommandError> {
    v.id = id;
    v.source_hash = hash;
    d.apply(&DrawingOp::InsertView { sheet: SHEET, view: v.clone() }).map_err(|e| bad(&e))?;
    Ok(v)
}

/// The finished Ex1 drawing of the flange in Part Studio `studio` of `doc`.
pub fn drawing(doc: &Document, studio: ElementId) -> Result<Drawing, CommandError> {
    let t = template::builtin("ANSI_A_INCH.dwt").ok_or_else(|| bad("no ANSI_A_INCH template"))?;
    let el = doc.element(studio).ok_or_else(|| bad("no studio"))?;
    let state = StudioState::of(el).ok_or_else(|| bad("not a Part Studio"))?;
    let build = crate::rebuild::build(&state.features);
    let source = source_of(studio, &state, &build);
    let features = state.features.clone();
    let part = ObjectRef { element: studio.0, part: part_key(Some(uj::PART)) };
    let hash = source.hash_of(part.part);
    let mut d = Drawing::from_template(&t, None);
    d.sources = vec![source];
    d.title.drawn_by = Some(DRAWN_BY.into());
    d.title.drawn_date = Some(DRAWN_DATE.into());
    d.sheets[0].id = SHEET;
    let ortho = |mut v: View| {
        v.hidden_lines = true;
        v.tangent_edges = TangentEdges::Phantom;
        v
    };
    let front = place(&mut d, ortho(View::base(part, NamedView::Front, Scale::new(1, 2), [66.0, 65.85])), FRONT, hash)?;
    let top = place(&mut d, ortho(projected_view(&front, Placement::Ortho([0.0, 1.0]), Projection::Third, [66.0, 170.8], None)), TOP, hash)?;
    let right = place(&mut d, ortho(projected_view(&front, Placement::Ortho([1.0, 0.0]), Projection::Third, [180.0, 65.85], None)), RIGHT, hash)?;
    let mut iso = projected_view(&front, Placement::Iso(1.0, 1.0), Projection::Third, [228.0, 152.0], None);
    iso.scale = Scale::new(1, 4);
    iso.scale_inherited = false;
    iso.shaded = true;
    iso.hidden_lines = false;
    place(&mut d, iso, ISO, hash)?;

    let f = Vg { g: project(&features, &front)?, v: front };
    let tp = Vg { g: project(&features, &top)?, v: top };
    let r = Vg { g: project(&features, &right)?, v: right };
    let (hx, hy) = (uj::HOLES_X / 2.0, uj::HOLES_Y / 2.0);
    let w = uj::LUG_W / 2.0;
    let hole_r = uj::HOLE_D / 2.0;

    // Top view.
    let rim = tp.circle("the flange's rim", [0.0, 0.0], uj::FLANGE_D / 2.0)?;
    let hole = |x: f64, y: f64| tp.circle("a counterbore", [x, y], uj::CBORE_D / 2.0);
    let holes = [hole(-hx, -hy)?, hole(-hx, hy)?, hole(hx, -hy)?, hole(hx, hy)?];
    let mut anns = vec![
        tp.dim(DimTool::Diameter, &[Pick::Edge(rim)], sheet_at(TOP_AT, [112.0, 197.0]))?,
        tp.dim(DimTool::Smart, &[centre(holes[0]), centre(holes[1])], sheet_at(TOP_AT, [25.0, 170.8]))?,
        tp.dim(DimTool::Smart, &[centre(holes[0]), centre(holes[2])], sheet_at(TOP_AT, [66.0, 136.8]))?,
        Annotation::new(AnnotationKind::Centermark(rim)),
        Annotation::new(AnnotationKind::HoleCallout(HoleCallout {
            edge: holes[2],
            text: { let t = sheet_at(TOP_AT, [100.0, 146.0]); [t[0] * IN, t[1] * IN] },
            prefix: "4x".into(),
            last: None,
        })),
    ];
    anns.extend(holes.iter().map(|h| Annotation::new(AnnotationKind::Centermark(*h))));
    for a in anns {
        d.apply(&DrawingOp::AddAnnotation { view: TOP, annotation: a }).map_err(|e| bad(&e))?;
    }

    // Front view.
    let bottom = f.hline("the flange's bottom", -uj::FLANGE_T, 0.0)?;
    let lug_top = f.hline("a lug's top", uj::TOP, -2.0)?;
    let inner = |x: f64| f.vline("a lug's inner face", x, 3.0);
    let anns = vec![
        f.dim(DimTool::Smart, &[Pick::Edge(bottom), Pick::Edge(lug_top)], sheet_at(FRONT_AT, [24.0, 97.6]))?,
        f.dim(DimTool::Smart, &[Pick::Edge(inner(-uj::SLOT / 2.0)?), Pick::Edge(inner(uj::SLOT / 2.0)?)], sheet_at(FRONT_AT, [66.0, 88.0]))?,
        // Line-to-line centerlines on the counterbored holes' hidden sides (D8.7).
        lines_centerline(f.vline("a hole's side", -hx - hole_r, -0.38)?, f.vline("a hole's side", -hx + hole_r, -0.38)?),
        lines_centerline(f.vline("a hole's side", hx - hole_r, -0.38)?, f.vline("a hole's side", hx + hole_r, -0.38)?),
    ];
    for a in anns {
        d.apply(&DrawingOp::AddAnnotation { view: FRONT, annotation: a }).map_err(|e| bad(&e))?;
    }

    // Right view.
    let side = |y: f64| r.vline("a lug's side", y, 3.0);
    let chamfer = r.find("the chamfer", |s| {
        matches!(*s, Shape::Line { a, b } if {
            let (lo, hi) = if a[1] < b[1] { (a, b) } else { (b, a) };
            near(lo[0], -w) && near(hi[0], -w + uj::CHAMFER_RUN) && near(hi[1], uj::TOP)
        })
    })?;
    let top_edge = r.hline("the lug's top", uj::TOP, 0.0)?;
    let flare = r.find("the flare", |s| {
        matches!(*s, Shape::Line { a, b } if {
            let slope = ((b[1] - a[1]) / (b[0] - a[0])).abs();
            (slope - 3f64.sqrt()).abs() < 1e-3 && a[0].max(b[0]) < -w + 1e-3 && a[1].min(b[1]) < 0.1
        })
    })?;
    let floor = r.hline("the flange's top", 0.0, -2.2)?;
    let boss = r.circle("the boss", [0.0, uj::CROSS_Z], uj::BOSS_D / 2.0)?;
    let cross = r.circle("the cross hole", [0.0, uj::CROSS_Z], uj::CROSS_D / 2.0)?;
    let b = uj::BOLT_D / 2.0 * std::f64::consts::FRAC_1_SQRT_2;
    let lug_hole = r.circle("a lug hole", [b, uj::CROSS_Z + b], uj::HOLE_D / 2.0)?;
    let anns = vec![
        r.dim(DimTool::Smart, &[Pick::Edge(side(-w)?), Pick::Edge(side(w)?)], sheet_at(RIGHT_AT, [180.0, 142.0]))?,
        r.dim(DimTool::Angular, &[Pick::Edge(chamfer), Pick::Edge(top_edge)], sheet_at(RIGHT_AT, [155.5, 129.8]))?,
        r.dim(DimTool::Angular, &[Pick::Edge(flare), Pick::Edge(floor)], sheet_at(RIGHT_AT, [140.0, 75.0]))?,
        r.dim(DimTool::Smart, &[Pick::Edge(boss)], sheet_at(RIGHT_AT, [148.0, 113.475]))?,
        r.dim(DimTool::Smart, &[Pick::Edge(cross)], sheet_at(RIGHT_AT, [212.0, 100.0]))?,
        Annotation::new(AnnotationKind::Centermark(cross)),
        Annotation::new(AnnotationKind::HoleCallout(HoleCallout {
            edge: lug_hole,
            text: { let t = sheet_at(RIGHT_AT, [214.0, 128.0]); [t[0] * IN, t[1] * IN] },
            prefix: "8x".into(),
            last: None,
        })),
        lines_centerline(r.vline("a hole's side", -hy - hole_r, -0.38)?, r.vline("a hole's side", -hy + hole_r, -0.38)?),
        lines_centerline(r.vline("a hole's side", hy - hole_r, -0.38)?, r.vline("a hole's side", hy + hole_r, -0.38)?),
        Annotation::new(AnnotationKind::CircleCenterline(CircleCenterline::ThreePoints([
            point(r.circle("a lug hole", [b, uj::CROSS_Z + b], uj::HOLE_D / 2.0)?),
            point(r.circle("a lug hole", [-b, uj::CROSS_Z + b], uj::HOLE_D / 2.0)?),
            point(r.circle("a lug hole", [b, uj::CROSS_Z - b], uj::HOLE_D / 2.0)?),
        ]))),
    ];
    for a in anns {
        d.apply(&DrawingOp::AddAnnotation { view: RIGHT, annotation: a }).map_err(|e| bad(&e))?;
    }
    Ok(d)
}
