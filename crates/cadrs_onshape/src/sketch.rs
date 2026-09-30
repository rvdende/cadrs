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

/// The plane an Onshape sketch lies on, as the default plane it names, if it is one.
pub fn default_plane(feature: &Value) -> Option<PlaneRef> {
    let ids = sketch_plane_ids(feature);
    match ids.first().map(String::as_str) {
        Some(TOP_ID) => Some(PlaneRef::Top),
        Some(FRONT_ID) => Some(PlaneRef::Front),
        Some(RIGHT_ID) => Some(PlaneRef::Right),
        _ => None,
    }
}

fn sketch_plane_ids(feature: &Value) -> Vec<String> {
    param(feature, "sketchPlane")
        .and_then(|p| p["queries"].as_array())
        .into_iter()
        .flatten()
        .flat_map(|q| q["deterministicIds"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from))
        .collect()
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
                let span = (e["endParameter"].as_f64().unwrap_or(0.0) - e["startParameter"].as_f64().unwrap_or(0.0)).abs();
                let (c, a, b) = (xf.apply(c), xf.apply(a), xf.apply(b));
                // cadrs arcs run counter-clockwise from start to end: pick the order whose
                // sweep matches Onshape's.
                let ccw = |p: Vec2, q: Vec2| {
                    let t = (q.y - c.y).atan2(q.x - c.x) - (p.y - c.y).atan2(p.x - c.x);
                    t.rem_euclid(TAU)
                };
                let forward = (ccw(a, b) - span).abs() <= (ccw(b, a) - span).abs();
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

    // Constraints and dimensions.
    // Model edges the constraints refer to (Onshape keeps them as bare topology ids): found by
    // where the constrained points are, and brought in as construction Use curves.
    let uses = model_edges(s, el, id, feature, &map, &g, &plane, parts, report)?;
    let g = sketch_of(s, el, id)?;
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
    let ctx = Ctx { map: &map, g: &g, plane: &plane, swap, uses: &uses };
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
            Ok(()) => moved += 1,
            Err(_) => refused += 1,
        }
    }
    (kept, moved, refused)
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
    report: &mut FeatureReport,
) -> Result<HashMap<String, CurveId>, CommandError> {
    // Per edge id: the points on it, and the points at its middle.
    let mut hints: std::collections::BTreeMap<String, (Vec<Vec2>, Vec<Vec2>)> = Default::default();
    let point_of = |v: &str| map.points.get(v).and_then(|p| g.points.get(*p)).map(|p| p.pos);
    let curve_points = |v: &str| -> Vec<Vec2> {
        match map.curves.get(v).and_then(|c| g.curves.get(*c)).map(|c| c.kind) {
            Some(CurveKind::Line { a, b }) => [a, b].iter().filter_map(|p| g.points.get(*p)).map(|p| p.pos).collect(),
            _ => Vec::new(),
        }
    };
    for c in feature["constraints"].as_array().into_iter().flatten() {
        let params = c["parameters"].as_array().cloned().unwrap_or_default();
        let midpoint = c["constraintType"].as_str() == Some("MIDPOINT");
        let mut local_points = Vec::new();
        for p in &params {
            if p["parameterId"].as_str().is_some_and(|i| i.starts_with("local"))
                && let Some(v) = p["value"].as_str()
            {
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
            if matches!(*eid, ORIGIN_ID | TOP_ID | FRONT_ID | RIGHT_ID) {
                continue;
            }
            let h = hints.entry(eid.to_string()).or_default();
            if midpoint {
                h.1.extend(local_points.iter().copied());
            } else {
                h.0.extend(local_points.iter().copied());
            }
        }
    }
    let frame = plane.frame();
    let tol = 1e-4;
    let mut out = HashMap::new();
    let mut unmatched = 0;
    for (eid, (on, mids)) in hints {
        if on.is_empty() && mids.is_empty() {
            unmatched += 1;
            continue;
        }
        // The edge satisfying the most hints (then the one closest to them).
        let mut best: Option<(usize, f64, cadrs_sketch::projection::Projected, cadrs_sketch::Link)> = None;
        for part in parts {
            for e in &part.solid.edges {
                let Some(shape) = cadrs_core::links::edge_curve(e).and_then(|c| cadrs_core::links::project(c, &frame)) else { continue };
                let mut hits = 0;
                let mut err = 0.0;
                for p in &on {
                    let d = cadrs_core::links::projected_distance(&shape, *p).unwrap_or(f64::MAX);
                    if d < tol {
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
                if hits == 0 {
                    continue;
                }
                if best.as_ref().is_none_or(|(h, e2, ..)| hits > *h || (hits == *h && err < *e2)) {
                    best = Some((hits, err, shape, cadrs_sketch::Link::Edge { feature: part.id.feature.0, edge: e.name }));
                }
            }
        }
        let Some((hits, _, shape, link)) = best else {
            unmatched += 1;
            continue;
        };
        if hits < on.len() + mids.len() {
            report.notes.push(format!("model edge {eid}: {hits} of {} constrained points on it", on.len() + mids.len()));
        }
        let before: std::collections::HashSet<CurveId> = sketch_of(s, el, id)?.curves.keys().collect();
        if s.run(&EditSketch { element: el, feature: id, op: SketchOp::Use { items: vec![(shape, link)] } }).is_err() {
            unmatched += 1;
            continue;
        }
        let after = sketch_of(s, el, id)?;
        if let Some(c) = after.curves.keys().find(|c| !before.contains(c)) {
            s.run(&EditSketch { element: el, feature: id, op: SketchOp::SetConstruction { curves: vec![c], construction: true } })?;
            out.insert(eid, c);
        }
    }
    if unmatched > 0 {
        report.notes.push(format!("{unmatched} model edge(s) not found"));
    }
    Ok(out)
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
                    [ORIGIN_ID] => Some(Ent::Origin),
                    [plane @ (TOP_ID | FRONT_ID | RIGHT_ID)] => Some(self.axis_of_plane(plane)),
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
                let o = match (first, second) {
                    (Some(Ent::Curve(l)), None) => Some(Orient::Line(CurveSpec::Id(l))),
                    _ => pt(first).zip(pt(second)).map(|(a, b)| Orient::Points(a, b)),
                };
                let Some(swap) = self.swap else {
                    return Dropped("the sketch's axes are turned against cadrs's");
                };
                match o {
                    Some(o) if (kind == "HORIZONTAL") != swap => one(ConstraintOf::Horizontal(o)),
                    Some(o) => one(ConstraintOf::Vertical(o)),
                    None if model(first) || model(second) => Dropped("to model geometry"),
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
            "PROJECTED" => match first {
                // The projected curve keeps its place; the link to the model is not imported.
                Some(Ent::Curve(l)) => one(ConstraintOf::FixCurve(CurveSpec::Id(l))),
                Some(Ent::Point(_, p)) => one(ConstraintOf::FixPoint(PointSpec::At(p))),
                _ => Dropped("unsupported references"),
            },
            "DISTANCE" | "LENGTH" | "DIAMETER" | "RADIUS" | "ANGLE" => {
                if c["parameters"].as_array().into_iter().flatten().any(|p| p["parameterId"] == "driven" && p["value"] == true) {
                    return Dropped("driven dimension");
                }
                match self.dimension(kind, c, first, second) {
                    Some(d) => Dimension(d),
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
                    (Ent::Curve(a), Ent::Curve(b)) if is_round(a) && is_round(b) => DimensionKind::CircleCircle { a, b, far_a: false, far_b: false },
                    _ => return None,
                }
            }
        };
        // Drive the geometry where it already is: the measured value.
        let value = cadrs_sketch::dimension::measure(self.g, kind)?;
        Some(Dimension::new(kind, value, 10.0))
    }
}
