//! The full extrude of the OCCT backend (P3.3, [`Kernel::extrude_with`]).
//!
//! - **Depth ends** (Blind, Up to vertex, Through all, and Up to face of a planar face parallel
//!   to the sketch plane) become one prism from the second end to the first.
//! - **Trimmed ends**: Up to face of another planar face cuts a long prism with the half space
//!   beyond the face's plane. Up to next, Up to part and Up to face of a curved face *conform*:
//!   the long prism is split by the target bodies (`prism − targets` and `prism ∩ targets`) and
//!   only the pieces that touch the start plane are kept, so the sweep stops at the first
//!   target face each point of the profile meets (entering or leaving a target). A piece that
//!   still reaches the far end of the long prism means part of the profile meets nothing, which
//!   is an error.
//! - **Surface** extrudes sweep the profile's curves into sheets; **Thin** extrudes thicken those
//!   sheets to either side (`BRepOffsetAPI_MakeThickSolid::MakeThickSolidBySimple`).
//!
//! Every face is tagged with its [`Origin`] through the operations' histories, as the blind
//! extrude does; faces a trim makes are the end cap (or the start cap, for the second end) of
//! the region under them.

use nalgebra::{Point2, Point3, Unit, Vector3};
use opencascade::primitives::{Face, Shape, Wire};
use opencascade::safe::EdgeGeometry;

use super::{
    LINEAR_EPS, MIN_DEPTH, OcctKernel, curve_edge, face_count, faces_of, generated, occt,
    oriented, profile_faces, segment_distance, to_glam, to_na, union_tagged, Edges,
};
use crate::{
    BodyId, BodyKind, Chain, Curve2, ExtrudeEnd, ExtrudeSpec, FaceInput, Kernel, KernelError, Loop,
    OpResult, Origin, Plane, Profile, Region, Result,
};

/// A shape with a tag per face (MapShapes order).
pub(super) type Tagged = (Shape, Vec<Option<Origin>>);

/// How far one end goes.
#[derive(Debug, Clone)]
enum Reach {
    Depth(f64),
    Trim { kind: TrimKind, far: f64 },
}

#[derive(Debug, Clone)]
enum TrimKind {
    /// Cut along the plane through `point` with normal `normal`.
    Plane { point: Point3<f64>, normal: Vector3<f64> },
    /// Stop at the faces of `bodies`, moved `shift` mm along the sweep.
    Conform { bodies: Vec<BodyId>, shift: f64 },
}

impl Reach {
    fn depth(&self) -> Option<f64> {
        match self {
            Reach::Depth(d) => Some(*d),
            Reach::Trim { .. } => None,
        }
    }
}

/// Which way a piece is swept: the first end (along the direction) or the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    First,
    Second,
}

fn too_thin(what: &str, depth: f64) -> KernelError {
    if depth.abs() < MIN_DEPTH && depth > -MIN_DEPTH {
        KernelError::InvalidParameter(format!(
            "the depth ({} mm) is below the kernel's modelling tolerance ({MIN_DEPTH} mm)",
            depth.abs()
        ))
    } else {
        KernelError::InvalidParameter(format!(
            "{what} is behind the start of the extrude (try the opposite direction)"
        ))
    }
}

fn check_depth(what: &str, depth: f64) -> Result<Reach> {
    if !depth.is_finite() {
        return Err(KernelError::InvalidParameter(format!("{what} is not a number")));
    }
    if depth < MIN_DEPTH {
        return Err(too_thin(what, depth));
    }
    Ok(Reach::Depth(depth))
}

impl OcctKernel {
    pub(super) fn extrude_full(&mut self, profile: &Profile, spec: &ExtrudeSpec) -> Result<OpResult> {
        let d = spec.direction.into_inner();
        let n = profile.plane.normal.into_inner();
        if d.dot(&n).abs() < 1e-6 {
            return Err(KernelError::InvalidParameter(
                "the extrude direction lies in the sketch plane".into(),
            ));
        }
        if profile.regions.is_empty() && spec.faces.is_empty() && profile.chains.is_empty() {
            return Err(KernelError::InvalidProfile("the profile has no regions".into()));
        }
        let o = profile.plane.origin + d * spec.start_offset;
        let (first, second) = if spec.symmetric {
            match spec.end {
                ExtrudeEnd::Blind(t) => {
                    let half = check_depth("the depth", t / 2.0)?;
                    (half.clone(), Some(half))
                }
                ExtrudeEnd::ThroughAll => (
                    self.resolve(profile, spec, o, d, ExtrudeEnd::ThroughAll)?,
                    Some(self.resolve(profile, spec, o, -d, ExtrudeEnd::ThroughAll)?),
                ),
                _ => {
                    return Err(KernelError::InvalidParameter(
                        "Symmetric works with Blind and Through all".into(),
                    ));
                }
            }
        } else {
            let first = self.resolve(profile, spec, o, d, spec.end)?;
            let second = spec
                .second
                .map(|e| self.resolve(profile, spec, o, -d, e))
                .transpose()?;
            (first, second)
        };
        let tagged = match spec.body {
            BodyKind::Solid => self.solid_extrude(profile, spec, o, d, first, second)?,
            // A thin wall is a profile of bands along the curves (see `crate::thin`).
            BodyKind::Thin { left, right } => {
                let bands = crate::thin::thin_profile(profile, left, right)?;
                self.solid_extrude(&bands, spec, o, d, first, second)?
            }
            BodyKind::Surface => {
                let unsupported = || KernelError::Unsupported("Up to next, part or face for surfaces");
                let f = first.depth().ok_or_else(unsupported)?;
                let b = match second.as_ref().map(Reach::depth) {
                    None => 0.0,
                    Some(Some(b)) => b,
                    Some(None) => return Err(unsupported()),
                };
                let start = o - d * b;
                let sheets = self.sweep_curves(profile, start - profile.plane.origin, d * (f + b))?;
                let shapes: Vec<Shape> = sheets.iter().map(|s| clone(&s.0)).collect();
                let tags = sheets.into_iter().flat_map(|s| s.1).collect();
                (Shape::try_compound(&shapes).map_err(occt)?, tags)
            }
        };
        self.insert(tagged.0, generated(tagged.1))
    }

    /// How far an end goes from `o` along `dir`.
    fn resolve(
        &self,
        profile: &Profile,
        spec: &ExtrudeSpec,
        o: Point3<f64>,
        dir: Vector3<f64>,
        end: ExtrudeEnd,
    ) -> Result<Reach> {
        match end {
            ExtrudeEnd::Blind(x) => check_depth("the depth", x),
            ExtrudeEnd::UpToVertex { point, offset } => {
                check_depth("the vertex", (point - o).dot(&dir) - offset)
            }
            ExtrudeEnd::ThroughAll => {
                let far = self.far_extent(&spec.scene, o, dir)?;
                match far {
                    Some(f) if f > MIN_DEPTH => Ok(Reach::Depth(f)),
                    _ => Err(KernelError::InvalidParameter(
                        "Through all: no part lies in the extrude direction".into(),
                    )),
                }
            }
            ExtrudeEnd::UpToFace { body, face, offset } => {
                let info = self
                    .faces(body)?
                    .into_iter()
                    .find(|f| f.id == face)
                    .ok_or_else(|| KernelError::OperationFailed(format!("unknown {face:?}")))?;
                if let Some(pl) = info.plane {
                    let m = pl.normal.into_inner();
                    if m.dot(&dir).abs() > 1.0 - 1e-9 {
                        return check_depth("the face", (pl.origin - o).dot(&dir) - offset);
                    }
                    // A plane along the extrude direction is never reached (and the sweep below
                    // would be endless: OCCT's fuse of such prisms never returns).
                    if m.dot(&dir).abs() < 1e-6 {
                        return Err(KernelError::InvalidParameter("the face is parallel to the extrude direction".into()));
                    }
                    // An oblique plane: how far the sweep must go to cross it everywhere.
                    let point = pl.origin - dir * offset;
                    let samples = profile_samples(self, profile, spec, o)?;
                    let far = samples
                        .iter()
                        .map(|p| (point - p).dot(&m) / dir.dot(&m))
                        .fold(f64::NEG_INFINITY, f64::max);
                    if far.is_nan() || far <= MIN_DEPTH {
                        return Err(too_thin("the face", far));
                    }
                    if far > 1e6 {
                        return Err(KernelError::InvalidParameter("the face is too nearly parallel to the extrude direction".into()));
                    }
                    Ok(Reach::Trim {
                        kind: TrimKind::Plane { point, normal: m },
                        far: far * 1.1 + 1.0,
                    })
                } else {
                    self.conform(vec![body], o, dir, offset, "the face")
                }
            }
            ExtrudeEnd::UpToPart { body, offset } => self.conform(vec![body], o, dir, offset, "the part"),
            ExtrudeEnd::UpToNext { offset } => {
                // The bodies something of the profile meets ahead of it.
                let samples = profile_samples(self, profile, spec, o)?;
                let mut ahead = Vec::new();
                for &b in &spec.scene {
                    let mut hit = false;
                    for p in &samples {
                        if self.ray_hits(b, *p, dir)?.iter().any(|h| h.t > 1e-7) {
                            hit = true;
                            break;
                        }
                    }
                    if hit {
                        ahead.push(b);
                    }
                }
                if ahead.is_empty() {
                    return Err(KernelError::InvalidParameter(
                        "Up to next: nothing lies in the extrude direction".into(),
                    ));
                }
                self.conform(ahead, o, dir, offset, "the next part")
            }
        }
    }

    fn conform(&self, bodies: Vec<BodyId>, o: Point3<f64>, dir: Vector3<f64>, offset: f64, what: &str) -> Result<Reach> {
        let far = self.far_extent(&bodies, o, dir)?.unwrap_or(f64::NEG_INFINITY) - offset;
        if far.is_nan() || far <= MIN_DEPTH {
            return Err(too_thin(what, far));
        }
        let diag = bodies
            .iter()
            .map(|b| self.bounding_box(*b).map(|a| a.diagonal()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(0.0, f64::max);
        Ok(Reach::Trim {
            kind: TrimKind::Conform { bodies, shift: -offset },
            far: far + 0.1 * diag + 1.0,
        })
    }

    /// The largest distance from `o` along `dir` of any of the bodies (`None` without bodies).
    fn far_extent(&self, bodies: &[BodyId], o: Point3<f64>, dir: Vector3<f64>) -> Result<Option<f64>> {
        let mut far: Option<f64> = None;
        for &b in bodies {
            let f = self.bounding_box(b)?.max_along(o, &dir);
            far = Some(far.map_or(f, |x: f64| x.max(f)));
        }
        Ok(far)
    }

    fn solid_extrude(
        &self,
        profile: &Profile,
        spec: &ExtrudeSpec,
        o: Point3<f64>,
        d: Vector3<f64>,
        first: Reach,
        second: Option<Reach>,
    ) -> Result<Tagged> {
        let depths = (first.depth(), second.as_ref().map(Reach::depth));
        match depths {
            (Some(f), None) => self.prism(profile, &spec.faces, o - profile.plane.origin, d * f),
            (Some(f), Some(Some(b))) => {
                self.prism(profile, &spec.faces, (o - d * b) - profile.plane.origin, d * (f + b))
            }
            _ => {
                let mut pieces = vec![self.piece(profile, spec, o, d, first, Side::First)?];
                if let Some(s) = second {
                    pieces.push(self.piece(profile, spec, o, -d, s, Side::Second)?);
                }
                union_tagged(pieces)
            }
        }
    }

    /// One end's piece: the prism from `o` along `dir`, trimmed if the end asks for it.
    fn piece(
        &self,
        profile: &Profile,
        spec: &ExtrudeSpec,
        o: Point3<f64>,
        dir: Vector3<f64>,
        reach: Reach,
        side: Side,
    ) -> Result<Tagged> {
        let len = match &reach {
            Reach::Depth(d) => *d,
            Reach::Trim { far, .. } => *far,
        };
        let (shape, mut tags) = self.prism(profile, &spec.faces, o - profile.plane.origin, dir * len)?;
        if side == Side::Second {
            // The far cap of the second end is the body's start cap.
            for t in tags.iter_mut().flatten() {
                *t = match *t {
                    Origin::StartCap { region } => Origin::EndCap { region },
                    Origin::EndCap { region } => Origin::StartCap { region },
                    other => other,
                };
            }
        }
        let Reach::Trim { kind, far } = reach else {
            return Ok((shape, tags));
        };
        let trimmed = match kind {
            TrimKind::Plane { point, normal } => {
                // Keep the side the profile is on (the sketch plane's origin may lie anywhere,
                // even on the face's plane: an arc's radial end face through it).
                let sides: Vec<f64> = profile_samples(self, profile, spec, o)?
                    .iter()
                    .map(|p| (p - point).dot(&normal))
                    .collect();
                let above = sides.iter().any(|s| *s > LINEAR_EPS);
                let below = sides.iter().any(|s| *s < -LINEAR_EPS);
                let side_of_profile = match (above, below) {
                    (true, false) => 1.0,
                    (false, true) => -1.0,
                    (true, true) => {
                        return Err(KernelError::InvalidParameter(
                            "the face's plane crosses the profile".into(),
                        ));
                    }
                    (false, false) => {
                        return Err(KernelError::InvalidParameter(
                            "the face's plane passes through the start of the extrude".into(),
                        ));
                    }
                };
                let away = -normal * side_of_profile;
                let size = 4.0 * (far + profile_size(profile)) + 10.0;
                let cutter = half_space(point, normal, away, size)?;
                let (cut, h) = shape.try_subtract_h(&cutter).map_err(occt)?;
                let cutter_tags = vec![None; face_count(&cutter)?];
                let tags = super::carry(&h, &[&tags, &cutter_tags], face_count(&cut)?);
                let near = near_pieces((cut, tags), o, dir)?;
                if near.is_empty() {
                    return Err(KernelError::InvalidParameter(
                        "the face leaves nothing to extrude".into(),
                    ));
                }
                union_tagged(near)?
            }
            TrimKind::Conform { bodies, shift } => {
                let moved: Vec<Shape> = bodies
                    .iter()
                    .map(|b| Ok(self.body(*b)?.translated(to_glam(dir * shift))))
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
                let mut near = near_pieces((out, out_tags), o, dir)?;
                near.extend(near_pieces((inn, inn_tags), o, dir)?);
                if near.is_empty() {
                    return Err(KernelError::InvalidParameter(
                        "the target leaves nothing to extrude".into(),
                    ));
                }
                let (s, t) = union_tagged(near)?;
                // A piece still at the far end: part of the profile met nothing.
                let reach = s
                    .vertices_info()
                    .map_err(occt)?
                    .iter()
                    .map(|(p, _)| (Point3::from(to_na(*p)) - o).dot(&dir))
                    .chain(s.edges_geometry().map_err(occt)?.iter().map(|e| (Point3::from(to_na(e.mid)) - o).dot(&dir)))
                    .fold(f64::NEG_INFINITY, f64::max);
                if reach > far - 1e-6 * far.max(1.0) {
                    return Err(KernelError::InvalidParameter(
                        "part of the profile doesn't meet the target".into(),
                    ));
                }
                (s, t)
            }
        };
        // The faces the trim made are the far cap of the region under them.
        let (shape, mut tags) = trimmed;
        let infos: Vec<crate::FaceInfo> = faces_of(&shape)
            .iter()
            .enumerate()
            .map(|(i, f)| super::face_info(crate::FaceId(i as u64), f))
            .collect();
        for (t, info) in tags.iter_mut().zip(&infos) {
            if t.is_none() {
                let region = region_under(profile, &spec.faces, info.center, dir);
                *t = Some(match side {
                    Side::First => Origin::EndCap { region },
                    Side::Second => Origin::StartCap { region },
                });
            }
        }
        Ok((shape, tags))
    }

    /// The prism of the profile's regions and input faces, moved `shift` from the sketch plane
    /// and swept by `vec`: fused into one shape, each face tagged.
    pub(super) fn prism(
        &self,
        profile: &Profile,
        faces: &[FaceInput],
        shift: Vector3<f64>,
        vec: Vector3<f64>,
    ) -> Result<Tagged> {
        let shifted = Plane {
            origin: profile.plane.origin + shift,
            ..profile.plane
        };
        let mut solids: Vec<Tagged> = Vec::new();
        if !profile.regions.is_empty() {
            for (i, (face, region)) in profile_faces(&shifted, &profile.regions)?
                .iter()
                .zip(&profile.regions)
                .enumerate()
            {
                let (solid, h) = face.try_extrude_h(to_glam(vec)).map_err(occt)?;
                let origins = super::sweep_origins(face, &shifted, i, region, &solid, &h)?;
                solids.push((solid, origins));
            }
        }
        for input in faces {
            let body = self.body(input.body)?;
            let face = faces_of(body)
                .into_iter()
                .nth(input.face.0 as usize)
                .ok_or_else(|| KernelError::OperationFailed(format!("unknown {:?}", input.face)))?;
            let (solid, h) = face.try_extrude_h(to_glam(vec)).map_err(occt)?;
            let mut origins = vec![None; face_count(&solid)?];
            // The face's edges in its own MapShapes order, as the history lists them.
            let body_edges = Edges::of(body);
            let face_edges = Edges::of_face(&face);
            for (i, generated) in h.edges.iter().enumerate() {
                let Some(edge) = face_edges.list.get(i).and_then(|e| body_edges.index(e)) else {
                    continue;
                };
                for &f in generated {
                    if let Some(o) = origins.get_mut(f) {
                        o.get_or_insert(Origin::FromEdge {
                            body: input.body,
                            edge: crate::EdgeId(edge as u64),
                        });
                    }
                }
            }
            for (list, origin) in [
                (&h.first, Origin::StartCap { region: input.source }),
                (&h.last, Origin::EndCap { region: input.source }),
            ] {
                for &f in list {
                    if let Some(o) = origins.get_mut(f) {
                        o.get_or_insert(origin);
                    }
                }
            }
            let solid = if shift.norm() > 0.0 {
                solid.translated(to_glam(shift))
            } else {
                solid
            };
            solids.push((solid, origins));
        }
        union_tagged(solids)
    }

    /// The sheets swept by the profile's curves: one per region loop and one per open chain,
    /// each face tagged with the curve that swept it.
    fn sweep_curves(&self, profile: &Profile, shift: Vector3<f64>, vec: Vector3<f64>) -> Result<Vec<Tagged>> {
        let plane = Plane {
            origin: profile.plane.origin + shift,
            ..profile.plane
        };
        let mut out = Vec::new();
        let mut add = |curves: Vec<Curve2>, closed: bool, region: u64, lp: usize| -> Result<()> {
            let wire = if closed {
                super::loop_wire(&plane, &curves)?
            } else {
                chain_wire(&plane, &curves)?
            };
            let (shape, h) = wire.try_extrude_h(to_glam(vec)).map_err(occt)?;
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
            return Err(KernelError::InvalidProfile("nothing to extrude".into()));
        }
        Ok(out)
    }

}

pub(super) fn clone(shape: &Shape) -> Shape {
    shape.translated(glam::DVec3::ZERO)
}

/// Tags the faces a sweep generated from each input edge with the profile curve the edge lies on
/// (the curve passing nearest the edge's middle).
pub(super) fn edge_origins(
    edges: &[EdgeGeometry],
    generated_by_edge: &[Vec<usize>],
    curves: &[(u64, Vec<Point3<f64>>)],
    region: u64,
    tags: &mut [Option<Origin>],
) {
    for (i, generated) in generated_by_edge.iter().enumerate() {
        let Some(e) = edges.get(i) else { continue };
        let mid = Point3::from(to_na(e.mid));
        let nearest = curves
            .iter()
            .map(|(id, pts)| {
                let d = pts
                    .windows(2)
                    .map(|w| segment_distance(mid, w[0], w[1]))
                    .fold(f64::INFINITY, f64::min);
                (*id, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((curve, _)) = nearest else { continue };
        for &f in generated {
            if let Some(o) = tags.get_mut(f) {
                o.get_or_insert(Origin::ProfileCurve { region, curve });
            }
        }
    }
}

/// An open wire through a chain's curves, each starting where the previous one ended.
pub(super) fn chain_wire(plane: &Plane, curves: &[Curve2]) -> Result<Wire> {
    if curves.is_empty() {
        return Err(KernelError::InvalidProfile("empty chain".into()));
    }
    let n = curves.len();
    let starts: Vec<Point2<f64>> = curves.iter().map(Curve2::start).collect();
    let edges = (0..n)
        .map(|i| {
            let to = if i + 1 < n { starts[i + 1] } else { curves[n - 1].end() };
            curve_edge(plane, &curves[i], starts[i], to, false)
        })
        .collect::<Result<Vec<_>>>()?;
    Wire::try_from_edges(&edges).map_err(occt)
}

/// A slab on the `away` side of the plane through `point` (normal `normal`), `size` across and
/// deep.
fn half_space(point: Point3<f64>, normal: Vector3<f64>, away: Vector3<f64>, size: f64) -> Result<Shape> {
    let m = Unit::new_normalize(normal);
    let helper = if m.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
    let x_dir = Unit::new_normalize(helper - m.into_inner() * helper.dot(&m));
    let plane = Plane {
        origin: point,
        x_dir,
        normal: m,
    };
    let h = size;
    let p = |x: f64, y: f64| Point2::new(x, y);
    let square = Region {
        outer: Loop {
            curves: vec![
                Curve2::Line { a: p(-h, -h), b: p(h, -h), source: None },
                Curve2::Line { a: p(h, -h), b: p(h, h), source: None },
                Curve2::Line { a: p(h, h), b: p(-h, h), source: None },
                Curve2::Line { a: p(-h, h), b: p(-h, -h), source: None },
            ],
        },
        holes: vec![],
        source: None,
    };
    let face: Face = profile_faces(&plane, &[square])?
        .into_iter()
        .next()
        .ok_or_else(|| KernelError::OperationFailed("no cutting face".into()))?;
    face.try_extrude(to_glam(away.normalize() * size)).map_err(occt)
}

/// The solids of `shape` that touch the start plane (through `o`, across `dir`), with their
/// faces' tags.
fn near_pieces((shape, tags): Tagged, o: Point3<f64>, dir: Vector3<f64>) -> Result<Vec<Tagged>> {
    let parent = faces_of(&shape);
    let mut out = Vec::new();
    for solid in shape.sub_shapes(opencascade::safe::SubKind::Solid).map_err(occt)? {
        let faces = faces_of(&solid);
        let piece_tags: Vec<Option<Origin>> = faces
            .iter()
            .map(|f| {
                parent
                    .iter()
                    .position(|p| p.is_same(f))
                    .and_then(|i| tags.get(i).copied().flatten())
            })
            .collect();
        let touches = faces.iter().enumerate().any(|(i, f)| {
            let info = super::face_info(crate::FaceId(i as u64), f);
            info.plane.is_some_and(|pl| {
                pl.normal.dot(&dir).abs() > 1.0 - 1e-9 && ((info.center - o).dot(&dir)).abs() < 1e-7 * (1.0 + info.center.coords.norm())
            })
        });
        if touches {
            out.push((solid, piece_tags));
        }
    }
    Ok(out)
}

/// The region of the profile under a point, looking back along `dir` onto the start plane (the
/// first region, or the first input face, if none is).
fn region_under(profile: &Profile, faces: &[FaceInput], p: Point3<f64>, dir: Vector3<f64>) -> u64 {
    // Back along `dir` onto the sketch plane, in its coordinates.
    let (origin, n) = (profile.plane.origin, profile.plane.normal.into_inner());
    let q = p - dir * ((p - origin).dot(&n) / dir.dot(&n));
    let pt = Point2::new(
        (q - origin).dot(&profile.plane.x_dir),
        (q - origin).dot(&profile.plane.y_dir()),
    );
    for (i, r) in profile.regions.iter().enumerate() {
        let inside = |lp: &Loop| point_in_loop(lp, pt);
        if inside(&r.outer) && !r.holes.iter().any(inside) {
            return r.source.unwrap_or(i as u64);
        }
    }
    profile
        .regions
        .first()
        .map(|r| r.source.unwrap_or(0))
        .or(faces.first().map(|f| f.source))
        .unwrap_or(0)
}

/// Even-odd test against the loop's curves, sampled.
pub(super) fn point_in_loop(lp: &Loop, p: Point2<f64>) -> bool {
    let pts: Vec<Point2<f64>> = lp
        .curves
        .iter()
        .flat_map(|c| (0..24).map(move |k| c.point_at(k as f64 / 24.0)))
        .collect();
    let mut inside = false;
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if x > p.x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Points of the profile on the start plane (through `o`): along every curve and the input
/// faces' edges.
fn profile_samples(k: &OcctKernel, profile: &Profile, spec: &ExtrudeSpec, o: Point3<f64>) -> Result<Vec<Point3<f64>>> {
    let d = spec.direction.into_inner();
    let n = profile.plane.normal.into_inner();
    let shift = d * ((o - profile.plane.origin).dot(&n) / d.dot(&n));
    let plane = Plane {
        origin: profile.plane.origin + shift,
        ..profile.plane
    };
    let mut out = Vec::new();
    let loops = profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .map(|l| l.curves.as_slice())
        .chain(profile.chains.iter().map(|c: &Chain| c.curves.as_slice()));
    for curves in loops {
        for c in curves {
            for i in 0..8 {
                // Just inside the curve's span, so rays don't graze a joint.
                out.push(plane.to_model(c.point_at((i as f64 + 0.5) / 8.0)));
            }
        }
    }
    for r in &profile.regions {
        // A point well inside each region too.
        let pts: Vec<Point2<f64>> = r.outer.curves.iter().map(|c| c.point_at(0.5)).collect();
        if !pts.is_empty() {
            let c = pts.iter().fold(Point2::origin(), |acc, p| acc + p.coords / pts.len() as f64);
            if point_in_loop(&r.outer, c) && !r.holes.iter().any(|h| point_in_loop(h, c)) {
                out.push(plane.to_model(c));
            }
        }
    }
    for input in &spec.faces {
        let body = k.body(input.body)?;
        if let Some(face) = faces_of(body).into_iter().nth(input.face.0 as usize) {
            for e in face.edges_geometry().map_err(occt)? {
                out.push(Point3::from(to_na(e.mid)) + shift);
            }
        }
    }
    Ok(out)
}

/// A size of the profile (mm): the largest distance of a curve point from the plane origin.
fn profile_size(profile: &Profile) -> f64 {
    profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| l.curves.iter())
        .chain(profile.chains.iter().flat_map(|c| c.curves.iter()))
        .flat_map(|c| (0..8).map(move |k| c.point_at(k as f64 / 8.0)))
        .map(|p| p.coords.norm())
        .fold(1.0, f64::max)
}

impl super::Edges {
    /// The distinct edges of a face, in its MapShapes order.
    pub(super) fn of_face(face: &Face) -> Self {
        let mut out = super::Edges {
            list: Vec::new(),
            by_hash: std::collections::HashMap::new(),
        };
        for edge in face.edges() {
            if out.index(&edge).is_none() {
                out.by_hash
                    .entry(edge.identity_hash())
                    .or_default()
                    .push(out.list.len());
                out.list.push(edge);
            }
        }
        out
    }
}
