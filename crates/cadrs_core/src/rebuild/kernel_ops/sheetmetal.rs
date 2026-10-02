//! Rebuilding the Sheet metal model feature (`crate::sheetmetal`, P3I.2). A child of
//! `rebuild::kernel_ops`, so it shares its helpers (face lookup, splitting, finishing parts).
//!
//! 1. The inputs become plain geometry for [`cadrs_sheetmetal::construct`]: a part's planar faces
//!    (outlines from their boundary loops), the straight edges two of them share, its cylinders
//!    (a fillet's round: bent or rolled); the picked sketch curves as chains; sketch regions.
//! 2. The definition it builds is checked in 3D ([`Model::validate`]) and laid flat
//!    ([`flatten`]): a flat that overlaps itself fails the feature with "Collision in sheet metal
//!    flat pattern" (SM1.5), keeping the context so the flat view can show where.
//! 3. The **folded solid**, per flat-pattern part: each planar wall's outline (less the relief
//!    cuts' material) extruded by the thickness on its material side; each rolled wall and each
//!    bend an annular sector extruded along its axis (the bend's relief cuts taken out as
//!    wedges); fused into one body. A fused volume short of the pieces' sum means walls run into
//!    each other in 3D: the feature fails ("Sheet metal walls intersect").

use super::*;
use cadrs_kernel::{Curve2, Extent, Kernel, Loop, Plane};
use cadrs_sheetmetal::construct::{self, ChainIn, ChainOpts, CylIn, EdgeIn, FaceIn, FaceOpts, Seg};
use cadrs_sheetmetal::flat::{FlatError, FlatPart, PieceSource};
use cadrs_sheetmetal::model::{P3, Surface, V3, Wall};
use cadrs_sheetmetal::poly::{self, P2, Polygon};
use cadrs_sheetmetal::{FlatPattern, Model, WallId, flatten};

use crate::applied::EdgeOrFace;
use crate::document::{EndCondition, EndType, UpTo};
use crate::sheetmetal::{SheetMetalContext, SheetMetalModelFeature, SheetMetalOp};
use crate::solid::Solid;

/// A stable key for a face, edge or curve name.
fn key_of<T: std::fmt::Debug>(x: &T) -> u64 {
    naming::stable_hash(format!("{x:?}").as_bytes())
}

fn v3(a: Vec3) -> V3 {
    V3::new(a[0], a[1], a[2])
}

fn p3(a: Vec3) -> P3 {
    P3::new(a[0], a[1], a[2])
}

/// A loop's points without repeats and without points in the middle of straight runs.
fn simplify(mut pts: Vec<P2>, tol: f64) -> Vec<P2> {
    pts.dedup_by(|a, b| (*a - *b).norm() < tol);
    while pts.len() > 2 && (pts[0] - pts[pts.len() - 1]).norm() < tol {
        pts.pop();
    }
    loop {
        let n = pts.len();
        if n < 4 {
            return pts;
        }
        let mut removed = false;
        for i in 0..n {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (d1, d2) = (b - a, c - b);
            if d1.perp(&d2).abs() <= 1e-9 * d1.norm() * d2.norm() + tol * 1e-3 && d1.dot(&d2) > 0.0 {
                pts.remove(i);
                removed = true;
                break;
            }
        }
        if !removed {
            return pts;
        }
    }
}

/// A planar face of a solid as a wall input (its outer loop and holes in its plane).
fn face_in(solid: &Solid, i: usize) -> Option<FaceIn> {
    let f = &solid.faces[i];
    let pl = f.plane?;
    let n = v3(pl.normal()).normalize();
    let u = v3(pl.u).normalize();
    let v = n.cross(&u);
    let origin = p3(pl.origin);
    let size = f
        .loops
        .iter()
        .flatten()
        .map(|p| (p3(*p) - origin).norm())
        .fold(1.0, f64::max);
    let tol = 1e-7 * size;
    let mut loops: Vec<Vec<P2>> = f
        .loops
        .iter()
        .map(|l| simplify(l.iter().map(|p| P2::new((p3(*p) - origin).dot(&u), (p3(*p) - origin).dot(&v))).collect(), tol))
        .filter(|l| l.len() >= 3)
        .collect();
    if loops.is_empty() {
        return None;
    }
    let outer = (0..loops.len()).max_by(|a, b| poly::signed_area(&loops[*a]).abs().total_cmp(&poly::signed_area(&loops[*b]).abs()))?;
    let outer = loops.remove(outer);
    Some(FaceIn { key: key_of(&f.name), origin, u, v, outline: Polygon::with_holes(outer, loops) })
}

/// Whether a polyline is straight (within `tol`).
fn straight(pts: &[Vec3], tol: f64) -> bool {
    if pts.len() < 2 {
        return false;
    }
    let (a, b) = (p3(pts[0]), p3(pts[pts.len() - 1]));
    let d = b - a;
    let len = d.norm();
    if len < tol {
        return false;
    }
    let dir = d / len;
    pts.iter().all(|p| {
        let w = p3(*p) - a;
        (w - dir * w.dot(&dir)).norm() <= tol
    })
}

/// A cylindrical face of a solid as a wall input, with its flat tangent neighbours among
/// `faces` (by solid face index → index in `faces`).
fn cyl_in(solid: &Solid, i: usize, planar: &[(usize, usize)], faces: &[FaceIn]) -> Option<CylIn> {
    let f = &solid.faces[i];
    if f.kind != Some(cadrs_kernel::SurfaceKind::Cylinder) {
        return None;
    }
    let (o, d) = f.axis?;
    let (o, axis) = (p3(o), v3(d).normalize());
    let radius = f.radius?;
    let pts: Vec<P3> = f.loops.iter().flatten().map(|p| p3(*p)).collect();
    if pts.is_empty() {
        return None;
    }
    let radial = |p: P3| {
        let w = p - o;
        (w - axis * w.dot(&axis)).normalize()
    };
    let x = radial(pts[0]);
    let y = axis.cross(&x);
    let mut angles: Vec<f64> = pts.iter().map(|p| {
        let r = radial(*p);
        r.dot(&y).atan2(r.dot(&x)).rem_euclid(std::f64::consts::TAU)
    }).collect();
    angles.sort_by(f64::total_cmp);
    // The arc it covers: the circle less the largest gap between the points' angles.
    let n = angles.len();
    let (mut gap, mut after) = (angles[0] + std::f64::consts::TAU - angles[n - 1], angles[0]);
    for w in angles.windows(2) {
        if w[1] - w[0] > gap {
            gap = w[1] - w[0];
            after = w[1];
        }
    }
    let sweep = std::f64::consts::TAU - gap;
    let start = x * after.cos() + y * after.sin();
    let end = x * (after + sweep).cos() + y * (after + sweep).sin();
    let zs = pts.iter().map(|p| (p - o).dot(&axis));
    let (z0, z1) = zs.fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(z), b.max(z)));
    // Outward normal away from the axis: a round on an outside corner.
    let convex = (0..f.triangle_count.min(4)).find_map(|k| {
        let vi = *solid.indices.get(3 * (f.first_triangle + k))? as usize;
        let (p, nrm) = (p3(*solid.positions.get(vi)?), v3(*solid.normals.get(vi)?));
        Some(nrm.dot(&radial(p)) > 0.0)
    })?;
    // Its flat neighbours, by the edges they share, tangent at its start or its end.
    let mut neighbours = (None, None);
    for e in &solid.edges {
        let [a, b] = e.name.faces;
        let other = if a == f.name { b } else if b == f.name { a } else { continue };
        let Some(&(_, fi)) = planar.iter().find(|(si, _)| solid.faces[*si].name == other) else { continue };
        let nrm = faces[fi].normal();
        if nrm.dot(&axis).abs() > 1e-6 {
            continue;
        }
        if nrm.dot(&start).abs() > 1.0 - 1e-6 && neighbours.0.is_none() {
            neighbours.0 = Some(fi);
        } else if nrm.dot(&end).abs() > 1.0 - 1e-6 && neighbours.1.is_none() {
            neighbours.1 = Some(fi);
        }
    }
    Some(CylIn { key: key_of(&f.name), axis_origin: o, axis, radius, convex, start, sweep, z: (z0, z1), neighbours })
}

/// The faces, cylinders and shared straight edges of one solid's chosen faces.
#[derive(Default)]
struct Gathered {
    faces: Vec<FaceIn>,
    cyls: Vec<CylIn>,
    edges: Vec<EdgeIn>,
    /// Curved faces that are neither cylinders nor planes (left out).
    skipped: usize,
}

impl Gathered {
    /// Adds the faces (solid face indices) of `solid`.
    fn add(&mut self, solid: &Solid, chosen: &[usize]) {
        let base = self.faces.len();
        let mut planar: Vec<(usize, usize)> = Vec::new();
        for &i in chosen {
            if let Some(f) = face_in(solid, i) {
                planar.push((i, self.faces.len()));
                self.faces.push(f);
            }
        }
        for &i in chosen {
            if planar.iter().any(|(s, _)| *s == i) {
                continue;
            }
            match cyl_in(solid, i, &planar, &self.faces) {
                Some(c) => self.cyls.push(c),
                None => self.skipped += 1,
            }
        }
        let size = solid.positions.iter().map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()).fold(1.0, f64::max);
        for e in &solid.edges {
            let [a, b] = e.name.faces;
            let find = |n| planar.iter().find(|(s, _)| solid.faces[*s].name == n).map(|(_, f)| *f);
            let (Some(fa), Some(fb)) = (find(a), find(b)) else { continue };
            if fa == fb || fa < base || fb < base || !straight(&e.points, 1e-6 * size) {
                continue;
            }
            self.edges.push(EdgeIn { key: key_of(&e.name), a: p3(e.points[0]), b: p3(e.points[e.points.len() - 1]), faces: (fa, fb) });
        }
    }
}

/// The key a picked edge or cylinder has in [`Gathered`].
fn pick_key(state: &State, b: &EdgeOrFace) -> Option<u64> {
    match b {
        EdgeOrFace::Edge(r) => {
            let (part, _) = super::applied::edge_ids(state, r)?;
            let solid = &part.part.solid;
            let (i, _) = solid.resolve_edge(&r.edge, |e| Some(e.distance(r.seed))).ok()?;
            Some(key_of(&solid.edges[i].name))
        }
        EdgeOrFace::Face(f) => {
            let (part, _) = face_ids(state, f)?;
            let solid = &part.part.solid;
            let (i, _) = solid.resolve_face(&f.face, None, Some(f.seed)).ok()?;
            Some(key_of(&solid.faces[i].name))
        }
    }
}

/// The face of a reference on its part: (part state, face index).
fn face_index<'a>(state: &'a State, f: &crate::document::FaceRef) -> Option<(&'a PartState, usize)> {
    let (part, _) = face_ids(state, f)?;
    let (i, _) = part.part.solid.resolve_face(&f.face, None, Some(f.seed)).ok()?;
    Some((part, i))
}

/// Each sketch's picked curves as chains, on its plane.
type SketchChains = Vec<(cadrs_sketch::PlaneFrame, Vec<ChainIn>)>;

/// One folded flat-pattern part: its walls, body and names, and its pieces' volume.
type Folded = (Vec<WallId>, BodyId, BodyNames, f64);

/// A sketch's curves joined into chains (only the picked ones, unless the whole sketch is),
/// with how many picked curves are gone.
fn chains_of(before: &[Feature], x: &SheetMetalModelFeature) -> Result<(SketchChains, usize), String> {
    let mut sketches: Vec<FeatureId> = x.sketches.clone();
    for c in &x.curves {
        if !sketches.contains(&c.sketch) {
            sketches.push(c.sketch);
        }
    }
    let mut out = Vec::new();
    let mut missing = 0;
    for s in sketches {
        let Some(sk) = before.iter().find(|f| f.id == s).and_then(|f| f.sketch()) else {
            missing += 1;
            continue;
        };
        let Some(plane) = sk.plane else {
            missing += 1;
            continue;
        };
        let mut g = sk.geometry.clone();
        if !x.sketches.contains(&s) {
            let want: Vec<cadrs_sketch::CurveId> = x.curves.iter().filter(|c| c.sketch == s).map(|c| c.curve).collect();
            missing += want.iter().filter(|c| !g.curves.contains_key(**c)).count();
            g.curves.retain(|id, _| want.contains(&id));
        }
        let key = |c: cadrs_sketch::CurveId| {
            use slotmap::Key;
            let mut bytes = s.0.as_bytes().to_vec();
            bytes.extend_from_slice(&c.data().as_ffi().to_le_bytes());
            naming::stable_hash(&bytes)
        };
        let mut chains = Vec::new();
        for ch in sketch_chains(s, &g) {
            let mut segs = Vec::new();
            for (pc, id) in &ch.pieces {
                segs.push(match pc {
                    cadrs_sketch::region::Piece::Line(a, b) => Seg::Line { a: P2::new(a.x, a.y), b: P2::new(b.x, b.y), key: key(*id) },
                    cadrs_sketch::region::Piece::Arc(a) => Seg::Arc { center: P2::new(a.center.x, a.center.y), radius: a.radius, start: a.start_angle, sweep: a.sweep, key: key(*id) },
                    _ => return Err("Only lines and arcs can be extruded as sheet metal".into()),
                });
            }
            chains.push(ChainIn { segs, closed: ch.closed });
        }
        if !chains.is_empty() {
            out.push((plane.frame(), chains));
        }
    }
    Ok((out, missing))
}

/// How far an end goes along `dir` from the sketch plane through `origin` (mm).
fn end_distance(state: &State, origin: P3, dir: V3, end: EndType, depth: f64, up_to: &Option<UpTo>) -> Result<f64, String> {
    let along = |p: Vec3| (p3(p) - origin).dot(&dir);
    match end {
        EndType::Blind | EndType::ThroughAll => Ok(depth),
        EndType::UpToVertex => match up_to {
            Some(UpTo::Vertex(v)) => Ok(along(v.point)),
            _ => Err("Select a vertex to extrude up to".into()),
        },
        EndType::UpToFace => match up_to {
            Some(UpTo::Face(f)) => {
                let (part, i) = face_index(state, f).ok_or("The face to extrude up to no longer exists")?;
                let face = &part.part.solid.faces[i];
                let c = face.center.or_else(|| face.loops.first()?.first().copied()).ok_or("The face has no boundary")?;
                Ok(along(c))
            }
            _ => Err("Select a face to extrude up to".into()),
        },
        EndType::UpToPart => match up_to {
            Some(UpTo::Part(p)) => {
                let part = state.part(*p).ok_or("The part to extrude up to no longer exists")?;
                part.part.solid.positions.iter().map(|p| along(*p)).reduce(f64::max).ok_or_else(|| "The part is empty".into())
            }
            _ => Err("Select a part to extrude up to".into()),
        },
        EndType::UpToNext => state
            .parts
            .iter()
            .filter(|p| p.part.kind == PartKind::Solid)
            .filter_map(|p| p.part.solid.positions.iter().map(|q| along(*q)).filter(|d| *d > 1e-6).reduce(f64::min))
            .reduce(f64::min)
            .ok_or_else(|| "There is no part in the way to extrude up to".into()),
    }
}

/// A polygon's loops as a kernel region on `plane` (2D points mapped by `to_plane`).
fn region_of(poly: &Polygon, source: u64, to_plane: &dyn Fn(P2) -> nalgebra::Point2<f64>) -> cadrs_kernel::Region {
    let lp = |l: &[P2], base: u64| Loop {
        curves: (0..l.len())
            .map(|k| Curve2::Line { a: to_plane(l[k]), b: to_plane(l[(k + 1) % l.len()]), source: Some(base + k as u64) })
            .collect(),
    };
    cadrs_kernel::Region {
        outer: lp(&poly.outer, 0),
        holes: poly.holes.iter().enumerate().map(|(h, l)| lp(l, (h as u64 + 1) << 20)).collect(),
        source: Some(source),
    }
}

/// An annular sector about the plane's origin (radii `ri..ro`, angles `a0..a0 + sweep`) as a
/// kernel region; a ring for a whole turn.
fn sector(ri: f64, ro: f64, a0: f64, sweep: f64, source: u64) -> cadrs_kernel::Region {
    let c = nalgebra::Point2::origin();
    let at = |r: f64, a: f64| nalgebra::Point2::new(r * a.cos(), r * a.sin());
    if sweep >= std::f64::consts::TAU - 1e-9 {
        return cadrs_kernel::Region {
            outer: Loop { curves: vec![Curve2::Circle { center: c, radius: ro, source: Some(0) }] },
            holes: vec![Loop { curves: vec![Curve2::Circle { center: c, radius: ri, source: Some(1) }] }],
            source: Some(source),
        };
    }
    let a1 = a0 + sweep;
    cadrs_kernel::Region {
        outer: Loop {
            curves: vec![
                Curve2::Arc { center: c, radius: ro, start_angle: a0, sweep, source: Some(0) },
                Curve2::Line { a: at(ro, a1), b: at(ri, a1), source: Some(1) },
                Curve2::Arc { center: c, radius: ri, start_angle: a1, sweep: -sweep, source: Some(2) },
                Curve2::Line { a: at(ri, a0), b: at(ro, a0), source: Some(3) },
            ],
        },
        holes: vec![],
        source: Some(source),
    }
}

fn kplane(origin: P3, x: V3, normal: V3) -> Plane {
    Plane { origin, x_dir: nalgebra::Unit::new_normalize(x), normal: nalgebra::Unit::new_normalize(normal) }
}

/// Where a piece's material is removed by relief cuts (in the piece's own coordinates).
fn removed_of(part: &FlatPart, s: PieceSource) -> Vec<Polygon> {
    part.cuts.iter().flat_map(|c| c.removed.iter().filter(|(p, _)| *p == s).flat_map(|(_, v)| v.iter().cloned())).collect()
}

/// Why a sheet metal model fails flat (X6), the collision first.
fn flat_error(flat: &FlatPattern) -> Option<String> {
    flat.errors
        .iter()
        .find(|e| matches!(e, FlatError::Collision { .. }))
        .or(flat.errors.first())
        .map(FlatError::message)
}

impl Rebuilder {
    /// The folded bodies of a model's flat-pattern parts: each with its walls, body, names and
    /// the sum of its pieces' volumes.
    fn fold(&mut self, op: cadrs_kernel::OpId, model: &Model, flat: &FlatPattern) -> Result<Vec<Folded>, String> {
        let t = model.params.thickness;
        let mut out = Vec::new();
        let release = |k: &mut cadrs_kernel::backend::occt::OcctKernel, made: &[(BodyId, BodyNames, f64)]| {
            for (b, _, _) in made {
                k.release(*b);
            }
        };
        for part in &flat.parts {
            let mut made: Vec<(BodyId, BodyNames, f64)> = Vec::new();
            for piece in &part.pieces {
                let removed = removed_of(part, piece.source);
                let r = match piece.source {
                    PieceSource::Wall(w) => match model.wall(w) {
                        Some(wall) => self.wall_bodies(op, wall, &removed, t),
                        None => continue,
                    },
                    PieceSource::Bend(j) => self.bend_body(op, model, j, &removed),
                };
                match r {
                    Ok(v) => made.extend(v),
                    Err(e) => {
                        release(&mut self.kernel, &made);
                        return Err(e);
                    }
                }
            }
            if made.is_empty() {
                continue;
            }
            let sum: f64 = made.iter().map(|(_, _, v)| *v).sum();
            let fused = if made.len() == 1 {
                let (b, n, _) = made.pop().expect("one");
                (b, n)
            } else {
                let first = made[0].0;
                let rest: Vec<BodyId> = made[1..].iter().map(|(b, _, _)| *b).collect();
                let r = self.kernel.boolean(BoolOp::Union, first, &rest).and_then(|res| {
                    let body = res.bodies[0];
                    let inputs: Vec<(BodyId, &BodyNames)> = made.iter().map(|(b, n, _)| (*b, n)).collect();
                    Ok((body, naming::name_body(&self.kernel, body, op, &res.history, &inputs)?))
                });
                release(&mut self.kernel, &made);
                r.map_err(|e| format!("Couldn't join the sheet metal walls: {e}"))?
            };
            out.push((part.walls.clone(), fused.0, fused.1, sum));
        }
        Ok(out)
    }

    /// A wall's solid: its outline (less the relief cuts) thickened on its material side.
    fn wall_bodies(&mut self, op: cadrs_kernel::OpId, wall: &Wall, removed: &[Polygon], t: f64) -> Result<Vec<(BodyId, BodyNames, f64)>, String> {
        let source = 0x5741_4c4c_0000_0000 | wall.id.0 as u64;
        let profile = match wall.surface {
            Surface::Planar { origin, u, v } => {
                let x = u.normalize();
                let n = u.cross(&v).normalize();
                let y = n.cross(&x);
                let to_plane = |q: P2| {
                    let p = wall.surface.point(q);
                    nalgebra::Point2::new((p - origin).dot(&x), (p - origin).dot(&y))
                };
                let polys = if removed.is_empty() {
                    vec![wall.outline.clone()]
                } else {
                    poly::difference(std::slice::from_ref(&wall.outline), removed)
                };
                let regions: Vec<cadrs_kernel::Region> = polys
                    .iter()
                    .filter(|p| p.area() > 1e-9)
                    .enumerate()
                    .map(|(k, p)| region_of(p, source ^ ((k as u64) << 40), &to_plane))
                    .collect();
                if regions.is_empty() {
                    return Ok(Vec::new());
                }
                cadrs_kernel::Profile::new(kplane(origin, x, n), regions)
            }
            Surface::Rolled { axis_origin, axis, start, radius, material_outside } => {
                let (ri, ro) = if material_outside { (radius, radius + t) } else { (radius - t, radius) };
                let Some((lo, hi)) = wall.outline.bounds() else { return Ok(Vec::new()) };
                let a = axis.normalize();
                let x = (start - a * start.dot(&a)).normalize();
                let origin = axis_origin + a * lo.y;
                let profile = cadrs_kernel::Profile::new(kplane(origin, x, a), vec![sector(ri, ro, lo.x / radius, (hi.x - lo.x) / radius, source)]);
                let r = self.kernel.extrude(&profile, Extent::Blind(hi.y - lo.y)).map_err(|e| format!("Sheet metal wall failed: {e}"))?;
                return self.named_pieces(op, r);
            }
        };
        let r = self.kernel.extrude(&profile, Extent::Blind(t)).map_err(|e| format!("Sheet metal wall failed: {e}"))?;
        self.named_pieces(op, r)
    }

    /// A bend's solid: the shell between its inner and outer radii over its sweep, less the
    /// relief cuts (each as the wedge its extent covers).
    fn bend_body(&mut self, op: cadrs_kernel::OpId, model: &Model, j: cadrs_sheetmetal::JointId, removed: &[Polygon]) -> Result<Vec<(BodyId, BodyNames, f64)>, String> {
        let Some(g) = model.bend_geometry(j) else { return Ok(Vec::new()) };
        let Some(bend) = model.joint(j).and_then(|x| x.bend()) else { return Ok(Vec::new()) };
        let source = 0x4245_4e44_0000_0000 | j.0 as u64;
        let span = g.ends.1 - g.ends.0;
        let len = span.dot(&g.axis);
        let base = if len >= 0.0 { g.ends.0 } else { g.ends.1 };
        let plane = kplane(base, g.start, g.axis);
        let profile = cadrs_kernel::Profile::new(plane, vec![sector(g.inner_radius, g.outer_radius, 0.0, g.sweep, source)]);
        let r = self.kernel.extrude(&profile, Extent::Blind(len.abs())).map_err(|e| format!("Sheet metal bend failed: {e}"))?;
        let mut made = self.named_pieces(op, r)?;
        if removed.is_empty() || made.len() != 1 {
            return Ok(made);
        }
        let allowance = bend.allowance(&model.params).unwrap_or(0.0);
        if allowance <= 1e-9 {
            return Ok(made);
        }
        // Each cut's extent along the bend (s) and across it (u, as a share of the sweep).
        let total = len.abs();
        let eps = 1e-3 * model.params.thickness.max(0.01);
        let mut tools: Vec<BodyId> = Vec::new();
        for cut in removed {
            let Some((lo, hi)) = cut.bounds() else { continue };
            let (s0, s1) = (lo.x.max(0.0), hi.x.min(total));
            let (u0, u1) = (lo.y.max(0.0), hi.y.min(allowance));
            if s1 - s0 < 1e-9 || u1 - u0 < 1e-9 {
                continue;
            }
            // Positions along the axis from the profile's base.
            let at = |s: f64| (g.ends.0 + span * (s / total) - base).dot(&g.axis);
            let (mut z0, mut z1) = (at(s0).min(at(s1)), at(s0).max(at(s1)));
            if z0 < 1e-9 {
                z0 -= eps;
            }
            if z1 > total - 1e-9 {
                z1 += eps;
            }
            let (mut a0, mut a1) = (u0 / allowance * g.sweep, u1 / allowance * g.sweep);
            if u0 < 1e-9 {
                a0 -= 1e-4;
            }
            if u1 > allowance - 1e-9 {
                a1 += 1e-4;
            }
            let wedge = cadrs_kernel::Profile::new(
                kplane(base + g.axis * z0, g.start, g.axis),
                vec![sector((g.inner_radius - eps).max(1e-6), g.outer_radius + eps, a0, a1 - a0, 0)],
            );
            match self.kernel.extrude(&wedge, Extent::Blind(z1 - z0)) {
                Ok(r) => tools.push(r.bodies[0]),
                Err(_) => continue,
            }
        }
        if tools.is_empty() {
            return Ok(made);
        }
        let (body, names, _) = made.pop().expect("one");
        let r = self.kernel.boolean(BoolOp::Subtract, body, &tools);
        for b in &tools {
            self.kernel.release(*b);
        }
        let cut = r.and_then(|res| {
            let nb = res.bodies[0];
            let n = naming::name_body(&self.kernel, nb, op, &res.history, &[(body, &names)])?;
            let v = self.kernel.mass_properties(nb)?.volume;
            Ok((nb, n, v))
        });
        self.kernel.release(body);
        match cut {
            Ok(x) => Ok(vec![x]),
            // A relief cut that takes the whole bend end away leaves nothing of it.
            Err(cadrs_kernel::KernelError::OperationFailed(m)) if m.contains("empty") => Ok(Vec::new()),
            Err(e) => Err(format!("Sheet metal bend relief failed: {e}")),
        }
    }

    /// Names a new body and measures it.
    fn named_pieces(&mut self, op: cadrs_kernel::OpId, r: cadrs_kernel::OpResult) -> Result<Vec<(BodyId, BodyNames, f64)>, String> {
        let body = r.bodies[0];
        let names = naming::name_body(&self.kernel, body, op, &r.history, &[]).map_err(|e| e.to_string());
        let volume = self.kernel.mass_properties(body).map(|m| m.volume).map_err(|e| e.to_string());
        match (names, volume) {
            (Ok(n), Ok(v)) => Ok(vec![(body, n, v)]),
            (Err(e), _) | (_, Err(e)) => {
                self.kernel.release(body);
                Err(e)
            }
        }
    }

    pub(in crate::rebuild) fn sheet_metal_model(&mut self, before: &[Feature], id: FeatureId, x: &SheetMetalModelFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(e) = x.params.validate().first() {
            return Err(e.message());
        }
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let p = x.params;
        let mut consumed: Vec<PartId> = Vec::new();
        let mut warning: Option<String> = None;
        let built = match x.operation {
            SheetMetalOp::Convert | SheetMetalOp::Thicken => {
                let mut g = Gathered::default();
                let mut missing = 0;
                if x.operation == SheetMetalOp::Convert {
                    for pid in &x.parts {
                        let Some(part) = state.part(*pid) else {
                            missing += 1;
                            continue;
                        };
                        let solid = &part.part.solid;
                        let excluded: Vec<usize> = x
                            .exclude
                            .iter()
                            .filter_map(|f| face_index(state, f).filter(|(p, _)| p.part.id == *pid).map(|(_, i)| i))
                            .collect();
                        let chosen: Vec<usize> = (0..solid.faces.len()).filter(|i| !excluded.contains(i)).collect();
                        g.add(solid, &chosen);
                        consumed.push(*pid);
                    }
                } else {
                    // Faces to thicken, by part (tangent propagation adds the flat faces that
                    // carry on in the same plane).
                    let mut by_part: Vec<(PartId, Vec<usize>)> = Vec::new();
                    for f in &x.faces {
                        let Some((part, i)) = face_index(state, f) else {
                            missing += 1;
                            continue;
                        };
                        match by_part.iter_mut().find(|(p, _)| *p == part.part.id) {
                            Some((_, v)) if !v.contains(&i) => v.push(i),
                            Some(_) => {}
                            None => by_part.push((part.part.id, vec![i])),
                        }
                    }
                    for (pid, chosen) in &mut by_part {
                        let Some(part) = state.part(*pid) else { continue };
                        let solid = &part.part.solid;
                        if x.tangent_propagation {
                            let mut k = 0;
                            while k < chosen.len() {
                                let f = &solid.faces[chosen[k]];
                                for e in &solid.edges {
                                    let [a, b] = e.name.faces;
                                    let other = if a == f.name { b } else if b == f.name { a } else { continue };
                                    let Some(oi) = solid.faces.iter().position(|g| g.name == other) else { continue };
                                    if chosen.contains(&oi) {
                                        continue;
                                    }
                                    let same_plane = match (f.plane, solid.faces[oi].plane) {
                                        (Some(p1), Some(p2)) => v3(p1.normal()).normalize().dot(&v3(p2.normal()).normalize()) > 1.0 - 1e-9,
                                        _ => f.kind == Some(cadrs_kernel::SurfaceKind::Cylinder) || solid.faces[oi].kind == Some(cadrs_kernel::SurfaceKind::Cylinder),
                                    };
                                    if same_plane {
                                        chosen.push(oi);
                                    }
                                }
                                k += 1;
                            }
                        }
                        g.add(solid, chosen);
                    }
                    // Sketch regions: each sketch's picked regions, joined where they touch.
                    let (groups, lost) = sweep_groups(before, &x.regions, &x.region_sketches, crate::document::BodyType::Solid);
                    missing += lost;
                    for grp in &groups {
                        let f = &grp.frame;
                        let polys: Vec<Polygon> = grp
                            .regions
                            .iter()
                            .map(|(_, r)| {
                                let ring = |l: &[cadrs_sketch::Vec2]| l.iter().map(|q| P2::new(q.x, q.y)).collect::<Vec<_>>();
                                Polygon::with_holes(ring(&r.outer), r.holes.iter().map(|h| ring(h)).collect())
                            })
                            .collect();
                        let first_key = grp.regions.iter().map(|(k, _)| *k).min().unwrap_or(0);
                        let mut joined = poly::union(&polys);
                        joined.sort_by(|a, b| {
                            let c = |p: &Polygon| p.bounds().map_or((0.0, 0.0), |(lo, _)| (lo.x, lo.y));
                            c(a).partial_cmp(&c(b)).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        let (u, v) = (v3(f.u).normalize(), v3(f.v).normalize());
                        for (k, outline) in joined.into_iter().enumerate() {
                            g.faces.push(FaceIn { key: first_key.wrapping_add(k as u64), origin: p3(f.origin), u, v, outline });
                        }
                    }
                }
                if missing > 0 {
                    warning = Some("A selected part, face or region no longer exists".into());
                }
                if g.skipped > 0 {
                    warning = Some(format!("{} curved face{} can't be made sheet metal and {} left out", g.skipped, if g.skipped == 1 { "" } else { "s" }, if g.skipped == 1 { "was" } else { "were" }));
                }
                if g.faces.is_empty() && g.cyls.is_empty() {
                    return Err("The selected parts, faces or regions no longer exist".into());
                }
                let bends: Vec<u64> = x.bends.iter().filter_map(|b| pick_key(state, b)).collect();
                if bends.len() < x.bends.len() {
                    warning = Some("A selected edge to bend no longer exists".into());
                }
                let o = FaceOpts { material_inside: x.flip_thickness, clearance: x.clearance, include_bends: x.include_bends, bends };
                construct::from_faces(p, &g.faces, &g.cyls, &g.edges, &o).map_err(|e| e.message())?
            }
            SheetMetalOp::Extrude => {
                let (groups, missing) = chains_of(before, x)?;
                if groups.is_empty() {
                    return Err("The selected sketch curves no longer exist".into());
                }
                if missing > 0 {
                    warning = Some("A selected sketch curve no longer exists".into());
                }
                // One sketch at a time (each its own plane), into one model.
                let mut all: Option<construct::Built> = None;
                for (frame, chains) in &groups {
                    let origin = p3(frame.origin);
                    let (fx, fy) = (v3(frame.u).normalize(), v3(frame.v).normalize());
                    let normal = fx.cross(&fy);
                    let dir = if x.flip_extrude { -normal } else { normal };
                    let d1 = end_distance(state, origin, dir, x.end, x.depth, &x.up_to)?;
                    let (z0, z1) = if x.symmetric && x.end == EndType::Blind {
                        (-x.depth / 2.0, x.depth / 2.0)
                    } else if let Some(EndCondition { end, depth, up_to, .. }) = &x.second {
                        let d2 = end_distance(state, origin, -dir, *end, *depth, up_to)?;
                        (-d2, d1)
                    } else {
                        (0.0, d1)
                    };
                    if z1 - z0 <= 1e-6 {
                        return Err("The extrude has no depth".into());
                    }
                    let arcs: Vec<u64> = x
                        .arcs_as_bends
                        .iter()
                        .map(|c| {
                            use slotmap::Key;
                            let mut bytes = c.sketch.0.as_bytes().to_vec();
                            bytes.extend_from_slice(&c.curve.data().as_ffi().to_le_bytes());
                            naming::stable_hash(&bytes)
                        })
                        .collect();
                    let o = ChainOpts { origin, x: fx, y: fy, z0, z1, dir, flip_side: x.flip_thickness, arcs_as_bends: arcs };
                    let b = construct::from_chains(p, chains, &o).map_err(|e| e.message())?;
                    if let Some(w) = b.warnings.first() {
                        warning = Some(w.clone());
                    }
                    all = Some(match all {
                        None => b,
                        Some(mut a) => {
                            a.model.walls.extend(b.model.walls);
                            a.model.joints.extend(b.model.joints);
                            a.walls.extend(b.walls);
                            a.joints.extend(b.joints);
                            a
                        }
                    });
                }
                all.ok_or("Nothing to extrude")?
            }
        };
        if !built.loop_picks.is_empty() && warning.is_none() {
            // Not a problem: the pick order decided (SM2.2). Nothing to report.
        }
        let model = built.model;
        // The definition must hold together in 3D.
        if let Some(e) = model.validate().first() {
            return Err(format!("Sheet metal model is inconsistent: {}", e.message()));
        }
        let flat = flatten(&model);
        let mut ctx = SheetMetalContext {
            feature: id,
            model: model.clone(),
            flat: flat.clone(),
            parts: Vec::new(),
            active: true,
            wall_keys: built.walls.clone(),
            joint_keys: built.joints.clone(),
        };
        let fail_keeping_context = |ctx: SheetMetalContext, why: String| {
            let mut next = (**state).clone();
            let mut all = (*next.sheet_metal).clone();
            all.retain(|c| c.feature != id);
            all.push(ctx);
            next.sheet_metal = Arc::new(all);
            Ok(Output {
                state: Arc::new(next),
                error: Some(why),
                warning: None,
                contacts: None,
                owned: Vec::new(),
                stage: None,
                axis: None,
                arrows: Vec::new(),
                dots: None,
                uses: Vec::new(),
            })
        };
        if let Some(why) = flat_error(&flat) {
            return fail_keeping_context(ctx, why);
        }
        let op = id.0;
        let folded = self.fold(op, &model, &flat)?;
        // Walls running into each other in 3D: the fused body is smaller than its pieces.
        for (_, body, _, sum) in &folded {
            let v = self.kernel.mass_properties(*body).map(|m| m.volume).unwrap_or(*sum);
            if v < sum - (1e-6 * sum + 1e-3) {
                for (_, b, _, _) in &folded {
                    self.kernel.release(*b);
                }
                return fail_keeping_context(ctx, "Sheet metal walls intersect".into());
            }
        }
        let geoms = state.geoms.clone();
        let mut next = State { geoms: geoms.clone(), ..(**state).clone() };
        if x.operation == SheetMetalOp::Convert && !x.keep_input {
            next.parts.retain(|p| !consumed.contains(&p.part.id));
        }
        let mut placed: Vec<Placed> = Vec::new();
        let mut walls_of: Vec<(PartId, Vec<WallId>)> = Vec::new();
        for (k, (walls, body, names, _)) in folded.into_iter().enumerate() {
            let pieces = self.split(body, op, &[(body, &names)]);
            self.kernel.release(body);
            let pieces = pieces?;
            for (n, pc) in pieces.into_iter().enumerate() {
                let want = PartId::new(id, k as u32);
                let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                let pid = if n == 0 && next.part(want).is_none() && !taken.contains(&want) { want } else { Self::new_id(id, &next, &taken) };
                if n == 0 {
                    walls_of.push((pid, walls.clone()));
                }
                placed.push((pid, pc));
            }
        }
        let mut o = self.finish(id, placed, next, op, geoms, PartKind::Solid)?;
        ctx.parts = walls_of;
        let mut next = (*o.state).clone();
        let mut all = (*next.sheet_metal).clone();
        all.retain(|c| c.feature != id);
        all.push(ctx);
        next.sheet_metal = Arc::new(all);
        o.state = Arc::new(next);
        o.warning = warning;
        Ok(o)
    }
}
