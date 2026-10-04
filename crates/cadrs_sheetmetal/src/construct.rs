//! Building a sheet metal [`Model`] from the Sheet metal model feature's inputs (P3I.2,
//! SM2.2–SM2.4). The feature's rebuild (`cadrs_core`) reads the parts, faces and sketch curves
//! and hands them over as plain geometry, so the rules live here and are tested without a kernel:
//!
//! - [`from_faces`] (**Convert** and **Thicken**): each planar face becomes a wall, offset by the
//!   clearance on the material side. Faces that share a straight edge are joined: the edges in
//!   *Edges or cylinders to bend* become **bends** in the order they were picked, as long as the
//!   two walls aren't joined by bends already (pick order decides the flat pattern, SM2.2); every
//!   other shared edge becomes a **rip** (an edge joint). A cylinder picked to bend becomes a bend
//!   between its two flat neighbours (its radius the cylinder's); a cylinder that isn't picked
//!   becomes a **rolled wall** joined to its flat neighbours by **tangent joints**.
//! - [`from_chains`] (**Extrude**): each sketch line becomes a planar wall, each arc a rolled
//!   wall (or, picked in *Arcs to extrude as bends*, a bend between the lines either side of it);
//!   lines that touch at an angle are joined by a bend of the model's radius, a line running into
//!   an arc by a tangent joint. A closed chain is ripped where its first and last lines meet.
//!
//! Walls and joints get **persistent ids** from the keys of the faces, edges and curves they
//! come from ([`stable_id`]), so the table, the flat view and later features can refer to them
//! across rebuilds. Joints are named as Onshape names them: "Bend A", "Joint B", … one letter
//! sequence in creation order (bends first, in pick order).

use std::f64::consts::{PI, TAU};

use crate::model::{
    BuildError, Joint, JointId, JointKind, Model, P3, RipStyle, SharpBuilder, SharpJointKind, Surface, V3, Wall, WallId, letters,
};
use crate::params::Params;
use crate::poly::{P2, Polygon, Seg2, V2};
use serde::{Deserialize, Serialize};

/// A 32-bit id from a 64-bit key (a hash of a face or edge name).
pub fn stable_id(key: u64) -> u32 {
    (key ^ (key >> 32)) as u32
}

// ---------------------------------------------------------------------------------------------
// Inputs

/// A planar face of a part (Convert, Thicken) or a sketch region (Thicken).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceIn {
    /// A stable key (a hash of the face's name).
    pub key: u64,
    pub origin: P3,
    /// Orthonormal in-plane axes; `u × v` is the face's outward normal (away from the part; the
    /// sketch normal for a region).
    pub u: V3,
    pub v: V3,
    /// The face in `(u, v)` coordinates. Straight edges only (curves come as polylines).
    pub outline: Polygon,
}

impl FaceIn {
    pub fn normal(&self) -> V3 {
        self.u.cross(&self.v).normalize()
    }

    fn point(&self, q: P2) -> P3 {
        self.origin + self.u * q.x + self.v * q.y
    }

    fn local(&self, p: P3) -> P2 {
        P2::new((p - self.origin).dot(&self.u), (p - self.origin).dot(&self.v))
    }
}

/// A cylindrical face (a fillet's round) between two planar faces it runs into tangentially.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CylIn {
    pub key: u64,
    pub axis_origin: P3,
    /// Unit axis.
    pub axis: V3,
    pub radius: f64,
    /// The face's outward normal points away from the axis (a round on an outside corner).
    pub convex: bool,
    /// Where it lies: angles about the axis (from `start`, right-handed) and heights along it.
    pub start: V3,
    pub sweep: f64,
    pub z: (f64, f64),
    /// The planar faces (indices into the faces) it runs into at its start and at its end.
    pub neighbours: (Option<usize>, Option<usize>),
}

/// A straight edge two faces share.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeIn {
    pub key: u64,
    pub a: P3,
    pub b: P3,
    pub faces: (usize, usize),
}

/// How [`from_faces`] lays the walls.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FaceOpts {
    /// The thickness goes against the faces' normals (into the part for Convert).
    pub material_inside: bool,
    /// The gap between the faces and the sheet ("Clearance from input").
    pub clearance: f64,
    /// The clearance holds for the bends too: walls move out so the bends' inside clears the
    /// input's edges by the clearance ("Include bends").
    pub include_bends: bool,
    /// Edges (by key) and cylinders (by key) to bend, in the order they were picked.
    pub bends: Vec<u64>,
    /// Joints changed after the model was made (Modify joint, the table: [`crate::joint_edit`]).
    #[serde(default)]
    pub edits: Vec<crate::joint_edit::JointEdit>,
}

/// What was built, with where each wall and joint came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Built {
    pub model: Model,
    /// The input key of each wall.
    pub walls: Vec<(u64, WallId)>,
    /// The input key of each joint (an edge's, or a cylinder's for its bend).
    pub joints: Vec<(u64, JointId)>,
    /// Picked edges whose walls bends already join (they stay rips: pick order decides).
    pub loop_picks: Vec<u64>,
    /// Picked edges or cylinders that aren't between two walls.
    pub stray_picks: Vec<u64>,
    /// Things left out, for the feature's warning.
    pub warnings: Vec<String>,
    /// The definition the model was built from, which later features (Flange, Hem, Make joint:
    /// [`crate::sharp_edit`]) add to.
    pub def: crate::sharp_edit::SharpDef,
}

impl Built {
    pub fn wall_key(&self, w: WallId) -> Option<u64> {
        self.walls.iter().find(|(_, id)| *id == w).map(|(k, _)| *k)
    }

    pub fn joint_key(&self, j: JointId) -> Option<u64> {
        self.joints.iter().find(|(_, id)| *id == j).map(|(k, _)| *k)
    }
}

/// Why the inputs don't make a sheet metal model.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstructError {
    /// Nothing to make walls of.
    NoWalls,
    /// The walls and joints don't fit together (a bend too big for its wall, …), with the input
    /// key of the joint or wall involved if known.
    Build { error: BuildError, key: Option<u64> },
    /// A rolled wall (or a bend from an arc) would have no inside: the thickness is larger than
    /// the radius on the material side.
    RadiusTooSmall { key: u64 },
    /// An arc doesn't run smoothly into the line next to it (only tangent arcs make rolled walls).
    NotTangent { key: u64 },
    /// The extrude has no depth.
    NoDepth,
}

impl ConstructError {
    /// The feature error text (X6).
    pub fn message(&self) -> String {
        match self {
            ConstructError::NoWalls => "Nothing to make sheet metal from".into(),
            ConstructError::Build { error, .. } => error.message(),
            ConstructError::RadiusTooSmall { .. } => "The thickness is larger than an arc's radius on the material side".into(),
            ConstructError::NotTangent { .. } => "An arc must run smoothly into the curves it meets".into(),
            ConstructError::NoDepth => "The extrude has no depth".into(),
        }
    }
}

impl std::fmt::Display for ConstructError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ConstructError {}

// ---------------------------------------------------------------------------------------------
// Joint names

/// Onshape's joint names: one letter sequence for all joints, "Bend A", "Joint B", …
#[derive(Default)]
struct Names(usize);

impl Names {
    fn bend(&mut self) -> String {
        self.0 += 1;
        format!("Bend {}", letters(self.0 - 1))
    }

    fn joint(&mut self) -> String {
        self.0 += 1;
        format!("Joint {}", letters(self.0 - 1))
    }
}

// ---------------------------------------------------------------------------------------------
// Convert and Thicken

/// Union–find over walls (bends join walls into parts).
struct Parts(Vec<usize>);

impl Parts {
    fn new(n: usize) -> Parts {
        Parts((0..n).collect())
    }

    fn find(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut i = i;
        while self.0[i] != r {
            let next = self.0[i];
            self.0[i] = r;
            i = next;
        }
        r
    }

    /// Joins the parts of `a` and `b`; false if they were one part already.
    fn join(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return false;
        }
        self.0[rb] = ra;
        true
    }
}

/// The line where two planes meet, as a point and a unit direction (`None` for parallel planes).
fn plane_line(n1: V3, h1: f64, n2: V3, h2: f64) -> Option<(P3, V3)> {
    let c = n1.dot(&n2);
    let den = 1.0 - c * c;
    if den < 1e-12 {
        return None;
    }
    let c1 = (h1 - c * h2) / den;
    let c2 = (h2 - c * h1) / den;
    Some((P3::from(n1 * c1 + n2 * c2), n1.cross(&n2).normalize()))
}

/// Projects `p` onto the line `(o, d)`.
fn onto_line(p: P3, (o, d): (P3, V3)) -> P3 {
    o + d * (p - o).dot(&d)
}

/// The intersection of the 2D lines through `p` along `d` and through `q` along `e`.
fn meet(p: P2, d: V2, q: P2, e: V2) -> Option<P2> {
    let den = d.perp(&e);
    if den.abs() < 1e-12 * d.norm() * e.norm() {
        return None;
    }
    let t = (q - p).perp(&e) / den;
    Some(p + d * t)
}

/// Whether the segment `a..b` (3D) lies along the loop edge `p..q` of `face`.
fn along(face: &FaceIn, p: P2, q: P2, a: P3, b: P3, tol: f64) -> bool {
    let d = q - p;
    let len = d.norm();
    if len < tol {
        return false;
    }
    let n = V2::new(-d.y, d.x) / len;
    let off = |x: P3| {
        let l = face.local(x);
        let plane = (x - face.origin).dot(&face.normal()).abs();
        ((l - p).dot(&n).abs(), plane, (l - p).dot(&d) / (len * len))
    };
    let (da, pa, ta) = off(a);
    let (db, pb, tb) = off(b);
    da < tol && db < tol && pa < tol && pb < tol && ta.max(tb) > 1e-6 && ta.min(tb) < 1.0 - 1e-6
}

/// One wall's offset outline: each outer edge moved onto the line where its neighbour's plane
/// meets this face's (moved) plane.
struct Laid {
    /// Signed offset along the face's outward normal.
    offset: f64,
}

/// Builds the walls and joints of a Convert or Thicken (see the module docs).
pub fn from_faces(p: Params, faces: &[FaceIn], cyls: &[CylIn], edges: &[EdgeIn], o: &FaceOpts) -> Result<Built, ConstructError> {
    if faces.is_empty() && cyls.is_empty() {
        return Err(ConstructError::NoWalls);
    }
    let t = p.thickness;
    let sign = if o.material_inside { -1.0 } else { 1.0 };
    let size = faces
        .iter()
        .filter_map(|f| f.outline.bounds())
        .map(|(lo, hi)| (hi - lo).norm())
        .fold(1.0, f64::max);
    let tol = 1e-6 * size;
    let normals: Vec<V3> = faces.iter().map(FaceIn::normal).collect();
    let mut out = Built::default();

    // Which picks become bends: edges and cylinders in pick order, unless the walls are joined
    // already (then the pick is left as a rip / rolled wall: the order decides, SM2.2).
    let mut parts = Parts::new(faces.len());
    let mut bend_edges: Vec<usize> = Vec::new();
    let mut bend_cyls: Vec<usize> = Vec::new();
    for key in &o.bends {
        if let Some(i) = edges.iter().position(|e| e.key == *key) {
            let e = &edges[i];
            if (normals[e.faces.0].dot(&normals[e.faces.1])).abs() > 1.0 - 1e-9 {
                out.stray_picks.push(*key);
            } else if parts.join(e.faces.0, e.faces.1) {
                bend_edges.push(i);
            } else {
                out.loop_picks.push(*key);
            }
        } else if let Some(i) = cyls.iter().position(|c| c.key == *key) {
            match cyls[i].neighbours {
                (Some(a), Some(b)) if a != b => {
                    if parts.join(a, b) {
                        bend_cyls.push(i);
                    } else {
                        out.loop_picks.push(*key);
                    }
                }
                _ => out.stray_picks.push(*key),
            }
        } else {
            out.stray_picks.push(*key);
        }
    }

    // Offsets: the clearance on the material side; with Include bends, enough more that the
    // inside of each bend clears the input's edge by the clearance.
    let mut laid: Vec<Laid> = faces.iter().map(|_| Laid { offset: sign * o.clearance }).collect();
    if o.include_bends && !o.material_inside {
        let r = p.bend_radius;
        let c = o.clearance;
        for &i in &bend_edges {
            let e = &edges[i];
            let (fa, fb) = e.faces;
            // Only outside corners: the other face lies behind this one.
            let centre_b = faces[fb].outline.bounds().map(|(lo, hi)| faces[fb].point(P2::from((lo.coords + hi.coords) / 2.0)));
            let convex = centre_b.is_some_and(|cb| (cb - e.a).dot(&normals[fa]) < 0.0);
            if !convex {
                continue;
            }
            let theta = normals[fa].dot(&normals[fb]).clamp(-1.0, 1.0).acos();
            let d = r - (r - c) * (theta / 2.0).cos();
            for f in [fa, fb] {
                laid[f].offset = laid[f].offset.max(d);
            }
        }
    }
    let height = |i: usize| normals[i].dot(&faces[i].origin.coords) + laid[i].offset;
    let moved_origin = |i: usize| faces[i].origin + normals[i] * laid[i].offset;

    // The neighbour across each straight loop edge: by a shared edge, or across a cylinder
    // picked to bend (whose two flat neighbours then meet at their virtual sharp).
    let mut across: Vec<Vec<Option<usize>>> = Vec::with_capacity(faces.len());
    for (fi, f) in faces.iter().enumerate() {
        let l = &f.outline.outer;
        let mut row = vec![None; l.len()];
        for (k, slot) in row.iter_mut().enumerate() {
            let (pk, qk) = (l[k], l[(k + 1) % l.len()]);
            for e in edges {
                let other = if e.faces.0 == fi { e.faces.1 } else if e.faces.1 == fi { e.faces.0 } else { continue };
                if along(f, pk, qk, e.a, e.b, tol) {
                    *slot = Some(other);
                    break;
                }
            }
            if slot.is_none() {
                // An edge where a cylinder picked to bend meets this face: its other neighbour.
                for &ci in &bend_cyls {
                    let c = &cyls[ci];
                    let (Some(a), Some(b)) = c.neighbours else { continue };
                    let other = if a == fi { b } else if b == fi { a } else { continue };
                    // The loop edge runs along the cylinder (parallel to its axis, at its radius).
                    let (pa, pb) = (f.point(pk), f.point(qk));
                    let par = (pb - pa).normalize().cross(&c.axis).norm() < 1e-6;
                    let dist = |x: P3| ((x - c.axis_origin) - c.axis * (x - c.axis_origin).dot(&c.axis)).norm();
                    if par && (dist(pa) - c.radius).abs() < 1e-5 * size.max(c.radius) {
                        *slot = Some(other);
                        break;
                    }
                }
            }
        }
        across.push(row);
    }

    // The walls: each face's outline with its edges moved onto the lines where the planes meet.
    let mut b = SharpBuilder::new(p);
    for (fi, f) in faces.iter().enumerate() {
        let l = &f.outline.outer;
        let n = l.len();
        let origin = moved_origin(fi);
        // Each loop edge as a line in the (moved) face's coordinates.
        let lines: Vec<(P2, V2)> = (0..n)
            .map(|k| {
                let (pk, qk) = (l[k], l[(k + 1) % n]);
                let d = qk - pk;
                if let Some(j) = across[fi][k]
                    && let Some(line) = plane_line(normals[fi], height(fi), normals[j], height(j))
                {
                    let x = line.0;
                    let q = P2::new((x - origin).dot(&f.u), (x - origin).dot(&f.v));
                    // The meeting line runs along the edge; keep the edge's direction.
                    let qn = meet(q, d, pk, V2::new(-d.y, d.x)).unwrap_or(q);
                    return (qn, d);
                }
                (pk, d)
            })
            .collect();
        let pts: Vec<P2> = (0..n)
            .map(|k| {
                let prev = lines[(k + n - 1) % n];
                let cur = lines[k];
                meet(prev.0, prev.1, cur.0, cur.1).unwrap_or(cur.0)
            })
            .collect();
        let (v, outline) = if o.material_inside {
            let flip = |q: &P2| P2::new(q.x, -q.y);
            (-f.v, Polygon::with_holes(pts.iter().map(flip).collect(), f.outline.holes.iter().map(|h| h.iter().map(flip).collect()).collect()))
        } else {
            (f.v, Polygon::with_holes(pts, f.outline.holes.clone()))
        };
        let w = b.wall(origin, f.u, v, outline);
        let id = WallId(stable_id(f.key));
        b.set_wall_id(w, id);
        out.walls.push((f.key, id));
    }

    // The joints, bends first (in pick order), then rips.
    let mut names = Names::default();
    let mut keys: Vec<u64> = Vec::new();
    let edge3 = |e: &EdgeIn| -> Option<(P3, P3)> {
        let (fa, fb) = e.faces;
        let line = plane_line(normals[fa], height(fa), normals[fb], height(fb))?;
        Some((onto_line(e.a, line), onto_line(e.b, line)))
    };
    for &i in &bend_edges {
        let e = &edges[i];
        let Some(edge) = edge3(e) else { continue };
        let j = b.bend(e.faces.0, e.faces.1, edge);
        b.set_joint_id(j, JointId(stable_id(e.key)), Some(names.bend()));
        keys.push(e.key);
    }
    for &ci in &bend_cyls {
        let c = &cyls[ci];
        let (Some(fa), Some(fb)) = c.neighbours else { continue };
        let Some(line) = plane_line(normals[fa], height(fa), normals[fb], height(fb)) else {
            out.stray_picks.push(c.key);
            continue;
        };
        // The bend's inner radius: the cylinder moved out with the walls, less the thickness
        // when the material lies towards the axis.
        let off = laid[fa].offset;
        let def = if c.convex { c.radius + off } else { c.radius - off };
        let away = c.convex != o.material_inside;
        let r = if away { def } else { def - t };
        if r <= 0.0 {
            return Err(ConstructError::RadiusTooSmall { key: c.key });
        }
        let (z0, z1) = c.z;
        let a = onto_line(c.axis_origin + c.axis * z0, line);
        let bb = onto_line(c.axis_origin + c.axis * z1, line);
        let j = b.joint(fa, fb, (a, bb), SharpJointKind::Bend { radius: Some(r), value: None });
        b.set_joint_id(j, JointId(stable_id(c.key)), Some(names.bend()));
        keys.push(c.key);
    }
    for (i, e) in edges.iter().enumerate() {
        if bend_edges.contains(&i) || normals[e.faces.0].dot(&normals[e.faces.1]).abs() > 1.0 - 1e-9 {
            continue;
        }
        let Some(edge) = edge3(e) else { continue };
        let j = b.rip(e.faces.0, e.faces.1, edge, RipStyle::EdgeJoint);
        b.set_joint_id(j, JointId(stable_id(e.key)), Some(names.joint()));
        keys.push(e.key);
    }
    crate::joint_edit::apply(&mut b, &o.edits);
    let mut model = b.build().map_err(|error| {
        let key = match &error {
            BuildError::EdgeNotOnWall { joint, .. }
            | BuildError::NoAngle { joint }
            | BuildError::InconsistentSide { joint }
            | BuildError::ButtNot90 { joint }
            | BuildError::WallTrimmedAway { joint, .. }
            | BuildError::TooSharp { joint } => keys.get(*joint).copied(),
            _ => None,
        };
        ConstructError::Build { error, key }
    })?;
    for (k, j) in keys.iter().zip(&model.joints) {
        out.joints.push((*k, j.id));
    }
    let built = (model.walls.len(), model.joints.len());

    // Cylinders left as they are: rolled walls joined to their flat neighbours by tangent joints.
    for (ci, c) in cyls.iter().enumerate() {
        if bend_cyls.contains(&ci) {
            continue;
        }
        let off = c.neighbours.0.or(c.neighbours.1).map_or(sign * o.clearance, |f| laid[f].offset);
        let def = if c.convex { c.radius + off } else { c.radius - off };
        let material_outside = c.convex != o.material_inside;
        if def <= 0.0 || (!material_outside && def - t <= 0.0) {
            return Err(ConstructError::RadiusTooSmall { key: c.key });
        }
        let id = WallId(stable_id(c.key));
        let surface = Surface::Rolled { axis_origin: c.axis_origin, axis: c.axis, start: c.start, radius: def, material_outside };
        let len = def * c.sweep;
        model.walls.push(Wall { id, surface, outline: Polygon::rect(P2::new(0.0, c.z.0), P2::new(len, c.z.1)) });
        out.walls.push((c.key, id));
        for (end, nb) in [(0.0, c.neighbours.0), (len, c.neighbours.1)] {
            let Some(fi) = nb else { continue };
            let Some(wall) = model.walls.iter().find(|w| w.id == WallId(stable_id(faces[fi].key))) else { continue };
            let (lo, hi) = (surface.point(P2::new(end, c.z.0)), surface.point(P2::new(end, c.z.1)));
            let on_a = Seg2::new(wall.surface.local(lo), wall.surface.local(hi));
            let on_b = Seg2::new(P2::new(end, c.z.0), P2::new(end, c.z.1));
            let jkey = c.key ^ faces[fi].key.rotate_left(17);
            let jid = JointId(stable_id(jkey));
            model.joints.push(Joint { id: jid, name: names.joint(), a: wall.id, b: id, kind: JointKind::Tangent { on_a, on_b } });
            out.joints.push((jkey, jid));
        }
    }
    out.def = crate::sharp_edit::SharpDef::new(b, &model, built.0, built.1);
    out.model = model;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Extrude

/// A piece of a sketch chain, in sketch coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Seg {
    Line { a: P2, b: P2, key: u64 },
    /// From `start` (radians) through `sweep` (counter-clockwise when positive).
    Arc { center: P2, radius: f64, start: f64, sweep: f64, key: u64 },
}

impl Seg {
    pub fn key(&self) -> u64 {
        match self {
            Seg::Line { key, .. } | Seg::Arc { key, .. } => *key,
        }
    }

    /// Unit tangent at the start and at the end, in the direction it runs.
    fn tangents(&self) -> (V2, V2) {
        match *self {
            Seg::Line { a, b, .. } => {
                let d = (b - a).normalize();
                (d, d)
            }
            Seg::Arc { start, sweep, .. } => {
                let s = sweep.signum();
                let t = |a: f64| V2::new(-a.sin(), a.cos()) * s;
                (t(start), t(start + sweep))
            }
        }
    }

    /// Signed turning (radians, counter-clockwise positive) along it.
    fn turning(&self) -> f64 {
        match self {
            Seg::Line { .. } => 0.0,
            Seg::Arc { sweep, .. } => *sweep,
        }
    }
}

/// A chain of sketch curves joined end to start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainIn {
    pub segs: Vec<Seg>,
    pub closed: bool,
}

/// Where and how far the chains are extruded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainOpts {
    /// The sketch plane: origin and orthonormal axes (`x × y` its normal).
    pub origin: P3,
    pub x: V3,
    pub y: V3,
    /// The extrude runs from `z0` to `z1` along the direction (`z0 < z1`).
    pub z0: f64,
    pub z1: f64,
    /// The extrude direction (the sketch normal, or against it).
    pub dir: V3,
    /// The thickness goes to the other side of the curves (the opposite-direction arrow).
    pub flip_side: bool,
    /// Arcs (by key) to make bends of instead of rolled walls.
    pub arcs_as_bends: Vec<u64>,
    /// Joints changed after the model was made (Modify joint, the table: [`crate::joint_edit`]).
    #[serde(default)]
    pub edits: Vec<crate::joint_edit::JointEdit>,
}

enum Piece {
    Line { a: P2, b: P2, key: u64 },
    Arc { center: P2, radius: f64, start: f64, sweep: f64, key: u64 },
}

/// How two consecutive pieces meet.
#[derive(Clone, Copy, PartialEq)]
enum Junction {
    /// A bend of the model's radius, or of this inner radius (an arc extruded as a bend).
    Bend(Option<f64>, u64),
    Tangent,
    Rip,
    /// Nothing (a closed chain's seam between curves).
    Seam,
}

/// Builds the walls and joints of a sheet metal Extrude (see the module docs).
pub fn from_chains(p: Params, chains: &[ChainIn], o: &ChainOpts) -> Result<Built, ConstructError> {
    if o.z1 - o.z0 <= 1e-9 {
        return Err(ConstructError::NoDepth);
    }
    if chains.iter().all(|c| c.segs.is_empty()) {
        return Err(ConstructError::NoWalls);
    }
    let t = p.thickness;
    let n = o.dir.normalize();
    let to3 = |q: P2| o.origin + o.x * q.x + o.y * q.y;
    let to3v = |d: V2| o.x * d.x + o.y * d.y;
    // The sketch normal: arcs turn counter-clockwise about it.
    let sk_n = o.x.cross(&o.y).normalize();
    let mut out = Built::default();
    let mut b = SharpBuilder::new(p);
    let mut names = Names::default();
    let mut bend_keys: Vec<u64> = Vec::new();
    // Rolled walls and tangent joints go in after the builder's walls and bends.
    struct Rolled {
        id: WallId,
        key: u64,
        surface: Surface,
        outline: Polygon,
    }
    let mut rolled: Vec<Rolled> = Vec::new();
    struct Tan {
        key: u64,
        a: (bool, usize),
        b: (bool, usize),
        at: P2,
    }
    let mut tangents: Vec<Tan> = Vec::new();

    for chain in chains {
        if chain.segs.is_empty() {
            continue;
        }
        // The material side: the side the chain turns towards (the inside of a U), else left.
        let turning: f64 = chain.segs.iter().map(Seg::turning).sum::<f64>()
            + chain
                .segs
                .windows(2)
                .map(|w| {
                    let (_, e) = w[0].tangents();
                    let (s, _) = w[1].tangents();
                    e.perp(&s).atan2(e.dot(&s))
                })
                .sum::<f64>();
        let left = (turning >= -1e-9) != o.flip_side;
        // The material side relative to the sketch normal, then the extrude direction: lines'
        // walls take their frame from the extrude direction.
        let left_n = if n.dot(&sk_n) >= 0.0 { left } else { !left };

        // Pieces: arcs picked as bends (between two lines that run into them) become a bend
        // between the lines, extended to where they meet.
        let segs = &chain.segs;
        let m = segs.len();
        let mut pieces: Vec<Piece> = Vec::new();
        let mut junctions: Vec<Junction> = Vec::new();
        // An arc extruded as a bend: picked, between two lines, under a half turn.
        let bend_arc = |i: usize| -> bool {
            matches!(&segs[i], Seg::Arc { key, sweep, .. } if o.arcs_as_bends.contains(key) && sweep.abs() < PI - 1e-6)
                && i > 0
                && i + 1 < m
                && matches!(segs[i - 1], Seg::Line { .. })
                && matches!(segs[i + 1], Seg::Line { .. })
        };
        let mut i = 0;
        while i < m {
            // The last segment this step consumed.
            let mut last = i;
            match &segs[i] {
                Seg::Line { a, b: bb, key } => {
                    pieces.push(Piece::Line { a: *a, b: *bb, key: *key });
                }
                Seg::Arc { center, radius, start, sweep, key } if bend_arc(i) => {
                    let _ = (center, start);
                    // The inner radius: the arc's own when the material is outside it.
                    let ccw = *sweep > 0.0;
                    let outside = left != ccw;
                    let r = if outside { *radius } else { radius - t };
                    if r <= 0.0 {
                        return Err(ConstructError::RadiusTooSmall { key: *key });
                    }
                    let Seg::Line { a: na, b: nb, key: nkey } = segs[i + 1].clone() else { unreachable!() };
                    let Some(Piece::Line { a: pa, b: pb, .. }) = pieces.last_mut() else { unreachable!() };
                    let sharp = meet(*pa, *pb - *pa, na, nb - na).ok_or(ConstructError::NotTangent { key: *key })?;
                    *pb = sharp;
                    junctions.push(Junction::Bend(Some(r), *key));
                    pieces.push(Piece::Line { a: sharp, b: nb, key: nkey });
                    last = i + 1;
                }
                Seg::Arc { center, radius, start, sweep, key } => {
                    if o.arcs_as_bends.contains(key) {
                        out.warnings.push("An arc to extrude as a bend must sit between two lines".into());
                    }
                    pieces.push(Piece::Arc { center: *center, radius: *radius, start: *start, sweep: *sweep, key: *key });
                }
            }
            // How it meets the next segment (the bend of an arc extruded as a bend is pushed with
            // the arc).
            if last + 1 < m && !bend_arc(last + 1) {
                let (_, e) = segs[last].tangents();
                let (s, _) = segs[last + 1].tangents();
                let both_lines = matches!(segs[last], Seg::Line { .. }) && matches!(segs[last + 1], Seg::Line { .. });
                let tangent = e.perp(&s).abs() < 1e-6 && e.dot(&s) > 0.0;
                junctions.push(if both_lines {
                    if tangent { Junction::Seam } else { Junction::Bend(None, 0) }
                } else if tangent {
                    Junction::Tangent
                } else {
                    return Err(ConstructError::NotTangent { key: segs[last + 1].key() });
                });
            }
            i = last + 1;
        }
        // Collinear lines in a row are one wall.
        let mut k = 0;
        while k + 1 < pieces.len() {
            if junctions[k] == Junction::Seam
                && let (Piece::Line { a, .. }, Piece::Line { b: nb, .. }) = (&pieces[k], &pieces[k + 1])
            {
                let (a, nb) = (*a, *nb);
                let key = match &pieces[k] {
                    Piece::Line { key, .. } => *key,
                    _ => unreachable!(),
                };
                pieces[k] = Piece::Line { a, b: nb, key };
                pieces.remove(k + 1);
                junctions.remove(k);
                continue;
            }
            k += 1;
        }
        // The closing junction of a closed chain: a rip between two lines.
        let closing = if chain.closed && pieces.len() > 1 {
            match (pieces.last(), pieces.first()) {
                (Some(Piece::Line { .. }), Some(Piece::Line { .. })) => Junction::Rip,
                _ => Junction::Seam,
            }
        } else {
            Junction::Seam
        };

        // Walls.
        let mut walls: Vec<(bool, usize)> = Vec::new(); // (planar, index in builder or rolled)
        for pc in &pieces {
            match *pc {
                Piece::Line { a, b: bb, key } => {
                    let len = (bb - a).norm();
                    let u = to3v((bb - a) / len.max(1e-300));
                    // Material on the left of the line (seen along the extrude direction's normal):
                    // u × v = n × u, so v = −n; on the right v = n.
                    let (v, y0, y1) = if left_n { (-n, -o.z1, -o.z0) } else { (n, o.z0, o.z1) };
                    let w = b.wall(to3(a), u, v, Polygon::rect(P2::new(0.0, y0), P2::new(len, y1)));
                    let id = WallId(stable_id(key));
                    b.set_wall_id(w, id);
                    out.walls.push((key, id));
                    walls.push((true, w));
                }
                Piece::Arc { center, radius, start, sweep, key } => {
                    let ccw = (sweep > 0.0) == (n.dot(&sk_n) >= 0.0);
                    let axis = if ccw { n } else { -n };
                    let material_outside = left_n != ccw;
                    if !material_outside && radius - t <= 0.0 {
                        return Err(ConstructError::RadiusTooSmall { key });
                    }
                    let st = to3v(V2::new(start.cos(), start.sin()));
                    let len = radius * sweep.abs().min(TAU);
                    let (z0, z1) = if ccw { (o.z0, o.z1) } else { (-o.z1, -o.z0) };
                    let id = WallId(stable_id(key));
                    out.walls.push((key, id));
                    rolled.push(Rolled {
                        id,
                        key,
                        surface: Surface::Rolled { axis_origin: to3(center), axis, start: st, radius, material_outside },
                        outline: Polygon::rect(P2::new(0.0, z0), P2::new(len, z1)),
                    });
                    walls.push((false, rolled.len() - 1));
                }
            }
        }
        // Joints.
        let count = pieces.len();
        let mut joins: Vec<(usize, usize, Junction)> = junctions.iter().enumerate().map(|(k, j)| (k, k + 1, *j)).collect();
        if closing != Junction::Seam {
            joins.push((count - 1, 0, closing));
        }
        for (ka, kb, j) in joins {
            let at = match &pieces[ka] {
                Piece::Line { b: e, .. } => *e,
                Piece::Arc { center, radius, start, sweep, .. } => *center + V2::new((start + sweep).cos(), (start + sweep).sin()) * *radius,
            };
            let edge = (to3(at) + n * o.z0, to3(at) + n * o.z1);
            match j {
                Junction::Bend(radius, arc_key) => {
                    let (Some(&(true, wa)), Some(&(true, wb))) = (walls.get(ka), walls.get(kb)) else { continue };
                    let jx = b.joint(wa, wb, edge, SharpJointKind::Bend { radius, value: None });
                    let key = if arc_key != 0 { arc_key } else { pieces_key(&pieces[ka]).rotate_left(7) ^ pieces_key(&pieces[kb]) };
                    b.set_joint_id(jx, JointId(stable_id(key)), Some(names.bend()));
                    bend_keys.push(key);
                }
                Junction::Rip => {
                    let (Some(&(true, wa)), Some(&(true, wb))) = (walls.get(ka), walls.get(kb)) else { continue };
                    let jx = b.rip(wa, wb, edge, RipStyle::EdgeJoint);
                    let key = pieces_key(&pieces[ka]).rotate_left(7) ^ pieces_key(&pieces[kb]) ^ 0x5249_5000;
                    b.set_joint_id(jx, JointId(stable_id(key)), Some(names.joint()));
                    bend_keys.push(key);
                }
                Junction::Tangent => tangents.push(Tan {
                    key: pieces_key(&pieces[ka]).rotate_left(7) ^ pieces_key(&pieces[kb]),
                    a: walls[ka],
                    b: walls[kb],
                    at,
                }),
                Junction::Seam => {}
            }
        }
    }
    crate::joint_edit::apply(&mut b, &o.edits);
    let mut model = b.build().map_err(|error| {
        let key = match &error {
            BuildError::EdgeNotOnWall { joint, .. }
            | BuildError::NoAngle { joint }
            | BuildError::InconsistentSide { joint }
            | BuildError::ButtNot90 { joint }
            | BuildError::WallTrimmedAway { joint, .. }
            | BuildError::TooSharp { joint } => bend_keys.get(*joint).copied(),
            _ => None,
        };
        ConstructError::Build { error, key }
    })?;
    for (k, j) in bend_keys.iter().zip(&model.joints) {
        out.joints.push((*k, j.id));
    }
    let built = (model.walls.len(), model.joints.len());
    let planar_ids: Vec<WallId> = b.walls.iter().map(|w| w.id.expect("set")).collect();
    for r in &rolled {
        model.walls.push(Wall { id: r.id, surface: r.surface, outline: r.outline.clone() });
        let _ = r.key;
    }
    for tj in tangents {
        let id_of = |(planar, i): (bool, usize)| if planar { planar_ids[i] } else { rolled[i].id };
        let (wa, wb) = (id_of(tj.a), id_of(tj.b));
        let (lo, hi) = (to3(tj.at) + n * o.z0, to3(tj.at) + n * o.z1);
        let seg = |(planar, i): (bool, usize), w: WallId| -> Seg2 {
            if planar {
                let s = model.wall(w).expect("wall").surface;
                Seg2::new(s.local(lo), s.local(hi))
            } else {
                // On a rolled wall: its start or its end (computed, not by atan2, which can
                // wrap a start point round to the end).
                let r = &rolled[i];
                let Surface::Rolled { axis_origin, axis, start, radius, .. } = r.surface else { unreachable!() };
                let len = r.outline.bounds().map_or(0.0, |(_, hi)| hi.x);
                let d = lo - axis_origin;
                let radial = (d - axis * d.dot(&axis)).normalize();
                let s = if radial.dot(&start) > 1.0 - 1e-9 && (len - radius * TAU).abs() > 1e-9 { 0.0 } else { len };
                Seg2::new(P2::new(s, (lo - axis_origin).dot(&axis)), P2::new(s, (hi - axis_origin).dot(&axis)))
            }
        };
        let (on_a, on_b) = (seg(tj.a, wa), seg(tj.b, wb));
        let jid = JointId(stable_id(tj.key));
        model.joints.push(Joint { id: jid, name: names.joint(), a: wa, b: wb, kind: JointKind::Tangent { on_a, on_b } });
        out.joints.push((tj.key, jid));
    }
    out.def = crate::sharp_edit::SharpDef::new(b, &model, built.0, built.1);
    out.model = model;
    Ok(out)
}

fn pieces_key(p: &Piece) -> u64 {
    match p {
        Piece::Line { key, .. } | Piece::Arc { key, .. } => *key,
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::*;
    use crate::flat::flatten;

    fn params() -> Params {
        Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, minimal_gap: 0.2, ..Default::default() }
    }

    /// The six faces of a box `[0,x]×[0,y]×[0,z]` (outward normals) and its twelve edges.
    fn block(x: f64, y: f64, z: f64) -> (Vec<FaceIn>, Vec<EdgeIn>) {
        let f = |key, origin: P3, u: V3, v: V3, w: f64, h: f64| FaceIn { key, origin, u, v, outline: Polygon::rect(P2::origin(), P2::new(w, h)) };
        let faces = vec![
            f(1, P3::new(0.0, y, 0.0), V3::x(), -V3::y(), x, y),                // bottom (−z)
            f(2, P3::new(0.0, 0.0, z), V3::x(), V3::y(), x, y),                 // top (+z)
            f(3, P3::new(0.0, 0.0, 0.0), V3::x(), V3::z(), x, z),               // south (−y)
            f(4, P3::new(x, y, 0.0), -V3::x(), V3::z(), x, z),                  // north (+y)
            f(5, P3::new(0.0, y, 0.0), -V3::y(), V3::z(), y, z),                // west (−x)
            f(6, P3::new(x, 0.0, 0.0), V3::y(), V3::z(), y, z),                 // east (+x)
        ];
        for fc in &faces {
            assert!((fc.normal().norm() - 1.0).abs() < 1e-12);
        }
        let e = |key, a: P3, b: P3, fa, fb| EdgeIn { key, a, b, faces: (fa, fb) };
        let edges = vec![
            e(10, P3::new(0.0, 0.0, 0.0), P3::new(x, 0.0, 0.0), 0, 2),
            e(11, P3::new(0.0, y, 0.0), P3::new(x, y, 0.0), 0, 3),
            e(12, P3::new(0.0, 0.0, 0.0), P3::new(0.0, y, 0.0), 0, 4),
            e(13, P3::new(x, 0.0, 0.0), P3::new(x, y, 0.0), 0, 5),
            e(14, P3::new(0.0, 0.0, z), P3::new(x, 0.0, z), 1, 2),
            e(15, P3::new(0.0, y, z), P3::new(x, y, z), 1, 3),
            e(16, P3::new(0.0, 0.0, z), P3::new(0.0, y, z), 1, 4),
            e(17, P3::new(x, 0.0, z), P3::new(x, y, z), 1, 5),
            e(18, P3::new(0.0, 0.0, 0.0), P3::new(0.0, 0.0, z), 2, 4),
            e(19, P3::new(x, 0.0, 0.0), P3::new(x, 0.0, z), 2, 5),
            e(20, P3::new(0.0, y, 0.0), P3::new(0.0, y, z), 3, 4),
            e(21, P3::new(x, y, 0.0), P3::new(x, y, z), 3, 5),
        ];
        (faces, edges)
    }

    #[test]
    fn a_block_with_nothing_picked_is_six_parts_joined_by_rips() {
        let (faces, edges) = block(100.0, 60.0, 40.0);
        let b = from_faces(params(), &faces, &[], &edges, &FaceOpts::default()).expect("builds");
        assert_eq!(b.model.walls.len(), 6);
        assert_eq!(b.model.joints.len(), 12);
        assert!(b.model.joints.iter().all(|j| matches!(j.kind, JointKind::Rip { .. })));
        assert_eq!(b.model.joints[0].name, "Joint A");
        let f = flatten(&b.model);
        assert!(f.is_ok(), "{:?}", f.errors);
        assert_eq!(f.parts.len(), 6);
    }

    #[test]
    fn picked_edges_bend_in_pick_order_and_a_loop_closing_pick_stays_a_rip() {
        let (faces, edges) = block(100.0, 60.0, 40.0);
        // Bottom to its four sides, then the south–west corner (whose walls are joined already).
        let o = FaceOpts { bends: vec![10, 11, 12, 13, 18], ..Default::default() };
        let b = from_faces(params(), &faces, &[], &edges, &o).expect("builds");
        assert_eq!(b.loop_picks, vec![18]);
        let bends: Vec<&str> = b.model.joints.iter().filter(|j| j.bend().is_some()).map(|j| j.name.as_str()).collect();
        assert_eq!(bends, ["Bend A", "Bend B", "Bend C", "Bend D"]);
        assert!(b.model.validate().is_empty(), "{:?}", b.model.validate());
        let f = flatten(&b.model);
        assert!(f.is_ok(), "{:?}", f.errors);
        // The open box and the separate top.
        assert_eq!(f.parts.len(), 2);
    }

    #[test]
    fn two_pick_orders_give_two_flats() {
        let (faces, edges) = block(100.0, 60.0, 40.0);
        // A: bottom–south, then south–east, then bottom–east (closes a loop: a rip).
        let a = from_faces(params(), &faces, &[], &edges, &FaceOpts { bends: vec![10, 19, 13], ..Default::default() }).unwrap();
        // B: bottom–east first, then bottom–south, then south–east (now the rip).
        let b = from_faces(params(), &faces, &[], &edges, &FaceOpts { bends: vec![13, 10, 19], ..Default::default() }).unwrap();
        assert_eq!(a.loop_picks, vec![13]);
        assert_eq!(b.loop_picks, vec![19]);
        let (fa, fb) = (flatten(&a.model), flatten(&b.model));
        assert!(fa.is_ok() && fb.is_ok());
        let size = |f: &crate::FlatPattern| {
            let part = f.parts.iter().max_by_key(|p| p.walls.len()).unwrap();
            let (lo, hi) = part.bounds().unwrap();
            ((hi.x - lo.x) * 1e3).round() / 1e3 + ((hi.y - lo.y) * 1e3).round() / 1e3 * 1e-4
        };
        assert_ne!(size(&fa), size(&fb));
    }

    #[test]
    fn clearance_moves_the_walls_out_and_keeps_the_sharps() {
        let (faces, edges) = block(100.0, 60.0, 40.0);
        let o = FaceOpts { clearance: 5.0, bends: vec![10], ..Default::default() };
        let b = from_faces(params(), &faces, &[], &edges, &o).expect("builds");
        assert!(b.model.validate().is_empty());
        // The bottom wall is at z = −5 and 110 × 70 (before its bend trims it).
        let bottom = b.model.wall(WallId(stable_id(1))).unwrap();
        let Surface::Planar { origin, .. } = bottom.surface else { panic!() };
        assert!((origin.z + 5.0).abs() < 1e-9);
        let (lo, hi) = bottom.outline.bounds().unwrap();
        assert!(((hi.x - lo.x) - 110.0).abs() < 1e-6, "{lo} {hi}");
    }

    #[test]
    fn include_bends_moves_the_walls_out_by_the_bend_clearance() {
        let (faces, edges) = block(100.0, 60.0, 40.0);
        let o = FaceOpts { include_bends: true, bends: vec![10], ..Default::default() };
        let b = from_faces(params(), &faces, &[], &edges, &o).expect("builds");
        let bottom = b.model.wall(WallId(stable_id(1))).unwrap();
        let Surface::Planar { origin, .. } = bottom.surface else { panic!() };
        let d = 3.0 * (1.0 - (std::f64::consts::FRAC_PI_4).cos());
        assert!((origin.z + d).abs() < 1e-9, "{}", origin.z);
    }

    #[test]
    fn an_l_shaped_part_with_both_inner_edges_bent_collides_flat() {
        // An L-shaped plate (60 × 60 less a 30 × 30 corner) with two walls standing on its
        // inner edges: flat, both walls fold into the notch and overlap.
        let l = Polygon::new(vec![
            P2::new(0.0, 0.0),
            P2::new(60.0, 0.0),
            P2::new(60.0, 30.0),
            P2::new(30.0, 30.0),
            P2::new(30.0, 60.0),
            P2::new(0.0, 60.0),
        ]);
        let faces = vec![
            // The bottom face (outward −z): (x, −y) coordinates.
            FaceIn { key: 1, origin: P3::origin(), u: V3::x(), v: -V3::y(), outline: l.map(|q| P2::new(q.x, -q.y)) },
            // Inner walls on y = 30 (x 30..60) and x = 30 (y 30..60), normals into the notch.
            FaceIn { key: 2, origin: P3::new(30.0, 30.0, 0.0), u: V3::x(), v: -V3::z(), outline: Polygon::rect(P2::new(0.0, -40.0), P2::new(30.0, 0.0)) },
            FaceIn { key: 3, origin: P3::new(30.0, 30.0, 0.0), u: -V3::z(), v: V3::y(), outline: Polygon::rect(P2::new(-40.0, 0.0), P2::new(0.0, 30.0)) },
        ];
        for f in &faces[1..] {
            assert!(f.normal().x > 0.5 || f.normal().y > 0.5);
        }
        let edges = vec![
            EdgeIn { key: 10, a: P3::new(30.0, 30.0, 0.0), b: P3::new(60.0, 30.0, 0.0), faces: (0, 1) },
            EdgeIn { key: 11, a: P3::new(30.0, 30.0, 0.0), b: P3::new(30.0, 60.0, 0.0), faces: (0, 2) },
            EdgeIn { key: 12, a: P3::new(30.0, 30.0, 0.0), b: P3::new(30.0, 30.0, 40.0), faces: (1, 2) },
        ];
        let o = FaceOpts { bends: vec![10, 11], ..Default::default() };
        let b = from_faces(params(), &faces, &[], &edges, &o).expect("builds");
        let f = flatten(&b.model);
        assert!(f.errors.iter().any(|e| matches!(e, crate::FlatError::Collision { .. })), "{:?}", f.errors);
    }

    fn chain_opts(depth: f64) -> ChainOpts {
        ChainOpts { origin: P3::origin(), x: V3::x(), y: V3::y(), z0: 0.0, z1: depth, dir: V3::z(), flip_side: false, arcs_as_bends: vec![], edits: vec![] }
    }

    #[test]
    fn an_open_u_extrudes_into_three_walls_and_two_bends() {
        let segs = vec![
            Seg::Line { a: P2::new(0.0, 30.0), b: P2::new(0.0, 0.0), key: 1 },
            Seg::Line { a: P2::new(0.0, 0.0), b: P2::new(50.0, 0.0), key: 2 },
            Seg::Line { a: P2::new(50.0, 0.0), b: P2::new(50.0, 30.0), key: 3 },
        ];
        let b = from_chains(params(), &[ChainIn { segs, closed: false }], &chain_opts(40.0)).expect("builds");
        assert_eq!(b.model.walls.len(), 3);
        assert_eq!(b.model.joints.iter().filter(|j| j.bend().is_some()).count(), 2);
        assert!(b.model.validate().is_empty(), "{:?}", b.model.validate());
        // The material is inside the U: the bends turn towards it.
        assert!(b.model.joints.iter().all(|j| j.bend().is_some_and(|x| x.toward_material)));
        let f = flatten(&b.model);
        assert!(f.is_ok());
        let (lo, hi) = f.parts[0].bounds().unwrap();
        // Outside sharps 30 + 50 + 30 = 110 along; less 4 outside setbacks, plus 2 allowances.
        let ba = crate::bend::bend_allowance(3.0, 2.0, FRAC_PI_2, 0.45);
        let flat = 110.0 - 4.0 * 5.0 + 2.0 * ba;
        let along = (hi.x - lo.x).max(hi.y - lo.y);
        assert!((along - flat).abs() < 1e-6, "{along} vs {flat}");
    }

    #[test]
    fn an_arc_extrudes_as_a_rolled_wall_or_as_a_bend() {
        // A line, a quarter arc of radius 10 turning left, a line.
        let segs = vec![
            Seg::Line { a: P2::new(0.0, 0.0), b: P2::new(40.0, 0.0), key: 1 },
            Seg::Arc { center: P2::new(40.0, 10.0), radius: 10.0, start: -FRAC_PI_2, sweep: FRAC_PI_2, key: 2 },
            Seg::Line { a: P2::new(50.0, 10.0), b: P2::new(50.0, 40.0), key: 3 },
        ];
        let chain = ChainIn { segs, closed: false };
        let rolled = from_chains(params(), std::slice::from_ref(&chain), &chain_opts(20.0)).expect("rolled");
        assert_eq!(rolled.model.walls.len(), 3);
        assert_eq!(rolled.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Tangent { .. })).count(), 2);
        assert!(rolled.model.validate().is_empty(), "{:?}", rolled.model.validate());
        let f = flatten(&rolled.model);
        assert!(f.is_ok(), "{:?}", f.errors);
        // Material inside the turn (left): the arc's inside is the definition surface's other
        // side, so the neutral radius is 10 − 2 + 0.5·2 = 9.
        let (lo, hi) = f.parts[0].bounds().unwrap();
        let along = (hi.x - lo.x).max(hi.y - lo.y);
        let expect = 40.0 + 30.0 + FRAC_PI_2 * 9.0;
        assert!((along - expect).abs() < 1e-6, "{along} vs {expect}");

        let mut o = chain_opts(20.0);
        o.arcs_as_bends = vec![2];
        let bent = from_chains(params(), &[chain], &o).expect("as a bend");
        assert_eq!(bent.model.walls.len(), 2);
        let bend = bent.model.joints.iter().find_map(|j| j.bend()).expect("a bend");
        // Inner radius: 10 − 2 (the material is inside the arc).
        assert!((bend.radius - 8.0).abs() < 1e-9);
        assert!(bent.model.validate().is_empty(), "{:?}", bent.model.validate());
        let f = flatten(&bent.model);
        let (lo, hi) = f.parts[0].bounds().unwrap();
        let along = (hi.x - lo.x).max(hi.y - lo.y);
        let expect = 40.0 + 30.0 + crate::bend::bend_allowance(8.0, 2.0, FRAC_PI_2, 0.45);
        assert!((along - expect).abs() < 1e-6, "{along} vs {expect}");
    }

    #[test]
    fn a_closed_rectangle_is_ripped_where_it_closes() {
        let p = [P2::new(0.0, 0.0), P2::new(40.0, 0.0), P2::new(40.0, 30.0), P2::new(0.0, 30.0)];
        let segs = (0..4).map(|i| Seg::Line { a: p[i], b: p[(i + 1) % 4], key: 1 + i as u64 }).collect();
        let b = from_chains(params(), &[ChainIn { segs, closed: true }], &chain_opts(20.0)).expect("builds");
        assert_eq!(b.model.joints.iter().filter(|j| j.bend().is_some()).count(), 3);
        assert_eq!(b.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Rip { .. })).count(), 1);
        let f = flatten(&b.model);
        assert!(f.is_ok(), "{:?}", f.errors);
    }

    #[test]
    fn a_tight_arc_on_the_material_side_fails() {
        let segs = vec![Seg::Arc { center: P2::origin(), radius: 1.5, start: 0.0, sweep: PI, key: 9 }];
        let e = from_chains(params(), &[ChainIn { segs, closed: false }], &chain_opts(10.0)).unwrap_err();
        assert_eq!(e, ConstructError::RadiusTooSmall { key: 9 });
    }
}
