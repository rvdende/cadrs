//! What the **Sheet metal table and flat view** panel draws and picks (P3I.3; SM1.2–SM1.4,
//! SM13.5), without Bevy:
//!
//! - [`joint_at`]: the bend or rip a point of the folded solid lies on (a picked face's or
//!   edge's point), so a click in the model selects its table row, and a row finds its faces.
//! - [`FlatScene`]: the flat pattern as the flat view shows it, its parts laid side by side:
//!   the pieces (walls and bend regions, after relief cuts), the bend centre and tangent lines
//!   with their labels' spots, and each rip's two edges. [`FlatScene::joint_at`] picks there.
//! - [`Tris`]: triangle meshes: the flat as a thin solid ([`slab`]) and filled outlines for
//!   highlights ([`fill`]).
//! - [`label_spot`]: where a joint's label floats next to the folded model.
//! - Walls (SM1.4's faces): [`wall_at`] finds the wall a point of the folded solid is on,
//!   [`to_flat`] carries such a point into the flat scene (a face's, edge's or vertex's
//!   highlight there), [`FlatScene::wall_at`] and [`FlatScene::part_at`] pick in the flat.

use crate::flat::{FlatPattern, PieceSource};
use crate::model::{JointId, JointKind, Model, P3, V3, WallId};
use crate::poly::{P2, Polygon, Seg2, V2};

// ---------------------------------------------------------------------------------------------
// The folded model

/// How far `p` is from the side face along a joint's edge on one of its walls: the strip from
/// the edge (on the definition surface) through the thickness along the material normal.
fn side_distance(m: &Model, w: crate::model::WallId, s: Seg2, p: P3) -> Option<f64> {
    let wall = m.wall(w)?;
    let (a, b) = (wall.surface.point(s.a), wall.surface.point(s.b));
    let n = wall.surface.normal_at(s.a);
    let t = m.params.thickness;
    let d = b - a;
    let len2 = d.norm_squared();
    if len2 < 1e-18 {
        return None;
    }
    let w = p - a;
    let along = (w.dot(&d) / len2).clamp(0.0, 1.0);
    let h = w.dot(&n).clamp(0.0, t);
    let q = a + d * along + n * h;
    Some((p - q).norm())
}

/// Whether `p` lies in a bend's region (between its radii, along its length, within its
/// sweep), with tolerance `tol`.
fn in_bend(m: &Model, j: JointId, p: P3, tol: f64) -> bool {
    let Some(g) = m.bend_geometry(j) else { return false };
    let span = g.ends.1 - g.ends.0;
    let len = span.norm();
    if len < 1e-12 {
        return false;
    }
    let ax = span / len;
    let w = p - g.ends.0;
    let s = w.dot(&ax);
    if s < -tol || s > len + tol {
        return false;
    }
    let radial = w - ax * s;
    let r = radial.norm();
    if r < g.inner_radius - tol || r > g.outer_radius + tol {
        return false;
    }
    if r < 1e-12 {
        return false;
    }
    // The angle from the start direction, turning about the bend's axis.
    let x = g.start;
    let y = g.axis.cross(&x);
    let phi = radial.dot(&y).atan2(radial.dot(&x));
    let slack = tol / r.max(1e-9);
    let phi = if phi < -slack { phi + std::f64::consts::TAU } else { phi };
    phi >= -slack && phi <= g.sweep + slack
}

/// The bend or rip a point of the folded solid lies on (a bend's faces and edges; a rip's two
/// side faces and their edges). `tol` is in mm.
pub fn joint_at(m: &Model, p: P3, tol: f64) -> Option<JointId> {
    for j in &m.joints {
        match &j.kind {
            JointKind::Bend(_) => {
                if in_bend(m, j.id, p, tol) {
                    return Some(j.id);
                }
            }
            JointKind::Rip { on_a, on_b, .. } => {
                let da = side_distance(m, j.a, *on_a, p).unwrap_or(f64::MAX);
                let db = side_distance(m, j.b, *on_b, p).unwrap_or(f64::MAX);
                if da.min(db) <= tol {
                    return Some(j.id);
                }
            }
            JointKind::Tangent { .. } => {}
        }
    }
    None
}

/// Where a joint's label floats by the folded model: a bend's outside, halfway along it and
/// halfway round; a rip's middle.
pub fn label_spot(m: &Model, j: JointId) -> Option<P3> {
    let joint = m.joint(j)?;
    match &joint.kind {
        JointKind::Bend(_) => {
            let g = m.bend_geometry(j)?;
            let mid = P3::from((g.ends.0.coords + g.ends.1.coords) / 2.0);
            let half = g.sweep / 2.0;
            Some(mid + g.rotate_vec(g.start, half) * g.outer_radius)
        }
        JointKind::Rip { on_a, .. } | JointKind::Tangent { on_a, .. } => {
            let wall = m.wall(joint.a)?;
            let mid = P2::from((on_a.a.coords + on_a.b.coords) / 2.0);
            Some(wall.surface.point(mid) + wall.surface.normal_at(mid) * (m.params.thickness / 2.0))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The flat view

/// A bend as the flat view draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneBend {
    pub joint: JointId,
    pub name: String,
    /// The centre line end to end, and its parts over material (drawn dashed).
    pub center: Seg2,
    pub center_visible: Vec<Seg2>,
    pub tangent_visible: Vec<Seg2>,
    pub up: bool,
    /// The bend region (after relief cuts), for highlighting and picking.
    pub region: Vec<Polygon>,
}

/// A rip (or tangent joint) as the flat view draws it: the two walls' edges.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneJoint {
    pub joint: JointId,
    pub name: String,
    pub edges: Vec<Seg2>,
}

/// The flat pattern laid out for the flat view (flat 2D; the view shows it as the XY plane, the
/// sheet from z = 0 to its thickness, seen from +Z).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatScene {
    /// Every piece's material (walls and bend regions after relief cuts).
    pub pieces: Vec<(PieceSource, Vec<Polygon>)>,
    pub outlines: Vec<Polygon>,
    pub bends: Vec<SceneBend>,
    pub joints: Vec<SceneJoint>,
    /// Tear reliefs' slits.
    pub slits: Vec<Seg2>,
    /// Where the pieces overlap (a collision, SM1.5).
    pub collisions: Vec<Polygon>,
    pub thickness: f64,
    /// How far each flat-pattern part is moved along X (sketches on a part's flat are drawn
    /// with it).
    pub shifts: Vec<V2>,
    /// The flat-pattern part each of [`FlatScene::pieces`] is in (its index in the pattern,
    /// the context's part order).
    pub piece_part: Vec<usize>,
    /// Forms on the flat (SM20.3): each one's outline lines and centermark.
    pub forms: Vec<(Vec<crate::forms::FormLine>, P2)>,
}

impl FlatScene {
    /// The flat pattern's parts side by side along +X, a gap apart, the first where it lies.
    pub fn new(m: &Model, flat: &FlatPattern) -> FlatScene {
        let mut out = FlatScene { thickness: m.params.thickness, ..Default::default() };
        let size = flat
            .parts
            .iter()
            .filter_map(|p| p.bounds())
            .map(|(lo, hi)| (hi - lo).norm())
            .fold(0.0, f64::max);
        let gap = (0.1 * size).max(5.0);
        let mut next_x: Option<f64> = None;
        for part in &flat.parts {
            let Some((lo, hi)) = part.bounds() else {
                out.shifts.push(V2::zeros());
                continue;
            };
            let dx = match next_x {
                None => 0.0,
                Some(x) => x - lo.x,
            };
            next_x = Some(hi.x + dx + gap);
            let shift = V2::new(dx, 0.0);
            out.shifts.push(shift);
            let mv = |q: P2| q + shift;
            for f in &part.forms {
                let lines = f.lines.iter().map(|l| crate::forms::FormLine { points: l.points.iter().map(|q| mv(*q)).collect(), closed: l.closed }).collect();
                out.forms.push((lines, mv(f.center)));
            }
            let mvs = |s: &Seg2| Seg2::new(s.a + shift, s.b + shift);
            for piece in &part.pieces {
                out.pieces.push((piece.source, piece.cut.iter().map(|p| p.map(mv)).collect()));
                out.piece_part.push(out.shifts.len() - 1);
            }
            out.outlines.extend(part.outline.iter().map(|p| p.map(mv)));
            out.slits.extend(part.slits().map(|s| mvs(&s)));
            for b in &part.bends {
                let region = part.piece(PieceSource::Bend(b.joint)).map(|p| p.cut.iter().map(|q| q.map(mv)).collect()).unwrap_or_default();
                out.bends.push(SceneBend {
                    joint: b.joint,
                    name: b.name.clone(),
                    center: mvs(&b.center),
                    center_visible: b.center_visible.iter().map(mvs).collect(),
                    tangent_visible: b.tangent_visible.iter().map(mvs).collect(),
                    up: b.up,
                    region,
                });
            }
            // Rips and tangent joints: each wall's edge where it is placed in this part.
            for j in &m.joints {
                if matches!(j.kind, JointKind::Bend(_)) {
                    continue;
                }
                let mut edges = Vec::new();
                for w in [j.a, j.b] {
                    let (Some(place), Some(wall), Some(s)) = (part.placement(w), m.wall(w), j.segment_on(w)) else { continue };
                    let s = Seg2::new(wall.flat_local(&m.params, s.a), wall.flat_local(&m.params, s.b));
                    edges.push(Seg2::new(mv(place.apply(s.a)), mv(place.apply(s.b))));
                }
                if edges.is_empty() {
                    continue;
                }
                match out.joints.iter_mut().find(|x| x.joint == j.id) {
                    Some(x) => x.edges.extend(edges),
                    None => out.joints.push(SceneJoint { joint: j.id, name: j.name.clone(), edges }),
                }
            }
        }
        // A collision's overlap shows where the parts were laid (one part: no shift).
        for e in &flat.errors {
            if let crate::flat::FlatError::Collision { region, .. } = e {
                out.collisions.extend(region.iter().cloned());
            }
        }
        out
    }

    /// The bounds of everything shown.
    pub fn bounds(&self) -> Option<(P2, P2)> {
        self.outlines
            .iter()
            .chain(self.pieces.iter().flat_map(|(_, v)| v.iter()))
            .filter_map(Polygon::bounds)
            .reduce(|(a, b), (c, d)| (P2::new(a.x.min(c.x), a.y.min(c.y)), P2::new(b.x.max(d.x), b.y.max(d.y))))
    }

    /// The joint at a flat point: a bend region it is in, else a rip edge within `tol`.
    pub fn joint_at(&self, p: P2, tol: f64) -> Option<JointId> {
        if let Some(b) = self.bends.iter().find(|b| b.region.iter().any(|r| r.contains(p))) {
            return Some(b.joint);
        }
        let near = |s: &Seg2| {
            let d = s.b - s.a;
            let l2 = d.norm_squared();
            let t = if l2 > 0.0 { ((p - s.a).dot(&d) / l2).clamp(0.0, 1.0) } else { 0.0 };
            (p - (s.a + d * t)).norm()
        };
        self.joints
            .iter()
            .filter_map(|j| j.edges.iter().map(near).reduce(f64::min).map(|d| (j.joint, d)))
            .filter(|(_, d)| *d <= tol)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(j, _)| j)
    }

    /// Whether a flat point is on material.
    pub fn on_material(&self, p: P2) -> bool {
        self.pieces.iter().any(|(_, v)| v.iter().any(|q| q.contains(p)))
    }

    /// The piece (its index in [`FlatScene::pieces`]) a flat point is on.
    pub fn piece_at(&self, p: P2) -> Option<usize> {
        self.pieces.iter().position(|(_, v)| v.iter().any(|q| q.contains(p)))
    }

    /// The flat-pattern part (its index, the context's part order) a flat point is on.
    pub fn part_at(&self, p: P2) -> Option<usize> {
        self.piece_at(p).and_then(|i| self.piece_part.get(i).copied())
    }

    /// The wall a flat point is on (not a bend region).
    pub fn wall_at(&self, p: P2) -> Option<WallId> {
        self.pieces.iter().find_map(|(s, v)| match s {
            PieceSource::Wall(w) if v.iter().any(|q| q.contains(p)) => Some(*w),
            _ => None,
        })
    }

    /// A wall's material in the flat (for its highlight).
    pub fn wall_region(&self, w: WallId) -> Vec<Polygon> {
        self.pieces.iter().filter(|(s, _)| *s == PieceSource::Wall(w)).flat_map(|(_, v)| v.iter().cloned()).collect()
    }

    /// A part's material in the flat.
    pub fn part_region(&self, part: usize) -> Vec<Polygon> {
        self.pieces.iter().zip(&self.piece_part).filter(|(_, i)| **i == part).flat_map(|((_, v), _)| v.iter().cloned()).collect()
    }

    /// Where a joint's label goes in the flat, and the way it reads off from there (a unit
    /// vector: the label sits on that side of the spot).
    ///
    /// - A bend: across from its centre line's midpoint, just past its tangent line on the side
    ///   with more room before the next bend, so it can only be read as this bend's.
    /// - A rip or tangent joint: at the middle of its first edge, outside the sheet.
    pub fn label_place(&self, j: JointId) -> Option<(P2, V2)> {
        let across = |s: &Seg2| {
            let d = s.dir();
            V2::new(-d.y, d.x)
        };
        if let Some(b) = self.bends.iter().find(|b| b.joint == j) {
            let mid = P2::from((b.center.a.coords + b.center.b.coords) / 2.0);
            let n = across(&b.center);
            // How far the nearest other bend is on each side (its midpoint, across this one).
            let room = |side: V2| {
                self.bends
                    .iter()
                    .filter(|o| o.joint != j)
                    .map(|o| (P2::from((o.center.a.coords + o.center.b.coords) / 2.0) - mid).dot(&side))
                    .filter(|d| *d > 1e-6)
                    .fold(f64::INFINITY, f64::min)
            };
            let side = if room(-n) > room(n) + 1e-6 { -n } else { n };
            // Just past the bend region's edge (its tangent line) on that side.
            let half = b.region.iter().flat_map(|r| r.outer.iter()).map(|q| (q - mid).dot(&side)).fold(0.0, f64::max);
            return Some((mid + side * half, side));
        }
        let x = self.joints.iter().find(|x| x.joint == j)?;
        let s = x.edges.first()?;
        let mid = P2::from((s.a.coords + s.b.coords) / 2.0);
        let n = across(s);
        let eps = (0.02 * s.len()).clamp(0.05, 1.0);
        let side = if self.on_material(mid + n * eps) && !self.on_material(mid - n * eps) { -n } else { n };
        Some((mid, side))
    }
}

/// The wall a point of the folded solid lies on (on one of its two faces, or their edges),
/// within `tol` mm: planar walls only.
pub fn wall_at(m: &Model, p: P3, tol: f64) -> Option<WallId> {
    crate::model_edit::wall_at(m, p, tol).map(|(w, _)| w)
}

/// A flat-scene point on wall `w`, back on the folded model: on the wall's definition face (the
/// inverse of [`to_flat`]).
pub fn from_flat(m: &Model, flat: &FlatPattern, scene: &FlatScene, w: WallId, q: P2) -> Option<P3> {
    let wall = m.wall(w)?;
    let (i, part) = flat.parts.iter().enumerate().find(|(_, x)| x.placement(w).is_some())?;
    let inv = part.placement(w)?.inverse()?;
    let local = inv.apply(q - scene.shifts.get(i).copied().unwrap_or_else(V2::zeros));
    let k = wall.flat_scale(&m.params);
    Some(wall.surface.point(P2::new(local.x / k.max(1e-12), local.y)))
}

/// A wall piece's outline corner or side near a flat point, for picking a model vertex or
/// edge in the flat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutlineHit {
    /// A corner (on wall `.0`, at `.1`).
    Corner(WallId, P2),
    /// A side (on wall `.0`): the point on it nearest the pointer.
    Side(WallId, P2),
}

impl FlatScene {
    /// The wall outline corner within `corner_tol` of `p`, else the side within `side_tol`
    /// (walls only: bend regions are joints).
    pub fn outline_at(&self, p: P2, corner_tol: f64, side_tol: f64) -> Option<OutlineHit> {
        let mut best_corner: Option<(f64, WallId, P2)> = None;
        let mut best_side: Option<(f64, WallId, P2)> = None;
        for (src, polys) in &self.pieces {
            let PieceSource::Wall(w) = src else { continue };
            for poly in polys {
                for l in std::iter::once(&poly.outer).chain(&poly.holes) {
                    let n = l.len();
                    for i in 0..n {
                        let (a, b) = (l[i], l[(i + 1) % n]);
                        let d = (a - p).norm();
                        if d <= corner_tol && best_corner.is_none_or(|(x, ..)| d < x) {
                            best_corner = Some((d, *w, a));
                        }
                        let e = b - a;
                        let l2 = e.norm_squared();
                        if l2 < 1e-18 {
                            continue;
                        }
                        let t = ((p - a).dot(&e) / l2).clamp(0.0, 1.0);
                        let q = a + e * t;
                        let d = (q - p).norm();
                        if d <= side_tol && best_side.is_none_or(|(x, ..)| d < x) {
                            best_side = Some((d, *w, q));
                        }
                    }
                }
            }
        }
        best_corner.map(|(_, w, q)| OutlineHit::Corner(w, q)).or(best_side.map(|(_, w, q)| OutlineHit::Side(w, q)))
    }
}

/// A point of the folded solid on wall `w`, where it lies in the flat scene.
pub fn to_flat(m: &Model, flat: &FlatPattern, scene: &FlatScene, w: WallId, p: P3) -> Option<P2> {
    let wall = m.wall(w)?;
    let (i, part) = flat.parts.iter().enumerate().find(|(_, x)| x.placement(w).is_some())?;
    let place = part.placement(w)?;
    let q = place.apply(wall.flat_local(&m.params, wall.surface.local(p)));
    Some(q + scene.shifts.get(i).copied().unwrap_or_else(V2::zeros))
}

// ---------------------------------------------------------------------------------------------
// Meshes

/// A triangle mesh (f32, for the GPU).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tris {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Tris {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    fn append(&mut self, o: Tris) {
        let base = self.positions.len() as u32;
        self.positions.extend(o.positions);
        self.normals.extend(o.normals);
        self.indices.extend(o.indices.into_iter().map(|i| i + base));
    }
}

/// A polygon (with holes) filled at height `z`, facing `+Z` (or `-Z` when `up` is false).
fn face(p: &Polygon, z: f64, up: bool) -> Tris {
    let mut flat: Vec<f64> = Vec::new();
    let mut holes: Vec<usize> = Vec::new();
    for q in &p.outer {
        flat.extend([q.x, q.y]);
    }
    for h in &p.holes {
        holes.push(flat.len() / 2);
        for q in h {
            flat.extend([q.x, q.y]);
        }
    }
    let Ok(idx) = earcutr::earcut(&flat, &holes, 2) else { return Tris::default() };
    let n = if up { [0.0, 0.0, 1.0] } else { [0.0, 0.0, -1.0] };
    let positions: Vec<[f32; 3]> = flat.chunks(2).map(|c| [c[0] as f32, c[1] as f32, z as f32]).collect();
    let normals = vec![n; positions.len()];
    let mut indices: Vec<u32> = idx.into_iter().map(|i| i as u32).collect();
    if !up {
        for t in indices.chunks_mut(3) {
            t.swap(1, 2);
        }
    }
    // earcut gives either winding: make the triangles face their normal.
    for t in indices.chunks_mut(3) {
        let (a, b, c) = (positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]);
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if (cross > 0.0) != up {
            t.swap(1, 2);
        }
    }
    Tris { positions, normals, indices }
}

/// Polygons filled at height `z`, facing up.
pub fn fill(polys: &[Polygon], z: f64) -> Tris {
    let mut out = Tris::default();
    for p in polys {
        out.append(face(p, z, true));
    }
    out
}

/// Polygons as a solid from `z0` to `z1`: top, bottom and side walls.
pub fn slab(polys: &[Polygon], z0: f64, z1: f64) -> Tris {
    let mut out = Tris::default();
    for p in polys {
        out.append(face(p, z1, true));
        out.append(face(p, z0, false));
        for l in std::iter::once(&p.outer).chain(&p.holes) {
            let n = l.len();
            for i in 0..n {
                let (a, b) = (l[i], l[(i + 1) % n]);
                let d = b - a;
                if d.norm() < 1e-12 {
                    continue;
                }
                // Outer loops run counter-clockwise, holes clockwise: outward is to the right.
                let nrm = V2::new(d.y, -d.x).normalize();
                let base = out.positions.len() as u32;
                let nn = [nrm.x as f32, nrm.y as f32, 0.0];
                for (q, z) in [(a, z0), (b, z0), (b, z1), (a, z1)] {
                    out.positions.push([q.x as f32, q.y as f32, z as f32]);
                    out.normals.push(nn);
                }
                out.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
    out
}

/// A unit vector's components as f32.
pub fn f32v(v: V3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flat::flatten;
    use crate::samples;

    #[test]
    fn a_bend_is_found_from_points_on_it_and_not_off_it() {
        let m = samples::l_bracket(crate::Params::default(), true).unwrap();
        let j = m.joints.iter().find(|j| j.bend().is_some()).unwrap().id;
        let g = m.bend_geometry(j).unwrap();
        let mid = P3::from((g.ends.0.coords + g.ends.1.coords) / 2.0);
        let r = (g.inner_radius + g.outer_radius) / 2.0;
        let inside = mid + g.rotate_vec(g.start, g.sweep / 3.0) * r;
        assert_eq!(joint_at(&m, inside, 1e-3), Some(j));
        let outer = mid + g.rotate_vec(g.start, g.sweep / 2.0) * g.outer_radius;
        assert_eq!(joint_at(&m, outer, 1e-3), Some(j));
        assert_eq!(label_spot(&m, j).map(|p| (p - outer).norm() < 1e-9), Some(true));
        // Well off the bend.
        let off = mid + g.rotate_vec(g.start, g.sweep / 2.0) * (g.outer_radius + 5.0);
        assert_eq!(joint_at(&m, off, 1e-3), None);
    }

    #[test]
    fn the_flat_scene_has_every_bend_and_rip_and_a_solid_mesh() {
        let m = samples::open_box(crate::Params::default(), crate::RipStyle::EdgeJoint).unwrap();
        let flat = flatten(&m);
        let s = FlatScene::new(&m, &flat);
        let bends = m.joints.iter().filter(|j| j.bend().is_some()).count();
        let others = m.joints.len() - bends;
        assert_eq!(s.bends.len(), bends);
        assert_eq!(s.joints.len(), others);
        for x in &s.joints {
            assert_eq!(x.edges.len(), 2, "{}: a rip has an edge on each wall", x.name);
        }
        // A point in each bend region picks it.
        for b in &s.bends {
            let c = P2::from((b.center.a.coords + b.center.b.coords) / 2.0);
            assert_eq!(s.joint_at(c, 0.1), Some(b.joint), "{}", b.name);
        }
        // The slab's top and bottom are the outline's area each.
        let mesh = slab(&s.outlines, 0.0, m.params.thickness);
        let area = |t: &Tris, z: f32| -> f64 {
            t.indices
                .chunks(3)
                .filter(|c| c.iter().all(|i| (t.positions[*i as usize][2] - z).abs() < 1e-6) && t.normals[c[0] as usize][2] > 0.5)
                .map(|c| {
                    let (a, b, d) = (t.positions[c[0] as usize], t.positions[c[1] as usize], t.positions[c[2] as usize]);
                    (((b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0])) / 2.0) as f64
                })
                .sum()
        };
        let want: f64 = s.outlines.iter().map(Polygon::area).sum();
        let got = area(&mesh, m.params.thickness as f32);
        assert!((got - want).abs() < 1e-3 * want, "top area {got} vs outline {want}");
    }

    #[test]
    fn walls_map_into_their_flat_pieces_and_parts_are_picked_there() {
        let m = samples::open_box(crate::Params::default(), crate::RipStyle::EdgeJoint).unwrap();
        let flat = flatten(&m);
        let s = FlatScene::new(&m, &flat);
        assert_eq!(s.piece_part.len(), s.pieces.len());
        for w in &m.walls {
            // A point well inside the wall's outline, on its definition face.
            let (lo, hi) = w.outline.bounds().unwrap();
            let c = P2::from((lo.coords + hi.coords) / 2.0);
            if !w.outline.contains(c) {
                continue;
            }
            let p = w.surface.point(c);
            assert_eq!(wall_at(&m, p, 1e-3), Some(w.id), "{:?}", w.id);
            let q = to_flat(&m, &flat, &s, w.id, p).expect("in the flat");
            assert_eq!(s.wall_at(q), Some(w.id), "{:?} lands on its own piece", w.id);
            let part = s.part_at(q).expect("on a part");
            assert!(flat.parts[part].walls.contains(&w.id));
            assert!(!s.wall_region(w.id).is_empty());
            assert!(s.part_region(part).iter().any(|r| r.contains(q)));
        }
    }

    #[test]
    fn labels_sit_beside_their_bend_and_outside_the_sheet_for_rips() {
        let m = samples::open_box(crate::Params::default(), crate::RipStyle::EdgeJoint).unwrap();
        let flat = flatten(&m);
        let s = FlatScene::new(&m, &flat);
        for b in &s.bends {
            let (at, dir) = s.label_place(b.joint).unwrap();
            let mid = P2::from((b.center.a.coords + b.center.b.coords) / 2.0);
            assert!(dir.dot(&b.center.dir()).abs() < 1e-9, "{}: across the line", b.name);
            assert!((at - mid).dot(&b.center.dir()).abs() < 1e-9, "{}: by the centre line's middle", b.name);
            assert!((at - mid).norm() > 0.5, "{}: past the bend region", b.name);
        }
        for x in &s.joints {
            let (at, dir) = s.label_place(x.joint).unwrap();
            assert!(!s.on_material(at + dir * 0.5), "{}: off the sheet", x.name);
        }
    }

    #[test]
    fn flat_points_map_back_onto_their_wall() {
        let m = samples::open_box(crate::Params::default(), crate::RipStyle::EdgeJoint).unwrap();
        let flat = flatten(&m);
        let s = FlatScene::new(&m, &flat);
        for w in &m.walls {
            let Some(&q) = w.outline.outer.first() else { continue };
            let p = w.surface.point(q);
            let f = to_flat(&m, &flat, &s, w.id, p).unwrap();
            let back = from_flat(&m, &flat, &s, w.id, f).unwrap();
            assert!((back - p).norm() < 1e-9, "{:?}", w.id);
            // A piece's corner is picked as a corner, a point along a side as a side.
            let piece = &s.wall_region(w.id)[0].outer;
            let (a, b) = (piece[0], piece[1]);
            assert!(matches!(s.outline_at(a, 0.01, 0.01), Some(OutlineHit::Corner(_, c)) if (c - a).norm() < 1e-9));
            let mid = P2::from((a.coords + b.coords) / 2.0);
            assert!(matches!(s.outline_at(mid, 1e-6, 0.01), Some(OutlineHit::Side(_, q)) if (q - mid).norm() < 1e-9));
        }
    }
}
