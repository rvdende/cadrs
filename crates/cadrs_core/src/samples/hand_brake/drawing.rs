//! The finished "Hand Brake Drawing" of the stand-in (P3C.6, `ex3-drawing.png`): ANSI A, mm,
//! 2 decimals, three sheets.
//!
//! - **Assembly** (P3C.5): the Hydraulic Brake Unit ([`super::assembly`]) isometric and shaded at
//!   1:2, its 10-row BOM at the top left and circle balloons 1–10 (Item No.) round the view.
//! - **Handle** (1:2): the Handle Plate's Front view, the Top view above it and a shaded
//!   Isometric view (1:4), with the dimensions of `ex3-step9.png`: 225.00 (→ 250.00), 20.00
//!   (→ 25.00), R30.00, Ø25.40, 73.00 (→ 78.00), R16.00, `3x Ø5.50`, `2x Ø8.25` on the small circle,
//!   75.00 from the small circle's centre to the end hole's, 8.00 in the Top view; centermarks on
//!   the holes and the small circle, and centerlines on the large hole and the small circle in the
//!   Top view.
//! - **Grip** (1:1): the Handle Grip's Front, Top and Isometric (1:4) views with Ø25.00, 170.00
//!   (→ 185.00), 35.00, 62.50, the callout `3x Ø5.50 THRU ⌴Ø9.75 ↧5.00` (→ `3x Ø6.60 THRU
//!   ⌴Ø11.25 ↧6.00`), a centerline along the axis, and 8.00 and 8.50 in the Top view.
//!
//! Every annotation is attached to the model through persistent names, as the drawing tools
//! attach them, so updating after the course's edits moves them with the model, and the ones on
//! the small circle (deleted in D14.4) dangle.

use std::sync::Arc;

use cadrs_drawing::annotation::{
    Annotation, AnnotationId, AnnotationKind, Centerline, CenterlineKind, DimFormat, DimKind, Dimension, EdgeRef,
    HoleCallout, Orient, Pick, PointOf, PointRef, Shape, resolve,
};
use cadrs_drawing::view::{Placement, projected_view};
use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Projection, Scale, SheetId, View, ViewId, template};
use cadrs_kernel::ProjectOptions;

use super::{BEFORE, GRIP, GRIP_D, GRIP_FIRST, GRIP_HOLES, GRIP_Y, GRIP_Z, MainGeometry, PLATE, THICKNESS};
use crate::command::CommandError;
use crate::document::Document;
use crate::drawing_source::{StudioState, part_key, source_of};
use crate::ids::{ElementId, PartId};
use crate::views::{ViewGeometry, ViewRequest};

const fn sid(n: u128) -> SheetId {
    SheetId::from_u128(0x4a2d_b2a7_5000_0000_0000_0000_0000_0000 | n)
}
const fn vid(n: u128) -> ViewId {
    ViewId::from_u128(0x4a2d_b2a7_7000_0000_0000_0000_0000_0000 | n)
}
const fn aid(n: u128) -> AnnotationId {
    AnnotationId::from_u128(0x4a2d_b2a7_a000_0000_0000_0000_0000_0000 | n)
}

pub const ASSEMBLY_SHEET: SheetId = sid(1);
pub const HANDLE_SHEET: SheetId = sid(2);
pub const GRIP_SHEET: SheetId = sid(3);
pub const HANDLE_FRONT: ViewId = vid(1);
pub const HANDLE_TOP: ViewId = vid(2);
pub const HANDLE_ISO: ViewId = vid(3);
pub const GRIP_FRONT: ViewId = vid(4);
pub const GRIP_TOP: ViewId = vid(5);
pub const GRIP_ISO: ViewId = vid(6);

/// The annotations the tests and the scenario look for.
pub const DIM_LENGTH: AnnotationId = aid(1);
pub const DIM_BAR: AnnotationId = aid(2);
pub const DIM_FILLET: AnnotationId = aid(3);
pub const DIM_LARGE: AnnotationId = aid(4);
pub const DIM_HEIGHT: AnnotationId = aid(5);
pub const DIM_END_R: AnnotationId = aid(6);
pub const DIM_HOLES: AnnotationId = aid(7);
pub const DIM_SMALL: AnnotationId = aid(8);
/// 75.00: from the small circle's centre (end a, which goes away) to the end hole's (end b).
pub const DIM_75: AnnotationId = aid(9);
pub const DIM_THICKNESS: AnnotationId = aid(10);
pub const CENTERMARK_SMALL: AnnotationId = aid(11);
pub const CENTERLINE_SMALL: AnnotationId = aid(12);
pub const CENTERLINE_LARGE: AnnotationId = aid(13);
/// Centermarks on the three Ø5.5 holes, the large hole and the end hole.
pub const CENTERMARKS: [AnnotationId; 5] = [aid(14), aid(15), aid(16), aid(17), aid(18)];
pub const DIM_GRIP_D: AnnotationId = aid(20);
pub const DIM_GRIP_LENGTH: AnnotationId = aid(21);
pub const DIM_GRIP_35: AnnotationId = aid(22);
pub const DIM_GRIP_62: AnnotationId = aid(23);
pub const CALLOUT_GRIP: AnnotationId = aid(24);
pub const CENTERLINE_GRIP: AnnotationId = aid(25);
pub const DIM_SLOT: AnnotationId = aid(26);
pub const DIM_SLOT_WALL: AnnotationId = aid(27);
/// The Assembly sheet's isometric view of the Hydraulic Brake Unit (P3C.5).
pub const ASSEMBLY_ISO: ViewId = vid(7);
/// The Assembly sheet's BOM table.
pub const ASSEMBLY_BOM: cadrs_drawing::TableId = cadrs_drawing::TableId(uuid::Uuid::from_u128(0x4a2d_b2a7_b000_0000_0000_0000_0000_0001));
/// The balloons 1–10, by item.
pub const fn balloon(item: usize) -> AnnotationId {
    aid(0x100 + item as u128)
}

fn bad(what: &str) -> CommandError {
    CommandError::Invalid(format!("Hand Brake Drawing: {what}"))
}

/// A view and its projection.
struct Vg {
    v: View,
    g: Arc<ViewGeometry>,
}

fn project(features: &[crate::Feature], v: &View, part: PartId) -> Result<Arc<ViewGeometry>, CommandError> {
    crate::views::project(
        features,
        ViewRequest {
            part: Some(part),
            frame: v.frame.view_frame(),
            options: ProjectOptions { tolerance: 0.01, hidden: true },
            shaded: false,
            props: Vec::new(),
            appearances: Vec::new(),
            cut: None,
            intersections: false,
        },
    )
    .map_err(|e| bad(&e))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

impl Vg {
    /// The references of the view's edges whose current shape passes `f` (model edges first).
    fn find(&self, what: &str, f: impl Fn(&Shape) -> bool) -> Result<EdgeRef, CommandError> {
        let mut best: Option<(bool, EdgeRef)> = None;
        for e in &self.g.projection.edges {
            let r = EdgeRef::of(e);
            if r.edge.is_none() && r.face.is_none() {
                continue;
            }
            let res = resolve(&self.v, &*self.g, &r);
            if res.found && f(&res.shape) && best.as_ref().is_none_or(|(m, _)| !m && res.from_model) {
                best = Some((res.from_model, EdgeRef { shape: res.shape, ..r }));
            }
        }
        if best.is_none() && std::env::var("CADRS_DEBUG_HAND_BRAKE").is_ok() {
            for e in &self.g.projection.edges {
                let r = EdgeRef::of(e);
                let res = resolve(&self.v, &*self.g, &r);
                eprintln!("{:?} {:?} model {} named {}", e.visibility, res.shape, res.from_model, r.edge.is_some() || r.face.is_some());
            }
        }
        best.map(|(_, r)| r).ok_or_else(|| bad(&format!("no {what} in the {} view", self.v.name)))
    }

    fn circle(&self, what: &str, c: [f64; 2], r: f64) -> Result<EdgeRef, CommandError> {
        self.find(what, |s| matches!(*s, Shape::Circle { center, radius, arc: None } if near(radius, r) && near(center[0], c[0]) && near(center[1], c[1])))
    }

    fn arc(&self, what: &str, c: [f64; 2], r: f64) -> Result<EdgeRef, CommandError> {
        self.find(what, |s| matches!(*s, Shape::Circle { center, radius, arc: Some(_) } if near(radius, r) && near(center[0], c[0]) && near(center[1], c[1])))
    }

    /// A horizontal line at `y` reaching over `x`.
    fn hline(&self, what: &str, y: f64, x: f64) -> Result<EdgeRef, CommandError> {
        self.find(what, |s| matches!(*s, Shape::Line { a, b } if near(a[1], y) && near(b[1], y) && a[0].min(b[0]) - 1e-4 <= x && x <= a[0].max(b[0]) + 1e-4))
    }

    /// A vertical line at `x` reaching over `y`.
    fn vline(&self, what: &str, x: f64, y: f64) -> Result<EdgeRef, CommandError> {
        self.find(what, |s| matches!(*s, Shape::Line { a, b } if near(a[0], x) && near(b[0], x) && a[1].min(b[1]) - 1e-4 <= y && y <= a[1].max(b[1]) + 1e-4))
    }
}

fn centre(e: EdgeRef) -> Pick {
    let c = e.shape.circle().map(|(c, _)| c).unwrap_or([0.0, 0.0]);
    Pick::Point(PointRef { edge: e, of: PointOf::Center, hint: c })
}

fn end(e: EdgeRef, at: [f64; 2]) -> Pick {
    Pick::Point(PointRef { edge: e, of: PointOf::End, hint: at })
}

fn dim(id: AnnotationId, kind: DimKind, text: [f64; 2], prefix: &str) -> Annotation {
    Annotation {
        id,
        kind: AnnotationKind::Dimension(Dimension {
            kind,
            text,
            format: DimFormat { prefix: prefix.into(), ..DimFormat::default() },
            last: None,
        }),
    }
}

fn distance(id: AnnotationId, a: Pick, b: Pick, orient: Orient, text: [f64; 2]) -> Annotation {
    dim(id, DimKind::Distance { a, b, orient }, text, "")
}

fn lines_centerline(id: AnnotationId, a: EdgeRef, b: EdgeRef, extend: [f64; 2]) -> Annotation {
    Annotation { id, kind: AnnotationKind::Centerline(Centerline { kind: CenterlineKind::Lines { a, b }, extend }) }
}

fn centermark(id: AnnotationId, e: EdgeRef) -> Annotation {
    Annotation { id, kind: AnnotationKind::Centermark(e) }
}

/// Places a view with its id and the source's hash of its part.
fn place(d: &mut Drawing, sheet: SheetId, mut v: View, id: ViewId, hash: Option<u64>) -> Result<View, CommandError> {
    v.id = id;
    v.source_hash = hash;
    d.apply(&DrawingOp::InsertView { sheet, view: v.clone() }).map_err(|e| bad(&e))?;
    Ok(v)
}

fn set_annotations(d: &mut Drawing, id: ViewId, anns: Vec<Annotation>) -> Result<(), CommandError> {
    for a in anns {
        d.apply(&DrawingOp::AddAnnotation { view: id, annotation: a }).map_err(|e| bad(&e))?;
    }
    Ok(())
}

/// The Assembly sheet (P3C.5, `ex3-drawing.png`): the Hydraulic Brake Unit `asm`, isometric
/// and shaded at 1:2 right of its BOM (Item No., Name, Quantity; top to bottom, snapped to the
/// frame's top-left corner), and circle balloons 1–10 showing each row's Item No., each on a
/// visible edge of one of its row's occurrences, spread round the view.
fn assembly_sheet(d: &mut Drawing, doc: &Document, asm: ElementId) -> Result<(), CommandError> {
    use cadrs_drawing::assembly::{Border, BomOrder, BomType, Callout, CalloutFields, bom_table};
    use crate::drawing_assembly as da;
    let src = da::live_source(doc, asm).ok_or_else(|| bad("no assembly"))?;
    let hash = src.hash_of(None);
    let state = da::AssemblyState::parse(&src.snapshot).ok_or_else(|| bad("the assembly's state"))?;
    d.sources.push(src);
    let sheet = &d.sheets[0];
    let frame = cadrs_drawing::standard::frame(sheet.format);
    // The BOM, compact (2.5 mm text, 5 mm rows), at the frame's top-left corner.
    let data = da::live_bom(doc, asm, BomType::Flattened, BomOrder::TopToBottom).map_err(|e| bad(&e.to_string()))?;
    let mut style = d.style.clone();
    style.table_text_height = 2.5;
    style.table_row_height = 5.0;
    let mut table = bom_table(data, cadrs_drawing::table::Corner::TopLeft, [frame.inner.min[0], frame.inner.max[1]], &style);
    table.id = ASSEMBLY_BOM;
    // Its rows grown to fit their text, as the app stores a placed table.
    let props = crate::drawing_export::reference_props(doc, Some(ObjectRef { element: asm.0, part: None }));
    let ctx = crate::drawing_export::drawing_context(super::DRAWING_NAME, d, 0, None);
    let table = cadrs_drawing::table::fit_rows(&table, &cadrs_drawing::rich::FieldContext { reference: &props, drawing: &ctx });
    let bom_right = frame.inner.min[0] + table.width();
    // The view: isometric, shaded, 1:2, centred right of the table and above the title block.
    let r = ObjectRef { element: asm.0, part: None };
    let mut v = View::base(r, NamedView::Isometric, Scale::new(1, 2), [0.0, 0.0]);
    // Seen from the front, left and above, so the handle runs up to the right towards the
    // cylinder as in `ex3-drawing.png`.
    let s3 = 3f64.sqrt();
    let s2 = 2f64.sqrt();
    v.frame = cadrs_drawing::Frame3::new([1.0 / s3, 1.0 / s3, -1.0 / s3], [1.0 / s2, -1.0 / s2, 0.0]);
    v.shaded = true;
    v.hidden_lines = false;
    v.tangent_edges = cadrs_drawing::style::TangentEdges::Phantom;
    let g = da::project(&state, &v).map_err(|e| bad(&e))?;
    let (lo, hi) = g.bounds.ok_or_else(|| bad("the assembly view is empty"))?;
    let k = v.scale.factor();
    let tb_top = cadrs_drawing::title_block::placement(frame.inner, sheet.format.size).max[1];
    let centre = [(bom_right + frame.inner.max[0]) / 2.0 - 6.0, (tb_top + frame.inner.max[1]) / 2.0 - 4.0];
    v.anchor = [centre[0] - k * (lo[0] + hi[0]) / 2.0, centre[1] - k * (lo[1] + hi[1]) / 2.0];
    let v = place(d, ASSEMBLY_SHEET, v, ASSEMBLY_ISO, hash)?;
    d.apply(&DrawingOp::AddTable { sheet: ASSEMBLY_SHEET, table: table.clone() }).map_err(|e| bad(&e))?;
    // The balloons: each row's first occurrence with visible edges, on its visible edge point
    // nearest the middle of its visible edges.
    use cadrs_kernel::ProjVisibility;
    let bom = table.bom.as_ref().ok_or_else(|| bad("no BOM"))?;
    // Every visible edge point with its occurrence (every other point: enough to measure room).
    let all_pts: Vec<([f64; 2], uuid::Uuid)> = g
        .projection
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| e.visibility == ProjVisibility::Visible)
        .filter_map(|(j, e)| da::edge_occurrence(&g, j).map(|o| (e, o.0)))
        .flat_map(|(e, o)| e.points.iter().step_by(2).map(move |p| ([p.x, p.y], o)))
        .collect();
    let mut attach: Vec<(usize, uuid::Uuid, [f64; 2])> = Vec::new();
    for (i, row) in bom.rows.iter().enumerate() {
        let found = row.occurrences.iter().find_map(|occ| {
            let pts: Vec<[f64; 2]> = g
                .projection
                .edges
                .iter()
                .enumerate()
                .filter(|(j, e)| e.visibility == ProjVisibility::Visible && da::edge_occurrence(&g, *j).is_some_and(|o| o.0 == *occ))
                .flat_map(|(_, e)| e.points.iter().map(|p| [p.x, p.y]))
                .collect();
            if pts.is_empty() {
                return None;
            }
            let n = pts.len() as f64;
            let m = [pts.iter().map(|p| p[0]).sum::<f64>() / n, pts.iter().map(|p| p[1]).sum::<f64>() / n];
            // On the part itself (P3C wrap-up: balloon 3 sat where the Handle Plate meets the
            // enclosure): the point with the most room from the other parts' edges, near the
            // part's middle.
            let room = |p: &[f64; 2]| {
                all_pts.iter().filter(|(_, o)| o != occ).map(|(q, _)| (q[0] - p[0]).hypot(q[1] - p[1])).fold(24.0, f64::min)
            };
            let score = |p: &[f64; 2]| room(p) - 0.08 * (p[0] - m[0]).hypot(p[1] - m[1]);
            let best = pts.iter().step_by(2).copied().max_by(|a, b| score(a).total_cmp(&score(b)))?;
            Some((*occ, best))
        });
        if let Some((occ, p)) = found {
            attach.push((i + 1, occ, p));
        }
    }
    // Next to each part (sheet mm), as `ex3-completed.png`: each balloon takes the nearest spot
    // round its leader's end that is clear of the parts' visible edges, the other balloons and
    // leaders, the BOM and the title block, preferring to lie outwards from the view's middle.
    let to_s = |p: [f64; 2]| v.to_sheet(p);
    let segs: Vec<([f64; 2], [f64; 2])> = g
        .projection
        .edges
        .iter()
        .filter(|e| e.visibility == ProjVisibility::Visible)
        .flat_map(|e| e.points.windows(2).map(|w| (to_s([w[0].x, w[0].y]), to_s([w[1].x, w[1].y]))).collect::<Vec<_>>())
        .collect();
    let (slo, shi) = segs.iter().flat_map(|(a, b)| [*a, *b]).fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
        ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])
    });
    let mid = [(slo[0] + shi[0]) / 2.0, (slo[1] + shi[1]) / 2.0];
    let tb = cadrs_drawing::title_block::placement(frame.inner, d.sheets[0].format.size);
    let (blo, bhi) = table.rect();
    let seg_dist = |p: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
        (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
    };
    let crosses = |a: [f64; 2], b: [f64; 2], c: [f64; 2], e: [f64; 2]| {
        let o = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
        o(a, b, c) * o(a, b, e) < 0.0 && o(c, e, a) * o(c, e, b) < 0.0
    };
    // Where the shaded parts are (a 1 mm grid of the sheet): a balloon is better off the parts.
    let cell = 1.0;
    let (gw, gh) = (((shi[0] - slo[0]) / cell).ceil().max(1.0) as usize + 1, ((shi[1] - slo[1]) / cell).ceil().max(1.0) as usize + 1);
    let mut solid = vec![false; gw * gh];
    for t in &g.shaded {
        let [a, b, c] = t.points.map(to_s);
        let (x0, x1) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
        let (y0, y1) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-12 {
            continue;
        }
        let (i0, i1) = (((x0 - slo[0]) / cell).floor().max(0.0) as usize, (((x1 - slo[0]) / cell).ceil() as usize).min(gw - 1));
        let (j0, j1) = (((y0 - slo[1]) / cell).floor().max(0.0) as usize, (((y1 - slo[1]) / cell).ceil() as usize).min(gh - 1));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let q = [slo[0] + (i as f64 + 0.5) * cell, slo[1] + (j as f64 + 0.5) * cell];
                let l1 = ((b[1] - c[1]) * (q[0] - c[0]) + (c[0] - b[0]) * (q[1] - c[1])) / det;
                let l2 = ((c[1] - a[1]) * (q[0] - c[0]) + (a[0] - c[0]) * (q[1] - c[1])) / det;
                if l1 >= 0.0 && l2 >= 0.0 && l1 + l2 <= 1.0 {
                    solid[j * gw + i] = true;
                }
            }
        }
    }
    // The share of a balloon's disc over the parts.
    let over_parts = |c: [f64; 2], r: f64| {
        let (mut n, mut hit) = (0, 0);
        let k = (r / cell).ceil() as i64;
        for dj in -k..=k {
            for di in -k..=k {
                let q = [c[0] + di as f64 * cell, c[1] + dj as f64 * cell];
                if (q[0] - c[0]).hypot(q[1] - c[1]) > r {
                    continue;
                }
                n += 1;
                let (i, j) = (((q[0] - slo[0]) / cell).floor(), ((q[1] - slo[1]) / cell).floor());
                if i >= 0.0 && j >= 0.0 && (i as usize) < gw && (j as usize) < gh && solid[j as usize * gw + i as usize] {
                    hit += 1;
                }
            }
        }
        if n == 0 { 0.0 } else { hit as f64 / n as f64 }
    };
    let r = 3.9;
    let clear = 1.2;
    let mut placed: Vec<([f64; 2], [f64; 2])> = Vec::new(); // (balloon centre, leader end)
    let mut spots = vec![[0.0, 0.0]; attach.len()];
    // The balloons nearest the view's edge first (they have the most room).
    let mut order: Vec<usize> = (0..attach.len()).collect();
    let out_of = |i: usize| {
        let p = to_s(attach[i].2);
        -(((p[0] - mid[0]) / (shi[0] - slo[0]).max(1.0)).powi(2) + ((p[1] - mid[1]) / (shi[1] - slo[1]).max(1.0)).powi(2))
    };
    order.sort_by(|a, b| out_of(*a).total_cmp(&out_of(*b)));
    for i in order {
        let at = to_s(attach[i].2);
        let out = {
            let d = [at[0] - mid[0], at[1] - mid[1]];
            let l = d[0].hypot(d[1]).max(1e-9);
            [d[0] / l, d[1] / l]
        };
        let mut best: Option<(f64, [f64; 2])> = None;
        for dist_mm in [9.0, 12.0, 15.0, 19.0, 24.0, 30.0, 38.0] {
            for k in 0..24 {
                let t = std::f64::consts::TAU * k as f64 / 24.0;
                let dir = [t.cos(), t.sin()];
                let c = [at[0] + dir[0] * dist_mm, at[1] + dir[1] * dist_mm];
                // Hard limits: on the sheet's frame, off the BOM and the title block.
                let inside = |lo: [f64; 2], hi: [f64; 2]| c[0] > lo[0] - r - 2.0 && c[0] < hi[0] + r + 2.0 && c[1] > lo[1] - r - 2.0 && c[1] < hi[1] + r + 2.0;
                if c[0] < frame.inner.min[0] + r + 3.0 || c[0] > frame.inner.max[0] - r - 3.0 || c[1] < frame.inner.min[1] + r + 3.0 || c[1] > frame.inner.max[1] - r - 3.0 || inside(blo, bhi) || inside(tb.min, tb.max) {
                    continue;
                }
                let nearest = placed.iter().map(|(o, _)| (o[0] - c[0]).hypot(o[1] - c[1])).fold(f64::MAX, f64::min);
                if nearest < 2.0 * r + 4.0 {
                    continue;
                }
                // Spread out: some room from the other balloons.
                let crowd = (16.0 - nearest).max(0.0);
                let hits = segs.iter().filter(|(a, b)| seg_dist(c, *a, *b) < r + clear).count();
                let leader_start = [c[0] - dir[0] * r, c[1] - dir[1] * r];
                let crossings = placed.iter().filter(|(o, e)| crosses(leader_start, at, *o, *e)).count();
                let through = placed.iter().filter(|(o, _)| seg_dist(*o, leader_start, at) < r + 0.5).count();
                let facing = 1.0 - (dir[0] * out[0] + dir[1] * out[1]);
                let cost = dist_mm + 60.0 * (hits.min(1) as f64) + 2.0 * hits.min(20) as f64 + 80.0 * crossings as f64 + 80.0 * through as f64 + 4.0 * facing + 40.0 * over_parts(c, r + 0.5) + 1.5 * crowd;
                if best.is_none_or(|(b, _)| cost < b) {
                    best = Some((cost, c));
                }
            }
        }
        let c = best.map(|b| b.1).unwrap_or([at[0] + out[0] * 15.0, at[1] + out[1] * 15.0]);
        placed.push((c, at));
        spots[i] = c;
    }
    let spots: Vec<[f64; 2]> = spots.into_iter().map(|p| v.from_sheet(p)).collect();
    let mut anns = Vec::new();
    for ((item, occ, a), t) in attach.iter().zip(spots) {
        anns.push(Annotation {
            id: balloon(*item),
            kind: AnnotationKind::Callout(Callout {
                occurrence: *occ,
                attach: *a,
                text: t,
                border: Border::Circle,
                size: 2,
                text_height: 3.0,
                fields: CalloutFields { center: "{Table: Item No.}".into(), ..Default::default() },
                last: None,
            }),
        });
    }
    set_annotations(d, ASSEMBLY_ISO, anns)?;
    let _ = v;
    Ok(())
}

/// The drawing of the Handle studio `studio` of `doc` (its workspace state is the drawing's
/// source: the drawing is up to date), with the Assembly sheet of `assembly` (P3C.5).
pub fn drawing(doc: &Document, studio: ElementId, assembly: Option<ElementId>) -> Result<Drawing, CommandError> {
    let t = template::builtin("ANSI_A_MM.dwt").ok_or_else(|| bad("no ANSI_A_MM template"))?;
    let el = doc.element(studio).ok_or_else(|| bad("no studio"))?;
    let state = StudioState::of(el).ok_or_else(|| bad("not a Part Studio"))?;
    let build = crate::rebuild::build(&state.features);
    let source = source_of(studio, &state, &build);
    let features = state.features.clone();
    let mut d = Drawing::from_template(&t, None);
    d.sources = vec![source.clone()];
    // Sheets: Assembly (the first, renamed), Handle, Grip.
    d.sheets[0].id = ASSEMBLY_SHEET;
    d.sheets[0].name = "Assembly".into();
    d.apply(&DrawingOp::InsertSheet { id: HANDLE_SHEET, after: Some(ASSEMBLY_SHEET) }).map_err(|e| bad(&e))?;
    d.apply(&DrawingOp::RenameSheet { id: HANDLE_SHEET, name: "Handle".into() }).map_err(|e| bad(&e))?;
    d.apply(&DrawingOp::InsertSheet { id: GRIP_SHEET, after: Some(HANDLE_SHEET) }).map_err(|e| bad(&e))?;
    d.apply(&DrawingOp::RenameSheet { id: GRIP_SHEET, name: "Grip".into() }).map_err(|e| bad(&e))?;
    if let Some(asm) = assembly {
        assembly_sheet(&mut d, doc, asm)?;
    }

    // ---- Handle: the plate at 1:2.
    let plate = ObjectRef { element: studio.0, part: part_key(Some(PLATE)) };
    let ph = source.hash_of(plate.part);
    let mut front = View::base(plate, NamedView::Front, Scale::new(1, 2), [42.0, 118.0]);
    front.hidden_lines = false;
    let front = place(&mut d, HANDLE_SHEET, front, HANDLE_FRONT, ph)?;
    let mut top = projected_view(&front, Placement::Ortho([0.0, 1.0]), Projection::Third, [42.0, 160.0], None);
    top.hidden_lines = true;
    let top = place(&mut d, HANDLE_SHEET, top, HANDLE_TOP, ph)?;
    let mut iso = projected_view(&front, Placement::Iso(1.0, 1.0), Projection::Third, [215.0, 165.0], None);
    iso.scale = Scale::new(1, 4);
    iso.scale_inherited = false;
    iso.shaded = true;
    iso.anchor = [205.0, 178.0];
    place(&mut d, HANDLE_SHEET, iso, HANDLE_ISO, ph)?;
    let f = Vg { g: project(&features, &front, PLATE)?, v: front };
    let tp = Vg { g: project(&features, &top, PLATE)?, v: top };
    let m = BEFORE;
    let p = MainGeometry::of(m);
    let xy = |q: cadrs_sketch::Vec2| [q.x, q.y];
    let left = f.vline("left edge", 0.0, -m.bar / 2.0)?;
    let top_edge = f.hline("top edge", 0.0, 10.0)?;
    let large = f.circle("large hole", xy(p.c1), super::LARGE_HOLE / 2.0)?;
    let end_hole = f.circle("end hole", xy(p.c2), m.end_hole / 2.0)?;
    let small = f.circle("small circle", xy(p.c0), m.end_hole / 2.0)?;
    let holes = [
        f.circle("first hole", xy(p.holes[0]), super::SMALL_HOLE / 2.0)?,
        f.circle("middle hole", xy(p.holes[1]), super::SMALL_HOLE / 2.0)?,
        f.circle("last hole", xy(p.holes[2]), super::SMALL_HOLE / 2.0)?,
    ];
    let end_arc = f.arc("R16 arc", xy(p.c2), super::END_R)?;
    let fillet = f.find("R30 fillet", |s| matches!(*s, Shape::Circle { radius, arc: Some(_), .. } if near(radius, super::FILLET_R)))?;
    let fc = fillet.shape.circle().map(|(c, _)| c).unwrap_or([0.0, 0.0]);
    let mut anns = vec![
        distance(DIM_LENGTH, end(left, [0.0, 0.0]), centre(large), Orient::Horizontal, [m.length / 2.0, 22.0]),
        distance(DIM_BAR, end(left, [0.0, -m.bar]), end(left, [0.0, 0.0]), Orient::Vertical, [-24.0, -m.bar / 2.0]),
        dim(DIM_FILLET, DimKind::Radius(fillet), [fc[0] + 40.0, 26.0], ""),
        dim(DIM_LARGE, DimKind::Diameter(large), [p.c1.x + 66.0, p.c1.y - 12.0], ""),
        distance(DIM_HEIGHT, Pick::Edge(top_edge), centre(end_hole), Orient::Vertical, [p.c2.x + 76.0, -40.0]),
        dim(DIM_END_R, DimKind::Radius(end_arc), [p.c2.x + 40.0, p.c2.y - 26.0], ""),
        dim(DIM_HOLES, DimKind::Diameter(holes[0]), [p.holes[0].x + 14.0, -m.bar - 16.0], "3x "),
        dim(DIM_SMALL, DimKind::Diameter(small), [p.c0.x - 50.0, p.c0.y - 14.0], "2x "),
        distance(DIM_75, centre(small), centre(end_hole), Orient::Horizontal, [(p.c0.x + p.c2.x) / 2.0, p.c2.y - 30.0]),
        centermark(CENTERMARK_SMALL, small),
    ];
    for (i, h) in holes.iter().chain([&large, &end_hole]).enumerate() {
        anns.push(centermark(CENTERMARKS[i], *h));
    }
    set_annotations(&mut d, HANDLE_FRONT, anns)?;
    // The Top view: the plate's thickness, and the centerlines of the large hole and the small
    // circle (between their hidden sides).
    let back = tp.hline("back face", 0.0, 10.0)?;
    let front_face = tp.hline("front face", -THICKNESS, 10.0)?;
    let side = |x: f64| tp.vline("a hole's side", x, -THICKNESS / 2.0);
    let (r0, r1) = (m.end_hole / 2.0, super::LARGE_HOLE / 2.0);
    let anns = vec![
        distance(DIM_THICKNESS, Pick::Edge(front_face), Pick::Edge(back), Orient::Aligned, [-14.0, -24.0]),
        lines_centerline(CENTERLINE_SMALL, side(p.c0.x - r0)?, side(p.c0.x + r0)?, [0.0, 0.0]),
        lines_centerline(CENTERLINE_LARGE, side(p.c1.x - r1)?, side(p.c1.x + r1)?, [0.0, 0.0]),
    ];
    set_annotations(&mut d, HANDLE_TOP, anns)?;

    // ---- Grip: the grip at 1:1.
    let grip = ObjectRef { element: studio.0, part: part_key(Some(GRIP)) };
    let gh = source.hash_of(grip.part);
    let mut front = View::base(grip, NamedView::Front, Scale::new(1, 1), [52.0, 104.0]);
    front.hidden_lines = false;
    // The grip's two half cylinders meet in smooth edges along it: not drawn.
    front.tangent_edges = cadrs_drawing::style::TangentEdges::Hidden;
    let front = place(&mut d, GRIP_SHEET, front, GRIP_FRONT, gh)?;
    let mut top = projected_view(&front, Placement::Ortho([0.0, 1.0]), Projection::Third, [52.0, 162.0], None);
    top.hidden_lines = true;
    let top = place(&mut d, GRIP_SHEET, top, GRIP_TOP, gh)?;
    let mut iso = projected_view(&front, Placement::Iso(1.0, 1.0), Projection::Third, [215.0, 175.0], None);
    iso.scale = Scale::new(1, 4);
    iso.scale_inherited = false;
    iso.shaded = true;
    iso.anchor = [226.0, 192.0];
    place(&mut d, GRIP_SHEET, iso, GRIP_ISO, gh)?;
    let f = Vg { g: project(&features, &front, GRIP)?, v: front };
    let tp = Vg { g: project(&features, &top, GRIP)?, v: top };
    let r = GRIP_D / 2.0;
    // The slot's walls meeting the cylinder (the grip's outline along its length; the lip's
    // outline, the whole Ø25, has no model edge: the Ø25 is read off its end circle).
    let wall = (r * r - (THICKNESS / 2.0) * (THICKNESS / 2.0)).sqrt();
    let (upper_wall, lower_wall) = (f.hline("slot's top edge", GRIP_Z + wall, 50.0)?, f.hline("slot's bottom edge", GRIP_Z - wall, 50.0)?);
    let left_end = f.vline("grip's left end", -GRIP_FIRST, GRIP_Z)?;
    let right_end = f.vline("grip's right end", super::GRIP_SECOND_BEFORE, GRIP_Z)?;
    let spec = super::hole_spec(super::HOLE_SIZE_BEFORE);
    let hole = |x: f64| f.circle("a grip hole", [x, GRIP_Z], spec.diameter.value / 2.0);
    let (g1, g2, g3) = (hole(GRIP_HOLES[0])?, hole(GRIP_HOLES[1])?, hole(GRIP_HOLES[2])?);
    let anns = vec![
        dim(
            DIM_GRIP_D,
            DimKind::Distance {
                a: end(f.vline("grip's left end (bottom)", -GRIP_FIRST, GRIP_Z - r + 1.0)?, [-GRIP_FIRST, GRIP_Z - r]),
                b: end(f.vline("grip's left end (top)", -GRIP_FIRST, GRIP_Z + r - 1.0)?, [-GRIP_FIRST, GRIP_Z + r]),
                orient: Orient::Vertical,
            },
            // Beside the dimension line, at its middle (`ex3-step11.png`).
            [-GRIP_FIRST - 20.0, GRIP_Z],
            "Ø",
        ),
        distance(DIM_GRIP_LENGTH, Pick::Edge(left_end), Pick::Edge(right_end), Orient::Aligned, [70.0, GRIP_Z - r - 16.0]),
        distance(DIM_GRIP_35, Pick::Edge(left_end), centre(g1), Orient::Horizontal, [7.5, 14.0]),
        distance(DIM_GRIP_62, centre(g1), centre(g2), Orient::Horizontal, [(GRIP_HOLES[0] + GRIP_HOLES[1]) / 2.0, 14.0]),
        Annotation {
            id: CALLOUT_GRIP,
            kind: AnnotationKind::HoleCallout(HoleCallout { edge: g3, text: [GRIP_HOLES[2] + 12.0, 20.0], prefix: "3x".into(), last: None }),
        },
        lines_centerline(CENTERLINE_GRIP, upper_wall, lower_wall, [0.0, 0.0]),
    ];
    set_annotations(&mut d, GRIP_FRONT, anns)?;
    let outer = tp.hline("grip's front silhouette", GRIP_Y - r, 50.0)?;
    let (slot_front, slot_back) = (tp.hline("slot's front wall", -THICKNESS, 50.0)?, tp.hline("slot's back wall", 0.0, 50.0)?);
    let anns = vec![
        distance(DIM_SLOT, Pick::Edge(slot_front), Pick::Edge(slot_back), Orient::Aligned, [195.0, -24.0]),
        distance(DIM_SLOT_WALL, Pick::Edge(outer), Pick::Edge(slot_front), Orient::Aligned, [-GRIP_FIRST - 20.0, -30.0]),
    ];
    set_annotations(&mut d, GRIP_TOP, anns)?;
    // The first sheet shown is Handle's neighbour, Assembly; its reference is none.
    Ok(d)
}
