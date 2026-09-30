//! The full revolve of the OCCT backend (P3.4, [`Kernel::revolve_with`]).
//!
//! - **Angle ends** (an angle, Up to vertex, and Up to face of a plane through the axis) become
//!   one revolution from the second end to the first: the profile is turned back to where the
//!   second end starts and revolved through both angles together.
//! - **Conforming ends** (Up to next, Up to part, Up to any other face) revolve the profile
//!   almost a whole turn and split that by the target bodies (turned back by the offset angle):
//!   `sweep − targets` and `sweep ∩ targets`; the pieces that hold the start cap are kept, so the
//!   sweep stops at the first target face each point of the profile meets. A kept piece that
//!   still has the far cap means part of the profile met nothing: an error.
//! - **Surface** revolves turn the profile's curves (region loops and open chains) into sheets;
//!   **Thin** revolves turn the bands of [`crate::thin::thin_profile`].
//!
//! Faces are tagged with their [`Origin`] through OCCT's history, as for the extrude: the side of
//! each profile curve, the start cap (the profile, or the second end's far face) and the end cap.

use std::f64::consts::TAU;

use nalgebra::{Point2, Point3, Rotation3, Unit, Vector3};
use opencascade::primitives::Shape;

use super::extrude::{Tagged, chain_wire, clone, edge_origins, point_in_loop};
use super::{
    OcctKernel, face_count, faces_of, generated, loop_wire, occt, oriented,
    profile_faces, sweep_origins, to_glam, union_tagged,
};
use crate::{
    BodyId, BodyKind, Curve2, FaceInput, Kernel, KernelError, OpResult, Origin, Plane, Profile, Region,
    Result, RevolveEnd, RevolveSpec,
};

/// The smallest angle a revolve turns (radians).
pub const MIN_ANGLE: f64 = 1e-7;

/// How far a conforming end turns before the targets trim it: just short of a whole turn, so
/// its far cap doesn't touch its start.
const FAR: f64 = TAU * (1.0 - 1e-4);

/// How far one end turns.
#[derive(Debug, Clone)]
enum Turn {
    Angle(f64),
    /// Up to the faces of these bodies, turned back by `offset` radians.
    Conform { bodies: Vec<BodyId>, offset: f64 },
}

impl Turn {
    fn angle(&self) -> Option<f64> {
        match self {
            Turn::Angle(a) => Some(*a),
            Turn::Conform { .. } => None,
        }
    }
}

/// The axis with a reference direction across it: angles are measured about `d` from `u`
/// (towards the profile).
#[derive(Debug, Clone, Copy)]
struct Frame {
    o: Point3<f64>,
    d: Vector3<f64>,
    u: Vector3<f64>,
    w: Vector3<f64>,
}

impl Frame {
    /// The angle of `p` about the axis, in 0..2π, counter-clockwise (`forward`) or clockwise.
    fn angle(&self, p: Point3<f64>, forward: bool) -> f64 {
        let v = p - self.o;
        let a = v.dot(&self.w).atan2(v.dot(&self.u)).rem_euclid(TAU);
        if forward { a } else { (TAU - a).rem_euclid(TAU) }
    }

    fn rotation(&self, angle: f64) -> Rotation3<f64> {
        Rotation3::from_axis_angle(&Unit::new_unchecked(self.d), angle)
    }

    /// The profile's plane turned by `angle` about the axis.
    fn turned(&self, plane: &Plane, angle: f64) -> Plane {
        let r = self.rotation(angle);
        Plane {
            origin: self.o + r * (plane.origin - self.o),
            x_dir: Unit::new_normalize(r * plane.x_dir.into_inner()),
            normal: Unit::new_normalize(r * plane.normal.into_inner()),
        }
    }

    /// A copy of `shape` turned by `angle` about the axis.
    fn turn_shape(&self, shape: &Shape, angle: f64) -> Shape {
        let back = to_glam(-self.o.coords);
        shape
            .translated(back)
            .rotated(to_glam(self.d), angle)
            .translated(to_glam(self.o.coords))
    }
}

fn invalid(why: impl Into<String>) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// Points along every curve of the profile, in model coordinates.
fn samples(profile: &Profile) -> Vec<Point3<f64>> {
    profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| l.curves.iter())
        .chain(profile.chains.iter().flat_map(|c| c.curves.iter()))
        .flat_map(|c| (0..32).map(move |k| c.point_at(k as f64 / 32.0)))
        .map(|p| profile.plane.to_model(p))
        .collect()
}

fn check_angle(what: &str, a: f64) -> Result<Turn> {
    if !a.is_finite() {
        return Err(invalid(format!("{what} is not a number")));
    }
    if a < MIN_ANGLE {
        return Err(invalid(format!("{what} is at or behind the start of the revolve (try the opposite direction)")));
    }
    if a > TAU + 1e-9 {
        return Err(invalid(format!("{what} is more than a full turn")));
    }
    Ok(Turn::Angle(a.min(TAU)))
}

impl OcctKernel {
    pub(super) fn revolve_full(&mut self, profile: &Profile, spec: &RevolveSpec) -> Result<OpResult> {
        if profile.regions.is_empty() && profile.chains.is_empty() && spec.faces.is_empty() {
            return Err(KernelError::InvalidProfile("the profile has no regions".into()));
        }
        if !spec.faces.is_empty() && !matches!(spec.body, BodyKind::Solid) {
            return Err(KernelError::Unsupported("faces as input to a surface or thin revolve"));
        }
        let spec_faces = spec.faces.clone();
        let d = spec.axis.dir.into_inner();
        let n = profile.plane.normal.into_inner();
        if d.cross(&n).norm() < 1e-9 {
            return Err(invalid("the revolve axis is normal to the sketch plane"));
        }
        let o = spec.axis.origin;
        let mut pts = samples(profile);
        for input in &spec.faces {
            let body = self.body(input.body)?;
            if let Some(face) = faces_of(body).into_iter().nth(input.face.0 as usize) {
                for e in face.edges_geometry().map_err(occt)? {
                    for p in [e.start, e.mid, e.end] {
                        pts.push(Point3::from(super::to_na(p)));
                    }
                }
            }
        }
        // An axis in the sketch plane must not cross the profile.
        let coplanar = d.dot(&n).abs() < 1e-9 && (o - profile.plane.origin).dot(&n).abs() < 1e-7;
        let size = pts.iter().map(|p| (p - o).norm()).fold(1.0, f64::max);
        if coplanar {
            let m = n.cross(&d);
            let side: Vec<f64> = pts.iter().map(|p| (p - o).dot(&m)).collect();
            let tol = 1e-9 * size;
            let lo = side.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = side.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if lo < -tol && hi > tol {
                return Err(invalid("the profile crosses the revolve axis"));
            }
        }
        // The reference direction: towards the profile, across the axis.
        let off_axis = |p: &Point3<f64>| {
            let v = p - o;
            v - d * v.dot(&d)
        };
        let u = pts
            .iter()
            .map(off_axis)
            .max_by(|a, b| a.norm().total_cmp(&b.norm()))
            .filter(|v| v.norm() > 1e-9 * size)
            .ok_or_else(|| invalid("the profile lies on the revolve axis"))?
            .normalize();
        let frame = Frame { o, d, u, w: d.cross(&u) };

        let (first, second) = if spec.full {
            (Turn::Angle(TAU), None)
        } else if spec.symmetric {
            match spec.end {
                RevolveEnd::Angle(t) => {
                    let half = check_angle("the angle", t / 2.0)?;
                    (half.clone(), Some(half))
                }
                _ => return Err(invalid("Symmetric works with an angle")),
            }
        } else {
            let first = self.resolve_turn(profile, spec, &frame, spec.end, true)?;
            let second = spec
                .second
                .map(|e| self.resolve_turn(profile, spec, &frame, e, false))
                .transpose()?;
            (first, second)
        };
        let tagged = match (first.angle(), second.as_ref().map(Turn::angle)) {
            (Some(f), None) => self.turn(profile, &spec_faces, spec.body, &frame, 0.0, f)?,
            (Some(f), Some(Some(b))) => {
                let total = f + b;
                if total > TAU + 1e-9 {
                    return Err(invalid("the two ends add up to more than a full turn"));
                }
                self.turn(profile, &spec_faces, spec.body, &frame, -b, total.min(TAU))?
            }
            _ => {
                if matches!(spec.body, BodyKind::Surface) {
                    return Err(KernelError::Unsupported("Up to next, part or face for surfaces"));
                }
                let mut pieces = vec![self.turn_piece(profile, &spec_faces, spec.body, &frame, first, true)?];
                if let Some(s) = second {
                    pieces.push(self.turn_piece(profile, &spec_faces, spec.body, &frame, s, false)?);
                }
                union_tagged(pieces)?
            }
        };
        self.insert(tagged.0, generated(tagged.1))
    }

    /// How far an end turns (forward: counter-clockwise about the axis).
    fn resolve_turn(
        &self,
        profile: &Profile,
        spec: &RevolveSpec,
        frame: &Frame,
        end: RevolveEnd,
        forward: bool,
    ) -> Result<Turn> {
        match end {
            RevolveEnd::Angle(a) => check_angle("the angle", a),
            RevolveEnd::UpToVertex { point, offset } => {
                if (point - frame.o).cross(&frame.d).norm() < 1e-9 {
                    return Err(invalid("the vertex lies on the revolve axis"));
                }
                check_angle("the vertex", frame.angle(point, forward) - offset)
            }
            RevolveEnd::UpToFace { body, face, offset } => {
                let info = self
                    .faces(body)?
                    .into_iter()
                    .find(|f| f.id == face)
                    .ok_or_else(|| KernelError::OperationFailed(format!("unknown {face:?}")))?;
                if let Some(pl) = info.plane {
                    let m = pl.normal.into_inner();
                    let through_axis =
                        m.dot(&frame.d).abs() < 1e-9 && (pl.origin - frame.o).dot(&m).abs() < 1e-7 * (1.0 + pl.origin.coords.norm());
                    if through_axis {
                        return check_angle("the face", frame.angle(info.center, forward) - offset);
                    }
                }
                Ok(Turn::Conform { bodies: vec![body], offset })
            }
            RevolveEnd::UpToPart { body, offset } => Ok(Turn::Conform { bodies: vec![body], offset }),
            RevolveEnd::UpToNext { offset } => {
                // The bodies the sweep meets.
                let (sweep, _) = self.turn(profile, &spec.faces, BodyKind::Solid, frame, 0.0, if forward { FAR } else { -FAR })?;
                let mut ahead = Vec::new();
                for &b in &spec.scene {
                    let (common, _) = sweep.try_intersect_h(self.body(b)?).map_err(occt)?;
                    if common.mass_properties().volume > 1e-9 {
                        ahead.push(b);
                    }
                }
                if ahead.is_empty() {
                    return Err(invalid("Up to next: nothing lies in the revolve's way"));
                }
                Ok(Turn::Conform { bodies: ahead, offset })
            }
        }
    }

    /// The profile turned `sweep` radians (negative: clockwise) about the axis, starting
    /// `start` radians from its plane: one shape, each face tagged.
    fn turn(&self, profile: &Profile, faces: &[FaceInput], body: BodyKind, frame: &Frame, start: f64, sweep: f64) -> Result<Tagged> {
        if sweep.abs() < MIN_ANGLE {
            return Err(invalid("the revolve angle is zero"));
        }
        let plane = if start == 0.0 {
            profile.plane
        } else {
            frame.turned(&profile.plane, start)
        };
        let full = sweep.abs() >= TAU - 1e-9;
        let sweep = if full { TAU * sweep.signum() } else { sweep };
        let (o, d) = (to_glam(frame.o.coords), to_glam(frame.d));
        match body {
            BodyKind::Solid => {
                let mut solids = Vec::new();
                if !profile.regions.is_empty() {
                    solids.push(self.turn_regions(&plane, &profile.regions, o, d, sweep)?);
                }
                for input in faces {
                    solids.push(self.turn_face(input, frame, start, o, d, sweep)?);
                }
                union_tagged(solids)
            }
            BodyKind::Thin { left, right } => {
                let bands = crate::thin::thin_profile(&Profile { plane, ..profile.clone() }, left, right)?;
                self.turn_regions(&plane, &bands.regions, o, d, sweep)
            }
            BodyKind::Surface => {
                let sheets = turn_curves(profile, &plane, o, d, sweep)?;
                let shapes: Vec<Shape> = sheets.iter().map(|s| clone(&s.0)).collect();
                let tags = sheets.into_iter().flat_map(|s| s.1).collect();
                Ok((Shape::try_compound(&shapes).map_err(occt)?, tags))
            }
        }
    }

    /// A planar face of a body turned `sweep` about the axis, from `start` radians: its sides are
    /// generated from the body's edges, its caps named after the input's `source`.
    fn turn_face(&self, input: &FaceInput, frame: &Frame, start: f64, o: glam::DVec3, d: glam::DVec3, sweep: f64) -> Result<Tagged> {
        let body = self.body(input.body)?;
        let face = faces_of(body)
            .into_iter()
            .nth(input.face.0 as usize)
            .ok_or_else(|| KernelError::OperationFailed(format!("unknown {:?}", input.face)))?;
        let shape = Shape::from(&face);
        let shape = if start == 0.0 { shape } else { frame.turn_shape(&shape, start) };
        let (solid, h) = shape.try_revolve_h(o, d, sweep).map_err(occt)?;
        let mut origins = vec![None; face_count(&solid)?];
        // The face's edges in its own MapShapes order (a turned copy keeps it), as the history
        // lists them.
        let body_edges = super::Edges::of(body);
        let face_edges = super::Edges::of_face(&face);
        for (i, generated) in h.edges.iter().enumerate() {
            let Some(edge) = face_edges.list.get(i).and_then(|e| body_edges.index(e)) else {
                continue;
            };
            for &f in generated {
                if let Some(t) = origins.get_mut(f) {
                    t.get_or_insert(Origin::FromEdge { body: input.body, edge: crate::EdgeId(edge as u64) });
                }
            }
        }
        for (list, origin) in [
            (&h.first, Origin::StartCap { region: input.source }),
            (&h.last, Origin::EndCap { region: input.source }),
        ] {
            for &f in list {
                if let Some(t) = origins.get_mut(f) {
                    t.get_or_insert(origin);
                }
            }
        }
        Ok((solid, origins))
    }

    fn turn_regions(&self, plane: &Plane, regions: &[Region], o: glam::DVec3, d: glam::DVec3, sweep: f64) -> Result<Tagged> {
        let solids = profile_faces(plane, regions)?
            .iter()
            .zip(regions)
            .enumerate()
            .map(|(i, (face, region))| {
                let (solid, h) = face.try_revolve_h(o, d, sweep).map_err(occt)?;
                let origins = sweep_origins(face, plane, i, region, &solid, &h)?;
                Ok((solid, origins))
            })
            .collect::<Result<Vec<_>>>()?;
        union_tagged(solids)
    }

    /// One end's piece: the profile turned `turn` (forward or back), trimmed by its targets.
    fn turn_piece(&self, profile: &Profile, spec_faces: &[FaceInput], body: BodyKind, frame: &Frame, turn: Turn, forward: bool) -> Result<Tagged> {
        let sign = if forward { 1.0 } else { -1.0 };
        let (shape, mut tags) = match &turn {
            Turn::Angle(a) => self.turn(profile, spec_faces, body, frame, 0.0, sign * a)?,
            Turn::Conform { .. } => self.turn(profile, spec_faces, body, frame, 0.0, sign * FAR)?,
        };
        let swap = |tags: &mut Vec<Option<Origin>>| {
            if !forward {
                // The far cap of the second end is the body's start cap.
                for t in tags.iter_mut().flatten() {
                    *t = match *t {
                        Origin::StartCap { region } => Origin::EndCap { region },
                        Origin::EndCap { region } => Origin::StartCap { region },
                        other => other,
                    };
                }
            }
        };
        let Turn::Conform { bodies, offset } = turn else {
            swap(&mut tags);
            return Ok((shape, tags));
        };
        // The targets, turned back towards the start by the offset angle.
        let moved: Vec<Shape> = bodies
            .iter()
            .map(|b| Ok(frame.turn_shape(self.body(*b)?, -sign * offset)))
            .collect::<Result<_>>()?;
        let tool = if moved.len() == 1 {
            clone(&moved[0])
        } else {
            Shape::try_compound(&moved).map_err(occt)?
        };
        let tool_tags = vec![None; face_count(&tool)?];
        let (out, ho) = shape.try_subtract_h(&tool).map_err(occt)?;
        let out_tags = super::carry(&ho, &[&tags, &tool_tags], face_count(&out)?);
        let (inn, hi) = shape.try_intersect_h(&tool).map_err(occt)?;
        let inn_tags = super::carry(&hi, &[&tags, &tool_tags], face_count(&inn)?);
        let starts = |t: &Vec<Option<Origin>>| t.iter().any(|x| matches!(x, Some(Origin::StartCap { .. })));
        let mut kept: Vec<Tagged> = solids_of((out, out_tags))?.into_iter().filter(|p| starts(&p.1)).collect();
        kept.extend(solids_of((inn, inn_tags))?.into_iter().filter(|p| starts(&p.1)));
        if kept.is_empty() {
            return Err(invalid("the target leaves nothing to revolve"));
        }
        let (shape, mut tags) = union_tagged(kept)?;
        if tags.iter().any(|t| matches!(t, Some(Origin::EndCap { .. }))) {
            return Err(invalid("part of the profile doesn't meet the target"));
        }
        // The faces the trim made are the far cap of the region they end.
        let infos: Vec<crate::FaceInfo> = faces_of(&shape)
            .iter()
            .enumerate()
            .map(|(i, f)| super::face_info(crate::FaceId(i as u64), f, &shape))
            .collect();
        for (t, info) in tags.iter_mut().zip(&infos) {
            if t.is_none() {
                *t = Some(Origin::EndCap {
                    region: region_at(profile, frame, info.center, forward),
                });
            }
        }
        swap(&mut tags);
        Ok((shape, tags))
    }
}

/// The solids of a tagged shape, each with its faces' tags.
fn solids_of((shape, tags): Tagged) -> Result<Vec<Tagged>> {
    let parent = faces_of(&shape);
    let mut out = Vec::new();
    for solid in shape.sub_shapes(opencascade::safe::SubKind::Solid).map_err(occt)? {
        let piece_tags = faces_of(&solid)
            .iter()
            .map(|f| {
                parent
                    .iter()
                    .position(|p| p.is_same(f))
                    .and_then(|i| tags.get(i).copied().flatten())
            })
            .collect();
        out.push((solid, piece_tags));
    }
    Ok(out)
}

/// The region of the profile a point swept from: the point turned back onto the profile's
/// plane (the first region if none holds it).
fn region_at(profile: &Profile, frame: &Frame, p: Point3<f64>, forward: bool) -> u64 {
    let a = frame.angle(p, forward);
    let back = frame.rotation(if forward { -a } else { a });
    let q = frame.o + back * (p - frame.o);
    let pl = &profile.plane;
    let pt = Point2::new((q - pl.origin).dot(&pl.x_dir), (q - pl.origin).dot(&pl.y_dir()));
    for (i, r) in profile.regions.iter().enumerate() {
        let inside = |lp: &crate::Loop| point_in_loop(lp, pt);
        if inside(&r.outer) && !r.holes.iter().any(inside) {
            return r.source.unwrap_or(i as u64);
        }
    }
    profile.regions.first().map_or(0, |r| r.source.unwrap_or(0))
}

/// The sheets swept by the profile's curves turning `sweep` about the axis: one per region loop
/// and one per open chain, each face tagged with the curve that swept it.
fn turn_curves(profile: &Profile, plane: &Plane, o: glam::DVec3, d: glam::DVec3, sweep: f64) -> Result<Vec<Tagged>> {
    let mut out = Vec::new();
    let mut add = |curves: Vec<Curve2>, closed: bool, region: u64, lp: usize| -> Result<()> {
        let wire = if closed { loop_wire(plane, &curves)? } else { chain_wire(plane, &curves)? };
        let (shape, h) = wire.try_revolve_h(o, d, sweep).map_err(occt)?;
        let edges = wire.edges_geometry().map_err(occt)?;
        let ids: Vec<(u64, Vec<Point3<f64>>)> = curves
            .iter()
            .enumerate()
            .map(|(ci, c)| {
                let id = c.source().unwrap_or_else(|| crate::unsourced_curve(lp, ci));
                (id, (0..=32).map(|k| plane.to_model(c.point_at(k as f64 / 32.0))).collect())
            })
            .collect();
        let mut tags = vec![None; face_count(&shape)?];
        edge_origins(&edges, &h.edges, &ids, region, &mut tags);
        out.push((shape, tags));
        Ok(())
    };
    for (i, region) in profile.regions.iter().enumerate() {
        let id = region.source.unwrap_or(i as u64);
        add(oriented(&region.outer, true)?, true, id, 0)?;
        for (k, hole) in region.holes.iter().enumerate() {
            add(oriented(hole, false)?, true, id, k + 1)?;
        }
    }
    for (i, chain) in profile.chains.iter().enumerate() {
        let id = chain.source.unwrap_or((profile.regions.len() + i) as u64);
        add(chain.curves.clone(), false, id, 0)?;
    }
    if out.is_empty() {
        return Err(KernelError::InvalidProfile("nothing to revolve".into()));
    }
    Ok(out)
}
