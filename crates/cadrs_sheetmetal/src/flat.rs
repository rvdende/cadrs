//! The flat pattern (SM1.2, SM1.5): every part of a [`Model`] laid flat.
//!
//! Each **part** is a set of walls the bends and tangent joints hold together (rips cut). Its
//! first wall (or the model's `fixed` wall) stays put, seen from its material side (the other
//! side with Flip direction up); the solver walks out from it over the joints in table order and
//! lays each next wall flat beside its parent: the parent's tangent line, then the bend region
//! (as wide as the bend allowance), then the child's tangent line. Rolled walls unroll at their
//! neutral radius (rolled K factor) and join their neighbours with no bend region.
//!
//! The result keeps the pieces apart (each wall and each bend region) for highlighting; the
//! bend centre and tangent lines with Up/Down (clipped to the material); the **corners** where
//! two bends of a wall meet; the **relief cuts** ([`crate::relief`]), each tied to the corner or
//! bend end that made it and applied only to the pieces around it, with the material it removes
//! from each piece in that piece's own coordinates (for the folded solid); and the outline: the
//! union of the cut pieces. Two problems are reported rather than hidden: bends that close a loop
//! the sheet can't unfold along, and pieces whose material overlaps once flat ("Collision in sheet
//! metal flat pattern", with the overlap).

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::model::{BendEnd, Joint, JointId, JointKind, Model, WallId};
use crate::params::{BendReliefKind, CornerRelief, CornerReliefKind};
use crate::poly::{self, Affine2, GRID, P2, Polygon, Seg2, V2, inward_normal, perp};
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
    /// What is left of it after the relief cuts (usually one polygon).
    pub cut: Vec<Polygon>,
}

/// A bend's lines in the flat (the dashed centreline and the tangent lines).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatBend {
    pub joint: JointId,
    pub name: String,
    /// The full lines, end to end of the bend.
    pub center: Seg2,
    /// The tangent line on the joint's wall `a`, and on wall `b`.
    pub tangent_a: Seg2,
    pub tangent_b: Seg2,
    /// The parts of the lines over material (relief cuts taken out): what drawings and DXF
    /// show.
    pub center_visible: Vec<Seg2>,
    pub tangent_visible: Vec<Seg2>,
    /// Bends up towards the viewer of the flat pattern.
    pub up: bool,
}

/// A corner: two bends of the same wall whose regions meet (SM7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlatCorner {
    pub bends: (JointId, JointId),
    /// Which end of each bend meets the corner.
    pub ends: (BendEnd, BendEnd),
    /// The wall both bends start from.
    pub wall: WallId,
    /// Where the two bend regions, extended along their bends, cross.
    pub zone: Polygon,
    /// The relief used (the model's or a Corner feature's).
    pub relief: CornerRelief,
}

/// What made a relief cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReliefSource {
    Corner { bends: (JointId, JointId) },
    BendEnd { bend: JointId, end: BendEnd },
    /// P3I.6: the model's `index`-th flat cut (SM14, [`crate::flat_edit`]).
    Flat { index: usize },
}

/// One relief cut.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReliefCut {
    pub source: ReliefSource,
    /// The cut's shapes in the flat (empty for a Tear, which removes no material).
    pub shapes: Vec<Polygon>,
    /// A Tear's slit (zero width) along the bend's end.
    pub slit: Option<Seg2>,
    /// The pieces the cut applies to (only these lose material).
    pub targets: Vec<PieceSource>,
    /// The material removed from each target, in the piece's own coordinates: for a wall, its
    /// local 2D (as [`crate::model::Wall::outline`]); for a bend region, `(s, u)` with `s` along
    /// the bend from its tangent lines' `a` end and `u` across from wall `a`'s tangent line
    /// (0 to the allowance).
    pub removed: Vec<(PieceSource, Vec<Polygon>)>,
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
    pub corners: Vec<FlatCorner>,
    pub cuts: Vec<ReliefCut>,
    /// The flat outline: the cut pieces' union (usually one polygon, with holes for cut-outs).
    pub outline: Vec<Polygon>,
}

impl FlatPart {
    /// Tear reliefs' slits: cut lines with no width, which the outline (a union of material)
    /// can't show. DXF export and drawings must draw these as well as the outline.
    pub fn slits(&self) -> impl Iterator<Item = Seg2> + '_ {
        self.cuts.iter().filter_map(|c| c.slit)
    }

    pub fn placement(&self, w: WallId) -> Option<&Affine2> {
        self.placements.iter().find(|(id, _)| *id == w).map(|(_, m)| m)
    }

    pub fn piece(&self, s: PieceSource) -> Option<&FlatPiece> {
        self.pieces.iter().find(|p| p.source == s)
    }

    pub fn bend(&self, j: JointId) -> Option<&FlatBend> {
        self.bends.iter().find(|b| b.joint == j)
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
    /// Two pieces' material overlaps once flat; `region` is the overlap, for the flat view.
    Collision { a: PieceSource, b: PieceSource, area: f64, region: Vec<Polygon> },
    /// The joint closes a loop of bends that doesn't unfold consistently.
    BendLoop { joint: JointId },
    /// The joint can't be laid flat.
    BadJoint { joint: JointId, problem: JointProblem },
    /// The joint names a wall the model doesn't have.
    MissingWall { joint: JointId },
}

impl FlatError {
    /// The feature error text (X6).
    pub fn message(&self) -> String {
        match self {
            FlatError::Collision { .. } => "Collision in sheet metal flat pattern".into(),
            FlatError::BendLoop { .. } => "The bends close a loop: the sheet can't be laid flat (make one of them a rip)".into(),
            FlatError::BadJoint { problem, .. } => problem.message(),
            FlatError::MissingWall { .. } => "A joint refers to a wall that no longer exists".into(),
        }
    }
}

impl std::fmt::Display for FlatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for FlatError {}

/// Why a joint can't be laid flat.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum JointProblem {
    /// The bend's own deduction on a bend of 180° or more, where deduction has no meaning.
    DeductionUndefined,
    /// The bend's two tangent lines differ in length.
    TangentLengths { a: f64, b: f64 },
    /// The joint's segment isn't on an edge of its first (`false`) or second wall.
    NotOnEdge { second: bool },
    /// The bend's allowance comes out negative (a deduction or K factor too large for this
    /// bend): the bend region would have no width.
    NegativeAllowance { allowance: f64 },
    /// The joint's geometry is degenerate.
    Degenerate,
}

impl JointProblem {
    pub fn message(&self) -> String {
        match self {
            JointProblem::DeductionUndefined => "A bend deduction can't apply to a bend of 180° or more".into(),
            JointProblem::TangentLengths { a, b } => format!("The bend's tangent lines are {a:.4} and {b:.4} long"),
            JointProblem::NotOnEdge { second: false } => "The joint isn't on the edge of its first wall".into(),
            JointProblem::NotOnEdge { second: true } => "The joint isn't on the edge of its second wall".into(),
            JointProblem::NegativeAllowance { allowance } => {
                format!("The bend's allowance is negative ({allowance:.4}): its deduction or K factor is too large for it")
            }
            JointProblem::Degenerate => "The joint is degenerate".into(),
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

/// A wall's outline in its own flat 2D.
fn local_outline(m: &Model, w: WallId) -> Option<Polygon> {
    let wall = m.wall(w)?;
    Some(wall.outline.map(|q| wall.flat_local(&m.params, q)))
}

fn local_seg(m: &Model, w: WallId, s: Seg2) -> Seg2 {
    let wall = m.wall(w).expect("checked");
    Seg2::new(wall.flat_local(&m.params, s.a), wall.flat_local(&m.params, s.b))
}

/// The flat width of a connecting joint (bend allowance; 0 for a tangent joint).
fn joint_width(m: &Model, j: &Joint) -> Result<f64, JointProblem> {
    match &j.kind {
        JointKind::Bend(b) => {
            let w = b.allowance(&m.params).filter(|w| w.is_finite()).ok_or(JointProblem::DeductionUndefined)?;
            if w < 0.0 { Err(JointProblem::NegativeAllowance { allowance: w }) } else { Ok(w) }
        }
        JointKind::Tangent { .. } => Ok(0.0),
        JointKind::Rip { .. } => Err(JointProblem::Degenerate),
    }
}

/// The placement of `child` laid beside `parent` (already placed) across joint `j`.
fn place_child(m: &Model, j: &Joint, parent: WallId, pm: &Affine2, child: WallId) -> Result<Affine2, JointProblem> {
    let width = joint_width(m, j)?;
    let sp = local_seg(m, parent, j.segment_on(parent).expect("joint touches parent"));
    let sc = local_seg(m, child, j.segment_on(child).expect("joint touches child"));
    let tol = 1e-6 * sp.len().max(1.0);
    if (sp.len() - sc.len()).abs() > tol {
        return Err(JointProblem::TangentLengths { a: sp.len(), b: sc.len() });
    }
    let lp = local_outline(m, parent).expect("wall");
    let lc = local_outline(m, child).expect("wall");
    let ip = inward_normal(&lp, sp).ok_or(JointProblem::NotOnEdge { second: parent != j.a })?;
    let ic = inward_normal(&lc, sc).ok_or(JointProblem::NotOnEdge { second: child != j.a })?;
    let fa = pm.apply(sp.a);
    let e_p = pm.apply_vec(sp.dir());
    let out = -pm.apply_vec(ip); // away from the parent, flat
    // L maps the child's edge direction onto the parent's and its "into" onto "away from parent".
    let src = nalgebra::Matrix2::from_columns(&[sc.dir(), ic]);
    let dst = nalgebra::Matrix2::from_columns(&[e_p, out]);
    let l = dst * src.try_inverse().ok_or(JointProblem::Degenerate)?;
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
                if tree.iter().any(|(id, _)| *id == j.id) {
                    continue;
                }
                let other = j.other(w);
                match place_child(m, j, w, &wm, other) {
                    Err(problem) => {
                        if !errors.iter().any(|e| matches!(e, FlatError::BadJoint { joint, .. } if *joint == j.id)) {
                            errors.push(FlatError::BadJoint { joint: j.id, problem });
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
                        } else {
                            placed.insert(other, cm);
                            part.walls.push(other);
                            queue.push_back(other);
                        }
                        tree.push((j.id, w));
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
                    cut: Vec::new(),
                });
            }
        }
        for (jid, parent) in &tree {
            let j = m.joint(*jid).expect("joint");
            let JointKind::Bend(b) = &j.kind else { continue };
            let Some(s) = strip_from(m, j, *parent, &placed) else { continue };
            let t_parent = s.line;
            let t_child = t_parent.offset(s.out * s.width);
            if s.width > 0.0 {
                part.pieces.push(FlatPiece {
                    source: PieceSource::Bend(j.id),
                    polygon: Polygon::new(vec![t_parent.a, t_parent.b, t_child.b, t_child.a]),
                    cut: Vec::new(),
                });
            }
            let (tangent_a, tangent_b) = if *parent == j.a { (t_parent, t_child) } else { (t_child, t_parent) };
            part.bends.push(FlatBend {
                joint: j.id,
                name: j.name.clone(),
                center: t_parent.offset(s.out * (s.width / 2.0)),
                tangent_a,
                tangent_b,
                center_visible: Vec::new(),
                tangent_visible: Vec::new(),
                up: b.toward_material != m.params.flip_direction_up,
            });
        }
        // Keep bends in table order.
        let rank: HashMap<JointId, usize> = m.joints.iter().enumerate().map(|(i, j)| (j.id, i)).collect();
        part.bends.sort_by_key(|b| rank[&b.joint]);
        apply_reliefs(m, &mut part, &tree, &placed);
        // What is left of each piece, the outline, and the visible bend lines.
        for i in 0..part.pieces.len() {
            let src = part.pieces[i].source;
            let shapes: Vec<Polygon> = part
                .cuts
                .iter()
                .filter(|c| c.targets.contains(&src))
                .flat_map(|c| c.shapes.iter().cloned())
                .collect();
            let piece = &part.pieces[i].polygon;
            part.pieces[i].cut = if shapes.is_empty() { vec![piece.clone()] } else { poly::difference(std::slice::from_ref(piece), &shapes) };
        }
        // The booleans round to a 1 nm grid: put their vertices back on the exact points (piece
        // and cut corners, and where their edges cross).
        let exact = exact_points(part.pieces.iter().map(|p| &p.polygon).chain(part.cuts.iter().flat_map(|c| c.shapes.iter())));
        let snap = |p: &Polygon| poly::snap_to(p, &exact, 10.0 * GRID);
        for pc in &mut part.pieces {
            pc.cut = pc.cut.iter().map(snap).collect();
        }
        let all: Vec<Polygon> = part.pieces.iter().flat_map(|p| p.cut.iter().cloned()).collect();
        part.outline = poly::union(&all).iter().map(snap).collect();
        for b in &mut part.bends {
            let region: Vec<Polygon> = part
                .pieces
                .iter()
                .find(|p| p.source == PieceSource::Bend(b.joint))
                .map(|p| p.cut.clone())
                .unwrap_or_default();
            b.center_visible = poly::clip_segment(b.center, &region);
            // A tangent line lies on the region's edge: clip a copy nudged a hair inside the
            // region, then nudge the pieces back.
            let mid = P2::from((b.center.a.coords + b.center.b.coords) / 2.0);
            b.tangent_visible = [b.tangent_a, b.tangent_b]
                .into_iter()
                .flat_map(|t| {
                    let n = perp(t.dir());
                    let nudge = if (mid - t.a).dot(&n) >= 0.0 { n } else { -n } * (10.0 * GRID);
                    poly::clip_segment(t.offset(nudge), &region).into_iter().map(move |s| s.offset(-nudge))
                })
                .collect();
        }
        // Collisions between what is left of the pieces.
        for i in 0..part.pieces.len() {
            for k in i + 1..part.pieces.len() {
                let (pa, pb) = (&part.pieces[i], &part.pieces[k]);
                let rough: f64 = pa.cut.iter().flat_map(|a| pb.cut.iter().map(move |b| poly::overlap_area(a, b))).sum();
                if rough <= 1e-12 {
                    continue;
                }
                let region: Vec<Polygon> = pa.cut.iter().flat_map(|a| pb.cut.iter().flat_map(move |b| poly::intersection(a, b))).map(|r| snap(&r)).collect();
                let area: f64 = region.iter().map(Polygon::area).sum();
                // Pieces that only share an edge leave slivers at most a grid step wide.
                let perim = pa.cut.iter().chain(pb.cut.iter()).map(poly::perimeter).fold(0.0, f64::max);
                if area > 4.0 * GRID * perim + 1e-12 {
                    errors.push(FlatError::Collision {
                        a: pa.source,
                        b: pb.source,
                        area,
                        region,
                    });
                }
            }
        }
        parts.push(part);
    }
    FlatPattern { parts, errors }
}

/// Every vertex of `polys`, and every point where two of their edges cross: the exact points
/// boolean results should land on.
fn exact_points<'a>(polys: impl Iterator<Item = &'a Polygon>) -> Vec<P2> {
    let mut edges: Vec<(P2, P2)> = Vec::new();
    let mut out = Vec::new();
    for p in polys {
        for l in std::iter::once(&p.outer).chain(p.holes.iter()) {
            for i in 0..l.len() {
                edges.push((l[i], l[(i + 1) % l.len()]));
                out.push(l[i]);
            }
        }
    }
    for i in 0..edges.len() {
        let (a, b) = edges[i];
        let d = b - a;
        for &(c, e) in &edges[i + 1..] {
            let f = e - c;
            let den = d.perp(&f);
            if den.abs() < 1e-15 {
                continue;
            }
            let w = c - a;
            let (t, u) = (w.perp(&f) / den, w.perp(&d) / den);
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                out.push(a + d * t);
            }
        }
    }
    out
}

/// Whether two placements of a wall agree (checked on its outline's points).
fn same_map(a: &Affine2, b: &Affine2, m: &Model, w: WallId) -> bool {
    let Some(o) = local_outline(m, w) else { return true };
    let size = o.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
    o.outer.iter().all(|p| (a.apply(*p) - b.apply(*p)).norm() < 1e-6 * size.max(1.0))
}

/// One bend's strip as seen from a wall it joins: the tangent line on that wall (flat), the unit
/// direction away from the wall, and the strip's width.
struct Strip {
    joint: JointId,
    wall: WallId,
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
    let i = inward_normal(&local_outline(m, w)?, s)?;
    Some(Strip {
        joint: j.id,
        wall: w,
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

/// The pieces around a bend: its region and its two walls.
fn bend_pieces(j: &Joint) -> [PieceSource; 3] {
    [PieceSource::Wall(j.a), PieceSource::Bend(j.id), PieceSource::Wall(j.b)]
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
    let mut cuts: Vec<ReliefCut> = Vec::new();
    let mut cornered: HashSet<(JointId, BendEnd)> = HashSet::new();
    let bends: Vec<&Joint> = tree.iter().filter_map(|(id, _)| m.joint(*id)).filter(|j| j.bend().is_some()).collect();
    let piece_poly = |part: &FlatPart, s: PieceSource| part.piece(s).map(|pc| pc.polygon.clone());

    // Corners: two bends of the same wall whose regions cross near one end of each.
    for (i, j1) in bends.iter().enumerate() {
        for j2 in &bends[i + 1..] {
            let Some(w) = [j1.a, j1.b].into_iter().find(|w| *w == j2.a || *w == j2.b) else { continue };
            let (Some(s1), Some(s2)) = (strip_from(m, j1, w, placed), strip_from(m, j2, w, placed)) else {
                continue;
            };
            if s1.out.perp(&s2.out).abs() < 1e-6 {
                continue; // parallel bends never meet
            }
            let corners = [(0.0, 0.0), (s1.width, 0.0), (s1.width, s2.width), (0.0, s2.width)];
            let Some(pts) = corners.iter().map(|(o1, o2)| band_point(&s1, *o1, &s2, *o2)).collect::<Option<Vec<P2>>>() else {
                continue;
            };
            let q = Polygon::new(pts.clone());
            let c = P2::from(pts.iter().fold(V2::zeros(), |acc, p| acc + p.coords) / 4.0);
            // A corner only where each bend region, at its real extent, reaches the zone. It may stop
            // a rip's trim (the thickness plus the minimal gap) short of it, but only where the
            // shared wall stops with it; where the wall carries on past the bend's end, that end
            // needs a bend relief, not a corner.
            let wall_poly = piece_poly(part, PieceSource::Wall(w));
            let probe = 1e-4 * size;
            let end_of = |s: &Strip| -> Option<BendEnd> {
                let ts: Vec<f64> = pts.iter().map(|q| along(s, *q)).collect();
                let (q0, q1) = (ts.iter().copied().fold(f64::INFINITY, f64::min), ts.iter().copied().fold(f64::NEG_INFINITY, f64::max));
                let len = s.line.len();
                let end = if along(s, c) >= len * 0.5 { BendEnd::End } else { BendEnd::Start };
                let (point, outward) = match end {
                    BendEnd::End => (s.line.b, s.line.dir()),
                    BendEnd::Start => (s.line.a, -s.line.dir()),
                };
                let carries_on = wall_poly.as_ref().is_some_and(|wp| wp.contains(point + outward * probe - s.out * probe));
                let slack = if carries_on { 0.0 } else { p.thickness + p.minimal_gap };
                let gap = (q0 - len).max(-q1).max(0.0);
                (gap <= slack + 1e-6 * size).then_some(end)
            };
            let (Some(e1), Some(e2)) = (end_of(&s1), end_of(&s2)) else { continue };
            cornered.insert((s1.joint, e1));
            cornered.insert((s2.joint, e2));
            let relief = m.corner_relief(s1.joint, s2.joint);
            let source = ReliefSource::Corner { bends: (s1.joint, s2.joint) };
            let mut targets: Vec<PieceSource> = bend_pieces(j1).into_iter().chain(bend_pieces(j2)).collect();
            targets.sort();
            targets.dedup();
            let mut shapes = Vec::new();
            // The shared wall's material past both tangent lines would stick into the bends.
            if let Some(wall) = piece_poly(part, PieceSource::Wall(s1.wall)) {
                let left = wall.clip_half_plane(s1.line.a, s1.out).clip_half_plane(s2.line.a, s2.out);
                if left.area() > 1e-12 {
                    shapes.push(left);
                }
            }
            if relief.kind == CornerReliefKind::Closed {
                // Mitre the two bend regions along Q's diagonal, the minimal gap apart. (An
                // approximation of Onshape's closed corner until the folded solid exists.)
                let (d0, d1) = (pts[0], pts[2]);
                let dn = perp((d1 - d0).normalize());
                for s in [&s1, &s2] {
                    let mid = P2::from((s.line.a.coords + s.line.b.coords) / 2.0) + s.out * (s.width / 2.0);
                    let away = if (mid - d0).dot(&dn) >= 0.0 { -dn } else { dn };
                    // Everything on the other side of the diagonal, plus half the gap.
                    let reach = 4.0 * (s1.width + s2.width) + 1.0;
                    let o = d0 - away * (p.minimal_gap / 2.0);
                    let along_d = (d1 - d0).normalize();
                    let half = Polygon::new(vec![o - along_d * reach, o + along_d * reach, o + along_d * reach + away * reach, o - along_d * reach + away * reach]);
                    if let Some(region) = piece_poly(part, PieceSource::Bend(s.joint)) {
                        shapes.extend(poly::intersection(&half, &region));
                    }
                }
            } else {
                shapes.push(q.clone());
                if let Some(extra) = relief::corner_shape(&relief, &q, (s1.width, s2.width)) {
                    shapes.push(extra);
                }
            }
            part.corners.push(FlatCorner {
                bends: (s1.joint, s2.joint),
                ends: (e1, e2),
                wall: s1.wall,
                zone: q,
                relief,
            });
            cuts.push(ReliefCut {
                source,
                shapes,
                slit: None,
                targets,
                removed: Vec::new(),
            });
        }
    }

    // Bend reliefs where one wall carries on past a bend's end.
    for j in &bends {
        let Some((_, parent)) = tree.iter().find(|(id, _)| *id == j.id) else { continue };
        let child = j.other(*parent);
        let (Some(sp), Some(sc)) = (strip_from(m, j, *parent, placed), strip_from(m, j, child, placed)) else {
            continue;
        };
        let (Some(pp), Some(cp)) = (piece_poly(part, PieceSource::Wall(*parent)), piece_poly(part, PieceSource::Wall(child))) else {
            continue;
        };
        let d = 1e-4 * size;
        let dir = sp.line.dir();
        // The parent's segment runs the same way as the bend's own (`a` ends match), so its
        // ends are the bend's Start and End.
        for (end, point, outward) in [(BendEnd::Start, sp.line.a, -dir), (BendEnd::End, sp.line.b, dir)] {
            if cornered.contains(&(j.id, end)) {
                continue;
            }
            let parent_on = pp.contains(point + outward * d - sp.out * d);
            let child_on = cp.contains(point + outward * d + sp.out * (sp.width + d));
            if !parent_on && !child_on {
                continue;
            }
            let relief = m.bend_relief(j.id, end);
            let source = ReliefSource::BendEnd { bend: j.id, end };
            let targets = bend_pieces(j).to_vec();
            if relief.kind == BendReliefKind::Tear {
                // A rip along the bend's end: nothing removed.
                cuts.push(ReliefCut {
                    source,
                    shapes: Vec::new(),
                    slit: Some(Seg2::new(point, point + sp.out * sp.width)),
                    targets,
                    removed: Vec::new(),
                });
                continue;
            }
            let mut shapes = Vec::new();
            if parent_on {
                let f = Frame {
                    origin: point,
                    x: outward,
                    y: sp.out,
                };
                shapes.push(relief::bend_relief_shape(&relief, p, sp.radius, sp.width, &f, size));
            }
            if child_on {
                let f = Frame {
                    origin: point + sp.out * sp.width,
                    x: outward,
                    y: sc.out,
                };
                shapes.push(relief::bend_relief_shape(&relief, p, sp.radius, sp.width, &f, size));
            }
            cuts.push(ReliefCut {
                source,
                shapes,
                slit: None,
                targets,
                removed: Vec::new(),
            });
        }
    }

    // P3I.6: material removed in the flat (SM14).
    cuts.extend(crate::flat_edit::part_cuts(m, part));
    // What each cut removes from each of its pieces, in the piece's own coordinates (snapped to
    // the exact points, mapped the same way).
    let exact = exact_points(part.pieces.iter().map(|p| &p.polygon).chain(cuts.iter().flat_map(|c| c.shapes.iter())));
    for cut in &mut cuts {
        for t in &cut.targets {
            let Some(pc) = part.piece(*t) else { continue };
            let removed: Vec<Polygon> = cut.shapes.iter().flat_map(|s| poly::intersection(s, &pc.polygon)).collect();
            if removed.is_empty() {
                continue;
            }
            let back: Option<Vec<Polygon>> = match t {
                PieceSource::Wall(w) => {
                    let wall = m.wall(*w);
                    let inv = placed.get(w).and_then(Affine2::inverse);
                    wall.zip(inv).map(|(wall, inv)| {
                        let k = wall.flat_scale(p);
                        let back = |q: P2| {
                            let l = inv.apply(q);
                            P2::new(l.x / k, l.y)
                        };
                        let mut local: Vec<P2> = exact.iter().map(|q| back(*q)).collect();
                        local.extend(wall.outline.outer.iter().copied());
                        removed.iter().map(|r| poly::snap_to(&r.map(back), &local, 10.0 * GRID)).collect()
                    })
                }
                PieceSource::Bend(jid) => part.bend(*jid).map(|b| {
                    let e = b.tangent_a.dir();
                    let n = {
                        let n = perp(e);
                        if (b.tangent_b.a - b.tangent_a.a).dot(&n) >= 0.0 { n } else { -n }
                    };
                    let to = |q: P2| P2::new((q - b.tangent_a.a).dot(&e), (q - b.tangent_a.a).dot(&n));
                    let local: Vec<P2> = exact.iter().map(|q| to(*q)).collect();
                    removed.iter().map(|r| poly::snap_to(&r.map(to), &local, 10.0 * GRID)).collect()
                }),
            };
            if let Some(b) = back {
                cut.removed.push((*t, b));
            }
        }
    }
    part.cuts = cuts;
}
