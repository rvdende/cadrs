//! **Export DXF/DWG of flat pattern** (P3I.6, SM15; `reference/onshape/sheetmetal/help/`
//! `feature-tools/sheetmetal-export-01-03.png`, `raw/help-sheet_metal_table.txt` "Exporting
//! DXF/DWG of flat pattern"): a sheet metal part's flat pattern as a DXF page in millimetres at
//! full size, written by the drawings' DXF writer ([`cadrs_drawing::dxf`]), with one layer per
//! kind of line:
//!
//! - `OUTLINE`: the outer boundary of the flat; `CUTOUTS`: its holes (round ones as CIRCLE);
//! - `TEAR_SLITS`: Tear reliefs' slits (cut lines with no width);
//! - `BEND_UP` / `BEND_DOWN`: the bend centrelines over material, by direction (both in the
//!   CENTER linetype, green and red) — *Include bend centerlines*;
//! - `BEND_TANGENT`: the bend tangent lines — *Include bend tangent lines*;
//! - `CBORE_CSINK`: the outer diameters of counterbored and countersunk holes in the part's
//!   walls (circles at the holes' centres) — *Include counterbore and countersink lines*;
//! - `FLAT_SKETCH`: the visible sketches on the flat pattern — *Include visible sketches*; their
//!   splines are SPLINE entities, or polylines with *Export splines as polylines*;
//! - `FORM_OUTLINES`: the outlines of the forms placed on the part (P3I.9, SM20.3: the form's
//!   construction-only Tag sketch, or else its footprint; a round one as CIRCLE) — *Include form
//!   feature outlines*;
//! - `FORM_CENTERMARKS`: a centermark (a cross) at each form's origin — *Include form feature
//!   centermarks*.
//!
//! *Set z-height to zero and normals to positive* holds always (the writer writes 2D entities
//! with z 0 and the default +Z normal). The file's layer table lists only the layers it uses.
//!
//! Several parts (scope *All flat pattern parts in the current model* or *in the Part Studio*)
//! are laid out side by side in one file, [`GAP`] apart, or written as one file each.

use cadrs_drawing::export::{Item, Layer, Page, Pen, Shape};
use cadrs_sheetmetal::flat::FlatPart;
use cadrs_sheetmetal::poly::{P2, Seg2};
use cadrs_sketch::{PlaneRef, Sketch};

use crate::document::{Feature, PartProps};
use crate::ids::{FeatureId, PartId};
use crate::rebuild::Build;
use crate::sheetmetal_flat::flat_target;

/// Line width written (mm).
const WIDTH: f64 = 0.25;

/// The space between parts laid out side by side (mm).
pub const GAP: f64 = 20.0;

/// The dialog's options (SM15.3), with Onshape's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatExportOptions {
    pub splines_as_polylines: bool,
    pub z_zero: bool,
    pub centerlines: bool,
    pub tangent_lines: bool,
    pub cbore_lines: bool,
    pub form_outlines: bool,
    pub form_centermarks: bool,
    pub sketches: bool,
}

impl Default for FlatExportOptions {
    fn default() -> Self {
        Self {
            splines_as_polylines: false,
            z_zero: true,
            centerlines: true,
            tangent_lines: false,
            cbore_lines: false,
            form_outlines: false,
            form_centermarks: false,
            sketches: true,
        }
    }
}

/// The dialog's Scope (SM15.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlatScope {
    /// Single flat pattern part only.
    #[default]
    Single,
    /// All flat pattern parts in the current model.
    Model,
    /// All flat pattern parts in the Part Studio.
    Studio,
}

impl FlatScope {
    pub const ALL: [FlatScope; 3] = [FlatScope::Single, FlatScope::Model, FlatScope::Studio];

    pub fn label(self) -> &'static str {
        match self {
            FlatScope::Single => "Single flat pattern part only",
            FlatScope::Model => "All flat pattern parts in the current model",
            FlatScope::Studio => "All flat pattern parts in the Part Studio",
        }
    }
}

/// Onshape's default file name: "<document> - Flat pattern of <part>".
pub fn default_file_name(document: &str, part: &str) -> String {
    format!("{document} - Flat pattern of {part}")
}

fn pen(layer: Layer) -> Pen {
    Pen::new(WIDTH, layer)
}

/// A loop's circle (centre, radius) when it is a circle's polygon: 12 or more points the same
/// distance from their centroid, evenly spread round it.
fn circle_of(l: &[P2]) -> Option<(P2, f64)> {
    if l.len() < 12 {
        return None;
    }
    let c = P2::from(l.iter().fold(nalgebra::Vector2::zeros(), |a, p| a + p.coords) / l.len() as f64);
    let r = l.iter().map(|p| (p - c).norm()).sum::<f64>() / l.len() as f64;
    let even = l.iter().all(|p| ((p - c).norm() - r).abs() <= 1e-6 * r.max(1.0));
    let step = std::f64::consts::TAU / l.len() as f64;
    let spread = (0..l.len()).all(|i| {
        let (a, b) = (l[i] - c, l[(i + 1) % l.len()] - c);
        let ang = a.perp(&b).atan2(a.dot(&b)).abs();
        (ang - step).abs() <= 0.25 * step
    });
    (even && spread).then_some((c, r))
}

/// The circle through three points (centre, radius), if they aren't in line.
fn circle3(a: P2, b: P2, c: P2) -> Option<(P2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.coords.norm_squared(), b.coords.norm_squared(), c.coords.norm_squared());
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let o = P2::new(ux, uy);
    Some((o, (a - o).norm()))
}

/// Whether points `l[i..=j]` (indices wrap) lie on one circular arc of small, even steps: the
/// arcs (reliefs, slot ends) the flat's polygons stand for.
fn arc_run(l: &[P2], i: usize, j: usize) -> Option<(P2, f64)> {
    let n = l.len();
    let at = |k: usize| l[k % n];
    let len = j - i;
    if len < 3 {
        return None;
    }
    let (o, r) = circle3(at(i), at(i + len / 2), at(j))?;
    let tol = 1e-6 * r.max(1.0);
    let mut step0 = None;
    for k in i..j {
        let (p, q) = (at(k) - o, at(k + 1) - o);
        if ((p.norm() - r).abs() > tol) || ((q.norm() - r).abs() > tol) {
            return None;
        }
        let step = p.perp(&q).atan2(p.dot(&q));
        if step.abs() > 0.6 {
            return None;
        }
        match step0 {
            None => step0 = Some(step),
            Some(s0) if (step - s0).abs() > 0.25 * s0.abs() + 1e-9 => return None,
            _ => {}
        }
    }
    Some((o, r))
}

/// A loop as DXF entities: a whole circle as CIRCLE; otherwise its straight edges as LINEs and
/// its runs of points on an arc as ARCs (counter-clockwise, as DXF has them).
fn loop_shapes(l: &[P2]) -> Vec<Shape> {
    if let Some((c, r)) = circle_of(l) {
        return vec![Shape::Circle { center: [c.x, c.y], radius: r }];
    }
    // Points in the middle of straight runs (where a cut crossed a piece's edge) dropped.
    let mut l: Vec<P2> = l.to_vec();
    loop {
        let n = l.len();
        let Some(k) = (0..n).find(|&k| {
            let (a, b, c) = (l[(k + n - 1) % n], l[k], l[(k + 1) % n]);
            let (u, v) = (b - a, c - b);
            n > 3 && u.perp(&v).abs() <= 1e-9 * u.norm() * v.norm() && u.dot(&v) > 0.0
        }) else {
            break;
        };
        l.remove(k);
    }
    let l = &l[..];
    let n = l.len();
    if n < 3 {
        return Vec::new();
    }
    // Start at a corner (the sharpest turn) or where a line meets an arc (the steps change
    // length most), so no arc is split by the loop's start.
    let turn = |k: usize| {
        let (a, b, c) = (l[(k + n - 1) % n], l[k], l[(k + 1) % n]);
        let (u, v) = (b - a, c - b);
        u.perp(&v).atan2(u.dot(&v)).abs() + (u.norm().max(1e-12) / v.norm().max(1e-12)).ln().abs()
    };
    let start = (0..n).max_by(|a, b| turn(*a).total_cmp(&turn(*b))).unwrap_or(0);
    let at = |k: usize| l[(start + k) % n];
    let pts: Vec<P2> = (0..=n).map(at).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        // The longest arc run from i.
        let mut best = None;
        let mut j = i + 3;
        while j <= n {
            match arc_run(&pts, i, j) {
                Some(c) => best = Some((j, c)),
                None if best.is_some() => break,
                None => {}
            }
            if best.is_none() && j > i + 3 {
                break;
            }
            j += 1;
        }
        match best {
            Some((j, (o, r))) => {
                let ang = |p: P2| (p.y - o.y).atan2(p.x - o.x).to_degrees();
                let (p, q) = (pts[i] - o, pts[i + 1] - o);
                let ccw = p.perp(&q) > 0.0;
                let (s, mut e) = if ccw { (ang(pts[i]), ang(pts[j])) } else { (ang(pts[j]), ang(pts[i])) };
                while e <= s {
                    e += 360.0;
                }
                out.push(Shape::Arc { center: [o.x, o.y], radius: r, start: s, end: e });
                i = j;
            }
            None => {
                out.push(Shape::Line { a: [pts[i].x, pts[i].y], b: [pts[i + 1].x, pts[i + 1].y] });
                i += 1;
            }
        }
    }
    out
}

/// A form's centermark: a cross on its centre along the flat's axes, a fifth of the outline's
/// reach each way (1 to 6 mm).
pub fn centermark(f: &cadrs_sheetmetal::forms::FlatForm) -> (Seg2, Seg2) {
    let reach = f.lines.iter().flat_map(|l| l.points.iter()).map(|p| (p - f.center).norm()).fold(0.0, f64::max);
    let h = (0.2 * reach).clamp(1.0, 6.0);
    let c = f.center;
    (Seg2::new(P2::new(c.x - h, c.y), P2::new(c.x + h, c.y)), Seg2::new(P2::new(c.x, c.y - h), P2::new(c.x, c.y + h)))
}

fn line(s: &Seg2) -> Shape {
    Shape::Line { a: [s.a.x, s.a.y], b: [s.b.x, s.b.y] }
}

/// One flat-pattern part as a page, in the flat's coordinates, with `sketches` (on its flat
/// pattern plane, so in the same coordinates) when the options include them.
pub fn flat_page(part: &FlatPart, sketches: &[&Sketch], o: &FlatExportOptions, name: &str) -> Page {
    flat_page_with(part, sketches, &[], o, name)
}

/// A sketch's spline as a DXF SPLINE: its cubic Bézier spans as one clamped cubic B-spline
/// (each inner joint a triple knot), its points the fit points.
fn spline_shape(sketch: &Sketch, id: cadrs_sketch::CurveId) -> Option<Shape> {
    let spans = sketch.spline_spans(id)?;
    if spans.is_empty() {
        return None;
    }
    let n = spans.len();
    let mut control = vec![[spans[0][0].x, spans[0][0].y]];
    for b in &spans {
        control.extend(b[1..].iter().map(|q| [q.x, q.y]));
    }
    let mut knots = vec![0.0; 4];
    for i in 1..n {
        knots.extend([i as f64; 3]);
    }
    knots.extend([n as f64; 4]);
    let fit = sketch.splines.get(id)?.points.iter().map(|p| {
        let q = sketch.pos(*p);
        [q.x, q.y]
    });
    let points = cadrs_sketch::spline::tessellate(&spans, 16).into_iter().map(|q| [q.x, q.y]).collect();
    Some(Shape::Spline { knots, control, fit: fit.collect(), points })
}

/// [`flat_page`] with the outer circles (centre, radius) of counterbored and countersunk
/// holes, written when the options include them ([`cbore_marks`]).
pub fn flat_page_with(part: &FlatPart, sketches: &[&Sketch], cbores: &[(P2, f64)], o: &FlatExportOptions, name: &str) -> Page {
    let mut page = Page { name: name.to_string(), ..Page::default() };
    let mut push = |s: Shape, l: Layer| page.items.push(Item::Stroke(s, pen(l)));
    for poly in &part.outline {
        for sh in loop_shapes(&poly.outer) {
            push(sh, Layer::FlatOutline);
        }
        for h in &poly.holes {
            for sh in loop_shapes(h) {
                push(sh, Layer::FlatCutout);
            }
        }
    }
    for s in part.slits() {
        push(line(&s), Layer::FlatSlit);
    }
    for b in &part.bends {
        if o.centerlines {
            for s in &b.center_visible {
                push(line(s), if b.up { Layer::BendUp } else { Layer::BendDown });
            }
        }
        if o.tangent_lines {
            for s in &b.tangent_visible {
                push(line(s), Layer::BendTangent);
            }
        }
    }
    for f in &part.forms {
        if o.form_outlines {
            for l in &f.lines {
                if l.closed && l.points.len() >= 3 {
                    for sh in loop_shapes(&l.points) {
                        push(sh, Layer::FormOutline);
                    }
                } else {
                    for w in l.points.windows(2) {
                        push(line(&Seg2::new(w[0], w[1])), Layer::FormOutline);
                    }
                }
            }
        }
        if o.form_centermarks {
            let (a, b) = centermark(f);
            push(line(&a), Layer::FormCentermark);
            push(line(&b), Layer::FormCentermark);
        }
    }
    if o.cbore_lines {
        for (c, r) in cbores {
            push(Shape::Circle { center: [c.x, c.y], radius: *r }, Layer::FlatCbore);
        }
    }
    if o.sketches {
        for sk in sketches {
            // Splines as SPLINE entities unless *Export splines as polylines* (the sketch page
            // writes them as polylines).
            let splines: Vec<cadrs_sketch::CurveId> = if o.splines_as_polylines {
                Vec::new()
            } else {
                sk.curves.iter().filter(|(_, c)| !c.construction && matches!(c.kind, cadrs_sketch::CurveKind::Spline { .. })).map(|(k, _)| k).collect()
            };
            let sp = if splines.is_empty() {
                crate::dxf_export::sketch_page(sk, name)
            } else {
                let mut rest = (*sk).clone();
                for k in &splines {
                    rest.curves.remove(*k);
                }
                crate::dxf_export::sketch_page(&rest, name)
            };
            for it in sp.items {
                if let Item::Stroke(s, _) = it {
                    push(s, Layer::FlatSketch);
                }
            }
            for k in splines {
                if let Some(s) = spline_shape(sk, k) {
                    push(s, Layer::FlatSketch);
                }
            }
        }
    }
    crate::dxf_export::fit(&mut page);
    page
}

fn shift(s: &Shape, d: [f64; 2]) -> Shape {
    let m = |p: [f64; 2]| [p[0] + d[0], p[1] + d[1]];
    match s {
        Shape::Line { a, b } => Shape::Line { a: m(*a), b: m(*b) },
        Shape::Polyline { points, closed } => Shape::Polyline { points: points.iter().map(|p| m(*p)).collect(), closed: *closed },
        Shape::Arc { center, radius, start, end } => Shape::Arc { center: m(*center), radius: *radius, start: *start, end: *end },
        Shape::Circle { center, radius } => Shape::Circle { center: m(*center), radius: *radius },
        Shape::Spline { knots, control, fit, points } => Shape::Spline {
            knots: knots.clone(),
            control: control.iter().map(|p| m(*p)).collect(),
            fit: fit.iter().map(|p| m(*p)).collect(),
            points: points.iter().map(|p| m(*p)).collect(),
        },
    }
}

/// Pages laid out side by side in one: the first where it is, each next one to its right,
/// [`GAP`] apart, their bottoms in line.
pub fn side_by_side(pages: &[Page], name: &str) -> Page {
    let mut out = Page { name: name.to_string(), ..Page::default() };
    let mut x = None;
    let mut y0 = 0.0;
    for p in pages {
        let (lo, hi) = cadrs_drawing::dxf::extents(p);
        let d = match x {
            None => {
                y0 = lo[1];
                [0.0, 0.0]
            }
            Some(at) => [at - lo[0], y0 - lo[1]],
        };
        x = Some(hi[0] + d[0] + GAP);
        for it in &p.items {
            if let Item::Stroke(s, pen) = it {
                out.items.push(Item::Stroke(shift(s, d), pen.clone()));
            }
        }
    }
    crate::dxf_export::fit(&mut out);
    out
}

/// A flat-pattern part of a built Part Studio: its model, index in the model's flat, part and
/// name.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatPartRef {
    pub model: FeatureId,
    pub index: usize,
    pub part: PartId,
    pub name: String,
}

/// Every flat-pattern part of a build, in list order, named as the Parts list names them
/// (`props`: the Part Studio's renames).
pub fn flat_parts_named(build: &Build, props: &[PartProps]) -> Vec<FlatPartRef> {
    let mut out = flat_parts(build);
    for r in &mut out {
        if let Some(n) = props.iter().find(|p| p.part == r.part).and_then(|p| p.name.clone()) {
            r.name = n;
        }
    }
    out
}

/// Every flat-pattern part of a build, in list order (with the parts' own names, "Part N"; see
/// [`flat_parts_named`] for renamed parts).
pub fn flat_parts(build: &Build) -> Vec<FlatPartRef> {
    let mut out = Vec::new();
    for ctx in &build.sheet_metal {
        for (k, flat) in ctx.flat.parts.iter().enumerate() {
            let Some((pid, _)) = ctx.parts.iter().find(|(_, ws)| ws.first() == flat.walls.first()) else { continue };
            let Some(p) = build.parts.iter().find(|p| p.id == *pid) else { continue };
            out.push(FlatPartRef { model: ctx.feature, index: k, part: *pid, name: p.name.clone() });
        }
    }
    out
}

/// The parts a scope exports, starting from `part` (a flat-pattern part of the build), named
/// as the Parts list names them.
pub fn scope_parts(build: &Build, props: &[PartProps], part: PartId, scope: FlatScope) -> Vec<FlatPartRef> {
    let all = flat_parts_named(build, props);
    let Some(me) = all.iter().find(|r| r.part == part).cloned() else { return Vec::new() };
    match scope {
        FlatScope::Single => vec![me],
        FlatScope::Model => all.into_iter().filter(|r| r.model == me.model).collect(),
        FlatScope::Studio => all,
    }
}

/// The page of one flat-pattern part, with the sketches on its flat pattern plane that `shown`
/// says are visible.
pub fn part_page(build: &Build, features: &[Feature], r: &FlatPartRef, o: &FlatExportOptions, shown: &dyn Fn(FeatureId) -> bool) -> Option<Page> {
    let ctx = build.sheet_metal.iter().rev().find(|c| c.feature == r.model)?;
    let flat = ctx.flat.parts.get(r.index)?;
    let sketches: Vec<&Sketch> = features
        .iter()
        .filter(|f| shown(f.id))
        .filter_map(|f| {
            let sk = f.sketch()?;
            let PlaneRef::Feature(fp) = sk.plane? else { return None };
            (flat_target(features, fp.feature) == Some((r.model, r.index))).then_some(&sk.geometry)
        })
        .collect();
    let cbores = if o.cbore_lines { cbore_marks(build, features, r) } else { Vec::new() };
    Some(flat_page_with(flat, &sketches, &cbores, o, &r.name))
}

/// The outer circles of counterbored and countersunk holes in a flat-pattern part's planar
/// walls, in the flat's coordinates: the part's circular edges of a Hole feature's counterbore
/// or countersink diameter that lie on a wall's face, square to it, laid out with the wall.
pub fn cbore_marks(build: &Build, features: &[Feature], r: &FlatPartRef) -> Vec<(P2, f64)> {
    use crate::hole::HoleStyle;
    let radii: Vec<f64> = features
        .iter()
        .filter_map(|f| f.hole())
        .filter_map(|h| match h.spec.style {
            HoleStyle::Counterbore => Some(h.spec.cbore_diameter.value / 2.0),
            HoleStyle::Countersink => Some(h.spec.csink_diameter.value / 2.0),
            _ => None,
        })
        .filter(|r| *r > 0.0)
        .collect();
    let mut out: Vec<(P2, f64)> = Vec::new();
    if radii.is_empty() {
        return out;
    }
    let Some(ctx) = build.sheet_metal.iter().rev().find(|c| c.feature == r.model) else { return out };
    let Some(flat) = ctx.flat.parts.get(r.index) else { return out };
    let Some(part) = build.parts.iter().find(|p| p.id == r.part) else { return out };
    let t = ctx.model.params.thickness;
    for e in &part.solid.edges {
        let Some(c) = e.circle.as_ref() else { continue };
        if !radii.iter().any(|q| (q - c.radius).abs() <= 1e-6 * q.max(1.0)) {
            continue;
        }
        let p = nalgebra::Point3::new(c.center[0], c.center[1], c.center[2]);
        let axis = nalgebra::Vector3::new(c.normal[0], c.normal[1], c.normal[2]);
        for w in &flat.walls {
            let Some(wall) = ctx.model.wall(*w) else { continue };
            let cadrs_sheetmetal::model::Surface::Planar { origin, u, v } = wall.surface else { continue };
            let n = u.cross(&v);
            if n.norm() < 1e-12 || axis.norm() < 1e-12 || n.normalize().cross(&axis.normalize()).norm() > 1e-6 {
                continue;
            }
            let n = n.normalize();
            let d = (p - origin).dot(&n);
            // On the wall's definition face or the face a thickness away (either side).
            if d.abs() > t + 1e-6 {
                continue;
            }
            // Local 2D: origin + u·x + v·y (u, v square to each other).
            let q = P2::new((p - origin).dot(&u) / u.norm_squared(), (p - origin).dot(&v) / v.norm_squared());
            if !wall.outline.contains(q) {
                continue;
            }
            let Some(m) = flat.placement(*w) else { continue };
            let at = m.apply(q);
            if !out.iter().any(|(o, rr)| (*o - at).norm() < 1e-6 && (rr - c.radius).abs() < 1e-9) {
                out.push((at, c.radius));
            }
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_drawing::dxf::{DxfVersion, read_dxf};
    use cadrs_drawing::sheet_sketch::Entity;
    use cadrs_sheetmetal::poly::{Polygon, circle};
    use cadrs_sheetmetal::{Params, flatten, samples};

    fn layer_counts(d: &cadrs_drawing::dxf::DxfDrawing) -> std::collections::BTreeMap<String, usize> {
        let mut m = std::collections::BTreeMap::new();
        for l in &d.layers {
            *m.entry(l.clone()).or_insert(0) += 1;
        }
        m
    }

    #[test]
    fn an_open_box_flat_round_trips_through_dxf() {
        let mut m = samples::open_box(Params::default(), Default::default()).unwrap();
        // A round hole and a slot cut in the flat (the slot across a bend).
        let flat = flatten(&m);
        let b = &flat.parts[0].bends[0];
        let c = P2::from((b.center.a.coords + b.center.b.coords) / 2.0);
        let (lo, hi) = flat.parts[0].bounds().unwrap();
        let mid = P2::from((lo.coords + hi.coords) / 2.0);
        let d = b.center.dir();
        let n = cadrs_sheetmetal::poly::perp(d);
        let slot = Polygon::new(vec![c - d * 2.0 - n * 8.0, c + d * 2.0 - n * 8.0, c + d * 2.0 + n * 8.0, c - d * 2.0 + n * 8.0]);
        cadrs_sheetmetal::flat_edit::remove(&mut m, &flat, 0, &[circle(mid, 5.0, 64), slot]).unwrap();
        let flat = flatten(&m);
        assert!(flat.is_ok(), "{:?}", flat.errors);
        let part = &flat.parts[0];
        let o = FlatExportOptions { tangent_lines: true, ..Default::default() };
        let page = flat_page(part, &[], &o, "Box");
        for v in DxfVersion::ALL {
            let text = cadrs_drawing::dxf::write_dxf_version(&page, v);
            let back = read_dxf(&text).unwrap();
            assert_eq!(back.unit_mm, 1.0);
            let counts = layer_counts(&back);
            let holes: usize = part.outline.iter().map(|p| p.holes.len()).sum();
            let up: usize = part.bends.iter().filter(|b| b.up).map(|b| b.center_visible.len()).sum();
            let down: usize = part.bends.iter().filter(|b| !b.up).map(|b| b.center_visible.len()).sum();
            let tangent: usize = part.bends.iter().map(|b| b.tangent_visible.len()).sum();
            // One outline of lines (and arcs), two cut-outs: the round hole a CIRCLE, the slot
            // four LINEs.
            assert_eq!(part.outline.len(), 1);
            assert!(counts.get("OUTLINE").copied().unwrap_or(0) >= 4);
            assert_eq!(counts.get("CUTOUTS").copied().unwrap_or(0), 5);
            assert_eq!(holes, 2);
            assert_eq!(counts.get("BEND_UP").copied().unwrap_or(0), up);
            assert_eq!(counts.get("BEND_DOWN").copied().unwrap_or(0), down);
            assert_eq!(counts.get("BEND_TANGENT").copied().unwrap_or(0), tangent);
            // Four bends; the first broken by the slot into two pieces.
            assert_eq!(up + down, 5);
            // The round hole is a CIRCLE.
            assert!(back.entities.iter().zip(&back.layers).any(|(e, l)| l == "CUTOUTS" && matches!(e, Entity::Circle { radius, .. } if (radius - 5.0).abs() < 1e-9)));
            // Extents: the flat's.
            let (mut elo, mut ehi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for (e, l) in back.entities.iter().zip(&back.layers) {
                if l != "OUTLINE" {
                    continue;
                }
                let points = match e {
                    Entity::Line { a, b } => vec![*a, *b],
                    Entity::Arc { center, radius, start, end } => cadrs_drawing::sheet_sketch::arc_polyline(*center, *radius, *start, *end),
                    e => panic!("{e:?}"),
                };
                for p in &points {
                    for k in 0..2 {
                        elo[k] = elo[k].min(p[k]);
                        ehi[k] = ehi[k].max(p[k]);
                    }
                }
            }
            assert!((elo[0] - lo.x).abs() < 1e-9 && (elo[1] - lo.y).abs() < 1e-9 && (ehi[0] - hi.x).abs() < 1e-9 && (ehi[1] - hi.y).abs() < 1e-9, "{elo:?} {ehi:?} vs {lo:?} {hi:?}");
        }
        // Options off: no bend lines.
        let bare = flat_page(part, &[], &FlatExportOptions { centerlines: false, ..Default::default() }, "Box");
        let back = read_dxf(&cadrs_drawing::dxf::write_dxf(&bare)).unwrap();
        assert!(!back.layers.iter().any(|l| l.starts_with("BEND")));
    }

    #[test]
    fn parts_lay_out_side_by_side_gap_apart() {
        let p = Params::default();
        let a = flat_page(&flatten(&samples::l_bracket(p, true).unwrap()).parts[0], &[], &Default::default(), "A");
        let b = flat_page(&flatten(&samples::u_channel(p).unwrap()).parts[0], &[], &Default::default(), "B");
        let (alo, ahi) = cadrs_drawing::dxf::extents(&a);
        let (blo, bhi) = cadrs_drawing::dxf::extents(&b);
        let both = side_by_side(&[a.clone(), b.clone()], "Both");
        assert_eq!(both.items.len(), a.items.len() + b.items.len());
        let (lo, hi) = cadrs_drawing::dxf::extents(&both);
        assert!((lo[0] - alo[0]).abs() < 1e-9 && (lo[1] - alo[1]).abs() < 1e-9);
        let width = (ahi[0] - alo[0]) + GAP + (bhi[0] - blo[0]);
        assert!((hi[0] - lo[0] - width).abs() < 1e-9, "{} vs {width}", hi[0] - lo[0]);
    }

    #[test]
    fn arcs_in_loops_come_back_as_arcs() {
        // A 20 × 6 slot: two half circles of 16 steps joined by lines.
        let mut l = Vec::new();
        for (cx, a0) in [(7.0, -std::f64::consts::FRAC_PI_2), (-7.0, std::f64::consts::FRAC_PI_2)] {
            for k in 0..=16 {
                let a: f64 = a0 + std::f64::consts::PI * k as f64 / 16.0;
                l.push(P2::new(cx + 3.0 * a.cos(), 3.0 * a.sin()));
            }
        }
        let shapes = loop_shapes(&l);
        let arcs: Vec<&Shape> = shapes.iter().filter(|s| matches!(s, Shape::Arc { .. })).collect();
        let lines = shapes.iter().filter(|s| matches!(s, Shape::Line { .. })).count();
        assert_eq!((arcs.len(), lines), (2, 2), "{shapes:?}");
        for a in arcs {
            let Shape::Arc { radius, start, end, .. } = a else { unreachable!() };
            assert!((radius - 3.0).abs() < 1e-9 && ((end - start) - 180.0).abs() < 1e-6, "{a:?}");
        }
        // A rectangle stays four lines.
        let r = [P2::new(0.0, 0.0), P2::new(4.0, 0.0), P2::new(4.0, 2.0), P2::new(0.0, 2.0)];
        assert_eq!(loop_shapes(&r).len(), 4);
    }

    #[test]
    fn the_file_name_follows_onshape() {
        assert_eq!(default_file_name("Louver-n sheet metal", "Part 1"), "Louver-n sheet metal - Flat pattern of Part 1");
    }
}
