//! The flat pattern (SM1.2, SM1.5): every part of a [`Model`] laid flat.
//!
//! Each **part** is a set of walls the bends and tangent joints hold together (rips cut). Its
//! first wall (or the model's `fixed` wall) stays put, seen from its material side (the other
//! side with Flip direction up); the solver walks out from it over the joints in table order and
//! lays each next wall flat beside its parent: the parent's tangent line, then the bend region
//! (as wide as the bend allowance), then the child's tangent line. Rolled walls unroll at their
//! neutral radius (rolled K factor) and join their neighbours with no bend region.
//!
//! The result keeps the pieces apart (each wall and each bend region) for highlighting, the
//! bend centre and tangent lines with Up/Down, the corner and bend relief cuts
//! ([`crate::relief`]), and the outline: the union of the pieces minus the cuts. Two problems are
//! reported rather than hidden: bends that close a loop the sheet can't unfold along, and pieces
//! that overlap once flat ("Collision in sheet metal flat pattern").

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::model::{BendEnd, Joint, JointId, JointKind, Model, WallId};
use crate::poly::{self, Affine2, P2, Polygon, Seg2, V2, perp};
use crate::relief::{self, Frame};

/// What a flat piece is made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PieceSource {
    Wall(WallId),
    Bend(JointId),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatPiece {
    pub source: PieceSource,
    /// The piece as laid flat, before relief cuts.
    pub polygon: Polygon,
}

/// A bend's lines in the flat (the dashed centreline and the tangent lines).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatBend {
    pub joint: JointId,
    pub name: String,
    pub center: Seg2,
    /// The tangent line on the joint's wall `a`, and on wall `b`.
    pub tangent_a: Seg2,
    pub tangent_b: Seg2,
    /// Bends up towards the viewer of the flat pattern.
    pub up: bool,
}

/// One part laid flat.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FlatPart {
    pub walls: Vec<WallId>,
    /// Each wall's map from its own flat 2D (see [`crate::model::Wall::flat_local`]) into the
    /// flat pattern.
    pub placements: Vec<(WallId, Affine2)>,
    pub pieces: Vec<FlatPiece>,
    pub bends: Vec<FlatBend>,
    /// The corner and bend relief cuts.
    pub cuts: Vec<Polygon>,
    /// The flat outline: the pieces' union minus the cuts (usually one polygon, with holes for
    /// cut-outs).
    pub outline: Vec<Polygon>,
}

impl FlatPart {
    pub fn placement(&self, w: WallId) -> Option<&Affine2> {
        self.placements.iter().find(|(id, _)| *id == w).map(|(_, m)| m)
    }

    pub fn area(&self) -> f64 {
        self.outline.iter().map(Polygon::area).sum()
    }

    /// The outline's bounds.
    pub fn bounds(&self) -> Option<(P2, P2)> {
        self.outline.iter().filter_map(Polygon::bounds).reduce(|(a, b), (c, d)| {
            (P2::new(a.x.min(c.x), a.y.min(c.y)), P2::new(b.x.max(d.x), b.y.max(d.y)))
        })
    }
}

/// Why (part of) the flat pattern is wrong.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FlatError {
    /// Two pieces overlap once flat.
    Collision { a: PieceSource, b: PieceSource, area: f64 },
    /// The joint closes a loop of bends that doesn't unfold consistently.
    BendLoop { joint: JointId },
    /// The bend can't be laid flat (e.g. a deduction on a bend of 180° or more, or tangent
    /// lines of different lengths).
    BadJoint { joint: JointId, reason: String },
    /// The joint names a wall the model doesn't have.
    MissingWall { joint: JointId },
}

impl FlatError {
    /// The feature error text (X6).
    pub fn message(&self) -> String {
        match self {
            FlatError::Collision { .. } => "Collision in sheet metal flat pattern".into(),
            FlatError::BendLoop { .. } => "The bends close a loop: the sheet can't be laid flat (make one of them a rip)".into(),
            FlatError::BadJoint { reason, .. } => reason.clone(),
            FlatError::MissingWall { .. } => "A joint refers to a wall that no longer exists".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FlatPattern {
    pub parts: Vec<FlatPart>,
    pub errors: Vec<FlatError>,
}

impl FlatPattern {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// The part a wall belongs to.
    pub fn part_of(&self, w: WallId) -> Option<&FlatPart> {
        self.parts.iter().find(|p| p.walls.contains(&w))
    }
}

/// Overlap area, relative to the part's size squared, below which two pieces only touch: the
/// polygon booleans (`geo`) snap to an integer grid scaled to the shapes, which leaves slivers of
/// about 1e-9 of the size squared where pieces share an edge.
const OVERLAP_REL: f64 = 1e-7;

/// A wall's outline in its own flat 2D.
fn local_outline(m: &Model, w: WallId) -> Option<Polygon> {
    let wall = m.wall(w)?;
    Some(wall.outline.map(|q| wall.flat_local(&m.params, q)))
}

fn local_seg(m: &Model, w: WallId, s: Seg2) -> Seg2 {
    let wall = m.wall(w).expect("checked");
    Seg2::new(wall.flat_local(&m.params, s.a), wall.flat_local(&m.params, s.b))
}

/// The unit normal of `s` pointing into `poly` (`None` if neither side is inside).
fn into(poly: &Polygon, s: Seg2) -> Option<V2> {
    let n = perp(s.dir());
    let mid = P2::from((s.a.coords + s.b.coords) / 2.0);
    let size = poly.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
    for step in [1e-5, 1e-4, 1e-3] {
        let d = step * size.max(s.len());
        match (poly.contains(mid + n * d), poly.contains(mid - n * d)) {
            (true, false) => return Some(n),
            (false, true) => return Some(-n),
            _ => {}
        }
    }
    None
}

/// The flat width of a connecting joint (bend allowance; 0 for a tangent joint).
fn joint_width(m: &Model, j: &Joint) -> Result<f64, String> {
    match &j.kind {
        JointKind::Bend(b) => b
            .allowance(&m.params)
            .filter(|w| w.is_finite())
            .ok_or_else(|| format!("{}: a bend deduction can't apply to a bend of 180° or more", j.name)),
        JointKind::Tangent { .. } => Ok(0.0),
        JointKind::Rip { .. } => Err("a rip doesn't connect".into()),
    }
}

/// The placement of `child` laid beside `parent` (already placed) across joint `j`.
fn place_child(m: &Model, j: &Joint, parent: WallId, pm: &Affine2, child: WallId) -> Result<Affine2, String> {
    let width = joint_width(m, j)?;
    let sp = local_seg(m, parent, j.segment_on(parent).expect("joint touches parent"));
    let sc = local_seg(m, child, j.segment_on(child).expect("joint touches child"));
    let tol = 1e-6 * sp.len().max(1.0);
    if (sp.len() - sc.len()).abs() > tol.max(1e-6) {
        return Err(format!("{}: its tangent lines are {:.4} and {:.4} long", j.name, sp.len(), sc.len()));
    }
    let lp = local_outline(m, parent).expect("wall");
    let lc = local_outline(m, child).expect("wall");
    let ip = into(&lp, sp).ok_or_else(|| format!("{}: the joint isn't on the edge of its first wall", j.name))?;
    let ic = into(&lc, sc).ok_or_else(|| format!("{}: the joint isn't on the edge of its second wall", j.name))?;
    let fa = pm.apply(sp.a);
    let e_p = pm.apply_vec(sp.dir());
    let out = -pm.apply_vec(ip); // away from the parent, flat
    // L maps the child's edge direction onto the parent's and its "into" onto "away from parent".
    let src = nalgebra::Matrix2::from_columns(&[sc.dir(), ic]);
    let dst = nalgebra::Matrix2::from_columns(&[e_p, out]);
    let l = dst * src.try_inverse().ok_or("degenerate joint")?;
    let origin = fa + out * width;
    Ok(Affine2 {
        m: l,
        t: origin.coords - l * sc.a.coords,
    })
}

/// Lays every part of `m` flat.
pub fn flatten(m: &Model) -> FlatPattern {
    let mut errors = Vec::new();
    let walls: HashSet<WallId> = m.walls.iter().map(|w| w.id).collect();
    let mut usable: Vec<&Joint> = Vec::new();
    for j in &m.joints {
        if !walls.contains(&j.a) || !walls.contains(&j.b) {
            errors.push(FlatError::MissingWall { joint: j.id });
        } else {
            usable.push(j);
        }
    }

    let mut placed: HashMap<WallId, Affine2> = HashMap::new();
    let mut parts = Vec::new();
    // Walls in order, the fixed wall first.
    let mut order: Vec<WallId> = m.walls.iter().map(|w| w.id).collect();
    if let Some(f) = m.fixed.filter(|f| walls.contains(f)) {
        order.retain(|w| *w != f);
        order.insert(0, f);
    }
    let base = if m.params.flip_direction_up {
        Affine2 {
            m: nalgebra::Matrix2::new(-1.0, 0.0, 0.0, 1.0),
            t: V2::zeros(),
        }
    } else {
        Affine2::identity()
    };

    for root in order {
        if placed.contains_key(&root) {
            continue;
        }
        let mut part = FlatPart::default();
        let mut tree: Vec<(JointId, WallId)> = Vec::new(); // (joint, parent)
        placed.insert(root, base);
        part.walls.push(root);
        let mut queue = VecDeque::from([root]);
        while let Some(w) = queue.pop_front() {
            let wm = placed[&w];
            for j in usable.iter().filter(|j| j.connects() && (j.a == w || j.b == w)) {
                let other = j.other(w);
                if tree.iter().any(|(id, _)| *id == j.id) {
                    continue;
                }
                match place_child(m, j, w, &wm, other) {
                    Err(reason) => {
                        if !errors.iter().any(|e| matches!(e, FlatError::BadJoint { joint, .. } if *joint == j.id)) {
                            errors.push(FlatError::BadJoint { joint: j.id, reason });
                        }
                    }
                    Ok(cm) => {
                        if let Some(existing) = placed.get(&other) {
                            // A second path to an already placed wall: it must agree.
                            if part.walls.contains(&other)
                                && !same_map(existing, &cm, m, other)
                                && !errors.iter().any(|e| matches!(e, FlatError::BendLoop { joint } if *joint == j.id))
                            {
                                errors.push(FlatError::BendLoop { joint: j.id });
                            }
                            tree.push((j.id, w));
                        } else {
                            placed.insert(other, cm);
                            part.walls.push(other);
                            tree.push((j.id, w));
                            queue.push_back(other);
                        }
                    }
                }
            }
        }
        // Pieces and bend lines.
        for w in &part.walls {
            let pm = placed[w];
            part.placements.push((*w, pm));
            if let Some(local) = local_outline(m, *w) {
                part.pieces.push(FlatPiece {
                    source: PieceSource::Wall(*w),
                    polygon: local.map(|p| pm.apply(p)),
                });
            }
        }
        for (jid, parent) in &tree {
            let j = m.joint(*jid).expect("joint");
            let JointKind::Bend(b) = &j.kind else { continue };
            let Ok(width) = joint_width(m, j) else { continue };
            let pm = placed[parent];
            let sp = local_seg(m, *parent, j.segment_on(*parent).expect("joint"));
            let Some(ip) = local_outline(m, *parent).and_then(|o| into(&o, sp)) else { continue };
            let t_parent = sp.map(&pm);
            let out = -pm.apply_vec(ip);
            let t_child = t_parent.offset(out * width);
            if width > 0.0 {
                part.pieces.push(FlatPiece {
                    source: PieceSource::Bend(j.id),
                    polygon: Polygon::new(vec![t_parent.a, t_parent.b, t_child.b, t_child.a]),
                });
            }
            let (tangent_a, tangent_b) = if *parent == j.a { (t_parent, t_child) } else { (t_child, t_parent) };
            part.bends.push(FlatBend {
                joint: j.id,
                name: j.name.clone(),
                center: t_parent.offset(out * (width / 2.0)),
                tangent_a,
                tangent_b,
                up: b.toward_material != m.params.flip_direction_up,
            });
        }
        // Keep bends in table order.
        let rank: HashMap<JointId, usize> = m.joints.iter().enumerate().map(|(i, j)| (j.id, i)).collect();
        part.bends.sort_by_key(|b| rank[&b.joint]);
        apply_reliefs(m, &mut part, &tree, &placed);
        // The outline and the collision check on the cut pieces.
        let cut_pieces: Vec<(PieceSource, Vec<Polygon>)> = part
            .pieces
            .iter()
            .map(|p| (p.source, if part.cuts.is_empty() { vec![p.polygon.clone()] } else { poly::difference(std::slice::from_ref(&p.polygon), &part.cuts) }))
            .collect();
        let all: Vec<Polygon> = cut_pieces.iter().flat_map(|(_, v)| v.iter().cloned()).collect();
        part.outline = poly::union(&all);
        let extent = part.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
        let eps = (OVERLAP_REL * extent * extent).max(1e-9);
        for i in 0..cut_pieces.len() {
            for k in i + 1..cut_pieces.len() {
                let area: f64 = cut_pieces[i]
                    .1
                    .iter()
                    .flat_map(|a| cut_pieces[k].1.iter().map(move |b| poly::overlap_area(a, b)))
                    .sum();
                if area > eps {
                    errors.push(FlatError::Collision {
                        a: cut_pieces[i].0,
                        b: cut_pieces[k].0,
                        area,
                    });
                }
            }
        }
        parts.push(part);
    }
    FlatPattern { parts, errors }
}

/// Whether two placements of a wall agree (checked on its outline's points).
fn same_map(a: &Affine2, b: &Affine2, m: &Model, w: WallId) -> bool {
    let Some(o) = local_outline(m, w) else { return true };
    let size = o.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
    o.outer.iter().all(|p| (a.apply(*p) - b.apply(*p)).norm() < 1e-6 * size.max(1.0))
}

/// One bend's strip as seen from a wall it joins: the tangent line on that wall, the unit
/// direction away from the wall, and the strip's width.
struct Strip {
    joint: JointId,
    line: Seg2,
    out: V2,
    width: f64,
    radius: f64,
}

fn strip_from(m: &Model, j: &Joint, w: WallId, placed: &HashMap<WallId, Affine2>) -> Option<Strip> {
    let JointKind::Bend(b) = &j.kind else { return None };
    let width = joint_width(m, j).ok()?;
    let pm = placed.get(&w)?;
    let s = local_seg(m, w, j.segment_on(w)?);
    let i = into(&local_outline(m, w)?, s)?;
    Some(Strip {
        joint: j.id,
        line: s.map(pm),
        out: -pm.apply_vec(i),
        width,
        radius: b.radius,
    })
}

/// Where `(x − line.a)·out = o1` meets the other strip's `= o2`.
fn band_point(a: &Strip, o1: f64, b: &Strip, o2: f64) -> Option<P2> {
    let mtx = nalgebra::Matrix2::new(a.out.x, a.out.y, b.out.x, b.out.y);
    let rhs = nalgebra::Vector2::new(o1 + a.line.a.coords.dot(&a.out), o2 + b.line.a.coords.dot(&b.out));
    mtx.try_inverse().map(|inv| P2::from(inv * rhs))
}

/// Parameter of `p` along a strip's line (0 at `a`, its length at `b`).
fn along(s: &Strip, p: P2) -> f64 {
    (p - s.line.a).dot(&s.line.dir())
}

fn apply_reliefs(m: &Model, part: &mut FlatPart, tree: &[(JointId, WallId)], placed: &HashMap<WallId, Affine2>) {
    let p = &m.params;
    let size = part
        .pieces
        .iter()
        .filter_map(|pc| pc.polygon.bounds())
        .map(|(lo, hi)| (hi - lo).norm())
        .fold(0.0, f64::max)
        .max(1.0);
    let mut cuts: Vec<Polygon> = Vec::new();
    let mut cornered: HashSet<(JointId, BendEnd)> = HashSet::new();
    let bends: Vec<&Joint> = tree.iter().filter_map(|(id, _)| m.joint(*id)).filter(|j| j.bend().is_some()).collect();

    // Corners: two bends of the same wall whose regions cross near both their ends.
    for (i, j1) in bends.iter().enumerate() {
        for j2 in &bends[i + 1..] {
            let shared = [j1.a, j1.b].into_iter().find(|w| *w == j2.a || *w == j2.b);
            let Some(w) = shared else { continue };
            let (Some(s1), Some(s2)) = (strip_from(m, j1, w, placed), strip_from(m, j2, w, placed)) else {
                continue;
            };
            if s1.out.perp(&s2.out).abs() < 1e-6 {
                continue; // parallel bends never meet
            }
            let corners = [(0.0, 0.0), (s1.width, 0.0), (s1.width, s2.width), (0.0, s2.width)];
            let pts: Option<Vec<P2>> = corners.iter().map(|(o1, o2)| band_point(&s1, *o1, &s2, *o2)).collect();
            let Some(pts) = pts else { continue };
            let q = Polygon::new(pts.clone());
            let c = P2::from(pts.iter().fold(V2::zeros(), |acc, p| acc + p.coords) / 4.0);
            let near = 2.0 * (s1.width + s2.width) + 2.0 * p.thickness + p.minimal_gap + 2.0 * s1.radius.max(s2.radius) + 1e-6;
            let end_of = |s: &Strip| -> Option<BendEnd> {
                let t = along(s, c);
                let len = s.line.len();
                if (t - len).abs() <= near && t >= len * 0.5 {
                    Some(BendEnd::End)
                } else if t.abs() <= near && t < len * 0.5 {
                    Some(BendEnd::Start)
                } else {
                    None
                }
            };
            let (Some(e1), Some(e2)) = (end_of(&s1), end_of(&s2)) else { continue };
            cornered.insert((s1.joint, e1));
            cornered.insert((s2.joint, e2));
            let relief = m.corner_relief(s1.joint, s2.joint);
            // The shared wall's material past both tangent lines.
            if let Some(wall_piece) = part.pieces.iter().find(|pc| pc.source == PieceSource::Wall(w)) {
                let left = wall_piece.polygon.clip_half_plane(s1.line.a, s1.out).clip_half_plane(s2.line.a, s2.out);
                if !left.is_empty() && left.area() > 1e-12 {
                    cuts.push(left);
                }
            }
            if relief.kind == crate::params::CornerReliefKind::Closed {
                // Mitre the two bend regions along Q's diagonal.
                let (d0, d1) = (pts[0], pts[2]);
                let dn = perp((d1 - d0).normalize());
                for s in [&s1, &s2] {
                    let mid = P2::from((s.line.a.coords + s.line.b.coords) / 2.0) + s.out * (s.width / 2.0);
                    let keep = if (mid - d0).dot(&dn) >= 0.0 { dn } else { -dn };
                    if let Some(pc) = part.pieces.iter_mut().find(|pc| pc.source == PieceSource::Bend(s.joint)) {
                        pc.polygon = pc.polygon.clip_half_plane(d0, keep);
                    }
                }
            } else {
                cuts.push(q.clone());
                if let Some(extra) = relief::corner_shape(&relief, &q, (s1.width, s2.width)) {
                    cuts.push(extra);
                }
            }
        }
    }

    // Bend reliefs where one wall carries on past a bend's end.
    for j in &bends {
        let Some((_, parent)) = tree.iter().find(|(id, _)| *id == j.id) else { continue };
        let child = j.other(*parent);
        let (Some(sp), Some(sc)) = (strip_from(m, j, *parent, placed), strip_from(m, j, child, placed)) else {
            continue;
        };
        let wall_poly = |w: WallId| part.pieces.iter().find(|pc| pc.source == PieceSource::Wall(w)).map(|pc| pc.polygon.clone());
        let (Some(pp), Some(cp)) = (wall_poly(*parent), wall_poly(child)) else { continue };
        let d = 1e-4 * size;
        let dir = sp.line.dir();
        // The child's tangent line runs the same way as the parent's (offset by the width).
        for (end, point, outward) in [(BendEnd::Start, sp.line.a, -dir), (BendEnd::End, sp.line.b, dir)] {
            if cornered.contains(&(j.id, end)) {
                continue;
            }
            let parent_on = pp.contains(point + outward * d - sp.out * d);
            let child_on = cp.contains(point + outward * d + sp.out * (sp.width + d));
            let relief = m.bend_relief(j.id, end);
            let allowance = sp.width;
            if parent_on {
                let f = Frame {
                    origin: point,
                    x: outward,
                    y: sp.out,
                };
                cuts.push(relief::bend_relief_shape(&relief, p, sp.radius, allowance, &f, size));
            }
            if child_on {
                let f = Frame {
                    origin: point + sp.out * allowance,
                    x: outward,
                    y: sc.out,
                };
                cuts.push(relief::bend_relief_shape(&relief, p, sp.radius, allowance, &f, size));
            }
        }
    }
    part.cuts = cuts;
}
