//! Onshape sketches (`newSketch`) to cadrs sketches.
//!
//! Geometry comes from `sketches.json` (the solved sketch: every entity's position, in metres,
//! on Onshape's sketch plane); entity kinds, construction flags, constraints and dimensions
//! from the feature in `features.json`. The geometry is added curve by curve (so each Onshape
//! entity maps to the cadrs curve it made), then the constraints in one batch, then the
//! dimensions, measured from the geometry so they drive it where it already is.
//!
//! Onshape's sketch coordinates are mapped through the world (its `sketchMatrix`) into the
//! frame cadrs gives the sketch plane, so sketches on faces land right even where the two
//! programs pick different in-plane axes.

use std::collections::HashMap;
use std::f64::consts::TAU;

use cadrs_core::command::CommandError;
use cadrs_core::commands::{AddSketch, EditSketch, RenameFeature};
use cadrs_core::ids::{ElementId, FeatureId};
use cadrs_sketch::constraint::{ConstraintOf, ConstraintSpec, CurveSpec, Orient, PointSpec};
use cadrs_sketch::{
    CurveId, CurveKind, CurveRef, Dimension, DimensionKind, PlaneFrame, PlaneRef, PointId, PointRef, Sketch, SketchOp,
    Vec2, Vec3,
};
use serde_json::Value;

use crate::report::{FeatureReport, Outcome};
use crate::studio::Studio;

/// Onshape's deterministic ids for the default geometry.
pub const ORIGIN_ID: &str = "IB";
pub const TOP_ID: &str = "JDC";
pub const FRONT_ID: &str = "JCC";
pub const RIGHT_ID: &str = "JEC";

/// How an imported sketch's Onshape ids map to cadrs.
#[derive(Debug, Clone, Default)]
pub struct SketchMap {
    pub feature: FeatureId,
    /// Onshape entity id → cadrs curve.
    pub curves: HashMap<String, CurveId>,
    /// Onshape point id (`<entity>.start`, `.end`, `.center`, or a sketch point's id) → cadrs
    /// point.
    pub points: HashMap<String, PointId>,
    /// Arcs and circles Onshape runs clockwise in cadrs's sketch coordinates (cadrs stores them
    /// counter-clockwise): their left is outward, for the sides region queries name.
    pub reversed: std::collections::HashSet<CurveId>,
}

/// A 2D affine map from Onshape sketch coordinates (m) to cadrs sketch coordinates (mm).
#[derive(Debug, Clone, Copy)]
pub struct Xf {
    /// Onshape's sketch frame in the world (mm): origin, x axis, y axis.
    o: Vec3,
    x: Vec3,
    y: Vec3,
    /// cadrs's frame.
    to: PlaneFrame,
}

impl Xf {
    fn world(&self, p: [f64; 2]) -> Vec3 {
        let (a, b) = (p[0] * 1000.0, p[1] * 1000.0);
        [0, 1, 2].map(|i| self.o[i] + a * self.x[i] + b * self.y[i])
    }

    /// An Onshape sketch point (m) in cadrs sketch coordinates (mm).
    pub fn apply(&self, p: [f64; 2]) -> Vec2 {
        let w = self.world(p);
        let d = sub(w, self.to.origin);
        Vec2::new(dot(d, self.to.u), dot(d, self.to.v))
    }

    /// The sketch plane's world normal and origin (mm).
    pub fn plane(&self) -> (Vec3, Vec3) {
        (cross(self.x, self.y), self.o)
    }
}

/// Onshape's `sketchMatrix` (row-major 4×4, metres) as origin and axes in mm.
fn onshape_frame(m: &[f64]) -> Option<(Vec3, Vec3, Vec3)> {
    if m.len() != 16 {
        return None;
    }
    let col = |j: usize| [m[j], m[4 + j], m[8 + j]];
    let o = col(3).map(|v| v * 1000.0);
    Some((o, col(0), col(1)))
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn v2(v: &Value) -> Option<[f64; 2]> {
    Some([v["x"].as_f64()?, v["y"].as_f64()?])
}

/// The solved entities of one sketch from `sketches.json`.
pub struct Solved<'a> {
    pub matrix: Vec<f64>,
    entities: Vec<&'a Value>,
    by_id: HashMap<&'a str, &'a Value>,
}

impl<'a> Solved<'a> {
    pub fn find(sketches: &'a Value, feature_id: &str) -> Option<Self> {
        let s = sketches["sketches"].as_array()?.iter().find(|s| s["featureId"].as_str() == Some(feature_id))?;
        let matrix = s["sketchMatrix"].as_array()?.iter().filter_map(Value::as_f64).collect();
        let entities: Vec<&Value> = s["entities"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        let by_id = entities.iter().filter_map(|e| Some((e["sketchEntityId"].as_str()?, *e))).collect();
        Some(Self { matrix, entities, by_id })
    }

    fn point(&self, id: &str) -> Option<[f64; 2]> {
        v2(&self.by_id.get(id)?["position2d"])
    }
}

/// The plane an Onshape sketch lies on, as the default plane it names, if it is one (by
/// deterministic id or, in older documents that have none, by query: `Top.planeOp`, …).
pub fn default_plane(feature: &Value) -> Option<PlaneRef> {
    let p = param(feature, "sketchPlane")?;
    let ids = p["queries"][0]["deterministicIds"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    crate::refs::Pick::Opaque(ids).default_plane().or_else(|| crate::refs::picks(Some(p)).first()?.default_plane())
}

/// A feature parameter by id.
pub fn param<'a>(feature: &'a Value, id: &str) -> Option<&'a Value> {
    feature["parameters"].as_array()?.iter().find(|p| p["parameterId"].as_str() == Some(id))
}

/// The sketch's points (curve ends, centres, sketch points) in world coordinates (mm).
pub fn world_points(solved: &Solved) -> Vec<Vec3> {
    let Some((o, x, y)) = onshape_frame(&solved.matrix) else { return Vec::new() };
    let to = PlaneFrame { origin: o, u: x, v: y };
    let xf = Xf { o, x, y, to };
    solved.entities.iter().filter_map(|e| v2(&e["position2d"])).map(|p| xf.world(p)).collect()
}

/// The world plane (normal, point) a sketch lies on, from its solved matrix.
pub fn world_plane(solved: &Solved) -> Option<(Vec3, Vec3)> {
    let (o, x, y) = onshape_frame(&solved.matrix)?;
    Some((cross(x, y), o))
}

/// Imports the sketch `feature` onto `plane` as `id`. Returns the id maps.
#[allow(clippy::too_many_arguments)]
pub fn import(
    s: &mut dyn Studio,
    el: ElementId,
    id: FeatureId,
    feature: &Value,
    solved: &Solved,
    plane: PlaneRef,
    parts: &[cadrs_core::parts::Part],
    model_points: &HashMap<String, Vec<[f64; 3]>>,
    report: &mut FeatureReport,
) -> Result<SketchMap, CommandError> {
    let trace = |what: &str| {
        if std::env::var_os("CADRS_ONSHAPE_TRACE").is_some() {
            eprintln!("    sketch: {what}");
        }
    };
    trace("add");
    s.run(&AddSketch { element: el, feature: id, plane: Some(plane) })?;
    trace("added");
    if let Some(name) = feature["name"].as_str() {
        s.run(&RenameFeature { element: el, feature: id, name: name.to_string() })?;
    }
    let (o, x, y) = onshape_frame(&solved.matrix).ok_or_else(|| CommandError::Invalid("sketch without a matrix".into()))?;
    let xf = Xf { o, x, y, to: plane.frame() };
    let mut map = SketchMap { feature: id, ..Default::default() };

    // Geometry, curve by curve.
    let construction_of: HashMap<&str, bool> = feature["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e["entityId"].as_str()?, e["isConstruction"].as_bool().unwrap_or(false))))
        .collect();
    let mut endpoint_ids = std::collections::HashSet::new();
    let mut skipped: HashMap<String, usize> = HashMap::new();
    for e in &solved.entities {
        let Some(eid) = e["sketchEntityId"].as_str() else { continue };
        let construction = e["isConstruction"].as_bool().or_else(|| construction_of.get(eid).copied()).unwrap_or(false);
        let g = &e["geometry"];
        let mut reversed = false;
        let op = match e["sketchEntityType"].as_str().unwrap_or_default() {
            "skLineSegment" => {
                let (Some(a), Some(b)) = (
                    e["startPointId"].as_str().and_then(|p| solved.point(p)),
                    e["endPointId"].as_str().and_then(|p| solved.point(p)),
                ) else {
                    continue;
                };
                endpoint_ids.extend(e["startPointId"].as_str().into_iter().chain(e["endPointId"].as_str()).map(String::from));
                SketchOp::AddPolyline { points: vec![xf.apply(a), xf.apply(b)], closed: false, construction, label: "Add line" }
            }
            "skCircle" => {
                let (Some(c), Some(r)) = (v2(&g["center2d"]), g["radius"].as_f64()) else { continue };
                endpoint_ids.extend(e["centerId"].as_str().map(String::from));
                reversed = g["clockWise"].as_bool().unwrap_or(false);
                SketchOp::AddCircle { center: xf.apply(c), radius: r * 1000.0, construction }
            }
            "skArc" => {
                let (Some(c), Some(a), Some(b)) = (
                    v2(&g["center2d"]),
                    e["startPointId"].as_str().and_then(|p| solved.point(p)),
                    e["endPointId"].as_str().and_then(|p| solved.point(p)),
                ) else {
                    continue;
                };
                endpoint_ids.extend(
                    ["startPointId", "endPointId", "centerId"].iter().filter_map(|k| e[*k].as_str()).map(String::from),
                );
                let (t0, t1) = (e["startParameter"].as_f64().unwrap_or(0.0), e["endParameter"].as_f64().unwrap_or(0.0));
                // The arc's middle, in Onshape's coordinates: its parameter is the angle from
                // the start's, counter-clockwise, or clockwise for a `clockWise` arc (a mirrored
                // one). The span alone can't tell the two halves of a half circle apart.
                let dir = if g["clockWise"].as_bool().unwrap_or(false) { -1.0 } else { 1.0 };
                let (r, at_start) = (((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2)).sqrt(), (a[1] - c[1]).atan2(a[0] - c[0]));
                let tm = at_start + dir * (t1 - t0) / 2.0;
                let mid = xf.apply([c[0] + r * tm.cos(), c[1] + r * tm.sin()]);
                let (c, a, b) = (xf.apply(c), xf.apply(a), xf.apply(b));
                // cadrs arcs run counter-clockwise from start to end: the order whose sweep
                // passes through the middle.
                let ccw = |p: Vec2, q: Vec2| {
                    let t = (q.y - c.y).atan2(q.x - c.x) - (p.y - c.y).atan2(p.x - c.x);
                    t.rem_euclid(TAU)
                };
                let forward = ccw(a, mid) <= ccw(a, b);
                let (start, end) = if forward { (a, b) } else { (b, a) };
                reversed = !forward;
                SketchOp::AddArc { center: c, start, end, construction }
            }
            "skInterpolatedSpline" | "skInterpolatedSplineSegment" => {
                let pts: Vec<Vec2> = g["interpolationPoints2d"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(v2)
                    .map(|p| xf.apply(p))
                    .collect();
                let periodic = g["isPeriodic"].as_bool().unwrap_or(false);
                // Derivatives (m per unit parameter) map through the frame's linear part; a
                // zero one is a free (natural) end.
                let o = xf.apply([0.0, 0.0]);
                let deriv = |k: &str| {
                    v2(&g[k]).filter(|d| d[0] != 0.0 || d[1] != 0.0).map(|d| xf.apply(d) - o)
                };
                endpoint_ids.extend(e["startPointId"].as_str().into_iter().chain(e["endPointId"].as_str()).map(String::from));
                SketchOp::AddSpline {
                    points: pts,
                    periodic,
                    start_tangent: if periodic { None } else { deriv("startDerivative2d") },
                    end_tangent: if periodic { None } else { deriv("endDerivative2d") },
                    construction,
                }
            }
            "skSpline" => {
                // A control-point B-spline (a projected edge, say): only its features.json
                // entity has the geometry. Brought in as an interpolated spline through points
                // sampled on it.
                let Some(op) = bspline_entity(feature, eid).and_then(|b| sampled_bspline(b, &xf, construction)) else {
                    *skipped.entry("skSpline (unreadable geometry)".to_string()).or_default() += 1;
                    continue;
                };
                let note = "skSpline (control-point spline) imported as an interpolated spline through points on it";
                if !report.notes.iter().any(|n| n == note) {
                    report.notes.push(note.into());
                }
                op
            }
            "skPoint" => continue,
            other => {
                *skipped.entry(other.to_string()).or_default() += 1;
                continue;
            }
        };
        trace(&format!("entity {eid}"));
        let before: std::collections::HashSet<CurveId> = sketch_of(s, el, id)?.curves.keys().collect();
        match s.run(&EditSketch { element: el, feature: id, op }) {
            Ok(()) => {
                let after = sketch_of(s, el, id)?;
                if let Some(c) = after.curves.keys().find(|c| !before.contains(c)) {
                    map.curves.insert(eid.to_string(), c);
                    if reversed {
                        map.reversed.insert(c);
                    }
                }
            }
            Err(err) => report.notes.push(format!("entity {eid}: {err}")),
        }
    }
    // Standalone sketch points (not the ends or centres of curves).
    for e in &solved.entities {
        let (Some(eid), Some("skPoint")) = (e["sketchEntityId"].as_str(), e["sketchEntityType"].as_str()) else { continue };
        if endpoint_ids.contains(eid) {
            continue;
        }
        let Some(p) = v2(&e["position2d"]) else { continue };
        let p = xf.apply(p);
        if nearest_point(&sketch_of(s, el, id)?, p).is_none() {
            s.run(&EditSketch { element: el, feature: id, op: SketchOp::AddPoint { pos: p } }).ok();
        }
    }
    for (k, n) in skipped {
        report.notes.push(format!("{n} × {k} not imported"));
    }

    // Every Onshape point id to the cadrs point where it landed.
    let g = sketch_of(s, el, id)?;
    for e in &solved.entities {
        let (Some(eid), Some("skPoint")) = (e["sketchEntityId"].as_str(), e["sketchEntityType"].as_str()) else { continue };
        if let Some(p) = v2(&e["position2d"]).and_then(|p| nearest_point(&g, xf.apply(p))) {
            map.points.insert(eid.to_string(), p);
        }
    }

    // Where Onshape's x axis points in cadrs's sketch frame (the frames can differ on a face).
    let (p0, p1) = (xf.apply([0.0, 0.0]), xf.apply([0.001, 0.0]));
    let (ax, ay) = ((p1.x - p0.x).abs(), (p1.y - p0.y).abs());
    let swap = if ay < 1e-9 * ax.max(1.0) {
        Some(false)
    } else if ax < 1e-9 * ay.max(1.0) {
        Some(true)
    } else {
        None
    };
    // Constraints and dimensions.
    // Model edges the constraints refer to (Onshape keeps them as bare topology ids): found by
    // where the constrained points are, and brought in as construction Use curves.
    let vertices = model_vertices(feature, &map, &g, &plane, parts);
    let (uses, unmatched) = model_edges(s, el, id, feature, &map, &g, &plane, parts, &vertices, model_points, swap, report)?;
    let projected = projected_vertices(s, el, id, feature, &map, &plane, swap, parts, model_points, &vertices, &uses)?;
    // (A bare id no edge took may be a vertex or another sketch's point the search above found.)
    let left = unmatched.iter().filter(|e| !projected.contains_key(*e)).count();
    if left > 0 {
        report.notes.push(format!("{left} model edge(s) not found"));
    }
    let origin_at = origin_traces(s, el, id, feature, &plane)?;
    let pattern_lines = pattern_lines(s, el, id, feature, &map)?;
    let g = sketch_of(s, el, id)?;
    let ctx = Ctx { map: &map, g: &g, plane: &plane, swap, uses: &uses, vertices: &vertices, projected: &projected, origin_at, pattern_lines: &pattern_lines };
    let mut specs = Vec::new();
    let mut dims = Vec::new();
    let mut dropped: HashMap<String, usize> = HashMap::new();
    for c in feature["constraints"].as_array().into_iter().flatten() {
        let kind = c["constraintType"].as_str().unwrap_or_default();
        match ctx.constraint(kind, c) {
            Converted::Specs(v) => specs.extend(v),
            Converted::Dimension(d) => dims.push(d),
            Converted::Nothing => {}
            Converted::Dropped(why) => *dropped.entry(format!("{kind} ({why})")).or_default() += 1,
        }
    }

    trace(&format!("{} constraints, {} dimensions", specs.len(), dims.len()));
    // Constraints and dimensions must hold the geometry where Onshape solved it. Any that
    // would move it (a translation that means something else in cadrs, a tangency that picks
    // the other solution) are found on a copy of the sketch and left out.
    let ops: Vec<SketchOp> = specs
        .into_iter()
        .map(|c| SketchOp::AddConstraints(vec![c]))
        .chain(dims.into_iter().map(|d| SketchOp::SetDimension { dimension: d, moves: Vec::new(), radii: Vec::new() }))
        .collect();
    let (kept, moved, refused) = keep_in_place(&g, ops);
    if moved > 0 {
        report.notes.push(format!("{moved} constraint(s) or dimension(s) left out: they moved the geometry"));
    }
    if refused > 0 {
        report.notes.push(format!("{refused} constraint(s) or dimension(s) refused by the solver"));
    }
    let (constraints, dimensions): (Vec<SketchOp>, Vec<SketchOp>) = kept.into_iter().partition(|op| matches!(op, SketchOp::AddConstraints(_)));
    let specs: Vec<ConstraintSpec> = constraints
        .into_iter()
        .flat_map(|op| match op {
            SketchOp::AddConstraints(v) => v,
            _ => Vec::new(),
        })
        .collect();
    if !specs.is_empty()
        && let Err(e) = s.run(&EditSketch { element: el, feature: id, op: SketchOp::AddConstraints(specs) })
    {
        report.notes.push(format!("constraints: {e}"));
    }
    for op in dimensions {
        if let Err(e) = s.run(&EditSketch { element: el, feature: id, op }) {
            report.notes.push(format!("dimension: {e}"));
        }
    }
    let mut dropped: Vec<_> = dropped.into_iter().collect();
    dropped.sort();
    for (k, n) in dropped {
        report.notes.push(format!("{n} × {k} constraint dropped"));
    }
    // A constraint that still conflicts holds nothing in place (the solver leaves it unsolved,
    // so the check above can't see it move anything), but it puts the sketch in error: one
    // tied to the wrong model edge (a parallel to a projected line that isn't the edge
    // Onshape's was to). Left out.
    let a = cadrs_sketch::solve::analyze(&sketch_of(s, el, id)?);
    if a.has_conflicts() {
        if std::env::var_os("CADRS_ONSHAPE_DEBUG_MOVES").is_some() {
            let g = sketch_of(s, el, id)?;
            for c in &a.conflicting {
                eprintln!("CONFLICT {:?}", g.constraints.get(*c));
            }
            for d in &a.conflicting_dimensions {
                eprintln!("CONFLICT dim {:?}", g.dimensions.get(*d).map(|x| (x.kind, x.value)));
            }
        }
        let n = a.conflicting.len() + a.conflicting_dimensions.len();
        let op = SketchOp::Delete { curves: Vec::new(), points: Vec::new(), dimensions: a.conflicting_dimensions, constraints: a.conflicting };
        match s.run(&EditSketch { element: el, feature: id, op }) {
            Ok(()) => report.notes.push(format!("{n} constraint(s) or dimension(s) left out: they conflict with the others")),
            Err(e) => report.notes.push(format!("conflicting constraints: {e}")),
        }
    }

    // How far the solve moved the geometry from Onshape's (it should not move at all).
    let g = sketch_of(s, el, id)?;
    let mut worst: f64 = 0.0;
    for (oid, pid) in &map.points {
        if let (Some(p), Some(q)) = (solved.point(oid), g.points.get(*pid)) {
            worst = worst.max(xf.apply(p).distance(q.pos));
        }
    }
    if worst > 1e-4 {
        report.notes.push(format!("geometry moved by up to {worst:.4} mm while solving"));
    }
    if !report.notes.is_empty() && report.outcome == Outcome::Full {
        report.outcome = Outcome::Partial;
    }
    Ok(map)
}

/// The features.json geometry of sketch entity `eid` (a `BTCurveGeometrySpline`).
fn bspline_entity<'a>(feature: &'a Value, eid: &str) -> Option<&'a Value> {
    let e = feature["entities"].as_array()?.iter().find(|e| e["entityId"].as_str() == Some(eid))?;
    let g = &e["geometry"];
    g["btType"].as_str()?.starts_with("BTCurveGeometrySpline").then_some(g)
}

/// A B-spline (Onshape's `controlPoints`, `knots`, `degree`, `isPeriodic`; unweighted) as an
/// interpolated spline through 24 points on it.
fn sampled_bspline(g: &Value, xf: &Xf, construction: bool) -> Option<SketchOp> {
    if g["isRational"].as_bool().unwrap_or(false) {
        return None;
    }
    let degree = g["degree"].as_u64()? as usize;
    let cp: Vec<f64> = g["controlPoints"].as_array()?.iter().filter_map(Value::as_f64).collect();
    let knots: Vec<f64> = g["knots"].as_array()?.iter().filter_map(Value::as_f64).collect();
    let dim = if cp.len() == 2 * (knots.len().checked_sub(degree + 1)?) { 2 } else { 3 };
    let poles: Vec<[f64; 2]> = cp.chunks_exact(dim).map(|c| [c[0], c[1]]).collect();
    let n = poles.len();
    if n <= degree || knots.len() != n + degree + 1 {
        return None;
    }
    let periodic = g["isPeriodic"].as_bool().unwrap_or(false);
    let (lo, hi) = (knots[degree], knots[n]);
    // de Boor.
    let eval = |t: f64| -> [f64; 2] {
        let mut k = degree;
        while k < n - 1 && t >= knots[k + 1] {
            k += 1;
        }
        let mut d: Vec<[f64; 2]> = (0..=degree).map(|j| poles[j + k - degree]).collect();
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let i = j + k - degree;
                let den = knots[i + degree + 1 - r] - knots[i];
                let a = if den.abs() < 1e-300 { 0.0 } else { (t - knots[i]) / den };
                d[j] = [(1.0 - a) * d[j - 1][0] + a * d[j][0], (1.0 - a) * d[j - 1][1] + a * d[j][1]];
            }
        }
        d[degree]
    };
    const N: usize = 24;
    let pts: Vec<Vec2> = if periodic {
        (0..N).map(|i| xf.apply(eval(lo + (hi - lo) * i as f64 / N as f64))).collect()
    } else {
        (0..=N).map(|i| xf.apply(eval(lo + (hi - lo) * i as f64 / N as f64))).collect()
    };
    Some(SketchOp::AddSpline { points: pts, periodic, start_tangent: None, end_tangent: None, construction })
}

/// How far any point of `g` moved from where it is in `base` (points only in `g` ignored).
fn drift(base: &Sketch, g: &Sketch) -> f64 {
    base.points.iter().filter_map(|(id, p)| g.points.get(id).map(|q| q.pos.distance(p.pos))).fold(0.0, f64::max)
}

/// The ops (constraints, dimensions) that can be applied to `g` together without moving its
/// geometry: all of them when they hold it as it is, else those that do, tried one by one.
/// Returns them with how many moved it and how many the sketch refused.
fn keep_in_place(g: &Sketch, ops: Vec<SketchOp>) -> (Vec<SketchOp>, usize, usize) {
    const TOL: f64 = 1e-6;
    let mut all = g.clone();
    if SketchOp::Batch(ops.clone()).apply(&mut all).is_ok() && drift(g, &all) < TOL {
        return (ops, 0, 0);
    }
    let (mut kept, mut moved, mut refused) = (Vec::new(), 0, 0);
    let mut trial = g.clone();
    for op in ops {
        let mut next = trial.clone();
        match op.apply(&mut next) {
            Ok(()) if drift(g, &next) < TOL => {
                trial = next;
                kept.push(op);
            }
            Ok(()) => {
                moved += 1;
                if std::env::var_os("CADRS_ONSHAPE_DEBUG_MOVES").is_some() {
                    eprintln!("MOVED by {} mm: {op:?}", drift(g, &next));
                }
            }
            Err(_) => refused += 1,
        }
    }
    (kept, moved, refused)
}

/// The model vertices the sketch's COINCIDENT constraints name by bare topology id: an id all
/// of whose constrained sketch points sit at one spot where a part edge pierces the sketch plane
/// (a corner of the part the sketch lies on), with that edge as the link a Pierce constraint
/// fixes the point by: fully, as Onshape's coincidence with a vertex, and with no projected
/// curve drawn.
fn model_vertices(feature: &Value, map: &SketchMap, g: &Sketch, plane: &PlaneRef, parts: &[cadrs_core::parts::Part]) -> HashMap<String, (Vec2, cadrs_sketch::Link)> {
    let tol = 1e-4;
    let mut at: HashMap<String, Vec<Vec2>> = HashMap::new();
    for c in feature["constraints"].as_array().into_iter().flatten() {
        if c["constraintType"].as_str() != Some("COINCIDENT") {
            continue;
        }
        let params = c["parameters"].as_array().cloned().unwrap_or_default();
        let point = params.iter().find_map(|p| {
            let local = p["parameterId"].as_str().is_some_and(|i| i.starts_with("local"));
            local.then(|| p["value"].as_str().and_then(|v| map.points.get(v)).and_then(|q| g.points.get(*q)).map(|q| q.pos)).flatten()
        });
        let Some(point) = point else { continue };
        for p in params.iter().filter(|p| p["parameterId"].as_str().is_some_and(|i| i.starts_with("external"))) {
            let ids: Vec<&str> = p["queries"].as_array().into_iter().flatten().flat_map(|q| q["deterministicIds"].as_array().into_iter().flatten().filter_map(Value::as_str)).collect();
            if let [eid] = ids.as_slice()
                && !matches!(*eid, ORIGIN_ID | TOP_ID | FRONT_ID | RIGHT_ID)
            {
                at.entry(eid.to_string()).or_default().push(point);
            }
        }
    }
    let frame = plane.frame();
    let mut out = HashMap::new();
    for (eid, pts) in at {
        let p = pts[0];
        if pts.iter().any(|q| q.distance(p) > tol) {
            continue;
        }
        'search: for part in parts {
            for e in &part.solid.edges {
                let Some(curve) = cadrs_core::links::edge_curve(e) else { continue };
                if cadrs_core::links::crossings(curve, &frame).iter().any(|x| x.distance(p) < tol) {
                    out.insert(eid.clone(), (p, cadrs_sketch::Link::Edge { feature: part.id.feature.0, edge: e.name }));
                    break 'search;
                }
            }
        }
    }
    out
}

/// For a sketch whose constraints name the Part Studio's origin while the sketch's own origin is
/// elsewhere (a sketch on a face): two default planes' traces through the origin's projection,
/// brought in as construction Use lines (they follow the planes), and where they meet.
fn origin_traces(s: &mut dyn Studio, el: ElementId, id: FeatureId, feature: &Value, plane: &PlaneRef) -> Result<Option<(Vec2, CurveId, CurveId)>, CommandError> {
    let names_origin = feature["constraints"].as_array().into_iter().flatten().flat_map(|c| c["parameters"].as_array().into_iter().flatten()).any(|p| {
        p["queries"].as_array().into_iter().flatten().any(|q| q["deterministicIds"].as_array().is_some_and(|ids| matches!(ids.as_slice(), [i] if i.as_str() == Some(ORIGIN_ID))))
    });
    let frame = plane.frame();
    let q = frame.to_sketch([0.0; 3]);
    if !names_origin || q.length() < 1e-9 {
        return Ok(None);
    }
    // The default planes the projection lies on (seen edge-on from the sketch).
    let mut traces = Vec::new();
    for p in [PlaneRef::Top, PlaneRef::Front, PlaneRef::Right] {
        let Some(curve) = cadrs_core::links::plane_trace(&p.frame(), &frame) else { continue };
        let Some(shape) = cadrs_core::links::project(curve, &frame) else { continue };
        let cadrs_sketch::projection::Projected::Line(a, b) = shape else { continue };
        let ab = b - a;
        if ((q - a).x * ab.y - (q - a).y * ab.x).abs() / ab.length().max(1e-300) < 1e-6 {
            traces.push((shape, p, ab));
        }
    }
    let [(s1, p1, d1), (s2, p2, d2), ..] = traces.as_slice() else { return Ok(None) };
    if (d1.x * d2.y - d1.y * d2.x).abs() < 1e-9 * d1.length() * d2.length() {
        return Ok(None);
    }
    let mut lines = Vec::new();
    for (shape, p) in [(s1.clone(), *p1), (s2.clone(), *p2)] {
        let before = sketch_of(s, el, id)?;
        s.run(&EditSketch { element: el, feature: id, op: SketchOp::Use { items: vec![(shape, cadrs_sketch::Link::Plane(p))] } })?;
        let after = sketch_of(s, el, id)?;
        let Some(c) = after.curves.keys().find(|c| !before.curves.contains_key(*c)) else { return Ok(None) };
        s.run(&EditSketch { element: el, feature: id, op: SketchOp::SetConstruction { curves: vec![c], construction: true } })?;
        lines.push(c);
    }
    Ok(Some((q, lines[0], lines[1])))
}

/// The model vertices off the sketch plane its constraints name by bare topology id (Onshape
/// projects them onto the plane): each found where Onshape's final parts have it (else, for a
/// Horizontal or Vertical, the nearest model vertex level with its point), and brought in as a
/// point pierced by the straight part edge through it along the plane's normal (so it sits at
/// the projection and follows the edge). Returns id → where the point is.
#[allow(clippy::too_many_arguments)]
fn projected_vertices(
    s: &mut dyn Studio,
    el: ElementId,
    id: FeatureId,
    feature: &Value,
    map: &SketchMap,
    plane: &PlaneRef,
    swap: Option<bool>,
    parts: &[cadrs_core::parts::Part],
    model_points: &HashMap<String, Vec<[f64; 3]>>,
    vertices: &HashMap<String, (Vec2, cadrs_sketch::Link)>,
    uses: &HashMap<String, CurveId>,
) -> Result<HashMap<String, Vec2>, CommandError> {
    let mut out = HashMap::new();
    let frame = plane.frame();
    let unit = |v: Vec3| {
        let l = dot(v, v).sqrt().max(1e-300);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let n = unit(frame.normal());
    let near = |a: Vec3, b: Vec3| dot(sub(a, b), sub(a, b)).sqrt() < 1e-3;
    // The straight edge through a vertex along the normal (its line pierces the plane at the
    // vertex's projection).
    let normal_edge = |v: Vec3| {
        parts.iter().find_map(|part| {
            part.solid.edges.iter().find_map(|e| {
                let Some(cadrs_core::links::Curve3::Line(a, b)) = cadrs_core::links::edge_curve(e) else { return None };
                let along = dot(unit(sub(b, a)), n).abs() > 1.0 - 1e-9;
                (along && (near(a, v) || near(b, v))).then_some(cadrs_sketch::Link::Edge { feature: part.id.feature.0, edge: e.name })
            })
        })
    };
    // What holds a point at a vertex's projection: a point pierced by that edge, else (no edge
    // along the normal) an edge ending at the vertex, used: its projection ends there.
    let anchor = |v: Vec3| -> Option<(Option<cadrs_sketch::projection::Projected>, cadrs_sketch::Link)> {
        normal_edge(v).map(|l| (None, l)).or_else(|| {
            parts.iter().find_map(|part| {
                part.solid.edges.iter().find_map(|e| {
                    let curve = cadrs_core::links::edge_curve(e)?;
                    let cadrs_core::links::Curve3::Line(a, b) = curve else { return None };
                    if !(near(a, v) || near(b, v)) {
                        return None;
                    }
                    let shape = cadrs_core::links::project(curve, &frame)?;
                    Some((Some(shape), cadrs_sketch::Link::Edge { feature: part.id.feature.0, edge: e.name }))
                })
            })
        })
    };
    let g = sketch_of(s, el, id)?;
    for c in feature["constraints"].as_array().into_iter().flatten() {
        let kind = c["constraintType"].as_str().unwrap_or_default();
        let params = c["parameters"].as_array().cloned().unwrap_or_default();
        for p in params.iter().filter(|p| p["parameterId"].as_str().is_some_and(|i| i.starts_with("external"))) {
            let ids: Vec<&str> = p["queries"].as_array().into_iter().flatten().flat_map(|q| q["deterministicIds"].as_array().into_iter().flatten().filter_map(Value::as_str)).collect();
            let [vid] = ids.as_slice() else { continue };
            if matches!(*vid, ORIGIN_ID | TOP_ID | FRONT_ID | RIGHT_ID) || vertices.contains_key(*vid) || uses.contains_key(*vid) || out.contains_key(*vid) {
                continue;
            }
            // Where Onshape's final parts have it, when the model here has it there too.
            let recorded = match model_points.get(*vid).map(Vec::as_slice) {
                Some(&[v]) => anchor(v).map(|l| (frame.to_sketch(v), l)),
                _ => None,
            };
            // Else (gone from the final parts, or moved since): for a Horizontal or Vertical (or a
            // Distance along an axis), the nearest model vertex level with (that far from) the point.
            let level = || {
                let at = params
                    .iter()
                    .filter(|p| p["parameterId"].as_str().is_some_and(|i| i.starts_with("local")))
                    .find_map(|p| map.points.get(p["value"].as_str()?).and_then(|k| g.points.get(*k)).map(|x| x.pos))?;
                // Which coordinate they share in cadrs's frame (0: x, 1: y), and how far apart in it
                // (a Horizontal or Vertical Distance: its value, along the axis it measures).
                let length = || {
                    params.iter().find(|p| p["parameterId"] == "length").and_then(|p| p["expression"].as_str()).and_then(|e| crate::expr::eval(e, &HashMap::<String, String>::new()).ok()).filter(|q| q.len == 1).map(|q| q.v)
                };
                let direction = params.iter().find(|p| p["parameterId"] == "direction").and_then(|p| p["value"].as_str());
                let (axis, apart) = match (kind, direction, swap?) {
                    ("HORIZONTAL", _, sw) => (if sw { 0 } else { 1 }, 0.0),
                    ("VERTICAL", _, sw) => (if sw { 1 } else { 0 }, 0.0),
                    ("DISTANCE", Some("HORIZONTAL"), sw) => (if sw { 1 } else { 0 }, length()?),
                    ("DISTANCE", Some("VERTICAL"), sw) => (if sw { 0 } else { 1 }, length()?),
                    _ => return None,
                };
                let coord = |q: Vec2| if axis == 0 { q.x } else { q.y };
                let found = parts
                    .iter()
                    .flat_map(|part| part.solid.vertices.iter())
                    .map(|v| (frame.to_sketch(v.point), v.point))
                    .filter(|(q, _)| ((coord(*q) - coord(at)).abs() - apart).abs() < 1e-4 && q.distance(at) > 1e-6)
                    .filter_map(|(q, v)| anchor(v).map(|l| (q, l)))
                    .min_by(|a, b| a.0.distance(at).total_cmp(&b.0.distance(at)))
                    // Else an end of an earlier sketch's line there: that line used, its
                    // projection ending at the point.
                    .or_else(|| {
                        let doc = s.document().element(el)?;
                        let mut best: Option<(Vec2, (Option<cadrs_sketch::projection::Projected>, cadrs_sketch::Link))> = None;
                        for f in doc.features().iter().take_while(|f| f.id != id) {
                            let Some(sk) = f.sketch() else { continue };
                            let Some(pl) = sk.plane else { continue };
                            let other = pl.frame();
                            for (k, c) in &sk.geometry.curves {
                                let CurveKind::Line { a, b } = c.kind else { continue };
                                let (qa, qb) = (frame.to_sketch(other.to_world(sk.geometry.pos(a))), frame.to_sketch(other.to_world(sk.geometry.pos(b))));
                                if qa.distance(qb) < 1e-6 {
                                    continue;
                                }
                                for q in [qa, qb] {
                                    if ((coord(q) - coord(at)).abs() - apart).abs() < 1e-4 && q.distance(at) > 1e-6 && best.as_ref().is_none_or(|(b, _)| q.distance(at) < b.distance(at)) {
                                        best = Some((q, (Some(cadrs_sketch::projection::Projected::Line(qa, qb)), cadrs_sketch::Link::SketchCurve { feature: f.id.0, curve: k })));
                                    }
                                }
                            }
                        }
                        best
                    });
                if found.is_none() && std::env::var_os("CADRS_ONSHAPE_DEBUG_EDGES").is_some() {
                    let level: Vec<(Vec2, bool)> = parts.iter().flat_map(|part| part.solid.vertices.iter()).map(|v| (frame.to_sketch(v.point), anchor(v.point).is_some())).filter(|(q, _)| ((coord(*q) - coord(at)).abs() - apart).abs() < 1e-3).collect();
                    eprintln!("LEVEL {vid} {kind} at {at:?}: {} vertices level with it {level:?}", level.len());
                }
                found
            };
            let found = recorded.or_else(level);
            let Some((q, (shape, link))) = found else { continue };
            // A sketch point already there (as a rule, one held to the same model point): that
            // one, rather than a second point tied to it.
            let before = sketch_of(s, el, id)?;
            if nearest_point(&before, q).is_some() {
                out.insert(vid.to_string(), q);
                continue;
            }
            match shape {
                None => {
                    if s.run(&EditSketch { element: el, feature: id, op: SketchOp::AddPoint { pos: q } }).is_err() {
                        continue;
                    }
                    s.run(&EditSketch { element: el, feature: id, op: SketchOp::AddConstraints(vec![ConstraintOf::Pierce(PointSpec::At(q), link)]) }).ok();
                }
                Some(shape) => {
                    if s.run(&EditSketch { element: el, feature: id, op: SketchOp::Use { items: vec![(shape, link)] } }).is_err() {
                        continue;
                    }
                    let after = sketch_of(s, el, id)?;
                    let new: Vec<CurveId> = after.curves.keys().filter(|c| !before.curves.contains_key(*c)).collect();
                    s.run(&EditSketch { element: el, feature: id, op: SketchOp::SetConstruction { curves: new, construction: true } }).ok();
                    if nearest_point(&after, q).is_none() {
                        continue;
                    }
                }
            }
            out.insert(vid.to_string(), q);
        }
    }
    Ok(out)
}

/// The model edges the sketch's constraints name by bare topology id, found by geometry: for
/// each id, the part edge whose projection onto the sketch plane passes through the points
/// constrained to it (or has its middle at a Midpoint's point). Each is added as a Use curve
/// (linked, so it follows the edge) made construction (so it bounds no region, as in Onshape,
/// where the edge isn't a sketch curve). Returns id → curve.
#[allow(clippy::too_many_arguments)]
fn model_edges(
    s: &mut dyn Studio,
    el: ElementId,
    id: FeatureId,
    feature: &Value,
    map: &SketchMap,
    g: &Sketch,
    plane: &PlaneRef,
    parts: &[cadrs_core::parts::Part],
    vertices: &HashMap<String, (Vec2, cadrs_sketch::Link)>,
    model_points: &HashMap<String, Vec<[f64; 3]>>,
    swap: Option<bool>,
    report: &mut FeatureReport,
) -> Result<(HashMap<String, CurveId>, Vec<String>), CommandError> {
    // Per edge id: the points on it, and the points at its middle.
    let mut hints: std::collections::BTreeMap<String, (Vec<Vec2>, Vec<Vec2>)> = Default::default();
    // Per edge id: the centres of the circles concentric with it.
    let mut centres: HashMap<String, Vec<Vec2>> = HashMap::new();
    // Per edge id: points a Distance dimension holds off it, and how far (mm).
    // (A distance along one of cadrs's axes, 0: x, 1: y, holds along it.)
    let mut dists: HashMap<String, Vec<(Vec2, f64, Option<usize>)>> = HashMap::new();
    // Per edge id: lines of the sketch parallel (false) or perpendicular (true) to it.
    let mut squares: HashMap<String, Vec<(Vec2, Vec2, bool)>> = HashMap::new();
    let point_of = |v: &str| map.points.get(v).and_then(|p| g.points.get(*p)).map(|p| p.pos);
    // Points on a curve: a line's ends, three points round a circle, an arc's ends and middle
    // (counterclockwise from its start).
    let curve_points = |v: &str| -> Vec<Vec2> {
        let pos = |p: PointId| g.points.get(p).map(|p| p.pos);
        match map.curves.get(v).and_then(|c| g.curves.get(*c)).map(|c| c.kind) {
            Some(CurveKind::Line { a, b }) => [a, b].iter().filter_map(|p| pos(*p)).collect(),
            Some(CurveKind::Circle { center, radius }) => pos(center)
                .map(|c| (0..3).map(|k| f64::from(k) * std::f64::consts::TAU / 3.0).map(|t| Vec2::new(c.x + radius * t.cos(), c.y + radius * t.sin())).collect())
                .unwrap_or_default(),
            Some(CurveKind::Arc { center, start, end }) => {
                let (Some(c), Some(a), Some(b)) = (pos(center), pos(start), pos(end)) else { return Vec::new() };
                let (t0, mut t1) = ((a.y - c.y).atan2(a.x - c.x), (b.y - c.y).atan2(b.x - c.x));
                if t1 <= t0 {
                    t1 += std::f64::consts::TAU;
                }
                let (r, t) = (a.distance(c), (t0 + t1) / 2.0);
                vec![a, b, Vec2::new(c.x + r * t.cos(), c.y + r * t.sin())]
            }
            _ => Vec::new(),
        }
    };
    // A circle's or arc's centre (a CONCENTRIC constraint's local entity).
    let centre_of = |v: &str| -> Option<Vec2> {
        match map.curves.get(v).and_then(|c| g.curves.get(*c)).map(|c| c.kind) {
            Some(CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. }) => g.points.get(center).map(|p| p.pos),
            _ => point_of(v),
        }
    };
    for c in feature["constraints"].as_array().into_iter().flatten() {
        // A horizontal or vertical alignment with a model entity says nothing about which
        // edge its point is on (it is level with a vertex): finding an edge through the point
        // projected a stray line into the sketch.
        if matches!(c["constraintType"].as_str(), Some("HORIZONTAL" | "VERTICAL")) {
            continue;
        }
        let params = c["parameters"].as_array().cloned().unwrap_or_default();
        let midpoint = c["constraintType"].as_str() == Some("MIDPOINT");
        let kind = c["constraintType"].as_str();
        // An Offset of a circle or arc shares its centre; of a line, it is parallel.
        let round_offset = kind == Some("OFFSET")
            && params.iter().filter(|p| p["parameterId"].as_str().is_some_and(|i| i.starts_with("local"))).any(|p| {
                p["value"].as_str().and_then(|v| map.curves.get(v)).and_then(|k| g.curves.get(*k)).is_some_and(|x| matches!(x.kind, CurveKind::Circle { .. } | CurveKind::Arc { .. }))
            });
        let concentric = kind == Some("CONCENTRIC") || round_offset;
        let square = match kind {
            Some("PARALLEL" | "OFFSET") => Some(false),
            Some("PERPENDICULAR") => Some(true),
            _ => None,
        };
        // A distance from a point (or a line: both its ends): the edge is that far off it,
        // not through it.
        // A Horizontal or Vertical distance: along which of cadrs's axes.
        let along = match (params.iter().find(|p| p["parameterId"] == "direction").and_then(|p| p["value"].as_str()), swap) {
            (Some("HORIZONTAL"), Some(sw)) => Some(if sw { 1 } else { 0 }),
            (Some("VERTICAL"), Some(sw)) => Some(if sw { 0 } else { 1 }),
            _ => None,
        };
        let distance = (c["constraintType"].as_str() == Some("DISTANCE"))
            .then(|| params.iter().find(|p| p["parameterId"] == "length").and_then(|p| p["expression"].as_str()))
            .flatten()
            .and_then(|e| crate::expr::eval(e, &HashMap::<String, String>::new()).ok())
            .filter(|q| q.len == 1)
            .map(|q| q.v);
        let mut local_points = Vec::new();
        // A distance from a circle or arc: from its centre, its radius further.
        let round = distance.and_then(|d| {
            params.iter().filter(|p| p["parameterId"].as_str().is_some_and(|i| i.starts_with("local"))).find_map(|p| {
                let pos = |k: PointId| g.points.get(k).map(|x| x.pos);
                match map.curves.get(p["value"].as_str()?).and_then(|c| g.curves.get(*c)).map(|c| c.kind)? {
                    CurveKind::Circle { center, radius } => Some((pos(center)?, d + radius)),
                    CurveKind::Arc { center, start, .. } => Some((pos(center)?, d + pos(center)?.distance(pos(start)?))),
                    _ => None,
                }
            })
        });
        for p in &params {
            if p["parameterId"].as_str().is_some_and(|i| i.starts_with("local"))
                && let Some(v) = p["value"].as_str()
            {
                if concentric {
                    local_points.extend(centre_of(v));
                    continue;
                }
                local_points.extend(point_of(v));
                if !midpoint {
                    local_points.extend(curve_points(v));
                }
            }
        }
        for p in &params {
            if !p["parameterId"].as_str().is_some_and(|i| i.starts_with("external")) {
                continue;
            }
            let ids: Vec<&str> = p["queries"].as_array().into_iter().flatten().flat_map(|q| q["deterministicIds"].as_array().into_iter().flatten().filter_map(Value::as_str)).collect();
            let [eid] = ids.as_slice() else { continue };
            if matches!(*eid, ORIGIN_ID | TOP_ID | FRONT_ID | RIGHT_ID) || vertices.contains_key(*eid) {
                continue;
            }
            let h = hints.entry(eid.to_string()).or_default();
            // An Offset's master: the offset curves it is the master of (a corner's second
            // master has the second curve), parallel or concentric.
            if kind == Some("OFFSET") {
                let has_second = params.iter().any(|q| matches!(q["parameterId"].as_str(), Some("externalSecond" | "localSecond")) && (q["value"].is_string() || q["queries"].as_array().is_some_and(|a| !a.is_empty())));
                let keys: &[&str] = match p["parameterId"].as_str() {
                    Some("externalSecond") => &["localSecondOffset"],
                    _ if has_second => &["localOffset"],
                    _ => &["localOffset", "localSecondOffset"],
                };
                // The Offset's distance: the Distance on its first curve (both pairs are that far).
                let first = params.iter().find(|q| q["parameterId"] == "localOffset").and_then(|q| q["value"].as_str());
                let apart = first.and_then(|first| {
                    feature["constraints"].as_array().into_iter().flatten().filter(|d| d["constraintType"].as_str() == Some("DISTANCE")).find_map(|d| {
                        let ps = d["parameters"].as_array()?;
                        ps.iter().any(|p| p["value"].as_str() == Some(first)).then(|| ps.iter().find(|p| p["parameterId"] == "length").and_then(|p| p["expression"].as_str()))?
                    })
                })
                .and_then(|e| crate::expr::eval(e, &HashMap::<String, String>::new()).ok())
                .filter(|q| q.len == 1)
                .map(|q| q.v);
                for k in keys {
                    let Some(v) = params.iter().find(|q| q["parameterId"] == *k).and_then(|q| q["value"].as_str()) else { continue };
                    match map.curves.get(v).and_then(|c| g.curves.get(*c)).map(|c| c.kind) {
                        Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }) => centres.entry(eid.to_string()).or_default().extend(centre_of(v)),
                        _ => {
                            if let [a, b] = curve_points(v).as_slice() {
                                squares.entry(eid.to_string()).or_default().push((*a, *b, false));
                                if let Some(d) = apart {
                                    dists.entry(eid.to_string()).or_default().extend([(*a, d, None), (*b, d, None)]);
                                }
                            }
                        }
                    }
                }
                continue;
            }
            // Each line its own pair of ends (an Offset may have one each side).
            if let Some(perp) = square.filter(|_| !concentric && !local_points.is_empty() && local_points.len() % 2 == 0) {
                squares.entry(eid.to_string()).or_default().extend(local_points.chunks(2).map(|ab| (ab[0], ab[1], perp)));
            } else if let Some(r) = round {
                dists.entry(eid.to_string()).or_default().push((r.0, r.1, None));
            } else if distance.is_some() && matches!(local_points.as_slice(), [_] | [_, _]) {
                dists.entry(eid.to_string()).or_default().extend(local_points.iter().map(|p| (*p, distance.unwrap_or_default(), along)));
            } else if concentric {
                centres.entry(eid.to_string()).or_default().extend(local_points.iter().copied());
            } else if midpoint {
                h.1.extend(local_points.iter().copied());
            } else {
                h.0.extend(local_points.iter().copied());
            }
        }
    }
    let frame = plane.frame();
    let tol = 1e-4;
    let mut out = HashMap::new();
    let mut unmatched: Vec<String> = Vec::new();
    // The ids with the most hints first; an edge one of them took is another's last resort
    // (each id is its own edge).
    let count = |eid: &String, on: usize| on + centres.get(eid).map_or(0, Vec::len) + dists.get(eid).map_or(0, Vec::len) + squares.get(eid).map_or(0, Vec::len);
    let mut order: Vec<_> = hints.into_iter().collect();
    order.sort_by_key(|(eid, (on, mids))| std::cmp::Reverse(count(eid, on.len() + mids.len())));
    let mut taken: Vec<cadrs_sketch::Link> = Vec::new();
    for (eid, (on, mids)) in order {
        let centred = centres.get(&eid).cloned().unwrap_or_default();
        let off = dists.get(&eid).cloned().unwrap_or_default();
        let along = squares.get(&eid).cloned().unwrap_or_default();
        // Where Onshape's final parts have the edge, when they still do: its start, middle and
        // end on the sketch plane (they pick among the edges the hints fit equally: ids name
        // the final parts', which can differ from the model this sketch saw).
        let known: Vec<Vec2> = model_points.get(&eid).filter(|p| p.len() == 3).map(|p| p.iter().map(|q| frame.to_sketch(*q)).collect()).unwrap_or_default();
        if on.is_empty() && mids.is_empty() && centred.is_empty() && off.is_empty() && along.is_empty() {
            if std::env::var_os("CADRS_ONSHAPE_DEBUG_EDGES").is_some() {
                eprintln!("EDGE {eid}: no points to find it by");
            }
            unmatched.push(eid.clone());
            continue;
        }
        // The edge satisfying the most hints (then the one closest to them).
        // (Score, error, the edge as projected, its link, the hints on it.)
        type Best = ((usize, bool, usize), f64, cadrs_sketch::projection::Projected, cadrs_sketch::Link, usize);
        let mut best: Option<Best> = None;
        for part in parts {
            for e in &part.solid.edges {
                let Some(shape) = cadrs_core::links::edge_curve(e).and_then(|c| cadrs_core::links::project(c, &frame)) else { continue };
                let mut hits = 0;
                let mut err = 0.0;
                for p in &on {
                    let d = cadrs_core::links::projected_distance(&shape, *p).unwrap_or(f64::MAX);
                    // On a straight edge's line counts (a point at the end of an edge Onshape
                    // keeps whole that cadrs has in collinear pieces); the piece it is on is
                    // the closer.
                    let on_line = match shape {
                        cadrs_sketch::projection::Projected::Line(a, b) if (b - a).length() > 1e-9 => {
                            let ab = b - a;
                            ((*p - a).x * ab.y - (*p - a).y * ab.x).abs() / ab.length()
                        }
                        _ => d,
                    };
                    if on_line < tol {
                        hits += 1;
                    }
                    err += d.min(1.0);
                }
                for p in &mids {
                    let d = match shape {
                        cadrs_sketch::projection::Projected::Line(a, b) => p.distance(Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)),
                        _ => f64::MAX,
                    };
                    if d < tol {
                        hits += 1;
                    }
                    err += d.min(1.0);
                }
                for (a0, b0, perp) in &along {
                    // A straight edge square to (or along) the line: the nearest such.
                    let cadrs_sketch::projection::Projected::Line(a, b) = shape else { continue };
                    let (u, w) = (b - a, *b0 - *a0);
                    let (lu, lw) = (u.length(), w.length());
                    if lu < 1e-9 || lw < 1e-9 {
                        continue;
                    }
                    let (sin, cos) = ((u.x * w.y - u.y * w.x) / (lu * lw), u.dot(w) / (lu * lw));
                    if (if *perp { cos } else { sin }).abs() < 1e-9 {
                        hits += 1;
                    }
                    let mid = (*a0 + *b0) * 0.5;
                    err += cadrs_core::links::projected_distance(&shape, mid).unwrap_or(1e6);
                }
                for (p, want, axis) in &off {
                    // A straight edge: its line that far from the point (then the one whose
                    // segment the point is beside).
                    let cadrs_sketch::projection::Projected::Line(a, b) = shape else { continue };
                    let ab = b - a;
                    let l = ab.length();
                    if l < 1e-9 {
                        continue;
                    }
                    let t = (*p - a).dot(ab) / (l * l);
                    let coord = |v: Vec2, k: usize| if k == 0 { v.x } else { v.y };
                    // Along an axis: an edge across it (a vertical one for a horizontal distance),
                    // that far along it.
                    let across = match axis {
                        Some(k) if coord(ab, *k).abs() > 1e-9 * l => continue,
                        Some(k) => (coord(*p, *k) - coord(a, *k)).abs(),
                        None => ((*p - a).x * ab.y - (*p - a).y * ab.x).abs() / l,
                    };
                    if (across - want).abs() < tol {
                        hits += 1;
                    }
                    err += (across - want).abs().min(1.0) + (if t < 0.0 { -t } else if t > 1.0 { t - 1.0 } else { 0.0 }) * l;
                }
                for p in &centred {
                    let d = match shape {
                        cadrs_sketch::projection::Projected::Circle(c, _) | cadrs_sketch::projection::Projected::Arc { center: c, .. } => p.distance(c),
                        _ => f64::MAX,
                    };
                    if d < tol {
                        hits += 1;
                    }
                    err += d.min(1.0);
                }
                let fits = known.iter().filter(|q| cadrs_core::links::projected_distance(&shape, **q).is_some_and(|d| d < tol)).count();
                if hits == 0 {
                    continue;
                }
                // The most hints, then one no other id took, then the most of Onshape's own
                // points, then the closest.
                let link = cadrs_sketch::Link::Edge { feature: part.id.feature.0, edge: e.name };
                let score = (hits, !taken.contains(&link), fits);
                if best.as_ref().is_none_or(|(h, e2, ..)| score > *h || (score == *h && err < *e2)) {
                    best = Some((score, err, shape, link, hits));
                }
            }
        }
        let Some((_, _, shape, link, hits)) = best else {
            if std::env::var_os("CADRS_ONSHAPE_DEBUG_EDGES").is_some() {
                // The nearest any edge comes to the hints.
                let mut near = f64::MAX;
                for part in parts {
                    for e in &part.solid.edges {
                        let Some(shape) = cadrs_core::links::edge_curve(e).and_then(|c| cadrs_core::links::project(c, &frame)) else { continue };
                        for p in on.iter().chain(&mids) {
                            near = near.min(cadrs_core::links::projected_distance(&shape, *p).unwrap_or(f64::MAX));
                        }
                    }
                }
                eprintln!("EDGE {eid}: {} hints, nearest edge {near:.6} mm, {} parts", on.len() + mids.len(), parts.len());
            }
            unmatched.push(eid.clone());
            continue;
        };
        taken.push(link);
        if hits < on.len() + mids.len() + centred.len() + off.len() + along.len() {
            report.notes.push(format!("model edge {eid}: {hits} of {} constrained points on it", on.len() + mids.len() + centred.len() + off.len() + along.len()));
        }
        let current = sketch_of(s, el, id)?;
        // Used already (by another reference to the same edge, or to one of its ends): that curve.
        if let Some(c) = current.constraints.values().find_map(|c| match *c {
            cadrs_sketch::constraint::ConstraintOf::Use(CurveRef::Curve(k), l) if l == link => Some(k),
            _ => None,
        }) {
            out.insert(eid, c);
            continue;
        }
        let before: std::collections::HashSet<CurveId> = current.curves.keys().collect();
        if let Err(e) = s.run(&EditSketch { element: el, feature: id, op: SketchOp::Use { items: vec![(shape, link)] } }) {
            if std::env::var_os("CADRS_ONSHAPE_DEBUG_EDGES").is_some() {
                eprintln!("EDGE {eid}: found ({hits} hits), but Use failed: {e}");
            }
            unmatched.push(eid.clone());
            continue;
        }
        let after = sketch_of(s, el, id)?;
        if let Some(c) = after.curves.keys().find(|c| !before.contains(c)) {
            s.run(&EditSketch { element: el, feature: id, op: SketchOp::SetConstruction { curves: vec![c], construction: true } })?;
            out.insert(eid, c);
        }
    }
    Ok((out, unmatched))
}

/// The geometry of sketch `id`.
pub fn sketch_of(s: &dyn Studio, el: ElementId, id: FeatureId) -> Result<Sketch, CommandError> {
    s.document()
        .element(el)
        .and_then(|e| e.feature(id))
        .and_then(|f| f.sketch())
        .map(|f| f.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

fn nearest_point(g: &Sketch, p: Vec2) -> Option<PointId> {
    g.points
        .iter()
        .map(|(id, q)| (id, q.pos.distance(p)))
        .filter(|(_, d)| *d < 1e-5)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

enum Converted {
    Specs(Vec<ConstraintSpec>),
    Dimension(Dimension),
    /// Implied by the geometry (two ends placed at the same spot share their point).
    Nothing,
    Dropped(&'static str),
}

/// What a constraint parameter refers to.
#[derive(Debug, Clone, Copy)]
enum Ent {
    Point(PointId, Vec2),
    Curve(CurveId),
    Origin,
    XAxis,
    YAxis,
    /// A default plane seen edge-on, not through the origin along an axis.
    Model,
    /// A model vertex: the part edge piercing the sketch plane there.
    Vertex(cadrs_sketch::Link),
    /// The Part Studio's origin away from the sketch's: on both these default-plane traces.
    OriginTraces(CurveId, CurveId),
}

struct Ctx<'a> {
    map: &'a SketchMap,
    g: &'a Sketch,
    plane: &'a PlaneRef,
    /// How Onshape's sketch axes lie in cadrs's: `Some(false)` the same (up to sign),
    /// `Some(true)` swapped (Onshape's x along cadrs's y: horizontal is vertical), `None`
    /// turned by another angle (horizontal and vertical can't be said).
    swap: Option<bool>,
    /// Onshape model-edge ids → the construction Use curves standing for them.
    uses: &'a HashMap<String, CurveId>,
    /// Onshape model-vertex ids → where they are and the edge piercing the plane there.
    vertices: &'a HashMap<String, (Vec2, cadrs_sketch::Link)>,
    /// Onshape model-vertex ids off the plane → the construction point pierced at its projection.
    projected: &'a HashMap<String, Vec2>,
    /// Where the Part Studio's origin projects when that isn't the sketch's origin, and the
    /// two default planes' traces (construction Use lines) that meet there.
    origin_at: Option<(Vec2, CurveId, CurveId)>,
    /// Each sketch Linear pattern's (by its entity id) rows of points: the construction line
    /// added through each.
    pattern_lines: &'a HashMap<String, Vec<(usize, CurveId)>>,
}

impl Ctx<'_> {
    /// The entity a constraint's `local…` or `external…` parameter names.
    fn ent(&self, c: &Value, local: &str) -> Option<Ent> {
        let external = local.replacen("local", "external", 1);
        for p in c["parameters"].as_array()? {
            let pid = p["parameterId"].as_str().unwrap_or_default();
            if pid == local {
                let v = p["value"].as_str()?;
                if let Some(&pt) = self.map.points.get(v) {
                    return Some(Ent::Point(pt, self.g.points.get(pt)?.pos));
                }
                return self.map.curves.get(v).map(|&c| Ent::Curve(c));
            }
            if pid == external {
                let ids: Vec<&str> = p["queries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|q| q["deterministicIds"].as_array().into_iter().flatten().filter_map(Value::as_str))
                    .collect();
                return match ids.as_slice() {
                    [ORIGIN_ID] => Some(self.origin_at.map_or(Ent::Origin, |(_, a, b)| Ent::OriginTraces(a, b))),
                    [plane @ (TOP_ID | FRONT_ID | RIGHT_ID)] => Some(self.axis_of_plane(plane)),
                    [v] if self.vertices.contains_key(*v) => Some(Ent::Vertex(self.vertices[*v].1)),
                    [v] if let Some(&q) = self.projected.get(*v) => nearest_point(self.g, q).map(|p| Ent::Point(p, q)),
                    [edge] if self.uses.contains_key(*edge) => Some(Ent::Curve(self.uses[*edge])),
                    _ => Some(Ent::Model),
                };
            }
        }
        None
    }

    /// A default plane seen edge-on from the sketch: the cadrs sketch axis it lies along, if
    /// it goes through the sketch origin along one.
    fn axis_of_plane(&self, plane: &str) -> Ent {
        let n = match plane {
            TOP_ID => [0.0, 0.0, 1.0],
            FRONT_ID => [0.0, 1.0, 0.0],
            _ => [1.0, 0.0, 0.0],
        };
        let f = self.plane.frame();
        // Through the origin: the sketch origin lies on the plane (all three go through the
        // world origin).
        if dot(f.origin, n).abs() > 1e-6 {
            return Ent::Model;
        }
        let dir = cross(n, cross(f.u, f.v));
        let len = dot(dir, dir).sqrt();
        if len < 1e-9 {
            return Ent::Model;
        }
        if (dot(dir, f.u) / len).abs() > 1.0 - 1e-9 {
            Ent::XAxis
        } else if (dot(dir, f.v) / len).abs() > 1.0 - 1e-9 {
            Ent::YAxis
        } else {
            Ent::Model
        }
    }

    fn constraint(&self, kind: &str, c: &Value) -> Converted {
        use Converted::*;
        let first = self.ent(c, "localFirst");
        let second = self.ent(c, "localSecond");
        let pt = |e: Option<Ent>| match e {
            Some(Ent::Point(_, p)) => Some(PointSpec::At(p)),
            Some(Ent::Origin) => Some(PointSpec::Origin),
            _ => None,
        };
        let cv = |e: Option<Ent>| match e {
            Some(Ent::Curve(id)) => Some(CurveSpec::Id(id)),
            Some(Ent::XAxis) => Some(CurveSpec::XAxis),
            Some(Ent::YAxis) => Some(CurveSpec::YAxis),
            _ => None,
        };
        let model = |e: Option<Ent>| matches!(e, Some(Ent::Model));
        let one = |s: ConstraintSpec| Specs(vec![s]);
        match kind {
            "COINCIDENT" => match (first, second) {
                (Some(Ent::Point(a, _)), Some(Ent::Point(b, _))) if a == b => Nothing,
                // On a model vertex: pierced by the part edge there.
                (Some(Ent::Point(_, p)), Some(Ent::Vertex(l))) | (Some(Ent::Vertex(l)), Some(Ent::Point(_, p))) => one(ConstraintOf::Pierce(PointSpec::At(p), l)),
                // On the origin, away from the sketch's: on the two plane traces through it.
                (Some(Ent::Point(_, p)), Some(Ent::OriginTraces(a, b))) | (Some(Ent::OriginTraces(a, b)), Some(Ent::Point(_, p))) => {
                    Specs(vec![ConstraintOf::PointOnCurve(PointSpec::At(p), CurveSpec::Id(a)), ConstraintOf::PointOnCurve(PointSpec::At(p), CurveSpec::Id(b))])
                }
                _ if model(first) || model(second) => Dropped("to model geometry"),
                _ => {
                    if let (Some(a), Some(b)) = (pt(first), pt(second)) {
                        one(ConstraintOf::Coincident(a, b))
                    } else if let (Some(p), Some(l)) = (pt(first), cv(second)) {
                        one(ConstraintOf::PointOnCurve(p, l))
                    } else if let (Some(l), Some(p)) = (cv(first), pt(second)) {
                        one(ConstraintOf::PointOnCurve(p, l))
                    } else if let (Some(Ent::Curve(a)), Some(b)) = (first, cv(second)) {
                        self.collinear(a, b)
                    } else {
                        Dropped("unsupported references")
                    }
                }
            },
            "HORIZONTAL" | "VERTICAL" => {
                // A model vertex on the plane: the sketch point pierced there.
                let at_vertex = |e: Option<Ent>| match e {
                    Some(Ent::Vertex(l)) => self.vertices.values().find(|(_, x)| *x == l).map(|(p, _)| PointSpec::At(*p)),
                    e => pt(e),
                };
                let o = match (first, second) {
                    (Some(Ent::Curve(l)), None) => Some(Orient::Line(CurveSpec::Id(l))),
                    _ => at_vertex(first).zip(at_vertex(second)).map(|(a, b)| Orient::Points(a, b)),
                };
                let Some(swap) = self.swap else {
                    return Dropped("the sketch's axes are turned against cadrs's");
                };
                match o {
                    Some(o) if (kind == "HORIZONTAL") != swap => one(ConstraintOf::Horizontal(o)),
                    Some(o) => one(ConstraintOf::Vertical(o)),
                    None if model(first) || model(second) => Dropped("to model geometry"),
                    None if matches!(first, Some(Ent::Vertex(_))) || matches!(second, Some(Ent::Vertex(_))) => Dropped("level with a model vertex: cadrs has no such reference yet"),
                    None => Dropped("unsupported references"),
                }
            }
            "PARALLEL" | "PERPENDICULAR" | "TANGENT" | "EQUAL" | "CONCENTRIC" => match (cv(first), cv(second)) {
                (Some(a), Some(b)) => one(match kind {
                    "PARALLEL" => ConstraintOf::Parallel(a, b),
                    "PERPENDICULAR" => ConstraintOf::Perpendicular(a, b),
                    "TANGENT" => ConstraintOf::Tangent(a, b),
                    "EQUAL" => ConstraintOf::Equal(a, b),
                    _ => ConstraintOf::Concentric(a, b),
                }),
                // A point concentric with a circle (a circle's centre with a model edge): on its
                // centre.
                _ if kind == "CONCENTRIC" && matches!((first, second), (Some(Ent::Point(..)), Some(Ent::Curve(_))) | (Some(Ent::Curve(_)), Some(Ent::Point(..)))) => {
                    let (p, cid) = match (first, second) {
                        (Some(Ent::Point(..)), Some(Ent::Curve(c))) => (pt(first), c),
                        (_, _) => (pt(second), if let Some(Ent::Curve(c)) = first { c } else { unreachable!() }),
                    };
                    match (p, self.center_of(cid)) {
                        (Some(p), Some(c)) => one(ConstraintOf::Coincident(p, c)),
                        _ => Dropped("no centre"),
                    }
                }
                _ if kind == "CONCENTRIC" && matches!(second, Some(Ent::Origin)) => match first {
                    Some(Ent::Curve(cid)) => self.center_of(cid).map_or(Dropped("no centre"), |p| one(ConstraintOf::Coincident(p, PointSpec::Origin))),
                    _ => Dropped("unsupported references"),
                },
                _ if model(first) || model(second) => Dropped("to model geometry"),
                _ => Dropped("unsupported references"),
            },
            "MIDPOINT" => {
                let e1 = self.ent(c, "localEntity1");
                let e2 = self.ent(c, "localEntity2");
                let mid = self.ent(c, "localMidpoint");
                match (e1, e2, mid) {
                    (a, b, Some(m)) if pt(a).is_some() && pt(b).is_some() && pt(Some(m)).is_some() => {
                        one(ConstraintOf::Center(pt(Some(m)).unwrap(), pt(a).unwrap(), pt(b).unwrap()))
                    }
                    (Some(p @ (Ent::Point(..) | Ent::Origin)), Some(Ent::Curve(l)), None) | (Some(Ent::Curve(l)), Some(p @ (Ent::Point(..) | Ent::Origin)), None) => {
                        one(ConstraintOf::Midpoint(pt(Some(p)).unwrap(), CurveSpec::Id(l)))
                    }
                    _ if model(e1) || model(e2) => Dropped("to model geometry"),
                    _ => Dropped("unsupported references"),
                }
            }
            "FIX" => match first {
                Some(Ent::Point(_, p)) => one(ConstraintOf::FixPoint(PointSpec::At(p))),
                Some(Ent::Curve(l)) => one(ConstraintOf::FixCurve(CurveSpec::Id(l))),
                _ => Dropped("unsupported references"),
            },
            "MIRROR" => {
                let axis = self.ent(c, "localMirror");
                let Some(axis) = cv(axis) else {
                    return if model(axis) { Dropped("mirror line on the model") } else { Dropped("unsupported mirror line") };
                };
                match (first, second) {
                    (a, b) if pt(a).is_some() && pt(b).is_some() => one(ConstraintOf::SymmetricPoints(pt(a).unwrap(), pt(b).unwrap(), axis)),
                    (Some(Ent::Curve(a)), Some(Ent::Curve(b))) => one(ConstraintOf::SymmetricCurves(CurveSpec::Id(a), CurveSpec::Id(b), axis)),
                    _ => Dropped("unsupported references"),
                }
            }
            // Onshape's Offset: an offset curve parallel to (or concentric with) its master, and,
            // with a second pair, as far from its master as the first is from its own (one
            // distance for the Offset tool's curves; the first pair's is a Distance of its own).
            "OFFSET" => {
                let round = |e: Option<Ent>| matches!(e, Some(Ent::Curve(k)) if matches!(self.g.curves.get(k).map(|x| x.kind), Some(CurveKind::Circle { .. } | CurveKind::Arc { .. })));
                // Each offset curve with its own master (a corner's second side has its own).
                let master = self.ent(c, "localMaster");
                let second = self.ent(c, "localSecond").or(master);
                let mut out = Vec::new();
                for (offset, master) in [(self.ent(c, "localOffset"), master), (self.ent(c, "localSecondOffset"), second)] {
                    if offset.is_none() {
                        continue;
                    }
                    match (cv(offset), cv(master)) {
                        (Some(a), Some(b)) if round(offset) && round(master) => out.push(ConstraintOf::Concentric(a, b)),
                        (Some(a), Some(b)) if !round(offset) && !round(master) => out.push(ConstraintOf::Parallel(a, b)),
                        _ if model(master) => return Dropped("offset of a model edge cadrs didn't find"),
                        _ => return Dropped("unsupported references"),
                    }
                }
                let (o1, o2) = (self.ent(c, "localOffset"), self.ent(c, "localSecondOffset"));
                if let (Some(m1), Some(o1), Some(m2), Some(o2)) = (cv(master), cv(o1), cv(second), cv(o2)) {
                    out.push(ConstraintOf::EqualOffset(m1, o1, m2, o2));
                }
                if out.is_empty() { Dropped("unsupported references") } else { Specs(out) }
            }
            // A sketch Linear pattern (one row): each copy of a curve the seed's size (a line its
            // direction too), each row of points equally spaced along its construction line, the
            // rows' lines alike. (The step is the pattern's own direction line, already tied to
            // a seed and its first copy.)
            "LINEAR_PATTERN" => {
                let (instances, n1, n2) = pattern_instances(c);
                let Some(lines) = c["entityId"].as_str().and_then(|k| self.pattern_lines.get(k)) else {
                    return Dropped(if n2 > 1 { "two-way pattern: no cadrs equivalent yet" } else { "unsupported references" });
                };
                let mut out = Vec::new();
                let groups: std::collections::BTreeSet<usize> = instances.keys().map(|k| k.0).collect();
                for g in groups {
                    let ent = |i: usize| instances.get(&(g, i, 0)).and_then(|v| self.map.points.get(v).map(|p| Ent::Point(*p, self.g.points.get(*p).map(|x| x.pos).unwrap_or_default())).or_else(|| self.map.curves.get(v).map(|k| Ent::Curve(*k))));
                    match lines.iter().find(|(lg, _)| *lg == g) {
                        // A row of points.
                        Some((_, line)) => {
                            let l = CurveSpec::Id(*line);
                            for i in 1..n1 - 1 {
                                let (Some(p), Some(a), Some(b)) = (pt(ent(i)), pt(ent(i - 1)), pt(ent(i + 1))) else { return Dropped("unsupported references") };
                                out.push(ConstraintOf::PointOnCurve(p, l));
                                out.push(ConstraintOf::EqualDistance(p, a, b));
                            }
                        }
                        // A copied curve.
                        None => {
                            let Some(Ent::Curve(seed)) = ent(0) else { continue };
                            let line = matches!(self.g.curves.get(seed).map(|x| x.kind), Some(CurveKind::Line { .. }));
                            for i in 1..n1 {
                                let Some(Ent::Curve(k)) = ent(i) else { return Dropped("unsupported references") };
                                out.push(ConstraintOf::Equal(CurveSpec::Id(k), CurveSpec::Id(seed)));
                                if line {
                                    out.push(ConstraintOf::Parallel(CurveSpec::Id(k), CurveSpec::Id(seed)));
                                }
                            }
                        }
                    }
                }
                // The rows alike.
                if let Some(((_, first), rest)) = lines.split_first() {
                    for (_, l) in rest {
                        out.push(ConstraintOf::Parallel(CurveSpec::Id(*l), CurveSpec::Id(*first)));
                        out.push(ConstraintOf::Equal(CurveSpec::Id(*l), CurveSpec::Id(*first)));
                    }
                }
                if out.is_empty() { Dropped("unsupported references") } else { Specs(out) }
            }
            "PROJECTED" => match first {
                // The projected curve keeps its place; the link to the model is not imported.
                Some(Ent::Curve(l)) => one(ConstraintOf::FixCurve(CurveSpec::Id(l))),
                Some(Ent::Point(_, p)) => one(ConstraintOf::FixPoint(PointSpec::At(p))),
                _ => Dropped("unsupported references"),
            },
            "DISTANCE" | "LENGTH" | "DIAMETER" | "RADIUS" | "ANGLE" => {
                // A driven one shows its measured value and holds nothing.
                let driven = c["parameters"].as_array().into_iter().flatten().any(|p| p["parameterId"] == "driven" && p["value"] == true);
                match self.dimension(kind, c, first, second) {
                    Some(mut d) => {
                        d.driven = driven;
                        Dimension(d)
                    }
                    None if model(first) || model(second) => Dropped("to model geometry"),
                    None => Dropped("unsupported references"),
                }
            }
            _ => Dropped("no cadrs equivalent"),
        }
    }

    /// An Onshape dimension direction in cadrs's sketch axes (`None`: aligned, which holds in
    /// any frame).
    fn hv<'s>(&self, dir: Option<&'s str>) -> Option<&'s str> {
        match (dir, self.swap) {
            (Some("HORIZONTAL"), Some(true)) => Some("VERTICAL"),
            (Some("VERTICAL"), Some(true)) => Some("HORIZONTAL"),
            (Some("HORIZONTAL" | "VERTICAL"), None) => None,
            (d, _) => d,
        }
    }

    fn center_of(&self, c: CurveId) -> Option<PointSpec> {
        match self.g.curves.get(c)?.kind {
            CurveKind::Circle { center, .. } | CurveKind::Arc { center, .. } => Some(PointSpec::At(self.g.points.get(center)?.pos)),
            _ => None,
        }
    }

    /// Two curves "coincident": collinear lines (both ends of the first on the second), or the
    /// same circle.
    fn collinear(&self, a: CurveId, b: CurveSpec) -> Converted {
        match (self.g.curves.get(a).map(|c| c.kind), b) {
            (Some(CurveKind::Line { a: p, b: q }), b) => Converted::Specs(
                [p, q]
                    .into_iter()
                    .filter_map(|p| self.g.points.get(p))
                    .map(|p| ConstraintOf::PointOnCurve(PointSpec::At(p.pos), b))
                    .collect(),
            ),
            (Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }), CurveSpec::Id(b)) => {
                Converted::Specs(vec![ConstraintOf::Concentric(CurveSpec::Id(a), CurveSpec::Id(b)), ConstraintOf::Equal(CurveSpec::Id(a), CurveSpec::Id(b))])
            }
            _ => Converted::Dropped("unsupported references"),
        }
    }

    fn param_str(c: &Value, id: &str) -> Option<String> {
        c["parameters"].as_array()?.iter().find(|p| p["parameterId"] == id).and_then(|p| p["value"].as_str().map(String::from))
    }

    fn dimension(&self, kind: &str, c: &Value, first: Option<Ent>, second: Option<Ent>) -> Option<Dimension> {
        let pref = |e: Ent| match e {
            Ent::Point(p, _) => Some(PointRef::Point(p)),
            Ent::Origin => Some(PointRef::Origin),
            _ => None,
        };
        let cref = |e: Ent| match e {
            Ent::Curve(c) => Some(CurveRef::Curve(c)),
            Ent::XAxis => Some(CurveRef::XAxis),
            Ent::YAxis => Some(CurveRef::YAxis),
            _ => None,
        };
        let line_ends = |c: CurveId| match self.g.curves.get(c)?.kind {
            CurveKind::Line { a, b } => Some((a, b)),
            _ => None,
        };
        let is_round = |c: CurveId| matches!(self.g.curves.get(c).map(|c| c.kind), Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }));
        let kind = match kind {
            "LENGTH" => {
                let Some(Ent::Curve(l)) = first else { return None };
                let (a, b) = line_ends(l)?;
                match self.hv(Self::param_str(c, "direction").as_deref()) {
                    Some("HORIZONTAL") => DimensionKind::Horizontal { a, b },
                    Some("VERTICAL") => DimensionKind::Vertical { a, b },
                    _ => DimensionKind::Aligned { a, b },
                }
            }
            "DIAMETER" => match first? {
                Ent::Curve(c) if is_round(c) => DimensionKind::Diameter { curve: c },
                _ => return None,
            },
            "RADIUS" => match first? {
                Ent::Curve(c) if is_round(c) => DimensionKind::Radius { curve: c },
                _ => return None,
            },
            "ANGLE" => {
                let (a, b) = (cref(first?)?, cref(second?)?);
                DimensionKind::Angle { a, b, flip_a: false, flip_b: false }
            }
            _ => {
                // DISTANCE between two of: points, lines, circles.
                let (f, s) = (first?, second?);
                let dir = Self::param_str(c, "direction");
                match (f, s) {
                    (Ent::Point(a, _), Ent::Point(b, _)) => match self.hv(dir.as_deref()) {
                        Some("HORIZONTAL") => DimensionKind::Horizontal { a, b },
                        Some("VERTICAL") => DimensionKind::Vertical { a, b },
                        _ => DimensionKind::Aligned { a, b },
                    },
                    (p @ (Ent::Point(..) | Ent::Origin), l) | (l, p @ (Ent::Point(..) | Ent::Origin))
                        if cref(l).is_some() && !matches!(l, Ent::Curve(c) if is_round(c)) =>
                    {
                        DimensionKind::PointLine { p: pref(p)?, line: cref(l)? }
                    }
                    (Ent::Curve(a), Ent::Curve(b)) if !is_round(a) && !is_round(b) => {
                        // Parallel lines: from an end of the first to the second.
                        let (p, _) = line_ends(a)?;
                        DimensionKind::PointLine { p: PointRef::Point(p), line: CurveRef::Curve(b) }
                    }
                    (l @ (Ent::XAxis | Ent::YAxis), Ent::Curve(b)) | (Ent::Curve(b), l @ (Ent::XAxis | Ent::YAxis)) if !is_round(b) => {
                        let (p, _) = line_ends(b)?;
                        DimensionKind::PointLine { p: PointRef::Point(p), line: cref(l)? }
                    }
                    (p @ (Ent::Point(..) | Ent::Origin), Ent::Curve(circle)) | (Ent::Curve(circle), p @ (Ent::Point(..) | Ent::Origin)) if is_round(circle) => {
                        DimensionKind::PointCircle { p: pref(p)?, circle, far: false }
                    }
                    (l, Ent::Curve(circle)) | (Ent::Curve(circle), l) if is_round(circle) && cref(l).is_some() && !matches!(l, Ent::Curve(x) if is_round(x)) => {
                        DimensionKind::LineCircle { line: cref(l)?, circle, far: false }
                    }
                    (Ent::Curve(a), Ent::Curve(b)) if is_round(a) && is_round(b) => DimensionKind::CircleCircle { a, b, far_a: false, far_b: false, axis: None },
                    _ => return None,
                }
            }
        };
        // Drive the geometry where it already is: the measured value.
        let value = cadrs_sketch::dimension::measure(self.g, kind)?;
        Some(Dimension::new(kind, value, 10.0))
    }
}

/// A sketch Linear pattern's instances: `localInstance<entity>,<i>,<j>` → its entity id, with
/// the pattern's counts.
fn pattern_instances(c: &Value) -> (std::collections::BTreeMap<(usize, usize, usize), String>, usize, usize) {
    let mut out = std::collections::BTreeMap::new();
    let mut counts = (0, 0);
    for p in c["parameters"].as_array().into_iter().flatten() {
        let pid = p["parameterId"].as_str().unwrap_or_default();
        if let Some(rest) = pid.strip_prefix("localInstance")
            && let [g, i, j] = rest.split(',').filter_map(|x| x.parse::<usize>().ok()).collect::<Vec<_>>().as_slice()
            && let Some(v) = p["value"].as_str()
        {
            out.insert((*g, *i, *j), v.to_string());
        }
        let n = || p["expression"].as_str().and_then(|e| e.trim().parse::<f64>().ok()).map(|x| x as usize).unwrap_or(0);
        match pid {
            "patternc1" => counts.0 = n(),
            "patternc2" => counts.1 = n(),
            _ => {}
        }
    }
    (out, counts.0, counts.1)
}

/// For each sketch Linear pattern (one row of copies): a construction line through each of its
/// rows of points (a copied circle's centres), from the seed's to the last copy's, which the
/// pattern's constraints keep the points on.
fn pattern_lines(s: &mut dyn Studio, el: ElementId, id: FeatureId, feature: &Value, map: &SketchMap) -> Result<HashMap<String, Vec<(usize, CurveId)>>, CommandError> {
    let mut out = HashMap::new();
    for c in feature["constraints"].as_array().into_iter().flatten().filter(|c| c["constraintType"].as_str() == Some("LINEAR_PATTERN")) {
        let (instances, n1, n2) = pattern_instances(c);
        if n1 < 3 || n2 > 1 {
            continue;
        }
        let groups: std::collections::BTreeSet<usize> = instances.keys().map(|k| k.0).collect();
        let mut lines = Vec::new();
        for g in groups {
            let g_now = sketch_of(s, el, id)?;
            let pos = |i: usize| instances.get(&(g, i, 0)).and_then(|v| map.points.get(v)).and_then(|p| g_now.points.get(*p)).map(|p| p.pos);
            let (Some(a), Some(b)) = (pos(0), pos(n1 - 1)) else { continue };
            let before: std::collections::HashSet<CurveId> = g_now.curves.keys().collect();
            s.run(&EditSketch { element: el, feature: id, op: SketchOp::AddPolyline { points: vec![a, b], closed: false, construction: true, label: "Add line" } })?;
            if let Some(line) = sketch_of(s, el, id)?.curves.keys().find(|k| !before.contains(k)) {
                lines.push((g, line));
            }
        }
        if let Some(key) = c["entityId"].as_str() {
            out.insert(key.to_string(), lines);
        }
    }
    Ok(out)
}
