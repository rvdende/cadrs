//! The full extrude and revolve (P3.3, P3.4): solid, surface and thin bodies; profile regions,
//! open chains and planar faces of bodies; blind, up to vertex / face / part / next and through
//! all ends, symmetric and second ends; with every face tagged with its origin.
//!
//! Ends that are a depth (or an angle) make one sweep. Other ends are trimmed: an oblique
//! planar face cuts a long sweep with the half space beyond its plane; a curved face, a part or
//! "next" *conforms*: the long sweep is split by the targets (`sweep − targets` and
//! `sweep ∩ targets`) and the pieces that touch the start are kept, so every point of the
//! profile stops at the first target face it meets. A kept piece that still reaches the far end
//! means part of the profile met nothing: an error.

use super::boolean::boolean_bodies;
use super::build::Built;
use super::kernel::{NativeKernel, subset};
use super::math::*;
use super::profile::{self, MergedFace, Piece, Src};
use super::sweep::{self, SweepKind};
use super::tol;
use super::topo::{Body, co_range};
use crate::*;

/// A body with the origins of each face.
#[derive(Debug, Clone)]
pub struct Tagged {
    pub body: Body,
    pub tags: Vec<Vec<Origin>>,
}

impl Tagged {
    pub fn from_built(b: Built) -> Self {
        let mut tags = vec![Vec::new(); b.body.faces.len()];
        for (f, o) in b.origins {
            tags[f].push(o);
        }
        Self { body: b.body, tags }
    }
    pub fn into_result(self, k: &mut NativeKernel) -> OpResult {
        let mut history = History::default();
        for (f, ts) in self.tags.iter().enumerate() {
            for o in ts {
                history.generated.push((FaceId(f as u64), *o));
            }
        }
        let id = k.insert(self.body);
        OpResult {
            bodies: vec![id],
            history,
        }
    }
    fn has(&self, f: usize, pred: impl Fn(&Origin) -> bool) -> bool {
        self.tags[f].iter().any(pred)
    }
}

/// The boolean of two tagged bodies (the tool's faces untagged).
pub(crate) fn tagged_bool(a: &Tagged, b: &Tagged, op: BoolOp) -> Result<Tagged> {
    let r = boolean_bodies(&a.body, &b.body, op)?;
    let tags = r
        .from
        .iter()
        .map(|srcs| {
            let mut t: Vec<Origin> = Vec::new();
            for &(o, f) in srcs {
                let from = if o == 0 { &a.tags[f] } else { &b.tags[f] };
                for x in from {
                    if !t.contains(x) {
                        t.push(*x);
                    }
                }
            }
            t
        })
        .collect();
    Ok(Tagged { body: r.body, tags })
}

/// Keeps the solids for which `keep(face tags of the solid)` holds.
fn take_solids(t: &Tagged, keep: impl Fn(&[usize]) -> bool) -> Option<Tagged> {
    let mut faces: Vec<usize> = Vec::new();
    let mut nsolids = 0;
    for s in 0..t.body.solids.len() {
        let fs = t.body.solid_faces(s);
        if keep(&fs) {
            faces.extend(fs);
            nsolids += 1;
        }
    }
    if nsolids == 0 {
        return None;
    }
    let mut b = subset(&t.body, &faces, true);
    // Re-assemble shells and solids for the subset.
    b.shells.clear();
    b.solids.clear();
    let _ = super::boolean::assemble_shells(&mut b);
    let tags = faces.iter().map(|&f| t.tags[f].clone()).collect();
    Some(Tagged { body: b, tags })
}

/// Profile inputs: the merged regions (and faces of bodies) as pieces in the sketch plane.
fn inputs(k: &NativeKernel, profile: &Profile, faces: &[FaceInput]) -> Result<Vec<MergedFace>> {
    let regions = profile::regions(profile)?;
    let mut out = if regions.is_empty() {
        Vec::new()
    } else {
        profile::merge_regions(&regions)?
    };
    for fi in faces {
        out.push(face_as_profile(k, profile, fi)?);
    }
    Ok(out)
}

/// A planar face of a body as a profile face in the sketch plane's coordinates.
pub fn face_as_profile(
    k: &NativeKernel,
    profile: &Profile,
    input: &FaceInput,
) -> Result<MergedFace> {
    let b = k.get(input.body)?;
    let f = b
        .faces
        .get(input.face.0 as usize)
        .ok_or_else(|| KernelError::InvalidParameter(format!("unknown {:?}", input.face)))?;
    let super::surface::Surface::Plane { f: fr } = &f.surf else {
        return Err(KernelError::InvalidParameter(
            "Only planar faces can be extruded or revolved".into(),
        ));
    };
    let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
    let pn = profile.plane.normal.into_inner();
    if n.cross(&pn).norm() > tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "The face is not parallel to the sketch plane".into(),
        ));
    }
    // Model → sketch coordinates.
    let x = profile.plane.x_dir.into_inner();
    let y = profile.plane.y_dir().into_inner();
    let m = nalgebra::Matrix3::from_rows(&[x.transpose(), y.transpose(), pn.transpose()]);
    let to2 = Affine {
        m,
        t: -(m * profile.plane.origin.coords),
    };
    let flip = n.dot(&pn) < 0.0;
    let mut loops: Vec<Vec<Piece>> = Vec::new();
    for l in &f.loops {
        let mut lp: Vec<Piece> = Vec::new();
        for c in l {
            let e = &b.edges[c.edge];
            let c2 = flatten(e.curve.transformed(&to2));
            lp.push(Piece {
                c: c2,
                t0: e.t[0],
                t1: e.t[1],
                rev: !c.fwd,
                src: Src::Edge {
                    body: input.body.0,
                    edge: c.edge as u64,
                },
                more: vec![],
            });
        }
        if flip {
            lp = profile::reverse_loop(&lp);
        }
        loops.push(lp);
    }
    let _ = co_range;
    Ok(MergedFace {
        outer: loops.remove(0),
        holes: loops,
        regions: vec![input.source],
    })
}

fn flatten(c: super::curve::Curve) -> super::curve::Curve {
    use super::curve::Curve;
    match c {
        Curve::Line { o, d } => Curve::Line {
            o: P3::new(o.x, o.y, 0.0),
            d: V3::new(d.x, d.y, 0.0),
        },
        Curve::Circle { c, x, y, r } => Curve::Circle {
            c: P3::new(c.x, c.y, 0.0),
            x: V3::new(x.x, x.y, 0.0),
            y: V3::new(y.x, y.y, 0.0),
            r,
        },
        Curve::Ellipse { c, x, y, a, b } => Curve::Ellipse {
            c: P3::new(c.x, c.y, 0.0),
            x: V3::new(x.x, x.y, 0.0),
            y: V3::new(y.x, y.y, 0.0),
            a,
            b,
        },
        Curve::Nurbs(n) => {
            let mut n = (*n).clone();
            for h in &mut n.cps {
                h.z = 0.0;
            }
            Curve::Nurbs(std::sync::Arc::new(n))
        }
    }
}

/// Open chains as pieces.
fn chains(profile: &Profile) -> Result<Vec<Vec<Piece>>> {
    let mut out = Vec::new();
    for (i, ch) in profile.chains.iter().enumerate() {
        let region = ch.source.unwrap_or((profile.regions.len() + i) as u64);
        let mut ps = Vec::new();
        for (ci, c) in ch.curves.iter().enumerate() {
            let cs = c.source().unwrap_or(unsourced_curve(0, ci));
            ps.push(profile::piece_of(c, Src::Curve { region, curve: cs })?);
        }
        out.push(profile::chain_loop_open(ps)?);
    }
    Ok(out)
}

/// Sample points of the profile (and its input faces) in model space.
fn profile_points(faces: &[MergedFace], m: &Affine) -> Vec<P3> {
    let mut pts = Vec::new();
    for f in faces {
        for l in std::iter::once(&f.outer).chain(f.holes.iter()) {
            for p in l {
                for q in p.sample(0.05) {
                    pts.push(m.point(&P3::new(q.x, q.y, 0.0)));
                }
            }
        }
    }
    pts
}

#[derive(Debug, Clone)]
enum Reach {
    Depth(f64),
    Plane {
        point: P3,
        normal: V3,
        far: f64,
    },
    Conform {
        bodies: Vec<BodyId>,
        shift: f64,
        far: f64,
    },
}

fn too_short(what: &str, depth: f64) -> KernelError {
    if depth.abs() < super::build::MIN_DEPTH {
        KernelError::InvalidParameter(format!(
            "the depth ({:e} mm) is below the kernel's modelling tolerance",
            depth.abs()
        ))
    } else {
        KernelError::InvalidParameter(format!(
            "{what} is behind the start of the extrude (try the opposite direction)"
        ))
    }
}

fn depth(what: &str, d: f64) -> Result<Reach> {
    if !d.is_finite() {
        return Err(KernelError::InvalidParameter(format!(
            "{what} is not a number"
        )));
    }
    if d < super::build::MIN_DEPTH {
        return Err(too_short(what, d));
    }
    Ok(Reach::Depth(d))
}

fn far_extent(k: &NativeKernel, bodies: &[BodyId], o: &P3, d: &V3) -> Result<Option<f64>> {
    let mut far: Option<f64> = None;
    for &b in bodies {
        let (_, hi) = k.extent_along(b, o, d)?;
        far = Some(far.map_or(hi, |x: f64| x.max(hi)));
    }
    Ok(far)
}

fn resolve(
    k: &NativeKernel,
    faces: &[MergedFace],
    m: &Affine,
    spec: &ExtrudeSpec,
    o: P3,
    dir: V3,
    end: ExtrudeEnd,
) -> Result<Reach> {
    match end {
        ExtrudeEnd::Blind(x) => depth("the depth", x),
        ExtrudeEnd::UpToVertex { point, offset } => {
            depth("the vertex", (point - o).dot(&dir) - offset)
        }
        ExtrudeEnd::ThroughAll => match far_extent(k, &spec.scene, &o, &dir)? {
            Some(f) if f > super::build::MIN_DEPTH => Ok(Reach::Depth(f)),
            _ => Err(KernelError::InvalidParameter(
                "Through all: no part lies in the extrude direction".into(),
            )),
        },
        ExtrudeEnd::UpToFace { body, face, offset } => {
            let info = k
                .faces(body)?
                .into_iter()
                .find(|f| f.id == face)
                .ok_or_else(|| KernelError::InvalidParameter(format!("unknown {face:?}")))?;
            if let Some(pl) = info.plane {
                let mn = pl.normal.into_inner();
                if mn.dot(&dir).abs() > 1.0 - tol::ANGULAR {
                    return depth("the face", (pl.origin - o).dot(&dir) - offset);
                }
                if mn.dot(&dir).abs() < tol::ANGULAR_POINT {
                    return Err(KernelError::InvalidParameter(
                        "the face is parallel to the extrude direction".into(),
                    ));
                }
                let point = pl.origin - dir * offset;
                let pts = profile_points(faces, m);
                let far = pts
                    .iter()
                    .map(|p| (point - p).dot(&mn) / dir.dot(&mn))
                    .fold(f64::NEG_INFINITY, f64::max);
                if far.is_nan() || far <= super::build::MIN_DEPTH {
                    return Err(too_short("the face", far));
                }
                Ok(Reach::Plane {
                    point,
                    normal: mn,
                    far: far * 1.1 + 1.0,
                })
            } else {
                conform(k, vec![body], o, dir, offset, "the face")
            }
        }
        ExtrudeEnd::UpToPart { body, offset } => conform(k, vec![body], o, dir, offset, "the part"),
        ExtrudeEnd::UpToNext { offset } => {
            let pts = profile_points(faces, m);
            let mut ahead = Vec::new();
            for &b in &spec.scene {
                let mut hit = false;
                for p in pts.iter().step_by((pts.len() / 64).max(1)) {
                    if k.ray_hits(b, *p, dir)?.iter().any(|h| h.t > 1e-7) {
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
            conform(k, ahead, o, dir, offset, "the next part")
        }
    }
}

fn conform(
    k: &NativeKernel,
    bodies: Vec<BodyId>,
    o: P3,
    dir: V3,
    offset: f64,
    what: &str,
) -> Result<Reach> {
    let far = far_extent(k, &bodies, &o, &dir)?.unwrap_or(f64::NEG_INFINITY) - offset;
    if far.is_nan() || far <= super::build::MIN_DEPTH {
        return Err(too_short(what, far));
    }
    let mut diag: f64 = 0.0;
    for b in &bodies {
        diag = diag.max(k.bounding_box(*b)?.diagonal());
    }
    Ok(Reach::Conform {
        bodies,
        shift: -offset,
        far: far + 0.1 * diag + 1.0,
    })
}

/// A box on the far side of the plane through `point` (normal `normal`), towards `away`.
fn half_space(point: P3, normal: V3, away: V3, size: f64) -> Result<Body> {
    let n = if normal.dot(&away) > 0.0 {
        normal
    } else {
        -normal
    };
    let f = Frame::from_z(point, n);
    let plane = Plane {
        origin: point,
        x_dir: nalgebra::Unit::new_normalize(f.x),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let sq = Region {
        outer: Loop {
            curves: vec![
                Curve2::Line {
                    a: nalgebra::Point2::new(-size, -size),
                    b: nalgebra::Point2::new(size, -size),
                    source: None,
                },
                Curve2::Line {
                    a: nalgebra::Point2::new(size, -size),
                    b: nalgebra::Point2::new(size, size),
                    source: None,
                },
                Curve2::Line {
                    a: nalgebra::Point2::new(size, size),
                    b: nalgebra::Point2::new(-size, size),
                    source: None,
                },
                Curve2::Line {
                    a: nalgebra::Point2::new(-size, size),
                    b: nalgebra::Point2::new(-size, -size),
                    source: None,
                },
            ],
        },
        holes: vec![],
        source: None,
    };
    let prof = Profile::new(plane, vec![sq]);
    let regions = profile::regions(&prof)?;
    let mf = profile::merge_regions(&regions)?;
    let kind = sweep::Prism { plane, dir: n };
    Ok(sweep::sweep(&plane, &mf, &kind, 0.0, size, false)?.body)
}

/// The region whose face lies under `p` (mapped onto the profile plane by `to2`).
fn region_under<'a>(
    faces: &'a [MergedFace],
    to2: impl Fn(&P3) -> P2 + 'a,
) -> impl Fn(&P3) -> u64 + 'a {
    let polys: Vec<(Vec<P2>, Vec<Vec<P2>>, u64)> = faces
        .iter()
        .map(|f| {
            (
                profile::loop_polygon(&f.outer, 1e-3),
                f.holes
                    .iter()
                    .map(|h| profile::loop_polygon(h, 1e-3))
                    .collect(),
                f.regions.first().copied().unwrap_or(0),
            )
        })
        .collect();
    move |p: &P3| {
        let q = to2(p);
        for (o, hs, r) in &polys {
            if point_in_polygon(&q, o) && !hs.iter().any(|h| point_in_polygon(&q, h)) {
                return *r;
            }
        }
        polys.first().map(|x| x.2).unwrap_or(0)
    }
}

/// One end's piece: the sweep, trimmed if the end asks for it. `first`: the end along the
/// direction (else the second end, whose far cap is the body's start cap).
#[allow(clippy::too_many_arguments)]
fn piece(
    k: &NativeKernel,
    faces: &[MergedFace],
    plane: &Plane,
    o: P3,
    dir: V3,
    reach: Reach,
    first: bool,
) -> Result<Tagged> {
    let len = match &reach {
        Reach::Depth(d) => *d,
        Reach::Plane { far, .. } | Reach::Conform { far, .. } => *far,
    };
    let start = (o - plane.origin).dot(&dir);
    let kind = sweep::Prism { plane: *plane, dir };
    let mut t = sweep_united(faces, &kind, start, start + len, false)?;
    let swap = |t: &mut Tagged| {
        for ts in &mut t.tags {
            for x in ts.iter_mut() {
                *x = match *x {
                    Origin::StartCap { region } => Origin::EndCap { region },
                    Origin::EndCap { region } => Origin::StartCap { region },
                    other => other,
                };
            }
        }
    };
    let trimmed = match reach {
        Reach::Depth(_) => {
            if !first {
                swap(&mut t);
            }
            return Ok(t);
        }
        Reach::Plane { point, normal, far } => {
            let pts = profile_points(faces, &super::build::plane_map(plane, &dir, start));
            let above = pts.iter().any(|p| (p - point).dot(&normal) > 1e-7);
            let below = pts.iter().any(|p| (p - point).dot(&normal) < -1e-7);
            let side = match (above, below) {
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
            let bb = t.body.bbox();
            let size = 4.0 * (far + bb.diagonal()) + 10.0;
            let cutter = Tagged {
                tags: Vec::new(),
                body: half_space(point, normal, -normal * side, size)?,
            };
            let cutter = Tagged {
                tags: vec![Vec::new(); cutter.body.faces.len()],
                ..cutter
            };
            let cut = tagged_bool(&t, &cutter, BoolOp::Subtract)?;
            let near = take_solids(&cut, |fs| {
                fs.iter()
                    .any(|&f| cut.has(f, |o| matches!(o, Origin::StartCap { .. })))
            });
            near.ok_or_else(|| {
                KernelError::InvalidParameter("the face leaves nothing to extrude".into())
            })?
        }
        Reach::Conform { bodies, shift, far } => {
            let mut tool: Option<Body> = None;
            for b in &bodies {
                let moved = k.get(*b)?.transformed(&Affine::translation(dir * shift));
                tool = Some(match tool {
                    None => moved,
                    Some(t0) => boolean_bodies(&t0, &moved, BoolOp::Union)?.body,
                });
            }
            let tool = tool.ok_or_else(|| {
                KernelError::InvalidParameter("Up to part: no part to extrude up to".into())
            })?;
            let tool = Tagged {
                tags: vec![Vec::new(); tool.faces.len()],
                body: tool,
            };
            let is_start = |x: &Tagged, fs: &[usize]| {
                fs.iter()
                    .any(|&f| x.has(f, |o| matches!(o, Origin::StartCap { .. })))
            };
            let out = tagged_bool(&t, &tool, BoolOp::Subtract).ok();
            let inn = tagged_bool(&t, &tool, BoolOp::Intersect).ok();
            let a = out
                .as_ref()
                .and_then(|x| take_solids(x, |fs| is_start(x, fs)));
            let b = inn
                .as_ref()
                .and_then(|x| take_solids(x, |fs| is_start(x, fs)));
            let kept = match (a, b) {
                (Some(a), Some(b)) => tagged_bool(&a, &b, BoolOp::Union)?,
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => {
                    return Err(KernelError::InvalidParameter(
                        "the target leaves nothing to extrude".into(),
                    ));
                }
            };
            // A piece still at the far end: part of the profile met nothing.
            let (_, hi) = super::kernel::extent_along(&kept.body, &o, &dir);
            if hi > far - tol::REL_POINT * far.max(1.0)
                || kept
                    .tags
                    .iter()
                    .any(|ts| ts.iter().any(|o| matches!(o, Origin::EndCap { .. })))
            {
                return Err(KernelError::InvalidParameter(
                    "part of the profile doesn't meet the target".into(),
                ));
            }
            kept
        }
    };
    // New faces are the far cap of the region under them.
    let mut t = trimmed;
    let x = plane.x_dir.into_inner();
    let y = plane.y_dir().into_inner();
    let pn = plane.normal.into_inner();
    let to2 = move |p: &P3| {
        // Project along the sweep onto the sketch plane.
        let w = p - plane.origin;
        let s = w.dot(&pn) / dir.dot(&pn);
        let q = w - dir * s;
        P2::new(q.dot(&x), q.dot(&y))
    };
    let under = region_under(faces, to2);
    for f in 0..t.body.faces.len() {
        if t.tags[f].is_empty() {
            let (_, c) = super::mass::face_area(&t.body, f);
            let region = under(&c);
            t.tags[f].push(Origin::EndCap { region });
        }
    }
    if !first {
        swap(&mut t);
    }
    Ok(t)
}

pub fn extrude_with(
    k: &mut NativeKernel,
    profile: &Profile,
    spec: &ExtrudeSpec,
) -> Result<OpResult> {
    let d = spec.direction.into_inner();
    let n = profile.plane.normal.into_inner();
    if d.dot(&n).abs() < tol::ANGULAR_POINT {
        return Err(KernelError::InvalidParameter(
            "the extrude direction lies in the sketch plane".into(),
        ));
    }
    if profile.regions.is_empty() && spec.faces.is_empty() && profile.chains.is_empty() {
        return Err(KernelError::InvalidProfile(
            "the profile has no regions".into(),
        ));
    }
    if spec.body == BodyKind::Surface && !spec.faces.is_empty() {
        return Err(KernelError::InvalidParameter(
            "Surface extrudes take sketch curves, not faces".into(),
        ));
    }
    let (profile_used, faces) = match spec.body {
        BodyKind::Thin { left, right } => {
            let bands = crate::thin::thin_profile(profile, left, right)?;
            let f = profile::fuse_sides(inputs(k, &bands, &spec.faces)?);
            (bands, f)
        }
        _ => {
            let f = profile::fuse_sides(inputs(k, profile, &spec.faces)?);
            (profile.clone(), f)
        }
    };
    let o = profile.plane.origin + d * spec.start_offset;
    let m = super::build::plane_map(&profile.plane, &d, spec.start_offset);
    let (first, second) = if spec.symmetric {
        match spec.end {
            ExtrudeEnd::Blind(t) => {
                let half = depth("the depth", t / 2.0)?;
                (half.clone(), Some(half))
            }
            ExtrudeEnd::ThroughAll => (
                resolve(k, &faces, &m, spec, o, d, ExtrudeEnd::ThroughAll)?,
                Some(resolve(k, &faces, &m, spec, o, -d, ExtrudeEnd::ThroughAll)?),
            ),
            _ => {
                return Err(KernelError::InvalidParameter(
                    "Symmetric works with Blind and Through all".into(),
                ));
            }
        }
    } else {
        let first = resolve(k, &faces, &m, spec, o, d, spec.end)?;
        let second = spec
            .second
            .map(|e| resolve(k, &faces, &m, spec, o, -d, e))
            .transpose()?;
        (first, second)
    };
    let plane = profile.plane;
    if spec.body == BodyKind::Surface {
        let (Reach::Depth(f), b) = (
            first,
            second
                .map(|s| match s {
                    Reach::Depth(b) => Ok(b),
                    _ => Err(KernelError::Unsupported(
                        "Up to next, part or face for surfaces",
                    )),
                })
                .transpose()?
                .unwrap_or(0.0),
        ) else {
            return Err(KernelError::Unsupported(
                "Up to next, part or face for surfaces",
            ));
        };
        let start = spec.start_offset - b;
        let kind = sweep::Prism { plane, dir: d };
        let ch = chains(&profile_used)?;
        let mut built = sweep::sweep_full(&faces, &ch, &kind, start, start + f + b, false, false)?;
        // A surface's faces face the sweep direction crossed with its curves (an outer loop's
        // walls inwards, holes' outwards, a chain's to the left of the way it runs about the
        // direction), as OCCT's extruded wires did: a thicken or an offset of them goes the way
        // the documents made with it expect.
        for face in &mut built.body.faces {
            Body::reverse_face(face);
        }
        let t = Tagged::from_built(built);
        return Ok(t.into_result(k));
    }
    let t = match (&first, &second) {
        (Reach::Depth(f), None) => piece(k, &faces, &plane, o, d, Reach::Depth(*f), true)?,
        (Reach::Depth(f), Some(Reach::Depth(b))) => {
            let kind = sweep::Prism { plane, dir: d };
            let start = spec.start_offset - b;
            sweep_united(&faces, &kind, start, start + f + b, false)?
        }
        _ => {
            let a = piece(k, &faces, &plane, o, d, first, true)?;
            match second {
                Some(s) => {
                    // The second end sweeps the same profile the other way.
                    let b = piece(k, &faces, &plane, o, -d, s, false)?;
                    tagged_bool(&a, &b, BoolOp::Union)?
                }
                None => a,
            }
        }
    };
    // Several input faces and regions: one body.
    let t = if faces.len() > 1 && t.body.solids.len() > 1 && !spec.faces.is_empty() {
        unite_solids(t)?
    } else {
        t
    };
    Ok(t.into_result(k))
}

/// Sweeps faces into a solid; faces whose boxes overlap (thin walls' bands) are united.
/// A straight sweep of a profile's regions along any vector (an oblique prism).
pub fn prism_along(k: &mut NativeKernel, profile: &Profile, v: V3) -> Result<OpResult> {
    let len = v.norm();
    if len < tol::DEGENERATE || v.dot(&profile.plane.normal).abs() < tol::REL * len {
        return Err(KernelError::InvalidParameter(
            "A prism needs a direction off the profile's plane".into(),
        ));
    }
    let faces = inputs(k, profile, &[])?;
    let kind = sweep::Prism {
        plane: profile.plane,
        dir: v / len,
    };
    Ok(sweep_united(&faces, &kind, 0.0, len, false)?.into_result(k))
}

fn sweep_united(
    faces: &[MergedFace],
    kind: &dyn SweepKind,
    s0: f64,
    s1: f64,
    whole: bool,
) -> Result<Tagged> {
    let t = Tagged::from_built(sweep::sweep_full(faces, &[], kind, s0, s1, whole, true)?);
    if faces.len() < 2 {
        return Ok(t);
    }
    let boxes: Vec<Bbox> = faces
        .iter()
        .map(|f| {
            let mut bb = Bbox::empty();
            for q in profile::loop_polygon(&f.outer, 1e-2) {
                bb.add(&P3::new(q.x, q.y, 0.0));
            }
            bb
        })
        .collect();
    let mut overlap = false;
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            let (a, b) = (&boxes[i], &boxes[j]);
            if a.min.x < b.max.x - tol::POINT
                && b.min.x < a.max.x - tol::POINT
                && a.min.y < b.max.y - tol::POINT
                && b.min.y < a.max.y - tol::POINT
            {
                overlap = true;
            }
        }
    }
    if overlap { unite_solids(t) } else { Ok(t) }
}

/// Fuses the separate solids of a tagged body that overlap or touch.
fn unite_solids(t: Tagged) -> Result<Tagged> {
    let n = t.body.solids.len();
    let parts: Vec<Tagged> = (0..n)
        .filter_map(|s| {
            let fs = t.body.solid_faces(s);
            take_solids(&t, |x| x == fs.as_slice())
        })
        .collect();
    // Pairwise, a level at a time, the pairs of a level side by side (one by one onto the
    // first, each union grew with the body so far: pi7zoll-gehäuse 4.5% slower). The order
    // decides which region's name a merged face keeps: changing it renames those faces.
    use rayon::prelude::*;
    let mut level = parts;
    while level.len() > 1 {
        level = level
            .par_chunks(2)
            .map(|c| match c {
                [a, b] => tagged_bool(a, b, BoolOp::Union),
                [a] => Ok(a.clone()),
                _ => unreachable!("chunks of one or two"),
            })
            .collect::<Result<Vec<Tagged>>>()?;
    }
    level
        .pop()
        .ok_or_else(|| KernelError::InvalidParameter("No solid to unite".into()))
}

/// The revolve frame: z along the axis, x from the axis towards the profile (which must lie on
/// one side of it, in a plane through it).
pub fn revolve_frame(
    profile: &Profile,
    faces: &[MergedFace],
    chains: &[Vec<Piece>],
    axis: &Axis,
) -> Result<Frame> {
    let n = profile.plane.normal.into_inner();
    let z = axis.dir.into_inner();
    if z.dot(&n).abs() > tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "The revolve axis must lie in the sketch plane".into(),
        ));
    }
    if (axis.origin - profile.plane.origin).dot(&n).abs() > tol::POINT {
        return Err(KernelError::InvalidParameter(
            "The revolve axis must lie in the sketch plane".into(),
        ));
    }
    let x = z.cross(&n).normalize();
    let m = super::build::plane_map(&profile.plane, &V3::zeros(), 0.0);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut add = |pieces: &[Piece]| {
        for p in pieces {
            for q in p.sample(1e-3) {
                let w = m.point(&P3::new(q.x, q.y, 0.0));
                let d = (w - axis.origin).dot(&x);
                lo = lo.min(d);
                hi = hi.max(d);
            }
        }
    };
    for f in faces {
        add(&f.outer);
        for h in &f.holes {
            add(h);
        }
    }
    for c in chains {
        add(c);
    }
    let x = if hi > tol::POINT && lo < -tol::POINT {
        return Err(KernelError::InvalidParameter(
            "The profile crosses the revolve axis".into(),
        ));
    } else if hi > tol::POINT {
        x
    } else {
        -x
    };
    Ok(Frame {
        o: axis.origin,
        x,
        y: z.cross(&x),
        z,
    })
}

pub fn revolve_with(
    k: &mut NativeKernel,
    profile: &Profile,
    spec: &RevolveSpec,
) -> Result<OpResult> {
    if spec.body == BodyKind::Surface && !spec.faces.is_empty() {
        return Err(KernelError::InvalidParameter(
            "Surface revolves take sketch curves, not faces".into(),
        ));
    }
    let used = match spec.body {
        BodyKind::Thin { left, right } => crate::thin::thin_profile(profile, left, right)?,
        _ => profile.clone(),
    };
    let faces = inputs(k, &used, &spec.faces)?;
    let ch = if spec.body == BodyKind::Surface {
        chains(&used)?
    } else {
        Vec::new()
    };
    if faces.is_empty() && ch.is_empty() {
        return Err(KernelError::InvalidProfile("no regions".into()));
    }
    if axis_off_plane(profile, &spec.axis) {
        return revolve_off_plane(k, profile, spec, &faces, &ch);
    }
    let f = revolve_frame(profile, &faces, &ch, &spec.axis)?;
    let kind = sweep::Revolve {
        plane: profile.plane,
        f,
    };
    let solid = spec.body != BodyKind::Surface;
    // An end's angle, or None for a conforming end.
    enum End {
        Angle(f64),
        Conform(Vec<BodyId>, f64),
    }
    let end_of = |k: &NativeKernel, e: &RevolveEnd, sign: f64| -> Result<End> {
        let ang_of = |p: &P3| {
            let l = f.local(p);
            wrap(sign * l.y.atan2(l.x), 0.0, TAU)
        };
        Ok(match *e {
            RevolveEnd::Angle(a) => End::Angle(a),
            RevolveEnd::UpToVertex { point, offset } => End::Angle(ang_of(&point) - offset),
            RevolveEnd::UpToFace { body, face, offset } => {
                let info = k
                    .faces(body)?
                    .into_iter()
                    .find(|x| x.id == face)
                    .ok_or_else(|| KernelError::InvalidParameter(format!("unknown {face:?}")))?;
                match info.plane {
                    Some(pl)
                        if pl.normal.dot(&f.z).abs() < tol::ANGULAR
                            && (pl.origin - f.o).dot(&pl.normal).abs() < tol::POINT =>
                    {
                        End::Angle(ang_of(&info.center) - offset)
                    }
                    _ => End::Conform(vec![body], offset),
                }
            }
            RevolveEnd::UpToPart { body, offset } => End::Conform(vec![body], offset),
            RevolveEnd::UpToNext { offset } => {
                // The scene bodies the almost-whole turn overlaps.
                let pts = profile_points(
                    &faces,
                    &super::build::plane_map(&profile.plane, &V3::zeros(), 0.0),
                );
                let sweep_all =
                    sweep::sweep_full(&faces, &[], &kind, 0.0, TAU * (1.0 - 1e-4), false, true)?
                        .body;
                let _ = pts;
                let mut ahead = Vec::new();
                for &b in &spec.scene {
                    let bb = k.get(b)?;
                    if boolean_bodies(&sweep_all, bb, BoolOp::Intersect).is_ok() {
                        ahead.push(b);
                    }
                }
                if ahead.is_empty() {
                    return Err(KernelError::InvalidParameter(
                        "Up to next: nothing lies in the way of the revolve".into(),
                    ));
                }
                End::Conform(ahead, offset)
            }
        })
    };
    let (s0, s1, whole, conform) = if spec.full {
        (0.0, TAU, true, None)
    } else if spec.symmetric {
        let End::Angle(a) = end_of(k, &spec.end, 1.0)? else {
            return Err(KernelError::InvalidParameter(
                "Symmetric works with an angle".into(),
            ));
        };
        (-a.abs() / 2.0, a.abs() / 2.0, false, None)
    } else {
        let e1 = end_of(k, &spec.end, 1.0)?;
        let a0 = match &spec.second {
            Some(e) => match end_of(k, e, -1.0)? {
                End::Angle(a) => -a,
                End::Conform(..) => {
                    return Err(KernelError::Unsupported(
                        "a conforming second end of a revolve",
                    ));
                }
            },
            None => 0.0,
        };
        match e1 {
            End::Angle(a1) => {
                let (a0, a1) = if a1 >= a0 { (a0, a1) } else { (a1, a0) };
                (a0, a1, false, None)
            }
            End::Conform(b, off) => (a0, TAU * (1.0 - 1e-4) + a0, false, Some((b, off))),
        }
    };
    if s1 - s0 > TAU + tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "The revolve turns more than a whole turn".into(),
        ));
    }
    let whole = whole || (s1 - s0) >= TAU - tol::ANGULAR_FINE;
    let mut t = if solid {
        sweep_united(&faces, &kind, s0, if whole { s0 + TAU } else { s1 }, whole)?
    } else {
        Tagged::from_built(sweep::sweep_full(
            &faces,
            &ch,
            &kind,
            s0,
            if whole { s0 + TAU } else { s1 },
            whole,
            false,
        )?)
    };
    // A whole turn of a segment square to the axis is a disc or an annulus: no slit seam (and no
    // edges left over on the axis).
    if whole {
        super::unify::drop_slits(&mut t.body);
    }
    let Some((bodies, offset)) = conform else {
        return Ok(t.into_result(k));
    };
    if !solid {
        return Err(KernelError::Unsupported(
            "Up to next, part or face for surface revolves",
        ));
    }
    // Conform: split the almost-whole turn by the targets turned back by the offset.
    let rot = |a: f64| {
        let r = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(f.z), a)
            .into_inner();
        Affine {
            m: r,
            t: f.o.coords - r * f.o.coords,
        }
    };
    let mut tool: Option<Body> = None;
    for b in &bodies {
        let moved = k.get(*b)?.transformed(&rot(-offset));
        tool = Some(match tool {
            None => moved,
            Some(t0) => boolean_bodies(&t0, &moved, BoolOp::Union)?.body,
        });
    }
    let tool = tool.ok_or_else(|| {
        KernelError::InvalidParameter("Up to part: no part to extrude up to".into())
    })?;
    let tool = Tagged {
        tags: vec![Vec::new(); tool.faces.len()],
        body: tool,
    };
    let is_start = |x: &Tagged, fs: &[usize]| {
        fs.iter()
            .any(|&f| x.has(f, |o| matches!(o, Origin::StartCap { .. })))
    };
    let out = tagged_bool(&t, &tool, BoolOp::Subtract).ok();
    let inn = tagged_bool(&t, &tool, BoolOp::Intersect).ok();
    let a = out
        .as_ref()
        .and_then(|x| take_solids(x, |fs| is_start(x, fs)));
    let b = inn
        .as_ref()
        .and_then(|x| take_solids(x, |fs| is_start(x, fs)));
    let mut kept = match (a, b) {
        (Some(a), Some(b)) => tagged_bool(&a, &b, BoolOp::Union)?,
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => {
            return Err(KernelError::InvalidParameter(
                "the target leaves nothing to revolve".into(),
            ));
        }
    };
    if kept
        .tags
        .iter()
        .any(|ts| ts.iter().any(|o| matches!(o, Origin::EndCap { .. })))
    {
        return Err(KernelError::InvalidParameter(
            "part of the profile doesn't meet the target".into(),
        ));
    }
    // New faces: the end cap of the region under them (turned back onto the profile plane).
    let x = profile.plane.x_dir.into_inner();
    let y = profile.plane.y_dir().into_inner();
    let po = profile.plane.origin;
    let to2 = move |p: &P3| {
        let l = f.local(p);
        let rho = (l.x * l.x + l.y * l.y).sqrt();
        let q = f.world_point(&V3::new(rho, 0.0, l.z)) - po;
        P2::new(q.dot(&x), q.dot(&y))
    };
    let under = region_under(&faces, to2);
    for fi in 0..kept.body.faces.len() {
        if kept.tags[fi].is_empty() {
            let (_, c) = super::mass::face_area(&kept.body, fi);
            kept.tags[fi].push(Origin::EndCap { region: under(&c) });
        }
    }
    Ok(kept.into_result(k))
}

/// Whether a revolve axis leaves the sketch plane (or is parallel to it off it).
fn axis_off_plane(profile: &Profile, axis: &Axis) -> bool {
    let n = profile.plane.normal.into_inner();
    axis.dir.dot(&n).abs() > tol::ANGULAR
        || (axis.origin - profile.plane.origin).dot(&n).abs() > tol::POINT
}

/// A revolve about an axis off the sketch plane (SMLib's rotational sweep of a face about any
/// axis, [`sweep::Turn`]): angle ends only (an angle, symmetric, a second angle, or a whole
/// turn). Every point of the profile must leave the plane the same way: the profile lies on one
/// side of the plane through the axis along the plane's normal.
fn revolve_off_plane(
    k: &mut NativeKernel,
    profile: &Profile,
    spec: &RevolveSpec,
    faces: &[MergedFace],
    ch: &[Vec<Piece>],
) -> Result<OpResult> {
    let n = profile.plane.normal.into_inner();
    let z = spec.axis.dir.into_inner();
    let o = spec.axis.origin;
    if z.cross(&n).norm() < tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "The revolve axis must not be square to the sketch plane".into(),
        ));
    }
    let m = super::build::plane_map(&profile.plane, &V3::zeros(), 0.0);
    let mut pts: Vec<P3> = Vec::new();
    for l in faces
        .iter()
        .flat_map(|f| std::iter::once(&f.outer).chain(f.holes.iter()))
        .chain(ch.iter())
    {
        for p in l {
            pts.extend(
                p.sample(1e-3)
                    .into_iter()
                    .map(|q| m.point(&P3::new(q.x, q.y, 0.0))),
            );
        }
    }
    let size = pts.iter().map(|p| (p - o).norm()).fold(1.0, f64::max);
    // How fast each point leaves the plane: (z × (p − o))·n.
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in &pts {
        let d = z.cross(&(p - o)).dot(&n);
        lo = lo.min(d);
        hi = hi.max(d);
    }
    let eps = tol::REL * size;
    if hi > eps && lo < -eps {
        return Err(KernelError::InvalidParameter(
            "The profile lies on both sides of the revolve axis".into(),
        ));
    }
    if !(hi > eps || lo < -eps) {
        return Err(KernelError::InvalidParameter(
            "The profile turns within its plane about this axis".into(),
        ));
    }
    // x towards the profile's middle (any square to the axis when that is on it).
    let mid = pts.iter().fold(V3::zeros(), |s, p| s + (p - o)) / pts.len().max(1) as f64;
    let mut x = mid - z * mid.dot(&z);
    if x.norm() < tol::REL * size {
        x = n - z * n.dot(&z);
    }
    let x = x.normalize();
    let kind = sweep::Turn {
        plane: profile.plane,
        f: Frame {
            o,
            x,
            y: z.cross(&x),
            z,
        },
        up: hi > eps,
    };
    let (s0, s1) = if spec.full {
        (0.0, TAU)
    } else {
        let angle = |e: &RevolveEnd| match *e {
            RevolveEnd::Angle(a) => Ok(a),
            _ => Err(KernelError::Unsupported(
                "up-to ends for a revolve about an axis off the sketch plane",
            )),
        };
        let a1 = angle(&spec.end)?;
        if spec.symmetric {
            (-a1.abs() / 2.0, a1.abs() / 2.0)
        } else {
            let a0 = match &spec.second {
                Some(e) => -angle(e)?,
                None => 0.0,
            };
            (a0.min(a1), a0.max(a1))
        }
    };
    if s1 - s0 > TAU + tol::ANGULAR {
        return Err(KernelError::InvalidParameter(
            "The revolve turns more than a whole turn".into(),
        ));
    }
    let whole = (s1 - s0) >= TAU - tol::ANGULAR_FINE;
    let s1 = if whole { s0 + TAU } else { s1 };
    let mut t = if spec.body != BodyKind::Surface {
        sweep_united(faces, &kind, s0, s1, whole)?
    } else {
        Tagged::from_built(sweep::sweep_full(faces, ch, &kind, s0, s1, whole, false)?)
    };
    if whole {
        super::unify::drop_slits(&mut t.body);
    }
    Ok(t.into_result(k))
}

/// The union of two tagged bodies, every face keeping its tags.
pub fn tagged_union(a: &Tagged, b: &Tagged) -> Result<Tagged> {
    tagged_bool(a, b, BoolOp::Union)
}
