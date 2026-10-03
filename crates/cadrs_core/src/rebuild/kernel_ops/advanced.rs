//! Rebuilding the features of P3.7: Plane (PS12), Sweep (PS19), Loft (PS20) and Split (PS18.5).
//! A child of `rebuild::kernel_ops`, so it shares its helpers (face lookup, merging, splitting).

use super::applied::edge_ids;
use super::*;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{
    Curve2, ExtrudeEnd, ExtrudeSpec, Kernel, LoftEnd, LoftSection, LoftSpec, PathCurve, SplitTool, SweepControl,
    SweepSpec,
};
use cadrs_sketch::{PlaneFrame, PlaneRef, Sketch};
use nalgebra::{Point2, Point3, Unit, Vector3};

use crate::advanced::{
    LoftCondition, LoftFeature, LoftProfile, PathRef, ProfileControl, SplitFeature, SplitToolRef, SplitType, SweepFeature,
};
use crate::plane::{PlaneEntity, PlaneFeature, RefGeom};

/// The frame of a default plane or a Plane feature built so far.
pub(in crate::rebuild) fn plane_of(state: &State, p: &PlaneRef) -> Option<PlaneFrame> {
    match p {
        PlaneRef::Feature(f) => state.planes.get(&FeatureId(f.feature)).copied(),
        PlaneRef::Face(f) => Some(f.frame()),
        p => Some(p.frame()),
    }
}

/// A kernel plane from a frame.
fn kernel_plane(f: &PlaneFrame) -> cadrs_kernel::Plane {
    cadrs_kernel::Plane {
        origin: Point3::from(f.origin),
        x_dir: Unit::new_normalize(Vector3::from(f.u)),
        normal: Unit::new_normalize(Vector3::from(f.normal())),
    }
}

/// A sketch curve as exact kernel curves in its sketch's plane (a spline: its spans).
pub(super) fn sketch_curves2(g: &Sketch, id: CurveId) -> Option<Vec<Curve2>> {
    let p = |v: Vec2| Point2::new(v.x, v.y);
    let source = Some(crate::brep::curve_source(id));
    if let CurveKind::Spline { .. } = g.curves.get(id)?.kind {
        let spans = g.spline_spans(id)?;
        return (!spans.is_empty()).then(|| {
            spans.iter().map(|b| Curve2::Bezier { poles: b.map(p), source }).collect()
        });
    }
    Some(vec![match g.curves.get(id)?.kind {
        CurveKind::Line { a, b } => Curve2::Line { a: p(g.pos(a)), b: p(g.pos(b)), source },
        CurveKind::Arc { .. } => {
            let a = g.arc_geom(id)?;
            Curve2::Arc { center: p(a.center), radius: a.radius, start_angle: a.start_angle, sweep: a.sweep, source }
        }
        CurveKind::Circle { center, radius } => Curve2::Circle { center: p(g.pos(center)), radius, source },
        CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => {
            let e = g.ellipse_geom(id)?;
            if e.offset == 0.0 {
                Curve2::Ellipse { center: p(e.center), major_radius: e.major(), minor_radius: e.minor, rotation: e.u().angle(), source }
            } else {
                Curve2::OffsetEllipseArc {
                    center: p(e.center),
                    major_radius: e.major(),
                    minor_radius: e.minor,
                    rotation: e.u().angle(),
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                    offset: e.offset,
                    source,
                }
            }
        }
        CurveKind::EllipseArc { .. } => {
            let e = g.ellipse_arc_geom(id)?;
            Curve2::EllipseArc {
                center: p(e.e.center),
                major_radius: e.e.major(),
                minor_radius: e.e.minor,
                rotation: e.e.u().angle(),
                start: e.t0,
                sweep: e.sweep,
                source,
            }
        }
        CurveKind::Bezier { .. } => Curve2::Bezier { poles: g.bezier_geom(id)?.p.map(p), source },
        CurveKind::Spline { .. } => return None,
    }])
}

/// A sketch and its plane's frame.
fn sketch_of(before: &[Feature], sketch: FeatureId) -> Option<(&Sketch, PlaneFrame)> {
    let sk = before.iter().find(|f| f.id == sketch)?.sketch()?;
    Some((&sk.geometry, sk.plane?.frame()))
}

/// The point of a vertex reference (by name, else the vertex still at the stored point).
fn vertex_point(state: &State, v: &crate::document::VertexRef) -> Option<[f64; 3]> {
    let solid = state.part(v.part).map(|p| p.part.solid.clone())?;
    solid
        .vertex(&v.vertex)
        .map(|x| x.point)
        .or_else(|| solid.vertices.iter().find(|x| crate::solid::dist3(x.point, v.point) < 1e-6).map(|x| x.point))
}

impl Rebuilder {
    // -----------------------------------------------------------------------------------------
    // Plane

    /// What a Plane feature's entity is geometrically.
    fn ref_geom(&self, before: &[Feature], state: &State, e: &PlaneEntity) -> Result<RefGeom, String> {
        let lost = || "A selected entity no longer exists".to_string();
        Ok(match e {
            PlaneEntity::Origin => RefGeom::Point([0.0; 3]),
            PlaneEntity::Plane(p) => RefGeom::Plane(plane_of(state, p).ok_or_else(lost)?),
            PlaneEntity::Face(f) => {
                let (part, ids) = face_ids(state, f).ok_or_else(lost)?;
                if let Some(fr) = face_frame(part, f) {
                    return Ok(RefGeom::Plane(fr));
                }
                // A cylindrical or conical face (the Tangent type).
                let body = part.body.ok_or_else(lost)?;
                let infos = self.kernel.faces(body).map_err(|e| e.to_string())?;
                let info = ids.iter().find_map(|id| infos.iter().find(|i| i.id == *id)).ok_or_else(lost)?;
                let axis = info.axis.ok_or("Select a planar, cylindrical or conical face")?;
                let (c, a) = ([axis.origin.x, axis.origin.y, axis.origin.z], [axis.dir.x, axis.dir.y, axis.dir.z]);
                match info.kind {
                    cadrs_kernel::SurfaceKind::Cylinder => {
                        RefGeom::Cone { point: c, axis: a, radius: info.radius.ok_or_else(lost)?, slope: 0.0 }
                    }
                    cadrs_kernel::SurfaceKind::Cone => {
                        // The radius along the axis, fitted to the face's mesh: r = r0 + k·h.
                        let solid = &part.part.solid;
                        let (i, _) = solid.resolve_face(&f.face, None, Some(f.seed)).map_err(|_| lost())?;
                        let face = &solid.faces[i];
                        let (mut n, mut sh, mut sr, mut shh, mut shr) = (0.0, 0.0, 0.0, 0.0, 0.0);
                        for t in face.first_triangle..face.first_triangle + face.triangle_count {
                            for j in 0..3 {
                                let p = solid.positions[solid.indices[3 * t + j] as usize];
                                let d = crate::solid::sub3(p, c);
                                let h = crate::solid::dot3(d, a);
                                let rad = crate::solid::sub3(d, [a[0] * h, a[1] * h, a[2] * h]);
                                let r = crate::solid::dot3(rad, rad).sqrt();
                                n += 1.0;
                                sh += h;
                                sr += r;
                                shh += h * h;
                                shr += h * r;
                            }
                        }
                        let den = n * shh - sh * sh;
                        if n < 3.0 || den.abs() < 1e-12 {
                            return Err(lost());
                        }
                        let slope = (n * shr - sh * sr) / den;
                        RefGeom::Cone { point: c, axis: a, radius: (sr - slope * sh) / n, slope }
                    }
                    _ => return Err("Select a planar, cylindrical or conical face".into()),
                }
            }
            PlaneEntity::Vertex(v) => RefGeom::Point(vertex_point(state, v).ok_or_else(lost)?),
            PlaneEntity::SketchPoint { sketch, point } => {
                let (g, frame) = sketch_of(before, *sketch).ok_or_else(lost)?;
                let p = g.points.get(*point).ok_or_else(lost)?.pos;
                RefGeom::Point(frame.to_world(p))
            }
            PlaneEntity::SketchCurve { sketch, curve } => {
                let (g, frame) = sketch_of(before, *sketch).ok_or_else(lost)?;
                let w = |v: Vec2| frame.to_world(v);
                match g.curves.get(*curve).ok_or_else(lost)?.kind {
                    CurveKind::Line { a, b } => {
                        let (pa, pb) = (w(g.pos(a)), w(g.pos(b)));
                        let d = crate::solid::sub3(pb, pa);
                        let l = crate::solid::dot3(d, d).sqrt();
                        if l < 1e-12 {
                            return Err(lost());
                        }
                        RefGeom::Line { point: pa, dir: [d[0] / l, d[1] / l, d[2] / l] }
                    }
                    CurveKind::Circle { center, radius } => RefGeom::Circle {
                        center: w(g.pos(center)),
                        normal: frame.normal(),
                        radius,
                        start: w(g.pos(center) + Vec2::new(radius, 0.0)),
                    },
                    CurveKind::Arc { center, start, .. } => {
                        let a = g.arc_geom(*curve).ok_or_else(lost)?;
                        let ccw = a.sweep >= 0.0;
                        let n = frame.normal();
                        RefGeom::Circle {
                            center: w(g.pos(center)),
                            normal: if ccw { n } else { [-n[0], -n[1], -n[2]] },
                            radius: a.radius,
                            start: w(g.pos(start)),
                        }
                    }
                    CurveKind::Ellipse { .. }
                    | CurveKind::EllipseOffset { .. }
                    | CurveKind::EllipseArc { .. }
                    | CurveKind::Spline { .. }
                    | CurveKind::Bezier { .. } => {
                        let pts = cadrs_sketch::hit::curve_polyline(g, *curve);
                        RefGeom::Curve(pts.into_iter().map(w).collect())
                    }
                }
            }
            PlaneEntity::Edge(r) => {
                let part = state
                    .part(r.part)
                    .filter(|p| p.part.solid.edge(&r.edge).is_some())
                    .or_else(|| state.parts.iter().find(|p| p.part.solid.edge(&r.edge).is_some()))
                    .ok_or_else(lost)?;
                let edge = part.part.solid.edge(&r.edge).ok_or_else(lost)?;
                // The kernel's exact curve where it has one.
                if let Some(c) = edge.circle {
                    return Ok(RefGeom::Circle { center: c.center, normal: c.normal, radius: c.radius, start: edge.points[0] });
                }
                match crate::links::edge_curve(edge) {
                    Some(crate::links::Curve3::Line(a, b)) => {
                        let d = crate::solid::sub3(b, a);
                        let l = crate::solid::dot3(d, d).sqrt().max(1e-300);
                        RefGeom::Line { point: a, dir: [d[0] / l, d[1] / l, d[2] / l] }
                    }
                    _ => RefGeom::Curve(edge.points.clone()),
                }
            }
        })
    }

    /// The Plane feature (PS12.2): its frame from its entities.
    pub(in crate::rebuild) fn plane(&mut self, before: &[Feature], id: FeatureId, x: &PlaneFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let geoms = x.entities.iter().map(|e| self.ref_geom(before, state, e)).collect::<Result<Vec<_>, _>>()?;
        let frame = crate::plane::frame(x, &geoms)?;
        let mut next = (**state).clone();
        next.planes.insert(id, frame);
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
    // Sweep

    /// A sweep path's curves as kernel path curves.
    fn path_curves(&self, before: &[Feature], state: &State, path: &[PathRef]) -> Result<Vec<PathCurve>, String> {
        let lost = || "A path curve no longer exists".to_string();
        let mut out = Vec::new();
        for p in path {
            match p {
                PathRef::Edge(r) => {
                    let (part, ids) = edge_ids(state, r).ok_or_else(lost)?;
                    let body = part.body.ok_or("A part has no body")?;
                    for edge in ids {
                        out.push(PathCurve::Edge { body, edge });
                    }
                }
                PathRef::SketchCurve { sketch, curve } => {
                    let (g, frame) = sketch_of(before, *sketch).ok_or_else(lost)?;
                    for c in sketch_curves2(g, *curve).ok_or_else(lost)? {
                        out.push(PathCurve::Sketch { plane: kernel_plane(&frame), curve: c });
                    }
                }
                PathRef::Curve(f) => {
                    let h = state.curves.get(f).ok_or_else(lost)?;
                    for (poles, weights) in h.beziers() {
                        out.push(PathCurve::Bezier3 { poles: poles.iter().map(|p| Point3::from(*p)).collect(), weights });
                    }
                }
                PathRef::Sketch(sketch) => {
                    let (g, frame) = sketch_of(before, *sketch).ok_or_else(lost)?;
                    for (cid, c) in &g.curves {
                        if c.construction {
                            continue;
                        }
                        for c in sketch_curves2(g, cid).unwrap_or_default() {
                            out.push(PathCurve::Sketch { plane: kernel_plane(&frame), curve: c });
                        }
                    }
                }
            }
        }
        if out.is_empty() {
            return Err(lost());
        }
        Ok(out)
    }

    /// Names a new body the feature `op` made (its inputs: the parts before it).
    pub(super) fn name_new(&self, state: &State, op: cadrs_kernel::OpId, r: &OpResult) -> cadrs_kernel::Result<(BodyId, BodyNames)> {
        let body = r.bodies[0];
        let inputs: Vec<(BodyId, &BodyNames)> = state.parts.iter().filter_map(|p| Some((p.body?, &*p.names))).collect();
        Ok((body, naming::name_body(&self.kernel, body, op, &r.history, &inputs)?))
    }

    /// Fuses the bodies a feature made into one (releasing them).
    fn fuse_made(&mut self, op: cadrs_kernel::OpId, what: &str, mut made: Vec<(BodyId, BodyNames)>) -> Result<(BodyId, BodyNames), String> {
        if made.len() == 1 {
            return Ok(made.pop().expect("one"));
        }
        let (first_b, _) = made[0];
        let rest: Vec<BodyId> = made[1..].iter().map(|(b, _)| *b).collect();
        let merged = self.kernel.boolean(BoolOp::Union, first_b, &rest).and_then(|res| {
            let body = res.bodies[0];
            let inputs: Vec<(BodyId, &BodyNames)> = made.iter().map(|(b, n)| (*b, n)).collect();
            Ok((body, naming::name_body(&self.kernel, body, op, &res.history, &inputs)?))
        });
        for (b, _) in &made {
            self.kernel.release(*b);
        }
        merged.map_err(|err| format!("{what} failed: {err}"))
    }

    /// The Sweep feature (PS19): each profile group swept along the path, fused, then combined
    /// with the parts like an extrude.
    pub(in crate::rebuild) fn sweep(&mut self, before: &[Feature], id: FeatureId, x: &SweepFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let (groups, missing) = sweep_groups(before, &x.regions, &x.sketches, x.body);
        let faces = self.face_inputs(&x.faces, state)?;
        if groups.is_empty() && faces.is_empty() {
            return Err("The selected profile no longer exists".into());
        }
        let path = self.path_curves(before, state, &x.path)?;
        let op = id.0;
        let control = match x.control {
            ProfileControl::None => SweepControl::None,
            ProfileControl::KeepOrientation => SweepControl::KeepOrientation,
            ProfileControl::LockDirection => {
                let d = x.lock_direction.as_ref().ok_or("Select a direction to lock")?;
                SweepControl::LockDirection(self.direction(before, state, d)?)
            }
        };
        let body_kind = match x.body {
            BodyType::Solid => cadrs_kernel::BodyKind::Solid,
            BodyType::Surface => cadrs_kernel::BodyKind::Surface,
            BodyType::Thin => {
                let (left, right) = x.thin.sides();
                cadrs_kernel::BodyKind::Thin { left, right }
            }
        };
        // Along a helix with a boolean: the profile grown by 1e-4 of its size about its middle.
        // A thread's profile usually sits exactly on the cylinder the helix winds round, and
        // OCCT's boolean doesn't join bodies that only touch along such a swept face (Onshape's
        // does); the hair's overlap joins them.
        let grow = x.op != BooleanOp::New && x.path.iter().any(|p| matches!(p, PathRef::Curve(_)));
        let mut sweeps: Vec<(cadrs_kernel::Profile, Vec<FaceInput>)> = groups
            .iter()
            .map(|g| {
                let p = crate::brep::profile(g);
                (if grow { grown(&p, 1e-4) } else { p }, Vec::new())
            })
            .collect();
        for (plane, input) in faces {
            sweeps.push((cadrs_kernel::Profile::new(plane, vec![]), vec![input]));
        }
        let mut made: Vec<(BodyId, BodyNames)> = Vec::new();
        for (profile, inputs) in &sweeps {
            let spec = SweepSpec { body: body_kind, path: path.clone(), control, faces: inputs.clone() };
            match self.kernel.sweep_with(profile, &spec).and_then(|r| self.name_new(state, op, &r)) {
                Ok(b) => made.push(b),
                Err(err) => {
                    for (b, _) in made {
                        self.kernel.release(b);
                    }
                    return Err(format!("Sweep failed: {err}"));
                }
            }
        }
        let tool = self.fuse_made(op, "Sweep", made)?;
        let merge = Merge { op: x.op, merge_all: x.merge_all, scope: &x.merge_scope, surface: x.body == BodyType::Surface };
        let geoms = state.geoms.clone();
        self.combine(id, &merge, tool, state, geoms).map(|mut o| {
            // PS11.1: the rest is swept; a warning (Onshape's yellow), as the extrude's.
            if missing > 0 {
                o.warning = o.warning.or(Some("A selected sketch region no longer exists".into()));
            }
            o
        })
    }

    // -----------------------------------------------------------------------------------------
    // Loft

    /// The kernel section of a loft profile.
    fn loft_section(&self, before: &[Feature], state: &State, p: &LoftProfile) -> Result<LoftSection, String> {
        let lost = || "A loft profile no longer exists".to_string();
        Ok(match p {
            LoftProfile::Regions { sketch, regions } => {
                let (groups, missing) = sweep_groups(before, regions, &[], BodyType::Solid);
                if missing > 0 {
                    return Err("A selected sketch region no longer exists".into());
                }
                let g = groups.into_iter().find(|_| true).ok_or_else(lost)?;
                let _ = sketch;
                LoftSection::Profile(crate::brep::profile(&g))
            }
            LoftProfile::Sketch(s) => {
                let (groups, _) = sweep_groups(before, &[], &[*s], BodyType::Solid);
                let g = groups.into_iter().next().ok_or_else(lost)?;
                LoftSection::Profile(crate::brep::profile(&g))
            }
            // P3.10 (PS20.1): any face, planar or not.
            LoftProfile::Face(f) => {
                let (part, ids) = face_ids(state, f).ok_or_else(lost)?;
                let body = part.body.ok_or_else(lost)?;
                let face = *ids.first().ok_or_else(lost)?;
                let source = naming::stable_hash(format!("{:?}", f.face).as_bytes());
                LoftSection::Face(cadrs_kernel::FaceInput { body, face, source })
            }
            LoftProfile::SketchPoint { sketch, point } => {
                let (g, frame) = sketch_of(before, *sketch).ok_or_else(lost)?;
                let q = frame.to_world(g.points.get(*point).ok_or_else(lost)?.pos);
                LoftSection::Point(Point3::from(q))
            }
            LoftProfile::Vertex(v) => LoftSection::Point(Point3::from(vertex_point(state, v).ok_or_else(lost)?)),
        })
    }

    /// The Loft feature (PS20).
    pub(in crate::rebuild) fn loft(&mut self, before: &[Feature], id: FeatureId, x: &LoftFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let sections = x
            .profiles
            .iter()
            .map(|p| self.loft_section(before, state, p))
            .collect::<Result<Vec<_>, _>>()?;
        // The picked directions (P3.11), resolved as the sweep's locked direction is.
        let dir = |d: &Option<crate::document::DirectionRef>| -> Result<[f64; 3], String> {
            let d = d.as_ref().ok_or("Select a direction for the profile condition")?;
            let v = self.direction(before, state, d)?;
            Ok([v.x, v.y, v.z])
        };
        let start_dir = if x.start.takes_direction() { Some(dir(&x.start_direction)?) } else { None };
        let end_dir = if x.end.takes_direction() { Some(dir(&x.end_direction)?) } else { None };
        let end = |c: LoftCondition, m: f64, d: Option<[f64; 3]>| LoftEnd {
            condition: match c {
                LoftCondition::NormalToProfile => cadrs_kernel::LoftCondition::NormalToProfile,
                LoftCondition::TangentToProfile => cadrs_kernel::LoftCondition::TangentToProfile,
                LoftCondition::MatchTangent => cadrs_kernel::LoftCondition::MatchTangent,
                LoftCondition::MatchCurvature => cadrs_kernel::LoftCondition::MatchCurvature,
                LoftCondition::NormalDirection => cadrs_kernel::LoftCondition::NormalDirection(d.unwrap_or([0.0, 0.0, 1.0])),
                LoftCondition::TangentDirection => cadrs_kernel::LoftCondition::TangentDirection(d.unwrap_or([0.0, 0.0, 1.0])),
                LoftCondition::None => cadrs_kernel::LoftCondition::Default,
            },
            magnitude: m,
        };
        let body_kind = match x.body {
            BodyType::Solid => cadrs_kernel::BodyKind::Solid,
            BodyType::Surface => cadrs_kernel::BodyKind::Surface,
            BodyType::Thin => {
                let (left, right) = x.thin.sides();
                cadrs_kernel::BodyKind::Thin { left, right }
            }
        };
        let op = id.0;
        let spec = LoftSpec {
            body: body_kind,
            sections,
            start: end(x.start, x.start_magnitude, start_dir),
            end: end(x.end, x.end_magnitude, end_dir),
            source: naming::stable_hash(id.0.as_bytes()),
        };
        let tool = self
            .kernel
            .loft_with(&spec)
            .and_then(|r| self.name_new(state, op, &r))
            .map_err(|e| format!("Loft failed: {e}"))?;
        // The picked directions as arrows at the first and last profile, pointing along the
        // loft (as the kernel turns them).
        let centres: Vec<Option<[f64; 3]>> = [spec.sections.first(), spec.sections.last()]
            .into_iter()
            .map(|s| s.and_then(|s| self.section_centre(s)))
            .collect();
        let mut arrows = Vec::new();
        if let (Some(a), Some(b)) = (centres[0], centres[1]) {
            let along = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            for (d, c) in [(start_dir, a), (end_dir, b)] {
                let Some(d) = d else { continue };
                let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                if n < 1e-12 {
                    continue;
                }
                let s = if d[0] * along[0] + d[1] * along[1] + d[2] * along[2] < 0.0 { -1.0 / n } else { 1.0 / n };
                arrows.push((c, [d[0] * s, d[1] * s, d[2] * s]));
            }
        }
        let merge = Merge { op: x.op, merge_all: x.merge_all, scope: &x.merge_scope, surface: x.body == BodyType::Surface };
        let geoms = state.geoms.clone();
        self.combine(id, &merge, tool, state, geoms).map(|mut o| {
            o.arrows = arrows;
            o
        })
    }

    /// Roughly the middle of a loft section (for its direction arrow): the mean of its outer
    /// loop's curve starts (a circle's or a long arc's centre), a face's centroid or the point.
    fn section_centre(&self, s: &LoftSection) -> Option<[f64; 3]> {
        match s {
            LoftSection::Profile(p) => {
                let region = p.regions.first()?;
                let pts: Vec<nalgebra::Point2<f64>> = region
                    .outer
                    .curves
                    .iter()
                    .map(|c| match *c {
                        cadrs_kernel::Curve2::Circle { center, .. } => center,
                        cadrs_kernel::Curve2::Arc { center, sweep, .. } if sweep.abs() > std::f64::consts::PI => center,
                        ref other => other.start(),
                    })
                    .collect();
                if pts.is_empty() {
                    return None;
                }
                let m = pts.iter().fold(nalgebra::Vector2::zeros(), |acc, q| acc + q.coords) / pts.len() as f64;
                let pl = &p.plane;
                let y = pl.normal.cross(&pl.x_dir);
                let w = pl.origin + pl.x_dir.into_inner() * m.x + y * m.y;
                Some([w.x, w.y, w.z])
            }
            LoftSection::Face(f) => {
                let c = self.kernel.faces(f.body).ok()?.into_iter().find(|i| i.id == f.face)?.center;
                Some([c.x, c.y, c.z])
            }
            LoftSection::Point(q) => Some([q.x, q.y, q.z]),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Split

    /// The Split feature (PS18.5): each part split by the tool; its largest piece keeps it.
    pub(in crate::rebuild) fn split_parts(&mut self, before: &[Feature], id: FeatureId, x: &SplitFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let lost = || "The split tool no longer exists".to_string();
        let mut bodies: Vec<(PartId, BodyId)> = Vec::new();
        // The Face type: the faces to split of each part.
        let mut faces: Vec<Vec<cadrs_kernel::FaceId>> = Vec::new();
        match x.split_type {
            SplitType::Part => {
                for p in &x.parts {
                    let part = state.part(*p).ok_or("A selected part no longer exists")?;
                    bodies.push((*p, part.body.ok_or("A part has no body")?));
                }
            }
            SplitType::Face => {
                for f in &x.faces {
                    let (part, ids) = face_ids(state, f).ok_or("A face to split no longer exists")?;
                    let body = part.body.ok_or("A part has no body")?;
                    match bodies.iter().position(|(p, _)| *p == part.part.id) {
                        Some(i) => faces[i].extend(ids),
                        None => {
                            bodies.push((part.part.id, body));
                            faces.push(ids);
                        }
                    }
                }
            }
        }
        let source = naming::stable_hash(id.0.as_bytes());
        // A sheet made for a sketch tool, released at the end.
        let mut sheet: Option<BodyId> = None;
        let tool = match x.tool.as_ref().ok_or_else(lost)? {
            SplitToolRef::Plane(p) => SplitTool::Plane(kernel_plane(&plane_of(state, p).ok_or_else(lost)?)),
            SplitToolRef::Face(f) => {
                let (part, ids) = face_ids(state, f).ok_or_else(lost)?;
                // Trim to face boundaries: a planar face splits only within its edges.
                match face_frame(part, f).filter(|_| !x.trim) {
                    Some(frame) => SplitTool::Plane(kernel_plane(&frame)),
                    None => SplitTool::Face { body: part.body.ok_or_else(lost)?, face: *ids.first().ok_or_else(lost)? },
                }
            }
            SplitToolRef::Sketch(s) => {
                // The sketch's curves swept both ways through the parts: a sheet.
                let (groups, _) = sweep_groups(before, &[], &[*s], BodyType::Surface);
                let g = groups.into_iter().next().ok_or_else(lost)?;
                let profile = crate::brep::profile(&g);
                let spec = ExtrudeSpec {
                    body: cadrs_kernel::BodyKind::Surface,
                    direction: profile.plane.normal,
                    start_offset: 0.0,
                    end: ExtrudeEnd::ThroughAll,
                    symmetric: true,
                    second: None,
                    scene: bodies.iter().map(|(_, b)| *b).collect(),
                    faces: Vec::new(),
                };
                let r = self.kernel.extrude_with(&profile, &spec).map_err(|e| format!("Split failed: {e}"))?;
                sheet = Some(r.bodies[0]);
                SplitTool::Body(r.bodies[0])
            }
        };
        // Keep both sides off: only the pieces on the front of the tool's plane (its back with
        // the flip) stay. A tool that isn't flat has no side.
        let side: Option<(Point3<f64>, Vector3<f64>)> = match (&tool, x.keep_both, x.split_type) {
            (_, true, _) | (_, _, SplitType::Face) => None,
            (SplitTool::Plane(p), false, SplitType::Part) => {
                let n = p.normal.into_inner();
                Some((p.origin, if x.flip { -n } else { n }))
            }
            _ => {
                if let Some(b) = sheet {
                    self.kernel.release(b);
                }
                return Err("Keeping one side needs a flat tool (a plane or a planar face)".into());
            }
        };
        let out = match x.split_type {
            SplitType::Part => self.per_part_keeping(
                id,
                "Split",
                state,
                bodies,
                |this, _, body| this.kernel.split(body, &tool, source),
                |this, body| match side {
                    None => true,
                    Some((o, n)) => this.kernel.mass_properties(body).is_ok_and(|m| (m.center_of_mass - o).dot(&n) > 0.0),
                },
            ),
            SplitType::Face => {
                self.per_part(id, "Split", state, bodies, |this, i, body| this.kernel.split_faces(body, &faces[i], &tool))
            }
        };
        if let Some(b) = sheet {
            self.kernel.release(b);
        }
        let mut out = out?;
        // A surface split with is used up unless Keep tools is on (as Onshape).
        if let Some(SplitToolRef::Face(f)) = &x.tool
            && !x.keep_tools
            && !x.parts.contains(&f.part)
            && state.part(f.part).is_some_and(|p| p.part.kind == crate::parts::PartKind::Surface)
        {
            let mut next = (*out.state).clone();
            next.parts.retain(|p| p.part.id != f.part);
            out.state = Arc::new(next);
        }
        Ok(out)
    }
}

/// The profile scaled by `1 + k` about the middle of its regions' curves (in its plane).
fn grown(p: &cadrs_kernel::Profile, k: f64) -> cadrs_kernel::Profile {
    let mut pts: Vec<Point2<f64>> = Vec::new();
    for r in &p.regions {
        for c in &r.outer.curves {
            for i in 0..8 {
                pts.push(c.point_at(i as f64 / 8.0));
            }
        }
    }
    if pts.is_empty() {
        return p.clone();
    }
    let n = pts.len() as f64;
    let m = Point2::new(pts.iter().map(|q| q.x).sum::<f64>() / n, pts.iter().map(|q| q.y).sum::<f64>() / n);
    let f = 1.0 + k;
    let sp = |q: Point2<f64>| m + (q - m) * f;
    let sc = |c: &Curve2| match c.clone() {
        Curve2::Line { a, b, source } => Curve2::Line { a: sp(a), b: sp(b), source },
        Curve2::Arc { center, radius, start_angle, sweep, source } => Curve2::Arc { center: sp(center), radius: radius * f, start_angle, sweep, source },
        Curve2::Circle { center, radius, source } => Curve2::Circle { center: sp(center), radius: radius * f, source },
        Curve2::Ellipse { center, major_radius, minor_radius, rotation, source } => {
            Curve2::Ellipse { center: sp(center), major_radius: major_radius * f, minor_radius: minor_radius * f, rotation, source }
        }
        Curve2::EllipseArc { center, major_radius, minor_radius, rotation, start, sweep, source } => Curve2::EllipseArc {
            center: sp(center),
            major_radius: major_radius * f,
            minor_radius: minor_radius * f,
            rotation,
            start,
            sweep,
            source,
        },
        Curve2::OffsetEllipseArc { center, major_radius, minor_radius, rotation, start, sweep, offset, source } => Curve2::OffsetEllipseArc {
            center: sp(center),
            major_radius: major_radius * f,
            minor_radius: minor_radius * f,
            rotation,
            start,
            sweep,
            offset: offset * f,
            source,
        },
        Curve2::Bezier { poles, source } => Curve2::Bezier { poles: poles.map(sp), source },
    };
    let lp = |l: &cadrs_kernel::Loop| cadrs_kernel::Loop { curves: l.curves.iter().map(sc).collect() };
    cadrs_kernel::Profile {
        plane: p.plane,
        regions: p.regions.iter().map(|r| cadrs_kernel::Region { outer: lp(&r.outer), holes: r.holes.iter().map(lp).collect(), source: r.source }).collect(),
        chains: p.chains.iter().map(|c| cadrs_kernel::Chain { curves: c.curves.iter().map(sc).collect(), source: c.source }).collect(),
    }
}
