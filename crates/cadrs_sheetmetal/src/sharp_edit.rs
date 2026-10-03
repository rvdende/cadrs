//! The sheet metal features that come after a Sheet metal model and **edit its definition**
//! (P3I.4; `reference/onshape/sheetmetal/simultaneous-sheet-metal.md` SM1.6, SM3, SM4, SM6):
//! **Flange**, **Hem** and **Make joint**.
//!
//! A model keeps the definition it was built from as a [`SharpDef`]: its walls as they reach
//! their virtual sharps (a [`SharpBuilder`]) plus the walls and joints added after the build
//! (rolled walls and their tangent joints). A later feature adds to that definition and the
//! model is built again, so the builder trims every wall for every bend and rip at once:
//!
//! - **Flange** ([`flange`]): each picked free edge's wall is moved to the flange's virtual sharp
//!   (by the alignment: Inner, Outer, Middle or Hold line), and a new wall reaching that sharp
//!   joins it with a bend. Flanges of the same feature that meet at a corner are mitred: a rip
//!   (edge joint) where their planes meet, or, for flanges in one plane, a cut along the corner's
//!   bisector.
//! - **Hem** ([`hem`]): Straight (180°), Rolled (a bend past 180° with no flat leg to speak of)
//!   or Tear drop (a bend past 180° and a straight leg back towards the wall, ending the gap
//!   away from it), with Outer or In place alignment and Simple or Closed corners.
//! - **Make joint** ([`make_joint`]): two walls' picked edges carried on (or cut back) to where
//!   the walls' planes meet, joined by a rip or a bend.
//!
//! Picks arrive as 3D points (an edge's polyline or a side face's boundary); [`locate`] finds the
//! wall edge they lie along. Lengths are millimetres, angles radians.

// NaN-safe checks: `!(x > 0.0)` is true for NaN too, which is what they mean.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde::{Deserialize, Serialize};

use crate::construct::stable_id;
use crate::model::{
    BuildError, HemAlignment, HemClip, Joint, JointId, JointKind, Model, P3, RipStyle, SharpBuilder, SharpJointKind, SharpWall, Surface, V3,
    Wall, WallId, letters, shift_edge,
};
use crate::poly::{P2, Polygon, Seg2, V2, inward_normal, perp};

/// A rolled hem has no flat leg; the builder needs a wall after every bend, so it gets one this
/// long (mm): too short to see or to change a flat length that matters.
pub const ROLLED_TAIL: f64 = 1e-3;

/// A model's definition as the features built it (see the module docs).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SharpDef {
    pub builder: SharpBuilder,
    /// Walls and joints added after the build (rolled walls and their tangent joints).
    pub extra_walls: Vec<Wall>,
    pub extra_joints: Vec<Joint>,
}

impl SharpDef {
    /// The definition of `built`, which `builder` built and which then had walls and joints
    /// added after its first `walls` walls and `joints` joints.
    pub fn new(builder: SharpBuilder, built: &Model, walls: usize, joints: usize) -> SharpDef {
        SharpDef {
            builder,
            extra_walls: built.walls.get(walls..).map(<[Wall]>::to_vec).unwrap_or_default(),
            extra_joints: built.joints.get(joints..).map(<[Joint]>::to_vec).unwrap_or_default(),
        }
    }

    /// Adds another definition's walls, joints and hems after these (an Extrude of several
    /// sketches builds one definition per sketch into one model).
    pub fn merge(&mut self, other: SharpDef) {
        let (nw, seq) = (self.builder.walls.len(), self.builder.seq);
        self.builder.walls.extend(other.builder.walls);
        self.builder.joints.extend(other.builder.joints.into_iter().map(|mut j| {
            j.a += nw;
            j.b += nw;
            j.seq += seq;
            j
        }));
        self.builder.hems.extend(other.builder.hems.into_iter().map(|mut h| {
            h.wall += nw;
            h.seq += seq;
            h
        }));
        self.builder.seq += other.builder.seq;
        self.extra_walls.extend(other.extra_walls);
        self.extra_joints.extend(other.extra_joints);
    }

    /// The model.
    pub fn build(&self) -> Result<Model, BuildError> {
        let mut m = self.builder.build()?;
        m.walls.extend(self.extra_walls.iter().cloned());
        m.joints.extend(self.extra_joints.iter().cloned());
        Ok(m)
    }

    /// The next letter of the joint names ("Bend A", "Joint B", …: one sequence, SM13.1).
    pub fn next_letter(&self) -> usize {
        let names = self
            .builder
            .joints
            .iter()
            .filter_map(|j| j.name.as_deref())
            .chain(self.builder.hems.iter().filter_map(|h| h.name.as_deref()))
            .chain(self.extra_joints.iter().map(|j| j.name.as_str()));
        names.filter_map(letter_index).map(|i| i + 1).max().unwrap_or(0)
    }

    fn sharp_index(&self, id: WallId) -> Option<usize> {
        self.builder.walls.iter().position(|w| w.id == Some(id))
    }
}

/// The letter index of a name like "Bend AB" (`None` for "Rip 1" or a name without letters).
fn letter_index(name: &str) -> Option<usize> {
    let last = name.rsplit(' ').next()?;
    if last.is_empty() || !last.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let mut i = 0usize;
    for b in last.bytes() {
        i = i * 26 + (b - b'A') as usize + 1;
    }
    Some(i - 1)
}

/// A new joint name with the next letter.
fn next_name(kind: &str, letter: &mut usize) -> String {
    *letter += 1;
    format!("{kind} {}", letters(*letter - 1))
}

// ---------------------------------------------------------------------------------------------
// Picks

/// Which face of the sheet a picked edge lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PickSide {
    /// The definition surface's face (the wall's own plane).
    Definition,
    /// The face a thickness away, on the material side.
    Material,
    /// A side face through the thickness.
    Side,
}

/// A picked free edge of a planar wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgePick {
    pub wall: WallId,
    /// The stretch picked, on the definition surface.
    pub a: P3,
    pub b: P3,
    pub side: PickSide,
    /// A joint already runs along it (it isn't free).
    pub joined: bool,
}

/// The planar wall edge that the points of a picked edge or side face lie along (`None` if they
/// don't lie along one).
pub fn locate(model: &Model, pts: &[P3]) -> Option<EdgePick> {
    let t = model.params.thickness;
    let size = model.walls.iter().filter_map(|w| w.outline.bounds()).map(|(lo, hi)| (hi - lo).norm()).fold(1.0, f64::max);
    let tol = 1e-5 * size.max(1.0);
    for w in &model.walls {
        let Surface::Planar { origin, .. } = w.surface else { continue };
        let n = w.surface.normal().expect("planar");
        let hs: Vec<f64> = pts.iter().map(|p| (p - origin).dot(&n)).collect();
        if hs.iter().any(|h| *h < -tol || *h > t + tol) {
            continue;
        }
        let qs: Vec<P2> = pts.iter().map(|p| w.surface.local(*p)).collect();
        let l = &w.outline.outer;
        for k in 0..l.len() {
            let (p, q) = (l[k], l[(k + 1) % l.len()]);
            let d = q - p;
            let len = d.norm();
            if len < tol {
                continue;
            }
            let dir = d / len;
            let nn = perp(dir);
            let on = qs.iter().all(|x| (x - p).dot(&nn).abs() <= tol && (x - p).dot(&dir) >= -tol && (x - p).dot(&dir) <= len + tol);
            if !on {
                continue;
            }
            let (lo, hi) = qs.iter().map(|x| (x - p).dot(&dir).clamp(0.0, len)).fold((f64::MAX, f64::MIN), |(a, b), s| (a.min(s), b.max(s)));
            if hi - lo < tol {
                continue;
            }
            let side = if hs.iter().all(|h| h.abs() <= tol) {
                PickSide::Definition
            } else if hs.iter().all(|h| (h - t).abs() <= tol) {
                PickSide::Material
            } else {
                PickSide::Side
            };
            let seg = Seg2::new(p + dir * lo, p + dir * hi);
            let joined = model.joints.iter().any(|j| j.segment_on(w.id).is_some_and(|s| overlaps(s, seg, tol)));
            return Some(EdgePick { wall: w.id, a: w.surface.point(seg.a), b: w.surface.point(seg.b), side, joined });
        }
    }
    None
}

/// Whether two segments lie on one line and overlap by more than `tol`.
fn overlaps(s: Seg2, e: Seg2, tol: f64) -> bool {
    if e.len() < tol || s.len() < tol {
        return false;
    }
    let dir = e.dir();
    let nn = perp(dir);
    if (s.a - e.a).dot(&nn).abs() > tol || (s.b - e.a).dot(&nn).abs() > tol {
        return false;
    }
    let (s0, s1) = ((s.a - e.a).dot(&dir), (s.b - e.a).dot(&dir));
    let (lo, hi) = (s0.min(s1).max(0.0), s0.max(s1).min(e.len()));
    hi - lo > tol
}

/// A picked edge on its wall's sharp outline.
#[derive(Clone, Copy, Debug)]
struct SharpEdge {
    /// The wall's index in the builder.
    wall: usize,
    /// The whole edge of the sharp outline the pick lies on (local 2D), and its unit normal into
    /// the wall.
    seg: Seg2,
    into: V2,
    /// The same in 3D: the edge's start, unit direction, the normal into the wall and the
    /// wall's material normal.
    origin: P3,
    e: V3,
    into3: V3,
    n: V3,
    /// The picked stretch: lengths along the edge from its start.
    pick: (f64, f64),
    /// The pick runs against the edge (its `a` nearer the edge's end).
    reversed: bool,
}

impl SharpEdge {
    fn len(&self) -> f64 {
        self.seg.len()
    }

    /// The point `s` along the edge, carried `x` out past it.
    fn at(&self, s: f64, x: f64) -> P3 {
        self.origin + self.e * s - self.into3 * x
    }
}

fn sharp_edge(def: &SharpDef, pick: &EdgePick) -> Result<SharpEdge, String> {
    let wi = def.sharp_index(pick.wall).ok_or("Only flat walls can take this feature")?;
    let w = &def.builder.walls[wi];
    let surf = Surface::Planar { origin: w.origin, u: w.u, v: w.v };
    let (qa, qb) = (surf.local(pick.a), surf.local(pick.b));
    let size = w.outline.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0).max(1.0);
    let tol = 1e-5 * size;
    let l = &w.outline.outer;
    for k in 0..l.len() {
        let (p, q) = (l[k], l[(k + 1) % l.len()]);
        let seg = Seg2::new(p, q);
        if seg.len() < tol {
            continue;
        }
        let dir = seg.dir();
        let nn = perp(dir);
        if (qa - p).dot(&nn).abs() > tol || (qb - p).dot(&nn).abs() > tol {
            continue;
        }
        let (s0, s1) = ((qa - p).dot(&dir), (qb - p).dot(&dir));
        let (lo, hi) = (s0.min(s1), s0.max(s1));
        if lo < -tol || hi > seg.len() + tol {
            continue;
        }
        let into = inward_normal(&w.outline, seg).ok_or("The edge isn't on the wall's boundary")?;
        let n = w.u.cross(&w.v).normalize();
        let e = (w.u * dir.x + w.v * dir.y).normalize();
        let into3 = (w.u * into.x + w.v * into.y).normalize();
        return Ok(SharpEdge { wall: wi, seg, into, origin: surf.point(p), e, into3, n, pick: (lo.max(0.0), hi.min(seg.len())), reversed: s0 > s1 });
    }
    Err("The edge isn't on the wall's boundary".into())
}

/// The frame of a picked edge, for features that need its geometry (a flange's direction, an
/// Up to entity distance): the picked stretch on the definition surface, its direction, the
/// direction out of the wall across it and the wall's material normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeFrame {
    pub a: P3,
    pub b: P3,
    pub e: V3,
    pub out: V3,
    pub n: V3,
    pub thickness: f64,
}

impl EdgeFrame {
    pub fn of(model: &Model, pick: &EdgePick) -> Option<EdgeFrame> {
        let w = model.wall(pick.wall)?;
        let n = w.surface.normal()?;
        let e = (pick.b - pick.a).try_normalize(1e-12)?;
        let mid = w.surface.local(P3::from((pick.a.coords + pick.b.coords) / 2.0));
        let d = w.surface.local(pick.a + e) - w.surface.local(pick.a);
        let into = inward_normal(&w.outline, Seg2::new(mid - d * 0.5, mid + d * 0.5))?;
        let Surface::Planar { u, v, .. } = w.surface else { return None };
        let out = -(u * into.x + v * into.y).normalize();
        Some(EdgeFrame { a: pick.a, b: pick.b, e, out, n, thickness: model.params.thickness })
    }

    /// The direction a flange runs at bend angle `angle`, turning towards the material side or
    /// away from it.
    pub fn flange_dir(&self, angle: f64, toward: bool) -> V3 {
        let side = if toward { self.n } else { -self.n };
        self.out * angle.cos() + side * angle.sin()
    }

    /// The bend angle and side that make a flange run along `d` (projected across the edge):
    /// `None` if `d` runs along the edge or keeps the wall flat.
    pub fn angle_of(&self, d: V3) -> Option<(f64, bool)> {
        let d = d - self.e * d.dot(&self.e);
        let d = d.try_normalize(1e-9)?;
        let (x, y) = (d.dot(&self.out), d.dot(&self.n));
        let angle = y.abs().atan2(x);
        (angle > 1e-6 && angle < PI - 1e-6).then_some((angle, y > 0.0))
    }

    /// The outer virtual sharp at the picked stretch's start (where the outsides of the wall and
    /// a flange with these settings meet): the point the flange's Distance is measured from.
    pub fn outer_sharp(&self, alignment: FlangeAlignment, angle: f64, toward: bool, radius: f64) -> P3 {
        let x = alignment.outer_offset(self.thickness, radius, angle);
        let level = if toward { 0.0 } else { self.thickness };
        self.a + self.out * x + self.n * level
    }
}

// ---------------------------------------------------------------------------------------------
// Flange

/// Where a flange stands relative to the edge it is made on (SM3.2,
/// `help/feature-tools/flange-alignment-01.png`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FlangeAlignment {
    /// The flange's inside face on the edge.
    #[default]
    Inner,
    /// Its outside face on the edge.
    Outer,
    /// Its middle on the edge.
    Middle,
    /// Its bend starting at the edge.
    HoldLine,
}

impl FlangeAlignment {
    pub const ALL: [FlangeAlignment; 4] = [FlangeAlignment::Inner, FlangeAlignment::Outer, FlangeAlignment::Middle, FlangeAlignment::HoldLine];

    pub fn label(self) -> &'static str {
        match self {
            FlangeAlignment::Inner => "Inner",
            FlangeAlignment::Outer => "Outer",
            FlangeAlignment::Middle => "Middle",
            FlangeAlignment::HoldLine => "Hold line",
        }
    }

    /// How far past the edge the outer virtual sharp lies (mm; along the wall). The inner and
    /// outer sharps lie `T·tan(θ/2)` apart along the wall; Hold line puts the bend's start (the
    /// outer setback `(R + T)·tan(θ/2)` short of the outer sharp) on the edge.
    pub fn outer_offset(self, t: f64, r: f64, angle: f64) -> f64 {
        let h = (angle / 2.0).tan();
        match self {
            FlangeAlignment::Inner => t * h,
            FlangeAlignment::Outer => 0.0,
            FlangeAlignment::Middle => t * h / 2.0,
            FlangeAlignment::HoldLine => (r + t) * h,
        }
    }
}

/// One edge to flange.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlangeEdge {
    pub pick: EdgePick,
    /// A stable key for the edge (the flange's wall and bend ids come from it).
    pub key: u64,
    /// Bend angle (radians, between 0 and 180°) and the side it turns to (towards the wall's
    /// material side or away).
    pub angle: f64,
    pub toward: bool,
    /// Distance from the outer virtual sharp to the flange's tip (SM3.3).
    pub distance: f64,
    /// A partial flange (SM3.7): how far in from the picked stretch's start and end the flange
    /// begins and ends.
    pub partial: Option<(f64, f64)>,
}

/// What all edges of a Flange feature share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlangeOpts {
    pub alignment: FlangeAlignment,
    /// This feature's bend radius (`None`: the model's).
    pub radius: Option<f64>,
    /// `None`: automatic miter; else the miter angle (radians) of flange ends at a corner.
    pub miter: Option<f64>,
    /// Hold adjacent edges: a partial flange moves only its own stretch of the wall's edge
    /// (false: the whole edge).
    pub hold_adjacent: bool,
    /// Per chain: a partial flange's bounds apply at the ends of each chain of edges meeting end
    /// to end (else at each edge's ends).
    pub per_chain: bool,
}

/// What a feature added for one of its edges: the edge's key, the new wall and the joint.
pub type Added = (u64, WallId, JointId);

/// One flange wall, before the miters.
struct Pending {
    se: SharpEdge,
    /// Its stretch along the edge (lengths from the edge's start).
    span: (f64, f64),
    /// Whether it reaches the edge's start and end (not cut short by a partial flange).
    whole: (bool, bool),
    origin: P3,
    u: V3,
    v: V3,
    nf: V3,
    /// Its length from the virtual sharp, and the sign of the flange direction along `v`.
    length: f64,
    sign: f64,
    outline: Polygon,
    /// How far the wall's edge moved out (the definition surface's sharp).
    x_d: f64,
}

impl Pending {
    fn local(&self, p: P3) -> P2 {
        P2::new((p - self.origin).dot(&self.u), (p - self.origin).dot(&self.v))
    }

    fn point(&self, q: P2) -> P3 {
        self.origin + self.u * q.x + self.v * q.y
    }
}

/// Adds a flange on each edge (SM3).
pub fn flange(def: &mut SharpDef, edges: &[FlangeEdge], o: &FlangeOpts) -> Result<Vec<Added>, String> {
    let p = def.builder.params;
    let t = p.thickness;
    let r = o.radius.unwrap_or(p.bend_radius);
    if !(r >= 0.0) {
        return Err("The bend radius must be at least 0".into());
    }
    // A partial flange's bounds arrive measured from the picked stretch's start and end as the
    // pick runs; the sharp outline's edge may run the other way: put them in its order.
    let ses: Vec<SharpEdge> = edges.iter().map(|fe| sharp_edge(def, &fe.pick)).collect::<Result<_, _>>()?;
    let mut chained: Vec<FlangeEdge> = edges.iter().zip(&ses).map(|(fe, se)| FlangeEdge { partial: fe.partial.map(|(d0, d1)| if se.reversed { (d1, d0) } else { (d0, d1) }), ..*fe }).collect();
    // Per chain (SM3.7): edges meeting end to end are one chain; the bounds apply at the chain's
    // free ends only: the first free end met (in the order the edges were picked) takes the first
    // bound, the chain's other free end the second. A lone edge takes both, as Per edge.
    if o.per_chain && edges.len() > 1 {
        let n = ses.len();
        let tol = 1e-6 * ses.iter().map(|x| x.len()).fold(1.0, f64::max);
        let end = |x: &SharpEdge, i: usize| if i == 0 { x.at(0.0, 0.0) } else { x.at(x.len(), 0.0) };
        let meet = |k: usize, i: usize, l: usize| (0..2).any(|j| (end(&ses[k], i) - end(&ses[l], j)).norm() < tol);
        let shared = |k: usize, i: usize| (0..n).any(|l| l != k && meet(k, i, l));
        // The chains (union-find over shared ends).
        let mut chain: Vec<usize> = (0..n).collect();
        fn root(c: &mut [usize], mut i: usize) -> usize {
            while c[i] != i {
                c[i] = c[c[i]];
                i = c[i];
            }
            i
        }
        for k in 0..n {
            for l in k + 1..n {
                if (0..2).any(|i| meet(k, i, l)) {
                    let (a, b) = (root(&mut chain, k), root(&mut chain, l));
                    chain[b] = a;
                }
            }
        }
        let (first, second) = edges[0].partial.unwrap_or((0.0, 0.0));
        let mut met: Vec<(usize, usize)> = Vec::new();
        for (k, fe) in chained.iter_mut().enumerate() {
            if fe.partial.is_none() {
                continue;
            }
            let r = root(&mut chain, k);
            let mut d = [0.0, 0.0];
            for (i, di) in d.iter_mut().enumerate() {
                if shared(k, i) {
                    continue;
                }
                let c = match met.iter_mut().find(|(q, _)| *q == r) {
                    Some((_, c)) => {
                        *c += 1;
                        *c - 1
                    }
                    None => {
                        met.push((r, 1));
                        0
                    }
                };
                *di = if c == 0 { first } else { second };
            }
            fe.partial = Some((d[0], d[1]));
        }
    }
    let edges = &chained[..];
    let mut pend: Vec<Pending> = Vec::new();
    for fe in edges {
        if fe.pick.joined {
            return Err("An edge to flange is already joined to another wall".into());
        }
        let theta = fe.angle;
        if !(theta > 1e-6 && theta < PI - 1e-6) {
            return Err("The bend angle must be between 0 and 180 degrees".into());
        }
        if !(fe.distance > 0.0) {
            return Err("The distance must be greater than zero".into());
        }
        let se = sharp_edge(def, &fe.pick)?;
        let h = (theta / 2.0).tan();
        let x_s = o.alignment.outer_offset(t, r, theta);
        let x_d = if fe.toward { x_s } else { x_s - t * h };
        let (p0, p1) = se.pick;
        let span = match fe.partial {
            Some((d0, d1)) => (p0 + d0.max(0.0), p1 - d1.max(0.0)),
            None => (p0, p1),
        };
        if span.1 - span.0 < 1e-6 {
            return Err("The partial flange has no length".into());
        }
        let whole = match fe.partial {
            Some((d0, d1)) => (d0 <= 1e-9, d1 <= 1e-9),
            None => (true, true),
        };
        let side = if fe.toward { se.n } else { -se.n };
        let f = -se.into3 * theta.cos() + side * theta.sin();
        let nf = if fe.toward { se.n * theta.cos() + se.into3 * theta.sin() } else { se.n * theta.cos() - se.into3 * theta.sin() };
        // Measured on the definition surface from its own sharp: the inner sharp lies
        // `T·tan(θ/2)` further along the flange than the outer one.
        let length = fe.distance - if fe.toward { 0.0 } else { t * h };
        if length <= 1e-9 {
            return Err("The flange is too short".into());
        }
        let origin = se.at(0.0, x_d);
        let u = se.e;
        let v = nf.cross(&u);
        let sign = f.dot(&v).signum();
        let outline = Polygon::rect(P2::new(span.0, 0.0f64.min(sign * length)), P2::new(span.1, 0.0f64.max(sign * length)));
        pend.push(Pending { se, span, whole, origin, u, v, nf, length, sign, outline, x_d });
    }

    // Miters where two flanges of the feature meet at a corner of the walls' sharps.
    let gap = p.minimal_gap;
    let mut rips: Vec<(usize, usize, (P3, P3))> = Vec::new();
    let size = pend.iter().map(|x| x.se.len() + x.length).fold(1.0, f64::max);
    let tol = 1e-6 * size;
    for k in 0..pend.len() {
        for l in k + 1..pend.len() {
            // A shared end of the two edges (on the sharps of the walls they are on).
            let ends = |x: &Pending| [(x.se.at(0.0, 0.0), 0usize), (x.se.at(x.se.len(), 0.0), 1usize)];
            let mut shared = None;
            for (pk, ek) in ends(&pend[k]) {
                for (pl, el) in ends(&pend[l]) {
                    if (pk - pl).norm() < tol {
                        shared = Some((pk, ek, el));
                    }
                }
            }
            let Some((corner, ek, el)) = shared else { continue };
            let reaches = |x: &Pending, e: usize| if e == 0 { x.whole.0 } else { x.whole.1 };
            if !reaches(&pend[k], ek) || !reaches(&pend[l], el) {
                continue;
            }
            match o.miter {
                None => {
                    let reach = (pend[k].length + pend[l].length + t) * 4.0;
                    extend_end(&mut pend[k], ek, reach);
                    extend_end(&mut pend[l], el, reach);
                    let coplanar = pend[k].nf.dot(&pend[l].nf) > 1.0 - 1e-9 && (pend[l].origin - pend[k].origin).dot(&pend[k].nf).abs() < tol;
                    if coplanar {
                        // In one plane: cut along the bisector of the corner, the gap apart.
                        let xk = pend[k].local(pend[k].se.at(if ek == 0 { 0.0 } else { pend[k].se.len() }, pend[k].x_d));
                        let dk = if ek == 0 { V2::x() } else { -V2::x() };
                        let ql = |x: f64| pend[k].local(pend[l].se.at(x, pend[l].x_d));
                        let (la, lb) = (ql(0.0), ql(pend[l].se.len()));
                        let dl = if el == 0 { (lb - la).normalize() } else { (la - lb).normalize() };
                        // Where the two flanges' bottom lines meet.
                        let Some(x) = line_meet(xk, dk, la, lb - la) else { continue };
                        let b = dk + dl;
                        if b.norm() < 1e-9 {
                            continue;
                        }
                        let mut nb = perp(b.normalize());
                        if nb.dot(&dk) < 0.0 {
                            nb = -nb;
                        }
                        let ok = pend[k].outline.clip_half_plane(x + nb * (gap / 2.0), nb);
                        let nb3 = pend[k].u * nb.x + pend[k].v * nb.y;
                        let x3 = pend[k].point(x);
                        let pl_ = &pend[l];
                        let nbl = -V2::new(nb3.dot(&pl_.u), nb3.dot(&pl_.v));
                        let xl = pl_.local(x3);
                        let ol = pl_.outline.clip_half_plane(xl + nbl * (gap / 2.0), nbl);
                        pend[k].outline = ok;
                        pend[l].outline = ol;
                    } else {
                        // Where their planes meet: each carried on to it, then a rip (an edge
                        // joint) between them.
                        let Some((o3, d3)) = plane_line(pend[k].nf, pend[k].nf.dot(&pend[k].origin.coords), pend[l].nf, pend[l].nf.dot(&pend[l].origin.coords))
                        else {
                            continue;
                        };
                        for (a, b) in [(k, l), (l, k)] {
                            let mid = {
                                let x = &pend[a];
                                x.point(P2::new((x.span.0 + x.span.1) / 2.0, x.sign * x.length / 2.0))
                            };
                            let nb = pend[b].nf;
                            let x = &pend[a];
                            let n2 = V2::new(x.u.dot(&nb), x.v.dot(&nb));
                            if n2.norm() < 1e-9 {
                                continue;
                            }
                            let s = if (mid - o3).dot(&nb) >= 0.0 { 1.0 } else { -1.0 };
                            let q = x.local(o3);
                            pend[a].outline = pend[a].outline.clip_half_plane(q, n2 * s);
                        }
                        // The rip's edge: what both walls have along the line.
                        let along = |x: &Pending| -> Option<(f64, f64)> {
                            let pts: Vec<f64> = x
                                .outline
                                .outer
                                .iter()
                                .map(|q| x.point(*q))
                                .filter(|q| ((q - o3) - d3 * (q - o3).dot(&d3)).norm() < 1e-6 * size)
                                .map(|q| (q - o3).dot(&d3))
                                .collect();
                            (pts.len() >= 2).then(|| pts.iter().fold((f64::MAX, f64::MIN), |(a, b), s| (a.min(*s), b.max(*s))))
                        };
                        let (Some(ik), Some(il)) = (along(&pend[k]), along(&pend[l])) else { continue };
                        let (lo, hi) = (ik.0.max(il.0), ik.1.min(il.1));
                        if hi - lo > tol {
                            rips.push((k, l, (o3 + d3 * lo, o3 + d3 * hi)));
                        }
                        let _ = corner;
                    }
                }
                Some(alpha) => {
                    // The miter plane runs through the corner's outside at `alpha` to the first
                    // flange (90° − alpha to the second). A wall's end is square, so each stops
                    // where its inside meets the plane: past the edge's end by `T·(1 − cot α)`
                    // (negative: short of it) for the first, `T·(1 − tan α)` for the second;
                    // 45° stops both at the edge's end.
                    let alpha = alpha.clamp(1e-3, FRAC_PI_2 - 1e-3);
                    let lim = 4.0 * t;
                    let ext = [(t * (1.0 - 1.0 / alpha.tan())).clamp(-size, lim), (t * (1.0 - alpha.tan())).clamp(-size, lim)];
                    for ((i, e), x) in [(k, ek), (l, el)].into_iter().zip(ext) {
                        let s0 = if e == 0 { pend[i].span.0 } else { pend[i].span.1 };
                        let out = if e == 0 { -1.0 } else { 1.0 };
                        if x > 0.0 {
                            extend_end(&mut pend[i], e, x + t);
                        }
                        let u = s0 + out * x;
                        pend[i].outline = pend[i].outline.clip_half_plane(P2::new(u, 0.0), V2::new(-out, 0.0));
                    }
                }
            }
        }
    }

    // The walls' edges moved to the flanges' sharps; the flange walls; their bends.
    let mut letter = def.next_letter();
    let mut out = Vec::new();
    let mut new_walls: Vec<usize> = Vec::new();
    for (fe, x) in edges.iter().zip(&pend) {
        // Hold adjacent edges: the rest of the edge stays where it is (only the flange's stretch
        // moves to its sharp); off, the whole edge moves.
        let (b0, b1) = if fe.partial.is_some() && o.hold_adjacent { x.span } else { (0.0, x.se.len()) };
        let seg = Seg2::new(x.se.seg.a + x.se.seg.dir() * b0, x.se.seg.a + x.se.seg.dir() * b1);
        let wall = &mut def.builder.walls[x.se.wall];
        if x.x_d.abs() > 1e-12 {
            wall.outline = shift_edge(&wall.outline, seg, x.se.into, -x.x_d).ok_or("The flange cuts its wall away")?;
        }
        if x.outline.is_empty() || x.outline.area() < 1e-9 {
            return Err("The flange's miter leaves nothing of it".into());
        }
        let id = WallId(stable_id(fe.key));
        if def.builder.walls.iter().any(|w| w.id == Some(id)) {
            return Err("This edge already has a flange".into());
        }
        def.builder.walls.push(SharpWall { origin: x.origin, u: x.u, v: x.v, outline: x.outline.clone(), id: Some(id) });
        let wi = def.builder.walls.len() - 1;
        new_walls.push(wi);
        let edge = (x.se.at(x.span.0, x.x_d), x.se.at(x.span.1, x.x_d));
        let j = def.builder.joint(x.se.wall, wi, edge, SharpJointKind::Bend { radius: o.radius, value: None });
        let jid = JointId(stable_id(fe.key.rotate_left(13) ^ 0x4245_4e44));
        def.builder.set_joint_id(j, jid, Some(next_name("Bend", &mut letter)));
        out.push((fe.key, id, jid));
    }
    for (k, l, edge) in rips {
        let j = def.builder.rip(new_walls[k], new_walls[l], edge, RipStyle::EdgeJoint);
        let key = edges[k].key.rotate_left(29) ^ edges[l].key ^ 0x5249_5000;
        let jid = JointId(stable_id(key));
        def.builder.set_joint_id(j, jid, Some(next_name("Joint", &mut letter)));
    }
    Ok(out)
}

/// Carries a pending flange's end at the edge's start (`end` 0) or end (1) on past the edge's
/// own end by `reach` (the miter then cuts it back).
fn extend_end(x: &mut Pending, end: usize, reach: f64) {
    let Some((lo, hi)) = x.outline.bounds() else { return };
    let (y0, y1) = (lo.y, hi.y);
    let s = if end == 0 { 0.0 - reach } else { x.se.len() + reach };
    let piece = if end == 0 { Polygon::rect(P2::new(s, y0), P2::new(x.span.0 + 1e-9, y1)) } else { Polygon::rect(P2::new(x.span.1 - 1e-9, y0), P2::new(s, y1)) };
    if let Some(u) = crate::poly::union(&[x.outline.clone(), piece]).into_iter().max_by(|a, b| a.area().total_cmp(&b.area())) {
        x.outline = u;
    }
    if end == 0 {
        x.span.0 = 0.0;
    } else {
        x.span.1 = x.se.len();
    }
}

/// Where the 2D lines through `p` along `d` and through `q` along `e` meet.
fn line_meet(p: P2, d: V2, q: P2, e: V2) -> Option<P2> {
    let den = d.perp(&e);
    if den.abs() < 1e-12 * d.norm() * e.norm() {
        return None;
    }
    let s = (q - p).perp(&e) / den;
    Some(p + d * s)
}

/// The line where the planes `n1·x = h1` and `n2·x = h2` meet (a point and unit direction).
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

// ---------------------------------------------------------------------------------------------
// Hem

/// Hem types (SM4.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HemKind {
    /// Folded back 180°.
    #[default]
    Straight,
    /// Rolled past 180°.
    Rolled,
    /// Rolled past 180° with a straight leg back towards the wall.
    TearDrop,
}

impl HemKind {
    pub const ALL: [HemKind; 3] = [HemKind::Straight, HemKind::Rolled, HemKind::TearDrop];

    pub fn label(self) -> &'static str {
        match self {
            HemKind::Straight => "Straight",
            HemKind::Rolled => "Rolled",
            HemKind::TearDrop => "Tear drop",
        }
    }
}

/// What all edges of a Hem feature share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HemOpts {
    pub kind: HemKind,
    /// Inner radius (mm): for a flattened straight hem, half the model's minimal gap.
    pub radius: f64,
    /// A rolled hem's angle (radians, more than 180°).
    pub angle: f64,
    /// A tear drop's gap between its end and the wall (mm).
    pub gap: f64,
    /// Straight and tear drop: from the hem's outermost point to its end (mm).
    pub total: f64,
    pub alignment: HemAlignment,
    /// Closed corners where hems of the feature meet (else Simple).
    pub closed: bool,
}

/// One edge to hem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HemEdge {
    pub pick: EdgePick,
    pub key: u64,
    /// Fold over the wall's material side (else the other side).
    pub toward: bool,
}

/// A hem's bend angle and leg length (from its tangent line) for these settings.
pub fn hem_shape(o: &HemOpts, t: f64) -> Result<(f64, f64), String> {
    let r = o.radius;
    if !(r >= 0.0) {
        return Err("The inner radius must be at least 0".into());
    }
    match o.kind {
        HemKind::Straight => {
            let len = crate::model::hem_length_from_total(o.total, r, t);
            if !(len > 1e-9) {
                return Err("The total length must be more than the inner radius plus the thickness".into());
            }
            Ok((PI, len))
        }
        HemKind::Rolled => {
            if !(o.angle > PI + 1e-6 && o.angle < TAU - 1e-6) {
                return Err("A rolled hem's angle must be between 180 and 360 degrees".into());
            }
            Ok((o.angle, ROLLED_TAIL))
        }
        HemKind::TearDrop => tear_drop(r, t, o.gap, o.total).map(|(beta, len)| (PI + beta, len)),
    }
}

/// A tear drop's leg: the angle `β` it leans back towards the wall past 180° and its length `ℓ`
/// (from its tangent line), so that its end is `gap` off the wall and `total` from the hem's
/// outermost point. With the bend's centre `R + T` in from the outermost point:
/// `R·(1 + cos β) − ℓ·sin β = gap` and `(R + T)·(1 + sin β) + ℓ·cos β = total`.
pub fn tear_drop(r: f64, t: f64, gap: f64, total: f64) -> Result<(f64, f64), String> {
    if !(gap >= 0.0) {
        return Err("The gap must be at least 0".into());
    }
    let leg = |b: f64| (r * (1.0 + b.cos()) - gap) / b.sin();
    let total_of = |b: f64| (r + t) * (1.0 + b.sin()) + b.cos() * leg(b);
    // The leg must have a length: β up to where it reaches zero.
    let mut hi = FRAC_PI_2;
    if leg(hi) <= 1e-9 {
        let mut lo = 1e-9;
        if leg(lo) <= 1e-9 {
            return Err("The gap is too large for the inner radius".into());
        }
        for _ in 0..200 {
            let m = (lo + hi) / 2.0;
            if leg(m) > 1e-9 { lo = m } else { hi = m }
        }
        hi = lo;
    }
    let (mut lo, mut up) = (1e-9, hi);
    let f = |b: f64| total_of(b) - total;
    if f(up) > 0.0 || f(lo) < 0.0 {
        return Err("The total length doesn't fit a tear drop with this radius and gap".into());
    }
    for _ in 0..200 {
        let m = (lo + up) / 2.0;
        if f(m) > 0.0 { lo = m } else { up = m }
    }
    let b = (lo + up) / 2.0;
    Ok((b, leg(b)))
}

/// Adds a hem on each edge (SM4).
pub fn hem(def: &mut SharpDef, edges: &[HemEdge], o: &HemOpts) -> Result<Vec<Added>, String> {
    let p = def.builder.params;
    let t = p.thickness;
    let (angle, length) = hem_shape(o, t)?;
    let mut letter = def.next_letter();
    let mut ses = Vec::new();
    for he in edges {
        if he.pick.joined {
            return Err("An edge to hem is already joined to another wall".into());
        }
        ses.push(sharp_edge(def, &he.pick)?);
    }
    // Corners where two hems of the feature meet on one wall.
    let size = ses.iter().map(|s| s.len()).fold(1.0, f64::max);
    let tol = 1e-6 * size;
    let trim = match o.alignment {
        HemAlignment::Outer => o.radius + t,
        HemAlignment::InPlace => 0.0,
    };
    let mut clips: Vec<Vec<HemClip>> = vec![Vec::new(); edges.len()];
    // Hems whose edge stops short of a corner: (hem, at the edge's start, where along it).
    let mut shorten: Vec<(usize, bool, f64)> = Vec::new();
    for k in 0..ses.len() {
        for l in 0..ses.len() {
            if k == l || ses[k].wall != ses[l].wall {
                continue;
            }
            let ends = |s: &SharpEdge| [(s.at(0.0, 0.0), s.e), (s.at(s.len(), 0.0), -s.e)];
            for (pk, dk) in ends(&ses[k]) {
                for (pl, dl) in ends(&ses[l]) {
                    if (pk - pl).norm() > tol {
                        continue;
                    }
                    let clip = if o.closed {
                        // Along the corner's bisector.
                        let nrm = (dk - dl).try_normalize(1e-9);
                        nrm.map(|nn| HemClip { point: pk + nn * (p.minimal_gap / 2.0), normal: nn, extend: length + o.radius + 2.0 * t + trim })
                    } else if k > l {
                        // Simple: the later hem (bend and leg) stops short of the earlier one's
                        // leg end (`trim + length` in from its edge), so the legs don't cross.
                        let q = pl + ses[l].into3 * (trim + length + p.minimal_gap / 2.0);
                        let s = (q - ses[k].origin).dot(&ses[k].e);
                        shorten.push((k, (pk - ses[k].at(0.0, 0.0)).norm() < tol, s));
                        None
                    } else {
                        // Simple: the earlier hem runs on to the corner.
                        None
                    };
                    clips[k].extend(clip);
                }
            }
        }
    }
    // Hems on two walls in one plane (flanges mitred into each other) meeting end to end at an
    // inside corner: the later one starts clear of the earlier one's bend.
    let mut spans: Vec<(f64, f64)> = ses.iter().map(|s| (0.0, s.len())).collect();
    for (k, start, s) in shorten {
        if start {
            spans[k].0 = spans[k].0.max(s);
        } else {
            spans[k].1 = spans[k].1.min(s);
        }
    }
    for k in 0..ses.len() {
        for l in 0..k {
            let (a, b) = (&ses[k], &ses[l]);
            if a.wall == b.wall || a.n.dot(&b.n).abs() < 1.0 - 1e-9 || (a.origin - b.origin).dot(&a.n).abs() > tol {
                continue;
            }
            for (i, pk) in [(0usize, a.at(0.0, 0.0)), (1, a.at(a.len(), 0.0))] {
                // (Mitred flanges' edges end the minimal gap apart.)
                if [b.at(0.0, 0.0), b.at(b.len(), 0.0)].iter().any(|q| (q - pk).norm() < tol + 2.0 * p.minimal_gap) {
                    let clear = o.radius + t + p.minimal_gap;
                    if i == 0 {
                        spans[k].0 = clear;
                    } else {
                        spans[k].1 = a.len() - clear;
                    }
                }
            }
        }
    }
    // Hems on two walls meeting at a box corner (flanges mitred into each other, folded the
    // same way): the later hem stops half the minimal gap clear of the earlier one (which reaches
    // `2R + T` in from its wall's face). Simple and Closed alike: a bend region ends square, so
    // the legs (`2R` in, where the earlier hem's leg already is) can't close any further.
    let depth = 2.0 * o.radius + t;
    let gap = p.minimal_gap;
    for k in 0..ses.len() {
        for l in 0..k {
            let (a, b) = (&ses[k], &ses[l]);
            if a.wall == b.wall || a.n.dot(&b.n).abs() > 1.0 - 1e-9 {
                continue;
            }
            // Ends of the two edges at one corner (the rip between the walls keeps them up to a
            // thickness and the gap apart).
            let near = 2.0 * (t + gap) + tol;
            let ends = |s: &SharpEdge| [(0usize, s.at(0.0, 0.0)), (1usize, s.at(s.len(), 0.0))];
            let Some(ek) = ends(a).into_iter().flat_map(|(i, p)| ends(b).into_iter().map(move |(j, q)| (i, j, (p - q).norm()))).find(|x| x.2 < near).map(|x| x.0) else {
                continue;
            };
            let wb = &def.builder.walls[b.wall];
            let hb = |x: P3| (x - wb.origin).dot(&b.n);
            // Where the earlier hem lies off its wall, and whether the later wall is on that side.
            let sigma = if edges[l].toward { 1.0 } else { -1.0 };
            let mid = a.at(a.len() / 2.0, 0.0);
            let same_side = if sigma > 0.0 { hb(mid) > t } else { hb(mid) < 0.0 };
            if !same_side {
                continue;
            }
            let level = if sigma > 0.0 { t + depth + gap / 2.0 } else { -(depth + gap / 2.0) };
            let slope = a.e.dot(&b.n);
            if slope.abs() < 1e-9 {
                continue;
            }
            // The later hem's edge stops where it is `level` off the earlier hem's wall.
            let s_end = if ek == 0 { 0.0 } else { a.len() };
            let s = s_end + (level - hb(a.at(s_end, 0.0))) / slope;
            if ek == 0 {
                spans[k].0 = spans[k].0.max(s);
            } else {
                spans[k].1 = spans[k].1.min(s);
            }
        }
    }
    let mut out = Vec::new();
    for (((he, se), cl), span) in edges.iter().zip(&ses).zip(clips).zip(spans) {
        if span.1 - span.0 < 1e-6 {
            return Err("A hem is too short to clear the hem it meets".into());
        }
        let edge = (se.at(span.0, 0.0), se.at(span.1, 0.0));
        let h = def.builder.hem(se.wall, edge, length, he.toward, o.alignment);
        let id = WallId(stable_id(he.key));
        let jid = JointId(stable_id(he.key.rotate_left(13) ^ 0x4845_4d00));
        def.builder.set_hem_id(h, jid, id, Some(next_name("Bend", &mut letter)));
        let hm = &mut def.builder.hems[h];
        hm.radius = Some(o.radius);
        hm.angle = angle;
        hm.clips = cl;
        out.push((he.key, id, jid));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Make joint

/// The joint Make joint makes (SM6.2, SM6.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JointSpec {
    Rip(RipStyle),
    /// A bend of this radius (`None`: the model's).
    Bend(Option<f64>),
}

/// Joins the walls of two picked edges (SM6.1): both edges carried on or cut back to where the
/// walls' planes meet, then a rip or a bend there. The first pick's wall is the joint's first
/// wall (Butt joint – Direction 1: it stops short and the second runs on over its end).
pub fn make_joint(def: &mut SharpDef, first: &EdgePick, second: &EdgePick, key: u64, spec: JointSpec) -> Result<JointId, String> {
    if first.joined || second.joined {
        return Err("An edge to join is already joined to another wall".into());
    }
    let s1 = sharp_edge(def, first)?;
    let s2 = sharp_edge(def, second)?;
    if s1.wall == s2.wall {
        return Err("Select edges of two different walls".into());
    }
    let w1 = &def.builder.walls[s1.wall];
    let w2 = &def.builder.walls[s2.wall];
    let (o1, o2) = (w1.origin, w2.origin);
    let Some((lo3, ld)) = plane_line(s1.n, s1.n.dot(&o1.coords), s2.n, s2.n.dot(&o2.coords)) else {
        return Err("The walls are parallel: they don't meet".into());
    };
    let mut spans = Vec::new();
    for s in [&s1, &s2] {
        if s.e.cross(&ld).norm() > 1e-6 {
            return Err("The edges must run along the line where the walls meet".into());
        }
        // How far past the edge the line lies.
        let x = (lo3 - s.origin).dot(&-s.into3);
        let (t0, t1) = ((s.origin - lo3).dot(&ld), (s.at(s.len(), 0.0) - lo3).dot(&ld));
        spans.push((x, t0.min(t1), t0.max(t1)));
    }
    let (lo, hi) = (spans[0].1.max(spans[1].1), spans[0].2.min(spans[1].2));
    if hi - lo < 1e-6 {
        return Err("The edges don't face each other".into());
    }
    for (s, (x, ..)) in [(&s1, spans[0]), (&s2, spans[1])] {
        let wall = &mut def.builder.walls[s.wall];
        if x.abs() > 1e-12 {
            wall.outline = shift_edge(&wall.outline, s.seg, s.into, -x).ok_or("The joint cuts a wall away")?;
        }
    }
    let edge = (lo3 + ld * lo, lo3 + ld * hi);
    let mut letter = def.next_letter();
    let (j, name) = match spec {
        JointSpec::Rip(style) => (def.builder.rip(s1.wall, s2.wall, edge, style), next_name("Joint", &mut letter)),
        JointSpec::Bend(radius) => (def.builder.joint(s1.wall, s2.wall, edge, SharpJointKind::Bend { radius, value: None }), next_name("Bend", &mut letter)),
    };
    let jid = JointId(stable_id(key ^ 0x4a4f_494e_0000));
    def.builder.set_joint_id(j, jid, Some(name));
    Ok(jid)
}

/// The kind of a model's joint that an edge pick may name, for the dialogs.
pub fn joint_kind_name(k: &JointKind) -> &'static str {
    match k {
        JointKind::Bend(b) if b.hem => "Hem",
        JointKind::Bend(_) => "Bend",
        JointKind::Rip { .. } => "Rip",
        JointKind::Tangent { .. } => "Tangent",
    }
}
