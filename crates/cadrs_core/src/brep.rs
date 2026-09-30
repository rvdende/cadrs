//! Extrudes through the solid-modelling kernel (P3.1): exact profiles in, a named display mesh
//! out.
//!
//! - [`profile`] turns the sketch regions an extrude selected into a [`cadrs_kernel::Profile`]
//!   made of the regions' exact boundary pieces (lines, arcs, ellipse arcs; whole circles and
//!   ellipses), never the polylines drawn on screen.
//! - [`extrude`] asks the kernel for the body and converts its tessellation into a [`Solid`].
//!
//! # Names (P3.2)
//! Faces, edges and vertices get persistent names ([`cadrs_kernel::naming`]) from the kernel's
//! history: the side swept by sketch curve `c` of region `r` of the extrude, the start or end
//! cap of region `r`. Sketches on faces, Use links and imprints store these names.
//!
//! A face the history says nothing about (a backend without history) is named from where it
//! lies instead, the P3.1 way:
//! - a planar face parallel to the sketch plane, on it or at the extrude's depth, is the start
//!   or end cap of the region that contains it;
//! - a face along the extrude direction is the side of the boundary piece its points lie on.
//!
//! Faces with the same name (a sketch curve split into pieces) become one face. Edges inside a
//! face (a curve's split point, a cylinder's seam) and between two caps of touching regions
//! are left out, as the prism mesh of [`crate::solid`] did. Planar faces get the same
//! [`PlaneFrame`]s the prism gave them, so sketches on faces keep their coordinates.

use std::collections::HashMap;

use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{self as kernel, BodyId, Kernel, Tessellation};
use cadrs_sketch::region::{Piece, Region};
use cadrs_sketch::{CurveId, EdgeName, FaceName, FaceOrigin, OpId, PlaneFrame, Vec2, Vec3};
use nalgebra::{Point2, Point3, Unit, Vector3};
use slotmap::Key;

use crate::solid::{EdgeCircle, Ruling, Solid, SolidEdge, SolidFace, SolidVertex, SurfaceGrid};

/// The regions of one sketch an extrude uses (with their keys, [`crate::RegionRef::key`]), and
/// the sketch's plane.
#[derive(Debug, Clone)]
pub struct ProfileGroup {
    pub frame: PlaneFrame,
    pub regions: Vec<(u64, Region)>,
    /// Chains of sketch curves (surface and thin extrudes of a whole sketch, PS4.10).
    pub chains: Vec<ChainGeom>,
}

impl ProfileGroup {
    /// A group of regions only.
    pub fn new(frame: PlaneFrame, regions: Vec<(u64, Region)>) -> Self {
        Self {
            frame,
            regions,
            chains: Vec::new(),
        }
    }
}

/// Sketch curves joined end to end (closed when the last ends where the first starts), with a
/// stable key.
#[derive(Debug, Clone)]
pub struct ChainGeom {
    pub source: u64,
    pub pieces: Vec<(Piece, CurveId)>,
    pub closed: bool,
}

/// The angle step of curved faces on screen: 5° (72 steps around a circle). Finer meshes cost
/// rebuild time (BRepMesh dominates an extrude's rebuild) without a visible difference once
/// the faces are smooth-shaded.
pub const ANGLE_STEP: f64 = std::f64::consts::PI / 36.0;

/// How finely parts are tessellated for display.
pub fn tessellation() -> Tessellation {
    Tessellation {
        deflection: 0.05,
        angle: ANGLE_STEP,
    }
}

// Small vector helpers (world coordinates are `[f64; 3]`).
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(a: Vec3) -> Vec3 {
    let l = dot(a, a).sqrt();
    if l < 1e-15 { a } else { scale(a, 1.0 / l) }
}
fn dist(a: Vec3, b: Vec3) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

/// The kernel's id for a sketch curve (kept on profile curves as their `source`).
pub fn curve_source(c: CurveId) -> u64 {
    c.data().as_ffi()
}

/// The pieces of a region's outer boundary and of each hole, with the curve each lies on, in
/// the direction the boundary runs (outer counter-clockwise, holes clockwise).
pub fn rings(region: &Region) -> Vec<Vec<(Piece, CurveId)>> {
    let mut out = Vec::new();
    if region.outer_pieces.is_empty() {
        // No exact pieces: the polygon's segments.
        let n = region.outer.len();
        out.push(
            (0..n)
                .map(|i| {
                    let c = region.outer_curves.get(i).copied().unwrap_or_default();
                    (Piece::Line(region.outer[i], region.outer[(i + 1) % n]), c)
                })
                .collect(),
        );
        for (h, curves) in region.holes.iter().zip(&region.hole_curves) {
            let n = h.len();
            out.push(
                (0..n)
                    .map(|i| {
                        let c = curves.get(i).copied().unwrap_or_default();
                        (Piece::Line(h[i], h[(i + 1) % n]), c)
                    })
                    .collect(),
            );
        }
        return out;
    }
    out.push(
        region
            .outer_pieces
            .iter()
            .copied()
            .zip(region.curves.iter().copied())
            .collect(),
    );
    for (pieces, curves) in region.hole_pieces.iter().zip(&region.hole_piece_curves) {
        out.push(pieces.iter().copied().zip(curves.iter().copied()).collect());
    }
    out
}

fn to_curve(piece: &Piece, curve: CurveId) -> kernel::Curve2 {
    let p = |v: Vec2| Point2::new(v.x, v.y);
    let source = Some(curve_source(curve));
    match *piece {
        Piece::Line(a, b) => kernel::Curve2::Line {
            a: p(a),
            b: p(b),
            source,
        },
        Piece::Arc(g) => kernel::Curve2::Arc {
            center: p(g.center),
            radius: g.radius,
            start_angle: g.start_angle,
            sweep: g.sweep,
            source,
        },
        Piece::Ellipse { g, t0, sweep } if g.offset != 0.0 => kernel::Curve2::OffsetEllipseArc {
            center: p(g.center),
            major_radius: g.major(),
            minor_radius: g.minor,
            rotation: g.u().angle(),
            start: t0,
            sweep,
            offset: g.offset,
            source,
        },
        Piece::Ellipse { g, t0, sweep } => kernel::Curve2::EllipseArc {
            center: p(g.center),
            major_radius: g.major(),
            minor_radius: g.minor,
            rotation: g.u().angle(),
            start: t0,
            sweep,
            source,
        },
        Piece::Bezier(g) => kernel::Curve2::Bezier { poles: g.p.map(p), source },
    }
}

/// The kernel profile of one sketch's regions, from their exact boundary pieces.
pub fn profile(group: &ProfileGroup) -> kernel::Profile {
    let f = &group.frame;
    let plane = kernel::Plane {
        origin: Point3::from(f.origin),
        x_dir: Unit::new_normalize(Vector3::from(f.u)),
        normal: Unit::new_normalize(Vector3::from(cross(f.u, f.v))),
    };
    let lp = |ring: &[(Piece, CurveId)]| kernel::Loop {
        curves: ring.iter().map(|(p, c)| to_curve(p, *c)).collect(),
    };
    let regions = group
        .regions
        .iter()
        .map(|(key, r)| {
            let rs = rings(r);
            kernel::Region {
                outer: lp(&rs[0]),
                holes: rs[1..].iter().map(|h| lp(h)).collect(),
                source: Some(*key),
            }
        })
        .collect::<Vec<_>>();
    let mut out = kernel::Profile::new(plane, regions);
    for c in &group.chains {
        let curves: Vec<kernel::Curve2> = c.pieces.iter().map(|(p, id)| to_curve(p, *id)).collect();
        if c.closed {
            out.regions.push(kernel::Region {
                outer: kernel::Loop { curves },
                holes: vec![],
                source: Some(c.source),
            });
        } else {
            out.chains.push(kernel::Chain {
                curves,
                source: Some(c.source),
            });
        }
    }
    out
}

/// A body the kernel made for an extrude, its names and its display mesh.
pub struct KernelPart {
    pub body: BodyId,
    pub names: BodyNames,
    pub solid: Solid,
    pub mass: kernel::MassProperties,
}

/// Extrudes the groups' regions `depth` mm along their planes' normals (`flip`: the other way)
/// into one body for the feature `op`, names its faces, edges and vertices, and tessellates it.
/// (A blind solid extrude; [`crate::rebuild`] builds every other kind through
/// [`kernel::Kernel::extrude_with`] and [`solid_of`].)
pub fn extrude(
    k: &mut dyn Kernel,
    op: OpId,
    groups: &[ProfileGroup],
    depth: f64,
    flip: bool,
) -> Result<KernelPart, String> {
    let extent = kernel::Extent::Blind(if flip { -depth } else { depth });
    let mut bodies: Vec<(BodyId, BodyNames)> = Vec::new();
    let release_all = |k: &mut dyn Kernel, bodies: &[(BodyId, BodyNames)]| {
        for (b, _) in bodies {
            k.release(*b);
        }
    };
    for g in groups {
        let named = k.extrude(&profile(g), extent).and_then(|r| {
            let body = r.bodies[0];
            let names = naming::name_body(k, body, op, &r.history, &[])?;
            Ok((body, names))
        });
        match named {
            Ok(b) => bodies.push(b),
            Err(e) => {
                release_all(k, &bodies);
                return Err(e.to_string());
            }
        }
    }
    let (body, names) = match bodies.as_slice() {
        [] => return Err("nothing to extrude".into()),
        [(b, n)] => (*b, n.clone()),
        [(first, _), rest @ ..] => {
            let tools: Vec<BodyId> = rest.iter().map(|(b, _)| *b).collect();
            let merged = k.boolean(kernel::BoolOp::Union, *first, &tools).and_then(|r| {
                let body = r.bodies[0];
                let inputs: Vec<(BodyId, &BodyNames)> = bodies.iter().map(|(b, n)| (*b, n)).collect();
                Ok((body, naming::name_body(k, body, op, &r.history, &inputs)?))
            });
            release_all(k, &bodies);
            merged.map_err(|e| e.to_string())?
        }
    };
    let n = groups.first().map(|g| normalize(g.frame.normal())).unwrap_or([0.0, 0.0, 1.0]);
    let geom = OpGeom::new(groups, if flip { scale_neg(n) } else { n }, depth);
    let geoms: Geoms = std::iter::once((op, std::sync::Arc::new(geom))).collect();
    let built = solid_of(k, body, &names, &geoms, Some(op))
        .and_then(|solid| Ok((solid, k.mass_properties(body).map_err(|e| e.to_string())?)));
    match built {
        Ok((solid, mass)) => Ok(KernelPart {
            body,
            names,
            solid,
            mass,
        }),
        Err(e) => {
            k.release(body);
            Err(e)
        }
    }
}

/// One extruded sketch, as the naming and the display see it.
#[derive(Debug, Clone)]
pub struct GroupGeom {
    frame: PlaneFrame,
    /// The plane's unit normal.
    n: Vec3,
    regions: Vec<(u64, Region)>,
    /// Every boundary ring: (region key, pieces with their curves).
    rings: Vec<(u64, Vec<(Piece, CurveId)>)>,
}

/// What an extrude swept, for naming faces the kernel's history leaves unnamed, for the planar
/// frames of its caps and sides, and for the silhouettes of its curved sides.
#[derive(Debug, Clone)]
pub struct OpGeom {
    groups: Vec<GroupGeom>,
    /// The unit direction of the sweep.
    dir: Vec3,
    /// The nominal depth (for naming unnamed faces).
    depth: f64,
    /// A revolve's axis and angles (P3.4); `None` for an extrude.
    revolve: Option<Revolution>,
}

impl OpGeom {
    /// The frame of the (first) sketch the operation swept: where an extrude starts, the plane
    /// of a revolve's profile.
    pub fn sketch_frame(&self) -> Option<PlaneFrame> {
        self.groups.first().map(|g| g.frame)
    }
}

/// Where a revolve turns: about the unit `axis` through `origin`, from `start` radians (from the
/// sketch plane, counter-clockwise about the axis) through `sweep`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Revolution {
    origin: Vec3,
    axis: Vec3,
    start: f64,
    sweep: f64,
}

impl Revolution {
    /// `v` turned by `angle` about the axis (a direction; add the origin for a point).
    fn turn(&self, v: Vec3, angle: f64) -> Vec3 {
        let k = self.axis;
        let (s, c) = angle.sin_cos();
        add(add(scale(v, c), scale(cross(k, v), s)), scale(k, dot(k, v) * (1.0 - c)))
    }

    fn turn_point(&self, p: Vec3, angle: f64) -> Vec3 {
        add(self.origin, self.turn(sub(p, self.origin), angle))
    }
}

impl OpGeom {
    /// A revolve's swept geometry (P3.4): the groups turned about `axis` through `origin`, from
    /// `start` radians through `sweep`.
    pub fn revolve(groups: &[ProfileGroup], origin: Vec3, axis: Vec3, start: f64, sweep: f64) -> Self {
        let axis = normalize(axis);
        Self {
            revolve: Some(Revolution {
                origin,
                axis,
                start,
                sweep,
            }),
            ..Self::new(groups, axis, 0.0)
        }
    }

    pub fn new(groups: &[ProfileGroup], dir: Vec3, depth: f64) -> Self {
        Self {
            groups: groups
                .iter()
                .map(|g| GroupGeom {
                    frame: g.frame,
                    n: normalize(g.frame.normal()),
                    regions: g.regions.clone(),
                    rings: g
                        .regions
                        .iter()
                        .flat_map(|(i, r)| rings(r).into_iter().map(move |ring| (*i, ring)))
                        .chain(g.chains.iter().map(|c| (c.source, c.pieces.clone())))
                        .collect(),
                })
                .collect(),
            dir: normalize(dir),
            depth,
            revolve: None,
        }
    }

    /// The same sweep moved by the rigid motion `x ↦ r·x + t` (`r` a rotation matrix, row by
    /// row): a Transform moved the parts with its faces (the profiles stay in their frames).
    pub fn moved(&self, r: [[f64; 3]; 3], t: Vec3) -> Self {
        let dir = |v: Vec3| [dot(r[0], v), dot(r[1], v), dot(r[2], v)];
        let point = |p: Vec3| add(dir(p), t);
        Self {
            groups: self
                .groups
                .iter()
                .map(|g| GroupGeom {
                    frame: PlaneFrame { origin: point(g.frame.origin), u: dir(g.frame.u), v: dir(g.frame.v) },
                    n: dir(g.n),
                    ..g.clone()
                })
                .collect(),
            dir: dir(self.dir),
            depth: self.depth,
            revolve: self.revolve.map(|rv| Revolution { origin: point(rv.origin), axis: dir(rv.axis), ..rv }),
        }
    }

    /// True when the sweep runs along the sketch normal (either way).
    fn along_normal(&self, g: &GroupGeom) -> bool {
        dot(self.dir, g.n).abs() > 1.0 - 1e-9
    }
}

/// The swept geometry of every operation whose faces a part can have, by operation.
pub type Geoms = HashMap<OpId, std::sync::Arc<OpGeom>>;

/// What a kernel face is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Class {
    Cap { group: usize, end: bool, region: u64 },
    /// `offset`: a thin wall's offset side of the piece.
    Side { group: usize, ring: usize, piece: usize, offset: bool },
    Unknown,
}

/// The distance from `p` to a boundary piece.
fn piece_distance(piece: &Piece, p: Vec2) -> f64 {
    match *piece {
        Piece::Line(a, b) => cadrs_sketch::geom::dist_point_segment(p, a, b),
        Piece::Arc(g) => g.distance(p),
        Piece::Ellipse { g, t0, sweep } => {
            let t = g.nearest_t(p);
            let (lo, len) = if sweep >= 0.0 { (t0, sweep) } else { (t0 + sweep, -sweep) };
            let rel = (t - lo).rem_euclid(std::f64::consts::TAU);
            if rel <= len + 1e-9 || len >= std::f64::consts::TAU - 1e-9 {
                g.point_at(t).distance(p)
            } else {
                p.distance(g.point_at(t0)).min(p.distance(g.point_at(t0 + sweep)))
            }
        }
        Piece::Bezier(g) => g.distance(p),
    }
}

/// True for a piece that is a whole circle or ellipse.
fn piece_closed(piece: &Piece) -> bool {
    match *piece {
        Piece::Line(..) | Piece::Bezier(_) => false,
        Piece::Arc(g) => g.sweep.abs() >= std::f64::consts::TAU - 1e-9,
        Piece::Ellipse { sweep, .. } => sweep.abs() >= std::f64::consts::TAU - 1e-9,
    }
}

/// Where a point on a piece lies along it, from 0 at its start to 1 at its end (by angle for
/// arcs, so it also works for concentric offsets of an arc).
fn piece_param(piece: &Piece, p: Vec2) -> f64 {
    use std::f64::consts::TAU;
    let (start, sweep, t) = match *piece {
        Piece::Line(a, b) => {
            let d = b - a;
            return (p - a).dot(d) / d.dot(d).max(1e-300);
        }
        Piece::Bezier(g) => return g.nearest_t(p),
        Piece::Arc(g) => (g.start_angle, g.sweep, (p - g.center).angle()),
        Piece::Ellipse { g, t0, sweep } => (t0, sweep, g.nearest_t(p)),
    };
    let rel = if sweep >= 0.0 {
        (t - start).rem_euclid(TAU)
    } else {
        (start - t).rem_euclid(TAU)
    };
    // A closed piece's start is also its end: count it as the start.
    let rel = if piece_closed(piece) && rel > TAU - 1e-9 { 0.0 } else { rel };
    rel / sweep.abs().max(1e-300)
}

/// Joins polylines that meet end to end into as few polylines as possible (closed ones end
/// where they start).
fn chain(mut parts: Vec<Vec<Vec3>>, tol: f64) -> Vec<Vec<Vec3>> {
    let mut out = Vec::new();
    parts.retain(|p| p.len() >= 2);
    while let Some(mut cur) = parts.pop() {
        loop {
            let (first, last) = (cur[0], *cur.last().unwrap());
            if dist(first, last) < tol && cur.len() > 2 {
                break;
            }
            let next = parts.iter().position(|p| {
                dist(p[0], last) < tol
                    || dist(*p.last().unwrap(), last) < tol
                    || dist(p[0], first) < tol
                    || dist(*p.last().unwrap(), first) < tol
            });
            let Some(i) = next else { break };
            let mut p = parts.swap_remove(i);
            if dist(p[0], last) < tol {
                cur.extend(p.drain(1..));
            } else if dist(*p.last().unwrap(), last) < tol {
                p.reverse();
                cur.extend(p.drain(1..));
            } else if dist(*p.last().unwrap(), first) < tol {
                p.pop();
                p.extend(cur);
                cur = p;
            } else {
                p.reverse();
                p.pop();
                p.extend(cur);
                cur = p;
            }
        }
        out.push(cur);
    }
    out
}

/// The name a face gets from where it lies (for faces the kernel's history doesn't name).
fn class_name(op: OpId, g: &OpGeom, c: Class) -> Option<FaceName> {
    match c {
        Class::Cap { end, region, .. } => Some(FaceName::new(op, FaceOrigin::Cap { region, end })),
        Class::Side { group, ring, piece, .. } => {
            let (region, pieces) = &g.groups[group].rings[ring];
            Some(FaceName::new(
                op,
                FaceOrigin::Side {
                    region: *region,
                    curve: curve_source(pieces[piece].1),
                },
            ))
        }
        Class::Unknown => None,
    }
}

/// Where a named face lies in the operation that made it: which cap, or the side of which
/// boundary piece (the first piece of its sketch curve).
fn class_of_name(geoms: &Geoms, name: &FaceName) -> Class {
    let Some(g) = geoms.get(&name.op) else {
        return Class::Unknown;
    };
    match name.origin {
        FaceOrigin::Cap { region, end } => g
            .groups
            .iter()
            .position(|gr| gr.regions.iter().any(|(i, _)| *i == region))
            .map_or(Class::Unknown, |group| Class::Cap { group, end, region }),
        FaceOrigin::Side { region, curve } => {
            let flags = kernel::thin::THIN_LEFT | kernel::thin::THIN_RIGHT;
            let (base, offset) = if curve & flags != 0 && curve & !flags != 0 {
                (curve & !flags, true)
            } else {
                (curve, false)
            };
            for (group, gr) in g.groups.iter().enumerate() {
                for (ring, (r, pieces)) in gr.rings.iter().enumerate() {
                    if *r != region {
                        continue;
                    }
                    if let Some(piece) = pieces.iter().position(|(_, c)| curve_source(*c) == base) {
                        return Class::Side { group, ring, piece, offset };
                    }
                }
            }
            Class::Unknown
        }
        _ => Class::Unknown,
    }
}

/// True for an edge left out of the display: between two caps at the same end of one
/// operation (touching regions, whose caps show as one face). Two pieces of one region's cap
/// (the same origin, told apart by their split index) meet only where a Face split cut it
/// (P3.8): that edge shows.
fn inside_caps(name: &EdgeName) -> bool {
    match (name.faces[0].origin, name.faces[1].origin) {
        (FaceOrigin::Cap { end: a, .. }, FaceOrigin::Cap { end: b, .. }) => {
            a == b && name.faces[0].op == name.faces[1].op && name.faces[0].origin != name.faces[1].origin
        }
        _ => false,
    }
}

/// The display mesh of a kernel body, with its faces, edges and vertices named (see the module
/// docs). `geoms` has the swept geometry of every operation the body's faces may come from;
/// `op` is the operation that made the body, whose geometry names what its history doesn't.
pub fn solid_of(
    k: &dyn Kernel,
    body: BodyId,
    names: &BodyNames,
    geoms: &Geoms,
    op: Option<OpId>,
) -> Result<Solid, String> {
    let mesh = k.tessellate(body, tessellation()).map_err(|e| e.to_string())?;
    let edge_info = k.edges(body).map_err(|e| e.to_string())?;
    let face_info = k.faces(body).map_err(|e| e.to_string())?;
    let vertex_info = k.vertices(body).map_err(|e| e.to_string())?;
    let face_count = face_info.len();

    let pos: Vec<Vec3> = mesh.positions.iter().map(|p| [p.x, p.y, p.z]).collect();
    let nrm: Vec<Vec3> = mesh.normals.iter().map(|n| [n.x, n.y, n.z]).collect();
    let size = pos.iter().fold(0.0f64, |m, p| m.max(p[0].abs()).max(p[1].abs()).max(p[2].abs()));
    let depth = op.and_then(|o| geoms.get(&o)).map_or(0.0, |g| g.depth);
    let extent = size.max(depth).max(1.0);
    let tol = 1e-6 * extent;
    let tol_piece = 1e-5 * extent;

    // The triangles of each kernel face.
    let mut face_tris: Vec<Vec<usize>> = vec![Vec::new(); face_count];
    for (t, f) in mesh.triangle_faces.iter().enumerate() {
        if let Some(list) = face_tris.get_mut(f.0 as usize) {
            list.push(t);
        }
    }

    // Every kernel face's name: from the history, else from where it lies.
    let own = op.and_then(|o| Some((o, geoms.get(&o)?)));
    let face_names: Vec<FaceName> = (0..face_count)
        .map(|fi| {
            let named = names.faces.get(fi).copied().filter(FaceName::is_stable);
            named
                .or_else(|| {
                    let (o, g) = own?;
                    let c = classify(g, &face_tris[fi], &mesh.indices, &pos, &nrm, tol, tol_piece);
                    class_name(o, g, c)
                })
                .unwrap_or_else(|| {
                    names.faces.get(fi).copied().unwrap_or(FaceName::new(
                        op.unwrap_or_default(),
                        FaceOrigin::Unnamed { index: fi as u32 },
                    ))
                })
        })
        .collect();
    // The display faces (one per name, in order of first appearance) and their kernel faces.
    let mut shown: Vec<FaceName> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (fi, name) in face_names.iter().enumerate() {
        if face_tris[fi].is_empty() {
            continue;
        }
        match shown.iter().position(|n| n == name) {
            Some(i) => members[i].push(fi),
            None => {
                shown.push(*name);
                members.push(vec![fi]);
            }
        }
    }

    // Edge names from the display faces (the history's names, unless a face was renamed from
    // geometry).
    let edge_names = naming::name_edges(&face_names, &edge_info, naming::joint_tolerance(&edge_info));
    let polyline = |id: kernel::EdgeId| -> Vec<Vec3> {
        mesh.edges
            .iter()
            .find(|(e, _)| *e == id)
            .map(|(_, pts)| pts.iter().map(|p| [p.x, p.y, p.z]).collect())
            .unwrap_or_default()
    };
    // Tangent-connected groups of kernel edges (P3.7, X12 "Tangent connected"): edges meeting at
    // a vertex with parallel tangents (within the kernel's `TANGENT_ANGLE`) are one group.
    let groups = tangent_groups(&edge_info, &vertex_info);
    let mut outline: HashMap<FaceName, Vec<Vec<Vec3>>> = HashMap::new();
    let mut named_edges: Vec<(EdgeName, Vec<Vec3>, Option<EdgeCircle>, u32)> = Vec::new();
    for (ei, (info, name)) in edge_info.iter().zip(&edge_names).enumerate() {
        let Some(name) = name else { continue };
        // A sliver (see `SLIVER`) isn't an edge anyone can see or pick: its neighbours meet.
        if info.length < SLIVER {
            continue;
        }
        let pts = polyline(info.id);
        if pts.len() < 2 {
            continue;
        }
        for f in name.faces {
            outline.entry(f).or_default().push(pts.clone());
        }
        if !inside_caps(name) {
            let circle = info.circle.map(|c| EdgeCircle {
                center: [c.center.x, c.center.y, c.center.z],
                normal: [c.normal.x, c.normal.y, c.normal.z],
                radius: c.radius,
            });
            named_edges.push((*name, pts, circle, groups[ei]));
        }
    }

    // The sketch frame of a planar face named `name` whose kernel plane is `kernel_plane`.
    let frame_of = |name: &FaceName, kernel_plane: Option<kernel::Plane>| -> Option<PlaneFrame> {
        // Planar faces: the prism's frame where the sweep makes one, else the kernel's plane.
        // A frame from the sweep only where it is the face's plane: a Transform may have moved
        // the face away from where its operation swept it (P3 Onshape import).
        let on_plane = |f: &PlaneFrame| {
            let Some(kp) = kernel_plane else { return true };
            let kn = kp.normal.into_inner();
            let kn = [kn.x, kn.y, kn.z];
            let off = sub(f.origin, [kp.origin.x, kp.origin.y, kp.origin.z]);
            dot(normalize(f.normal()), kn).abs() > 1.0 - 1e-9 && dot(off, kn).abs() < tol_piece
        };
        // The frame's normal must be the face's outward one: a Remove's cap becomes a face of the
        // part it cut, looking the other way (a pocket's floor), so turn the frame over there.
        let outward = |f: PlaneFrame| match kernel_plane {
            Some(kp) => {
                let kn = kp.normal.into_inner();
                if dot(f.normal(), [kn.x, kn.y, kn.z]) < 0.0 {
                    PlaneFrame { v: [-f.v[0], -f.v[1], -f.v[2]], ..f }
                } else {
                    f
                }
            }
            None => f,
        };
        geoms
            .get(&name.op)
            .and_then(|g| face_frame(g, class_of_name(geoms, name), kernel_plane))
            .filter(on_plane)
            .map(outward)
            .or_else(|| {
                let p = kernel_plane?;
                let (o, u, n) = (p.origin, p.x_dir.into_inner(), p.normal.into_inner());
                let v = n.cross(&u);
                Some(PlaneFrame {
                    origin: [o.x, o.y, o.z],
                    u: [u.x, u.y, u.z],
                    v: [v.x, v.y, v.z],
                })
            })
    };

    // Assemble the solid, face by face.
    let mut out = Solid::default();
    let mut kernel_planes: Vec<Option<kernel::Plane>> = Vec::new();
    let mut remap: Vec<u32> = vec![u32::MAX; pos.len()];
    for (name, fis) in shown.iter().zip(&members) {
        let first_triangle = out.triangle_count();
        for &fi in fis {
            for &t in &face_tris[fi] {
                for v in mesh.indices[t] {
                    let v = v as usize;
                    if remap[v] == u32::MAX {
                        remap[v] = out.positions.len() as u32;
                        out.positions.push(pos[v]);
                        out.normals.push(nrm[v]);
                    }
                    out.indices.push(remap[v]);
                }
            }
        }
        let triangle_count = out.triangle_count() - first_triangle;
        let class = class_of_name(geoms, name);
        let kernel_plane = face_info.get(fis[0]).and_then(|f| f.plane);
        let plane = frame_of(name, kernel_plane);
        kernel_planes.push(kernel_plane);
        let loops = chain(outline.remove(name).unwrap_or_default(), tol_piece)
            .into_iter()
            .map(|mut l| {
                if l.len() > 2 && dist(l[0], *l.last().unwrap()) < tol_piece {
                    l.pop();
                }
                l
            })
            .collect();
        let face_index = out.faces.len();
        // The exact centroid of its kernel faces together, and the axis of a face of revolution.
        let (area, moment) = fis.iter().filter_map(|&fi| face_info.get(fi)).fold((0.0, [0.0; 3]), |(a, m), f| {
            (a + f.area, [m[0] + f.center.x * f.area, m[1] + f.center.y * f.area, m[2] + f.center.z * f.area])
        });
        let center = (area > 0.0).then(|| [moment[0] / area, moment[1] / area, moment[2] / area]);
        let axis = face_info.get(fis[0]).and_then(|f| f.axis).map(|a| {
            let (o, d) = (a.origin, a.dir.into_inner());
            ([o.x, o.y, o.z], [d.x, d.y, d.z])
        });
        out.faces.push(SolidFace {
            name: *name,
            plane,
            first_triangle,
            triangle_count,
            loops,
            center,
            axis,
            area: (area > 0.0).then_some(area),
            kind: face_info.get(fis[0]).map(|f| f.kind),
            radius: face_info.get(fis[0]).and_then(|f| f.radius),
        });
        // Rulings of a curved side face: through the face's own mesh points, along the sweep,
        // as far as the face reaches (a boolean may have trimmed it).
        let revolved_here = |g: &OpGeom| match (g.revolve, axis) {
            // A face of revolution about the revolve's own axis (not one a Transform moved).
            (Some(rv), Some((o, d))) => {
                let off = sub(o, rv.origin);
                let along = cross(off, rv.axis);
                dot(normalize(d), rv.axis).abs() > 1.0 - 1e-9 && dot(along, along).sqrt() < tol_piece
            }
            _ => true,
        };
        if let (Class::Side { group, ring, piece, offset }, Some(g)) = (class, geoms.get(&name.op).filter(|g| revolved_here(g))) {
            let gr = &g.groups[group];
            let p = &gr.rings[ring].1[piece].0;
            if let Some(rv) = g.revolve {
                // A curved face of a revolve (a line's cone or cylinder too): its meridians.
                if plane.is_none() {
                    let verts: Vec<usize> = {
                        let mut v: Vec<usize> = fis
                            .iter()
                            .flat_map(|&fi| face_tris[fi].iter().flat_map(|&t| mesh.indices[t].map(|v| v as usize)))
                            .collect();
                        v.sort_unstable();
                        v.dedup();
                        v
                    };
                    meridians(&mut out, face_index, gr, rv, p, &verts, &pos, &nrm);
                }
            } else if !matches!(p, Piece::Line(..)) {
                let verts: Vec<usize> = {
                    let mut v: Vec<usize> = fis
                        .iter()
                        .flat_map(|&fi| face_tris[fi].iter().flat_map(|&t| mesh.indices[t].map(|v| v as usize)))
                        .collect();
                    v.sort_unstable();
                    v.dedup();
                    v
                };
                let tris: Vec<[usize; 3]> = fis
                    .iter()
                    .flat_map(|&fi| face_tris[fi].iter().map(|&t| mesh.indices[t].map(|v| v as usize)))
                    .collect();
                rulings(&mut out, face_index, gr, g.dir, p, &verts, &tris, &pos, &nrm, tol_piece, !offset);
            }
        }
    }

    // The other names of merged faces, each with the frame the face has under that name.
    for (alias, primary) in &names.aliases {
        let Some(i) = out.faces.iter().position(|f| f.name == *primary) else { continue };
        let plane = out.faces[i].plane.and_then(|_| frame_of(alias, kernel_planes[i]));
        out.face_aliases.push(crate::solid::FaceAlias { name: *alias, face: *primary, plane });
    }

    // One edge per name where its pieces join up.
    let mut edge_order: Vec<EdgeName> = Vec::new();
    let mut edge_parts: Vec<Vec<Vec<Vec3>>> = Vec::new();
    // The exact circle of each name, if all its kernel edges lie on one.
    let mut edge_circles: Vec<Option<EdgeCircle>> = Vec::new();
    let mut edge_groups: Vec<u32> = Vec::new();
    let same_circle = |a: &EdgeCircle, b: &EdgeCircle| {
        dist(a.center, b.center) < tol_piece
            && (a.radius - b.radius).abs() < tol_piece
            && dot(a.normal, b.normal).abs() > 1.0 - 1e-9
    };
    for (name, pts, circle, group) in named_edges {
        match edge_order.iter().position(|n| *n == name) {
            Some(i) => {
                edge_parts[i].push(pts);
                let keep = matches!((&edge_circles[i], &circle), (Some(a), Some(b)) if same_circle(a, b));
                if !keep {
                    edge_circles[i] = None;
                }
            }
            None => {
                edge_order.push(name);
                edge_parts.push(vec![pts]);
                edge_circles.push(circle);
                edge_groups.push(group);
            }
        }
    }
    for (((name, parts), circle), group) in edge_order.into_iter().zip(edge_parts).zip(edge_circles).zip(edge_groups) {
        for points in chain(parts, tol_piece) {
            out.edges.push(SolidEdge { name, points, circle, tangent_group: Some(group) });
        }
    }

    // Vertices where two or more of the shown edges end.
    let vertex_names = naming::name_vertices(&face_names, &edge_info, &vertex_info);
    for (v, name) in vertex_info.iter().zip(vertex_names) {
        let Some(name) = name else { continue };
        let shown_edges = v
            .edges
            .iter()
            .filter_map(|e| edge_names.get(e.0 as usize).copied().flatten())
            .filter(|n| !inside_caps(n))
            .count();
        if shown_edges >= 2 {
            out.vertices.push(SolidVertex {
                name,
                point: [v.point.x, v.point.y, v.point.z],
            });
        }
    }
    Ok(out)
}

/// The rulings of a curved side face (for its silhouettes), from its mesh points: the points
/// are grouped by where they lie across the sweep (a line along the sweep through each), and
/// each group gives a ruling from its lowest to its highest point, in order along the piece.
#[allow(clippy::too_many_arguments)]
fn rulings(
    out: &mut Solid,
    face_index: usize,
    g: &GroupGeom,
    dir: Vec3,
    piece: &Piece,
    verts: &[usize],
    tris: &[[usize; 3]],
    pos: &[Vec3],
    nrm: &[Vec3],
    tol: f64,
    on_piece: bool,
) {
    let dn = dot(dir, g.n);
    if dn.abs() < 1e-9 || verts.is_empty() {
        return;
    }
    // Each point: its foot on the sketch plane (back along the sweep), height, normal.
    let mut pts: Vec<(f64, Vec3, f64, Vec3)> = verts
        .iter()
        .map(|&v| {
            let h = dot(sub(pos[v], g.frame.origin), g.n) / dn;
            let foot = sub(pos[v], scale(dir, h));
            let param = piece_param(piece, g.frame.to_sketch(foot));
            (param, foot, h, normalize(nrm[v]))
        })
        .collect();
    // The face must lie on the swept piece (a Transform may have moved it off, P3 Onshape
    // import): no rulings otherwise.
    // (A curve's pieces may be cut apart, so any piece of the sketch will do.)
    let off_sketch = |foot: Vec3| {
        let q = g.frame.to_sketch(foot);
        g.rings.iter().flat_map(|(_, ps)| ps).all(|(p, _)| piece_distance(p, q) > 100.0 * tol)
    };
    if on_piece && pts.iter().any(|(_, foot, ..)| off_sketch(*foot)) {
        return;
    }
    // Where the face reaches along the ruling at `param`: its triangles laid out flat by
    // (place along the piece, height), crossed by the line at `param`. For a lone mesh point
    // (the face's own top and bottom edges are meshed at different places: a face merged from
    // several, or notched by a boolean).
    let flat: std::collections::HashMap<usize, (f64, f64)> = verts.iter().zip(&pts).map(|(&v, p)| (v, (p.0, p.2))).collect();
    let period = match *piece {
        Piece::Line(..) | Piece::Bezier(..) => None,
        Piece::Arc(a) => Some(std::f64::consts::TAU / a.sweep.abs().max(1e-300)),
        Piece::Ellipse { sweep, .. } => Some(std::f64::consts::TAU / sweep.abs().max(1e-300)),
    };
    let reach = |param: f64| -> Option<(f64, f64)> {
        let mut out: Option<(f64, f64)> = None;
        for t in tris {
            let Some(mut c) = t.iter().map(|v| flat.get(v).copied()).collect::<Option<Vec<(f64, f64)>>>() else { continue };
            if let Some(per) = period {
                let q0 = c[0].0;
                for q in c.iter_mut().skip(1) {
                    if q.0 - q0 > per / 2.0 {
                        q.0 -= per;
                    } else if q0 - q.0 > per / 2.0 {
                        q.0 += per;
                    }
                }
            }
            let shifts: &[f64] = match period {
                Some(per) => &[0.0, per, -per],
                None => &[0.0],
            };
            for shift in shifts {
                let x = param + shift;
                for k in 0..3 {
                    let (a, b) = (c[k], c[(k + 1) % 3]);
                    let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
                    if x < lo - 1e-9 || x > hi + 1e-9 {
                        continue;
                    }
                    let h = if hi - lo < 1e-12 { a.1.min(b.1) } else { a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0) };
                    let h2 = if hi - lo < 1e-12 { a.1.max(b.1) } else { h };
                    out = Some(match out {
                        Some((l, u)) => (l.min(h), u.max(h2)),
                        None => (h, h2),
                    });
                }
            }
        }
        out
    };
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (hmin, hmax) = pts
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.2), hi.max(p.2)));
    if hmax - hmin < tol {
        return;
    }
    // Group points with the same foot.
    let mut groups: Vec<(f64, Vec3, f64, f64, Vec3)> = Vec::new();
    for (param, foot, h, n) in pts {
        match groups.iter_mut().rev().take(4).find(|g| dist(g.1, foot) < 10.0 * tol) {
            Some(gr) => {
                gr.2 = gr.2.min(h);
                gr.3 = gr.3.max(h);
                gr.4 = add(gr.4, n);
            }
            None => groups.push((param, foot, h, h, n)),
        }
    }
    let mut along: Vec<(Vec3, Vec3, Vec3)> = groups
        .into_iter()
        .map(|(param, foot, lo, hi, n)| {
            // A lone point (an end the mesh didn't pair): as far as the face's triangles reach
            // there, else the face's whole height.
            let (lo, hi) = if hi - lo < tol { reach(param).filter(|(l, h)| h - l >= tol).unwrap_or((hmin, hmax)) } else { (lo, hi) };
            (add(foot, scale(dir, lo)), add(foot, scale(dir, hi)), normalize(n))
        })
        .collect();
    if along.len() < 2 {
        return;
    }
    if piece_closed(piece) {
        // Around a closed face, back to the first ruling.
        along.push(along[0]);
    }
    // A new run wherever the rulings jump (a gap between two pieces of the face): no
    // silhouette is looked for across it.
    let mut gaps: Vec<f64> = along.windows(2).map(|w| dist(w[0].0, w[1].0)).collect();
    gaps.sort_by(f64::total_cmp);
    let usual = gaps.get(gaps.len() / 2).copied().unwrap_or(0.0);
    let mut run = out.rulings.last().map_or(0, |r| r.run + 1);
    let mut prev: Option<Vec3> = None;
    for (start, end, normal) in along {
        if prev.is_some_and(|p| dist(p, start) > 4.0 * usual + tol) {
            run += 1;
        }
        prev = Some(start);
        out.rulings.push(Ruling {
            start,
            end,
            normal,
            face: face_index,
            run,
        });
    }
}

/// The meridians of a curved face a revolve swept (P3.4), for its silhouettes: the profile piece
/// turned to every 5° of the sweep (where the face reaches), with the surface's normal along
/// it. Each segment of the piece is a run of rulings across the meridians (a line's cone or
/// cylinder is one run of straight rulings; an arc's torus or sphere one run per segment), so
/// a silhouette is found between neighbouring meridians along each segment.
#[allow(clippy::too_many_arguments)]
fn meridians(
    out: &mut Solid,
    face_index: usize,
    g: &GroupGeom,
    rv: Revolution,
    piece: &Piece,
    verts: &[usize],
    pos: &[Vec3],
    nrm: &[Vec3],
) {
    use std::f64::consts::TAU;
    let w = |p: Vec2| g.frame.to_world(p);
    let dirw = |v: Vec2| add(scale(normalize(g.frame.u), v.x), scale(normalize(g.frame.v), v.y));
    // The piece's points and in-plane normals (one side, consistently).
    let profile: Vec<(Vec3, Vec3)> = match *piece {
        Piece::Line(a, b) => {
            let n = dirw((b - a).perp());
            vec![(w(a), n), (w(b), n)]
        }
        Piece::Arc(a) => {
            // Finer than the mesh (a quarter of its step): a chord of the meridian then lies
            // between the true surface and the mesh's facets, so its silhouette isn't hidden
            // behind them.
            let n = ((a.sweep.abs() / (ANGLE_STEP / 4.0)).ceil() as usize).max(2);
            (0..=n)
                .map(|k| {
                    let t = a.start_angle + a.sweep * k as f64 / n as f64;
                    let p = a.point_at(t);
                    (w(p), dirw(p - a.center))
                })
                .collect()
        }
        Piece::Ellipse { g: e, t0, sweep } => {
            let n = ((sweep.abs() / (ANGLE_STEP / 4.0)).ceil() as usize).max(2);
            (0..=n)
                .map(|k| {
                    let t = t0 + sweep * k as f64 / n as f64;
                    let (p, q) = (e.point_at(t), e.point_at(t + 1e-6));
                    (w(p), dirw((q - p).perp()))
                })
                .collect()
        }
        Piece::Bezier(b) => {
            let n = 64;
            (0..=n)
                .map(|k| {
                    let t = k as f64 / n as f64;
                    (w(b.point_at(t)), dirw(b.tangent_at(t).perp()))
                })
                .collect()
        }
    };
    if profile.len() < 2 || verts.is_empty() {
        return;
    }
    // The normals point out of the material, as the mesh's do (compared at the mesh point
    // nearest the first profile point, turned to where the sweep starts).
    let profile: Vec<(Vec3, Vec3)> = {
        let (p0, n0) = (rv.turn_point(profile[0].0, rv.start), rv.turn(profile[0].1, rv.start));
        let nearest = verts
            .iter()
            .min_by(|a, b| dist(pos[**a], p0).total_cmp(&dist(pos[**b], p0)))
            .map(|v| nrm[*v]);
        let flip = nearest.is_some_and(|m| dot(m, n0) < 0.0);
        profile
            .into_iter()
            .map(|(p, n)| (p, if flip { scale_neg(n) } else { n }))
            .collect()
    };
    // Angles about the axis, from the profile's half plane.
    let off = |p: Vec3| {
        let v = sub(p, rv.origin);
        sub(v, scale(rv.axis, dot(v, rv.axis)))
    };
    let Some(u0) = profile.iter().map(|(p, _)| off(*p)).find(|v| dot(*v, *v) > 1e-18).map(normalize) else {
        return;
    };
    let w0 = cross(rv.axis, u0);
    let angle = |p: Vec3| {
        let v = off(p);
        dot(v, w0).atan2(dot(v, u0))
    };
    // Where the face reaches, relative to the start of the sweep (in the sweep's direction).
    let sign = if rv.sweep < 0.0 { -1.0 } else { 1.0 };
    let rel = |a: f64| ((a - rv.start) * sign).rem_euclid(TAU);
    let mut have: Vec<f64> = verts.iter().map(|&v| rel(angle(pos[v]))).collect();
    have.sort_by(f64::total_cmp);
    let full = rv.sweep.abs() >= TAU - 1e-9;
    let n = ((rv.sweep.abs() / ANGLE_STEP).ceil() as usize).max(1);
    let step = rv.sweep.abs() / n as f64;
    // The largest gap between the face's points (around the circle for a whole turn).
    let (gap_lo, gap_hi) = {
        let mut best = (0.0, 0.0);
        for w in have.windows(2) {
            if w[1] - w[0] > best.1 - best.0 {
                best = (w[0], w[1]);
            }
        }
        if full {
            let wrap = (have[have.len() - 1], have[0] + TAU);
            if wrap.1 - wrap.0 > best.1 - best.0 {
                best = wrap;
            }
        }
        best
    };
    let lo = have[0];
    let hi = have[have.len() - 1];
    let reaches = |t: f64| -> bool {
        let slack = 1e-6;
        if gap_hi - gap_lo > 2.5 * step {
            // Not inside the gap.
            let inside = |x: f64| x > gap_lo + slack && x < gap_hi - slack;
            !(inside(t) || inside(t + TAU))
        } else if full {
            true
        } else {
            t >= lo - slack && t <= hi + slack
        }
    };
    let mut angles: Vec<f64> = (0..=n)
        .map(|k| k as f64 * step)
        .filter(|t| !(full && *t >= TAU - 1e-9))
        .filter(|t| reaches(*t))
        .collect();
    if angles.len() < 2 {
        return;
    }
    let closed = full && angles.len() == n;
    if closed {
        angles.push(angles[0]);
    }
    if !matches!(piece, Piece::Line(..)) {
        // A doubly curved face (a torus, a sphere): a grid of rows, one per angle, split where
        // the face has a gap.
        let mut rows: Vec<Vec<(Vec3, Vec3)>> = Vec::new();
        let mut prev: Option<f64> = None;
        for &t in &angles {
            let gap = prev.is_some_and(|p| (t - p).abs() > 1.5 * step && !(closed && t == angles[0]));
            if gap && rows.len() > 1 {
                out.grids.push(SurfaceGrid { face: face_index, rows: std::mem::take(&mut rows) });
            } else if gap {
                rows.clear();
            }
            prev = Some(t);
            let theta = rv.start + sign * t;
            rows.push(profile.iter().map(|(p, nn)| (rv.turn_point(*p, theta), rv.turn(*nn, theta))).collect());
        }
        if rows.len() > 1 {
            out.grids.push(SurfaceGrid { face: face_index, rows });
        }
        return;
    }
    for j in 0..profile.len() - 1 {
        let (a, b) = (profile[j], profile[j + 1]);
        let nm = normalize(add(a.1, b.1));
        let mut run = out.rulings.last().map_or(0, |r| r.run + 1);
        let mut prev: Option<f64> = None;
        for &t in &angles {
            if prev.is_some_and(|p| (t - p).abs() > 1.5 * step && !(closed && t == angles[0])) {
                run += 1;
            }
            prev = Some(t);
            let theta = rv.start + sign * t;
            out.rulings.push(Ruling {
                start: rv.turn_point(a.0, theta),
                end: rv.turn_point(b.0, theta),
                normal: rv.turn(nm, theta),
                face: face_index,
                run,
            });
        }
    }
}

/// For each kernel edge, the smallest index of the edges tangent-connected to it: edges meeting
/// at a vertex whose tangents there are parallel (within [`kernel::TANGENT_ANGLE`], Onshape's
/// tangent propagation) are joined, and so on along the chain (union–find).
/// Edges shorter than this (mm) are slivers: OCCT's fillet can leave one about 1e-6 mm long
/// where its edge crosses the join of two coplanar faces (the funnel's rim, PS21.15). They are
/// left out of the solid's edges and their two ends count as one vertex, so the edges on
/// either side join (Onshape, with one face there, has no such edge).
pub const SLIVER: f64 = 1e-4;

fn tangent_groups(edges: &[kernel::EdgeInfo], vertices: &[kernel::VertexInfo]) -> Vec<u32> {
    let n = edges.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let index: HashMap<u64, usize> = edges.iter().enumerate().map(|(i, e)| (e.id.0, i)).collect();
    let tol = naming::joint_tolerance(edges);
    let cos = kernel::TANGENT_ANGLE.cos();
    // Vertices joined by a sliver are one.
    let sliver = |i: usize| edges[i].length < SLIVER;
    let mut vparent: Vec<usize> = (0..vertices.len()).collect();
    let vindex: HashMap<u64, Vec<usize>> = {
        let mut m: HashMap<u64, Vec<usize>> = HashMap::new();
        for (vi, v) in vertices.iter().enumerate() {
            for id in &v.edges {
                m.entry(id.0).or_default().push(vi);
            }
        }
        m
    };
    for (id, vs) in &vindex {
        if index.get(id).is_some_and(|&i| sliver(i)) {
            for w in vs.windows(2) {
                let (a, b) = (find(&mut vparent, w[0]), find(&mut vparent, w[1]));
                if a != b {
                    vparent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut joints: HashMap<usize, (Vec<nalgebra::Point3<f64>>, Vec<u64>)> = HashMap::new();
    for (vi, v) in vertices.iter().enumerate() {
        let r = find(&mut vparent, vi);
        let j = joints.entry(r).or_default();
        j.0.push(v.point);
        j.1.extend(v.edges.iter().map(|e| e.0));
    }
    let mut roots: Vec<usize> = joints.keys().copied().collect();
    roots.sort_unstable();
    for r in roots {
        let (points, ids) = &joints[&r];
        let near = |p: &nalgebra::Point3<f64>| points.iter().any(|q| (p - q).norm() < tol);
        let mut seen = std::collections::HashSet::new();
        let at: Vec<(usize, Vec<nalgebra::Vector3<f64>>)> = ids
            .iter()
            .filter(|id| seen.insert(**id))
            .filter_map(|id| {
                let i = *index.get(id)?;
                if sliver(i) {
                    return None;
                }
                let e = &edges[i];
                let mut t = Vec::new();
                if near(&e.start) {
                    t.push(e.start_tangent);
                }
                if near(&e.end) {
                    t.push(e.end_tangent);
                }
                Some((i, t))
            })
            .collect();
        for a in 0..at.len() {
            for b in a + 1..at.len() {
                let tangent = at[a].1.iter().any(|x| at[b].1.iter().any(|y| x.dot(y).abs() >= cos));
                if tangent {
                    let (ra, rb) = (find(&mut parent, at[a].0), find(&mut parent, at[b].0));
                    if ra != rb {
                        parent[ra.max(rb)] = ra.min(rb);
                    }
                }
            }
        }
    }
    (0..n).map(|i| find(&mut parent, i) as u32).collect()
}

fn scale_neg(a: Vec3) -> Vec3 {
    scale(a, -1.0)
}

/// Which cap or side a kernel face is, from where it lies in the sweep `g` (see the module
/// docs).
fn classify(
    g: &OpGeom,
    tris: &[usize],
    indices: &[[u32; 3]],
    pos: &[Vec3],
    nrm: &[Vec3],
    tol: f64,
    tol_piece: f64,
) -> Class {
    let mut verts: Vec<usize> = tris
        .iter()
        .flat_map(|&t| indices[t].map(|v| v as usize))
        .collect();
    verts.sort_unstable();
    verts.dedup();
    // A revolve's faces are named from its history only.
    if verts.is_empty() || g.revolve.is_some() {
        return Class::Unknown;
    }
    for (gi, gr) in g.groups.iter().enumerate() {
        let heights: Vec<f64> = verts
            .iter()
            .map(|&v| dot(sub(pos[v], gr.frame.origin), g.dir))
            .collect();
        let normals_along = verts
            .iter()
            .all(|&v| dot(normalize(nrm[v]), gr.n).abs() > 1.0 - 1e-6);
        if normals_along {
            let end = if heights.iter().all(|h| h.abs() < tol) {
                Some(false)
            } else if heights.iter().all(|h| (h - g.depth).abs() < tol) {
                Some(true)
            } else {
                None
            };
            if let Some(end) = end {
                // The region containing the face: try triangle centroids, largest first.
                let mut by_area: Vec<(f64, Vec3)> = tris
                    .iter()
                    .map(|&t| {
                        let [a, b, c] = indices[t].map(|v| pos[v as usize]);
                        let area = dot(cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(c, a)));
                        (area, scale(add(add(a, b), c), 1.0 / 3.0))
                    })
                    .collect();
                by_area.sort_by(|a, b| b.0.total_cmp(&a.0));
                let region = by_area
                    .iter()
                    .take(16)
                    .find_map(|(_, c)| {
                        let p = gr.frame.to_sketch(*c);
                        gr.regions.iter().find(|(_, r)| r.contains(p)).map(|(i, _)| *i)
                    })
                    .or_else(|| gr.regions.first().map(|(i, _)| *i));
                if let Some(region) = region {
                    return Class::Cap {
                        group: gi,
                        end,
                        region,
                    };
                }
            }
            continue;
        }
        let across = verts.iter().all(|&v| dot(normalize(nrm[v]), gr.n).abs() < 1e-5);
        let within = heights.iter().all(|h| *h > -tol && *h < g.depth + tol);
        if !(across && within) {
            continue;
        }
        // The mesh points and the triangles' centroids: a flat face's corners also lie on the
        // arcs they join, but its centroids lie only on its line.
        let pts: Vec<Vec2> = verts
            .iter()
            .map(|&v| pos[v])
            .chain(tris.iter().map(|&t| {
                let [a, b, c] = indices[t].map(|v| pos[v as usize]);
                scale(add(add(a, b), c), 1.0 / 3.0)
            }))
            .map(|p| gr.frame.to_sketch(p))
            .collect();
        let mut best: Option<(usize, usize, usize)> = None;
        let mut best_count = 0;
        for (ri, (_, ring)) in gr.rings.iter().enumerate() {
            for (pi, (piece, _)) in ring.iter().enumerate() {
                let count = pts
                    .iter()
                    .filter(|p| piece_distance(piece, **p) < tol_piece)
                    .count();
                if count > best_count {
                    best_count = count;
                    best = Some((ri, pi, count));
                }
            }
        }
        if let Some((ring, piece, _)) = best {
            return Class::Side {
                group: gi,
                ring,
                piece,
                offset: false,
            };
        }
    }
    Class::Unknown
}

/// The sketch frame of a planar face, as the prism mesh gave it: a cap gets the sketch's axes
/// (v flipped on a face looking against the sketch normal) with its origin under the sketch
/// origin; a straight side gets its line and the sweep. `None` where the sweep doesn't make
/// such a frame (an oblique direction, a cap trimmed at an angle, a thin wall's offset side):
/// the kernel's plane is used then.
fn face_frame(g: &OpGeom, class: Class, kernel_plane: Option<kernel::Plane>) -> Option<PlaneFrame> {
    if let Some(rv) = g.revolve {
        // A revolve's cap: the sketch's frame turned to where the cap is (v flipped on a cap
        // looking against the turned sketch normal). Its planar sides use the kernel's plane.
        let Class::Cap { group, end, .. } = class else {
            return None;
        };
        let f = &g.groups[group].frame;
        let kp = kernel_plane?;
        let kn = kp.normal.into_inner();
        let angle = if end { rv.start + rv.sweep } else { rv.start };
        let (u, v) = (rv.turn(f.u, angle), rv.turn(f.v, angle));
        let facing = dot(normalize(cross(u, v)), [kn.x, kn.y, kn.z]);
        if facing.abs() < 1.0 - 1e-9 {
            return None;
        }
        let origin = rv.turn_point(f.origin, angle);
        return Some(if facing > 0.0 {
            PlaneFrame { origin, u, v }
        } else {
            PlaneFrame {
                origin,
                u,
                v: scale_neg(v),
            }
        });
    }
    match class {
        Class::Cap { group, end, .. } => {
            let gr = &g.groups[group];
            let f = &gr.frame;
            let kp = kernel_plane?;
            let kn = kp.normal.into_inner();
            if dot([kn.x, kn.y, kn.z], gr.n).abs() < 1.0 - 1e-9 {
                return None;
            }
            // Where the cap's plane is, along the normal.
            let c = [kp.origin.x, kp.origin.y, kp.origin.z];
            let origin = add(f.origin, scale(gr.n, dot(sub(c, f.origin), gr.n)));
            // The end cap looks along the sweep, the start cap against it.
            let outward = if end { g.dir } else { scale_neg(g.dir) };
            Some(if dot(outward, gr.n) > 0.0 {
                PlaneFrame { origin, u: f.u, v: f.v }
            } else {
                PlaneFrame {
                    origin,
                    u: f.u,
                    v: scale_neg(f.v),
                }
            })
        }
        Class::Side { group, ring, piece, offset: false } => {
            let gr = &g.groups[group];
            if !g.along_normal(gr) {
                return None;
            }
            let Piece::Line(a, b) = gr.rings[ring].1[piece].0 else {
                return None;
            };
            let (pa, pb) = (gr.frame.to_world(a), gr.frame.to_world(b));
            let t = normalize(sub(pb, pa));
            let forward = dot(g.dir, gr.n) > 0.0;
            // u × v must be the outward normal.
            let (u, v) = if forward { (t, g.dir) } else { (scale_neg(t), g.dir) };
            let outward = cross(u, v);
            let origin = scale(outward, dot(pa, outward));
            Some(PlaneFrame { origin, u, v })
        }
        _ => None,
    }
}

#[cfg(all(test, feature = "occt"))]
#[path = "brep_tests.rs"]
mod tests;
