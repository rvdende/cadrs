//! Rebuilding the surfacing features (`crate::surfacing`): Thicken, Helix and Fill. A child
//! of `rebuild::kernel_ops`, so it shares its helpers (face lookup, merging).

use super::applied::edge_ids;
use super::*;
use cadrs_kernel::{FillCurve, FillSpec, Kernel, ThickenSpec};

use crate::surfacing::{FillEdge, FillFeature, HelixFeature, HelixGeom, HelixType, ThickenFeature};

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn norm(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: Vec3) -> Vec3 {
    let l = norm(a);
    if l < 1e-15 { a } else { scale(a, 1.0 / l) }
}

/// `v` made square to the unit `axis` (and unit), else a direction square to it.
fn square_to(v: Vec3, axis: Vec3) -> Vec3 {
    for c in [v, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        let w = sub(c, scale(axis, dot(c, axis)));
        if norm(w) > 1e-6 {
            return unit(w);
        }
    }
    [1.0, 0.0, 0.0]
}

impl Rebuilder {
    // -----------------------------------------------------------------------------------------
    // Thicken

    pub(in crate::rebuild) fn thicken(&mut self, before: &[Feature], id: FeatureId, x: &ThickenFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let (along, against) = x.sides();
        let mut spec = ThickenSpec { bodies: Vec::new(), faces: Vec::new(), profiles: Vec::new(), along, against, source: id.0.as_u128() as u64 };
        let mut missing = 0;
        let mut consumed: Vec<PartId> = Vec::new();
        for p in &x.parts {
            match state.part(*p).and_then(|s| s.body) {
                Some(b) => {
                    spec.bodies.push(b);
                    consumed.push(*p);
                }
                None => missing += 1,
            }
        }
        for f in &x.faces {
            match face_ids(state, f) {
                Some((part, ids)) => {
                    let Some(body) = part.body else {
                        missing += 1;
                        continue;
                    };
                    for face in ids {
                        spec.faces.push(FaceInput { body, face, source: 0 });
                    }
                    if part.part.kind == PartKind::Surface && !consumed.contains(&part.part.id) {
                        consumed.push(part.part.id);
                    }
                }
                None => missing += 1,
            }
        }
        let (groups, lost_regions) = sweep_groups(before, &x.regions, &x.sketches, BodyType::Solid);
        missing += lost_regions;
        spec.profiles = groups.iter().map(crate::brep::profile).collect();
        if spec.bodies.is_empty() && spec.faces.is_empty() && spec.profiles.is_empty() {
            return Err("The selected surfaces or faces no longer exist".into());
        }
        let op = id.0;
        let r = self.kernel.thicken_surfaces(&spec).map_err(|e| format!("Thicken failed: {e}"))?;
        let tool = self.name_new(state, op, &r).map_err(|e| format!("Thicken failed: {e}"))?;
        let merge = Merge { op: x.op, merge_all: x.merge_all, scope: &x.merge_scope, surface: false };
        let geoms = state.geoms.clone();
        let mut o = self.combine(id, &merge, tool, state, geoms)?;
        if !x.keep_tools && !consumed.is_empty() {
            let mut next = (*o.state).clone();
            next.parts.retain(|p| !consumed.contains(&p.part.id));
            o.state = Arc::new(next);
        }
        if missing > 0 {
            o.warning = o.warning.or(Some("A selected surface or face no longer exists".into()));
        }
        Ok(o)
    }

    // -----------------------------------------------------------------------------------------
    // Helix

    /// The helix's geometry from its inputs.
    fn helix_geom(&self, before: &[Feature], state: &State, x: &HelixFeature) -> Result<HelixGeom, String> {
        let lost = || "The helix's face or axis no longer exists".to_string();
        let start_angle = x.start_angle.to_radians();
        match x.helix_type {
            HelixType::CylinderCone => {
                let f = x.face.as_ref().ok_or("Select a cylindrical or conical face")?;
                let (part, _) = face_ids(state, f).ok_or_else(lost)?;
                let solid = &part.part.solid;
                let (i, _) = solid.resolve_face(&f.face, None, Some(f.seed)).map_err(|_| lost())?;
                let face = &solid.faces[i];
                let (o, d) = face.axis.ok_or("The face must be cylindrical or conical")?;
                let d = unit(d);
                // The face's extent along its axis and its radius at each end.
                let pts: Vec<Vec3> = face.loops.iter().flatten().copied().collect();
                if pts.is_empty() {
                    return Err("The face has no boundary".into());
                }
                let t = |p: &Vec3| dot(sub(*p, o), d);
                let (tmin, tmax) = pts.iter().map(t).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
                let height = tmax - tmin;
                if height < 1e-6 {
                    return Err("The face has no height along its axis".into());
                }
                let radius_near = |t0: f64| {
                    let near: Vec<f64> = pts
                        .iter()
                        .filter(|p| (t(p) - t0).abs() < 1e-6 * (1.0 + height))
                        .map(|p| norm(sub(sub(*p, o), scale(d, t(p)))))
                        .collect();
                    if near.is_empty() { None } else { Some(near.iter().sum::<f64>() / near.len() as f64) }
                };
                let (rmin, rmax) = (radius_near(tmin).ok_or_else(lost)?, radius_near(tmax).ok_or_else(lost)?);
                // It starts at the end on the sketch plane of the extrude (or revolve) that made
                // the face, with angle 0 along that sketch's x axis.
                let frame = state.geoms.get(&f.face.op).and_then(|g| g.sketch_frame());
                let (lo, hi) = (add(o, scale(d, tmin)), add(o, scale(d, tmax)));
                let mut start_low = true;
                let mut x_ref = square_to([1.0, 0.0, 0.0], d);
                if let Some(fr) = frame {
                    let n = unit(fr.normal());
                    let off = |p: Vec3| dot(sub(p, fr.origin), n).abs();
                    start_low = off(lo) <= off(hi);
                    x_ref = square_to(fr.u, d);
                }
                if x.flip {
                    start_low = !start_low;
                }
                let (origin, axis, r0, r1) = if start_low { (lo, d, rmin, rmax) } else { (hi, scale(d, -1.0), rmax, rmin) };
                let (turns, h) = x.turns_and_height(height);
                // Past the face's end (Turns and pitch): a cone keeps its taper.
                let r_end = r0 + (r1 - r0) * h / height;
                Ok(HelixGeom { origin, axis, x: x_ref, r0, r1: r_end, height: h, turns, start_angle, clockwise: x.clockwise })
            }
            HelixType::Axis | HelixType::Circle => {
                let a = x.axis.as_ref().ok_or("Select an axis")?;
                let ax = self.axis(before, state, a)?;
                let d0 = [ax.dir.x, ax.dir.y, ax.dir.z];
                let d = if x.flip { scale(d0, -1.0) } else { d0 };
                let origin = [ax.origin.x, ax.origin.y, ax.origin.z];
                let radius = if x.helix_type == HelixType::Circle { self.circle_radius(before, state, a).ok_or("Select a circle")? } else { x.radius };
                let (turns, h) = x.turns_and_height(x.height);
                Ok(HelixGeom {
                    origin,
                    axis: d,
                    x: square_to([1.0, 0.0, 0.0], d),
                    r0: radius,
                    r1: radius,
                    height: h,
                    turns,
                    start_angle,
                    clockwise: x.clockwise,
                })
            }
        }
    }

    /// The radius of a circle an axis reference names (a sketch circle or arc, a circular edge).
    fn circle_radius(&self, before: &[Feature], state: &State, a: &AxisRef) -> Option<f64> {
        match a {
            AxisRef::SketchCurve { sketch, curve } => {
                let sk = before.iter().find(|f| f.id == *sketch)?.sketch()?;
                let g = &sk.geometry;
                match g.curves.get(*curve)?.kind {
                    CurveKind::Circle { radius, .. } => Some(radius),
                    CurveKind::Arc { .. } => Some(g.arc_geom(*curve)?.radius),
                    _ => None,
                }
            }
            AxisRef::Edge(r) => {
                let (part, _) = edge_ids(state, r)?;
                Some(part.part.solid.edge(&r.edge)?.circle?.radius)
            }
            _ => None,
        }
    }

    pub(in crate::rebuild) fn helix(&mut self, before: &[Feature], id: FeatureId, x: &HelixFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let g = self.helix_geom(before, state, x)?;
        if !(g.height.abs() > 1e-9 && g.turns > 0.0 && g.r0 > 0.0) {
            return Err("The helix has no size".into());
        }
        let mut next = (**state).clone();
        next.curves.insert(id, g);
        Ok(Output {
            state: Arc::new(next),
            error: None,
            warning: None,
            contacts: None,
            owned: Vec::new(),
            stage: None,
            axis: None,
            arrows: Vec::new(),
            dots: None,
            uses: Vec::new(),
        })
    }

    // -----------------------------------------------------------------------------------------
    // Fill

    pub(in crate::rebuild) fn fill(&mut self, before: &[Feature], id: FeatureId, x: &FillFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let lost = || "A boundary edge or curve no longer exists".to_string();
        let mut curves = Vec::new();
        // The surfaces the boundary edges are on (Add sews into them).
        let mut touched: Vec<PartId> = Vec::new();
        for e in &x.edges {
            match e {
                FillEdge::Edge(r) => {
                    let (part, ids) = edge_ids(state, r).ok_or_else(lost)?;
                    let body = part.body.ok_or_else(lost)?;
                    if part.part.kind == PartKind::Surface && !touched.contains(&part.part.id) {
                        touched.push(part.part.id);
                    }
                    for edge in ids {
                        curves.push(FillCurve::Edge { body, edge });
                    }
                }
                FillEdge::SketchCurve { sketch, curve } => {
                    let sk = before.iter().find(|f| f.id == *sketch).and_then(|f| f.sketch()).ok_or_else(lost)?;
                    let frame = sk.plane.ok_or_else(lost)?.frame();
                    let plane = cadrs_kernel::Plane {
                        origin: Point3::from(frame.origin),
                        x_dir: Unit::new_normalize(Vector3::from(frame.u)),
                        normal: Unit::new_normalize(Vector3::from(frame.normal())),
                    };
                    for c in super::advanced::sketch_curves2(&sk.geometry, *curve).ok_or_else(lost)? {
                        curves.push(FillCurve::Sketch { plane, curve: c });
                    }
                }
            }
        }
        let op = id.0;
        let r = self.kernel.fill(&FillSpec { curves, source: op.as_u128() as u64 }).map_err(|e| format!("Fill failed: {e}"))?;
        let tool = self.name_new(state, op, &r).map_err(|e| format!("Fill failed: {e}"))?;
        if x.add {
            // Sewn with the surfaces in the merge scope (or those the fill meets, and the ones
            // they meet in turn): a solid when they close.
            let scope: Vec<PartId> = if x.merge_scope.is_empty() { self.connected_surfaces(state, &touched) } else { x.merge_scope.clone() };
            let bodies: Vec<BodyId> = scope.iter().filter_map(|p| state.part(*p)?.body).collect();
            if !bodies.is_empty() {
                let mut all = bodies.clone();
                all.push(tool.0);
                if let Ok(sewn) = self.kernel.sew_solid(&all, 1e-3) {
                    let solid = self.name_new(state, op, &sewn).map_err(|e| format!("Fill failed: {e}"))?;
                    self.kernel.release(tool.0);
                    let merge = Merge { op: BooleanOp::New, merge_all: false, scope: &[], surface: false };
                    let geoms = state.geoms.clone();
                    let mut o = self.combine(id, &merge, solid, state, geoms)?;
                    let mut next = (*o.state).clone();
                    next.parts.retain(|p| !scope.contains(&p.part.id));
                    o.state = Arc::new(next);
                    return Ok(o);
                }
            }
        }
        let merge = Merge { op: BooleanOp::New, merge_all: false, scope: &[], surface: true };
        let geoms = state.geoms.clone();
        let mut o = self.combine(id, &merge, tool, state, geoms)?;
        if x.add {
            o.warning = Some("The fill doesn't close the surfaces it meets: it is a surface of its own".into());
        }
        Ok(o)
    }

    /// The surface parts connected to `seed` (sharing boundary points), `seed` included.
    fn connected_surfaces(&self, state: &State, seed: &[PartId]) -> Vec<PartId> {
        let surfaces: Vec<&PartState> = state.parts.iter().filter(|p| p.part.kind == PartKind::Surface).collect();
        let ends = |p: &PartState| -> Vec<Vec3> {
            p.part.solid.edges.iter().flat_map(|e| [e.points[0], *e.points.last().unwrap_or(&e.points[0])]).collect()
        };
        let mut out: Vec<PartId> = seed.to_vec();
        loop {
            let pts: Vec<Vec3> = surfaces.iter().filter(|p| out.contains(&p.part.id)).flat_map(|p| ends(p)).collect();
            let more: Vec<PartId> = surfaces
                .iter()
                .filter(|p| !out.contains(&p.part.id))
                .filter(|p| ends(p).iter().any(|q| pts.iter().any(|r| norm(sub(*q, *r)) < 1e-4)))
                .map(|p| p.part.id)
                .collect();
            if more.is_empty() {
                break;
            }
            out.extend(more);
        }
        out
    }
}
