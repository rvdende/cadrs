//! Edits of a sheet metal **definition** by the features after its Sheet metal model (P3I.5,
//! SM1.6): a sheet metal feature changes the [`Model`], and the flat pattern and the folded solid
//! are made again from it, so the three views stay in step.
//!
//! - [`bend_wall`] (**Bend**, SM9): a planar wall folded along a line. The wall splits at the
//!   line into the side that stays (the wall keeps its id) and a new wall turned about the bend
//!   axis; walls joined to the moving side turn with it. The bend region is cut out of the flat
//!   wall, as wide as the bend allowance, so **the flat pattern doesn't change size** (SM9.6).
//!   Where the band sits around the line is the **Bend alignment** (SM9.4).
//! - [`jog_wall`] (**Jog**, SM19.1): two opposite bends with a short wall between them, sized so
//!   the far wall is offset by the jog offset (measured from the anchor); without *Preserve
//!   material* the sheet is first stretched so the far end stays where it was.
//! - [`add_tab`] (**Tab**, SM5): a profile added to the walls parallel to it (bridging walls of one
//!   plane merges them).
//! - [`cut_walls`] (**Extrude → Remove** on an active model and Tab's subtraction scope, SM1.6,
//!   SM12.1): a tool's cross-section taken out of the walls it crosses, perpendicular to them.
//! - [`break_corner`] (**Corner break**, SM11; Fillet and Chamfer on corners, SM12.1): a wall's
//!   corner rounded or cut in its outline (so the flat shows it too).
//! - [`copy_walls`] (**Face pattern / Face mirror** of walls, SM12.2): walls copied with their
//!   bends and everything beyond them.
//! - Picks to the definition: [`wall_at`], [`corner_vertex_at`], [`corner_near`],
//!   [`bend_end_near`].
//!
//! Lengths are millimetres and angles radians. Every edit leaves checking the result to the
//! caller ([`Model::validate`] and [`crate::flatten`]).

use std::collections::{HashSet, VecDeque};

use nalgebra::{Rotation3, Unit};
use serde::{Deserialize, Serialize};

use crate::bend::BendValue;
use crate::flat::FlatPattern;
use crate::model::{Bend, BendEnd, Joint, JointId, JointKind, JointNamer, Model, P3, Surface, V3, Wall, WallId};
use crate::poly::{self, P2, Polygon, Seg2, V2, perp};

/// Why an edit can't be made (the feature's error).
#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    /// The wall (or face) to change isn't in the model.
    NoWall,
    /// Only flat walls can be bent, jogged or tabbed.
    NotPlanar,
    /// The bend line has no length, or runs along the wall's normal.
    BadLine,
    /// The bend line doesn't cross the wall.
    LineMissesWall,
    /// The bend region doesn't fit on the wall.
    NoRoom,
    /// The bend line splits the wall into more than two pieces.
    TooManyPieces,
    /// The bend crosses an earlier bend or joint (SM9.7).
    CrossesJoint,
    /// Walls on the side that moves are also joined to the side that stays.
    BendLoop,
    /// The bend angle is out of range, or makes no bend.
    BadAngle,
    /// The jog offset is smaller than the two bends need.
    JogTooSmall,
    /// The tab's profile isn't parallel to the walls, or touches none.
    NoTab,
    /// Walls to merge have their material on opposite sides.
    OppositeSides,
    /// The corner to break isn't a corner of a wall.
    NoCorner,
    /// The fillet or chamfer is bigger than the corner's edges.
    CornerTooBig,
    /// A cut takes a whole wall away.
    WallCutAway,
    /// Only flat walls joined by bends can be patterned or mirrored.
    CantCopy,
}

impl EditError {
    pub fn message(&self) -> &'static str {
        match self {
            EditError::NoWall => "The selected face isn't a sheet metal wall",
            EditError::NotPlanar => "Only flat sheet metal faces can be used",
            EditError::BadLine => "The bend line must be a line that isn't perpendicular to the face",
            EditError::LineMissesWall => "The bend line doesn't cross the face",
            EditError::NoRoom => "The bend doesn't fit on the face",
            EditError::TooManyPieces => "The bend line splits the face into more than two pieces",
            EditError::CrossesJoint => "The bend crosses an earlier bend or joint",
            EditError::BendLoop => "The side that bends is joined to the side that stays",
            EditError::BadAngle => "The bend angle must be between 1 and 359 degrees",
            EditError::JogTooSmall => "The jog offset is too small for its bends",
            EditError::NoTab => "The tab profile must be parallel to a wall and touch it",
            EditError::OppositeSides => "The walls to merge have their material on opposite sides",
            EditError::NoCorner => "Select a corner of a sheet metal wall",
            EditError::CornerTooBig => "The corner break is bigger than the corner's edges",
            EditError::WallCutAway => "The cut removes a whole wall",
            EditError::CantCopy => "Only flat walls joined by bends can be patterned or mirrored",
        }
    }
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for EditError {}

/// A persistent id for the `k`-th thing an edit made, from a seed (the feature's): stable across
/// rebuilds, so later features can name what it made.
pub fn derived_id(seed: u64, k: u32) -> u32 {
    // splitmix64 of the seed and the index; the high bit set keeps clear of the small ids the
    // Sheet metal model numbers its own walls and joints with.
    let mut z = seed ^ (u64::from(k).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z as u32) | 0x8000_0000
}

/// A free wall id from `seed` (skipping ids the model has).
fn free_wall(m: &Model, seed: u64, k: &mut u32) -> WallId {
    loop {
        let id = WallId(derived_id(seed, *k));
        *k += 1;
        if m.wall(id).is_none() {
            return id;
        }
    }
}

/// A free joint id from `seed`.
fn free_joint(m: &Model, seed: u64, k: &mut u32) -> JointId {
    loop {
        let id = JointId(derived_id(seed, *k));
        *k += 1;
        if m.joint(id).is_none() {
            return id;
        }
    }
}

fn namer(m: &Model) -> JointNamer {
    let mut n = JointNamer::default();
    n.taken = m.joints.iter().map(|j| j.name.clone()).collect();
    n
}

// ---------------------------------------------------------------------------------------------
// Rigid maps

/// A rigid motion of space: `p ↦ rot·p + t`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rigid {
    pub rot: Rotation3<f64>,
    pub t: V3,
}

impl Rigid {
    pub fn identity() -> Rigid {
        Rigid { rot: Rotation3::identity(), t: V3::zeros() }
    }

    pub fn translation(t: V3) -> Rigid {
        Rigid { rot: Rotation3::identity(), t }
    }

    /// A turn by `angle` about the axis through `at` along `axis`.
    pub fn about(at: P3, axis: V3, angle: f64) -> Rigid {
        let rot = Rotation3::from_axis_angle(&Unit::new_normalize(axis), angle);
        Rigid { rot, t: at.coords - rot * at.coords }
    }

    pub fn point(&self, p: P3) -> P3 {
        P3::from(self.rot * p.coords + self.t)
    }

    pub fn vec(&self, v: V3) -> V3 {
        self.rot * v
    }

    /// This map after `first`.
    pub fn after(&self, first: &Rigid) -> Rigid {
        Rigid { rot: self.rot * first.rot, t: self.rot * first.t + self.t }
    }
}

/// A model moved rigidly (`rot` a rotation, `t` a translation): its walls' surfaces move, their
/// outlines and the joints on them (in the walls' own coordinates) stay. (P3I.8: a Derived copy
/// of a sheet metal part placed elsewhere.)
pub fn moved_model(m: &Model, rot: nalgebra::Matrix3<f64>, t: V3) -> Model {
    let r = Rigid { rot: Rotation3::from_matrix_unchecked(rot), t };
    let mut out = m.clone();
    for w in &mut out.walls {
        w.surface = map_surface(w.surface, &r);
    }
    out
}

fn map_surface(s: Surface, f: &Rigid) -> Surface {
    match s {
        Surface::Planar { origin, u, v } => Surface::Planar { origin: f.point(origin), u: f.vec(u), v: f.vec(v) },
        Surface::Rolled { axis_origin, axis, start, radius, material_outside } => Surface::Rolled {
            axis_origin: f.point(axis_origin),
            axis: f.vec(axis),
            start: f.vec(start),
            radius,
            material_outside,
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Small geometry

fn size_of(p: &Polygon) -> f64 {
    p.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0).max(1.0)
}

/// The half plane `(x − p)·n ≥ 0` as a big square (for the polygon booleans).
fn half_plane(p: P2, n: V2, size: f64) -> Polygon {
    let n = n.normalize();
    let d = perp(n);
    let r = 4.0 * size;
    Polygon::new(vec![p - d * r, p + d * r, p + d * r + n * r, p - d * r + n * r])
}

/// The pieces of `poly` where `(x − p)·n ≥ 0`.
fn half(poly: &Polygon, p: P2, n: V2) -> Vec<Polygon> {
    let size = size_of(poly) + (p.coords.norm());
    let hp = half_plane(p, n, size);
    let out: Vec<Polygon> = poly::intersection(poly, &hp).into_iter().filter(|q| q.area() > 1e-9).collect();
    exact(out, &[poly, &hp])
}

/// Boolean results put back on the exact points they came from: the inputs' vertices and where
/// their edges cross. (The booleans round to a 1 nm grid; a wall's edge a hair off its bend's
/// tangent line would leave the folded wall and bend apart.)
fn exact(result: Vec<Polygon>, inputs: &[&Polygon]) -> Vec<Polygon> {
    let edges = |q: &Polygon| -> Vec<(P2, P2)> {
        std::iter::once(&q.outer).chain(q.holes.iter()).flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()]))).collect()
    };
    let mut pts: Vec<P2> = inputs.iter().flat_map(|q| q.outer.iter().chain(q.holes.iter().flatten()).copied()).collect();
    let all: Vec<Vec<(P2, P2)>> = inputs.iter().map(|q| edges(q)).collect();
    for i in 0..all.len() {
        for k in i + 1..all.len() {
            for (a, b) in &all[i] {
                for (c, d) in &all[k] {
                    let (r, s2) = (b - a, d - c);
                    let den = r.perp(&s2);
                    if den.abs() < 1e-15 {
                        continue;
                    }
                    let t = (c - a).perp(&s2) / den;
                    let u = (c - a).perp(&r) / den;
                    if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) {
                        pts.push(a + r * t);
                    }
                }
            }
        }
    }
    result.iter().map(|q| poly::snap_to(q, &pts, 10.0 * poly::GRID)).collect()
}

/// Parameter intervals (along `d` from `o`) of the line `o + d·s` inside `poly`.
fn line_intervals(poly: &Polygon, o: P2, d: V2) -> Vec<(f64, f64)> {
    let big = 4.0 * (size_of(poly) + o.coords.norm());
    poly::clip_segment(Seg2::new(o - d * big, o + d * big), std::slice::from_ref(poly))
        .into_iter()
        .map(|s| ((s.a - o).dot(&d), (s.b - o).dot(&d)))
        .map(|(a, b)| (a.min(b), a.max(b)))
        .collect()
}

fn overlap(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            let (lo, hi) = (x.0.max(y.0), x.1.min(y.1));
            if hi - lo > 1e-6 {
                out.push((lo, hi));
            }
        }
    }
    out.sort_by(|p, q| p.0.total_cmp(&q.0));
    out
}

/// The joint's two segments, mutable.
fn segments_mut(j: &mut Joint) -> (&mut Seg2, &mut Seg2) {
    match &mut j.kind {
        JointKind::Bend(b) => (&mut b.on_a, &mut b.on_b),
        JointKind::Rip { on_a, on_b, .. } | JointKind::Tangent { on_a, on_b } => (on_a, on_b),
    }
}

/// The joint's segment on wall `w`, mutable.
fn segment_on_mut(j: &mut Joint, w: WallId) -> Option<&mut Seg2> {
    let (a, b) = (j.a, j.b);
    let (sa, sb) = segments_mut(j);
    if w == a {
        Some(sa)
    } else if w == b {
        Some(sb)
    } else {
        None
    }
}

/// The walls joined (through any joint) to `starts`, without passing through the walls in
/// `stop`. `Err` if a stop wall is reached through a joint other than those in `skip`.
fn reachable(m: &Model, starts: &[WallId], stop: &[WallId], skip: &[JointId]) -> Result<Vec<WallId>, EditError> {
    let mut seen: HashSet<WallId> = starts.iter().copied().collect();
    let mut out: Vec<WallId> = starts.to_vec();
    let mut queue: VecDeque<WallId> = starts.iter().copied().collect();
    while let Some(w) = queue.pop_front() {
        for j in m.joints.iter().filter(|j| (j.a == w || j.b == w) && !skip.contains(&j.id)) {
            let o = j.other(w);
            if stop.contains(&o) {
                return Err(EditError::BendLoop);
            }
            if seen.insert(o) {
                out.push(o);
                queue.push_back(o);
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Bend (SM9)

/// Where the bend sits around its line (SM9.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BendAlignment {
    /// The line at the middle of the bend region in the flat.
    #[default]
    BendLine,
    /// The bend region starts at the line.
    HoldLine,
    /// The bend region ends at the line.
    HoldOtherLine,
    /// The bent wall's inside face on the line, folded.
    Inner,
    /// Its outside face on the line.
    Outer,
    /// Its mid plane on the line.
    Middle,
}

impl BendAlignment {
    pub const ALL: [BendAlignment; 6] = [
        BendAlignment::BendLine,
        BendAlignment::HoldLine,
        BendAlignment::HoldOtherLine,
        BendAlignment::Inner,
        BendAlignment::Outer,
        BendAlignment::Middle,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BendAlignment::BendLine => "Bend line",
            BendAlignment::HoldLine => "Hold line",
            BendAlignment::HoldOtherLine => "Hold other line",
            BendAlignment::Inner => "Inner",
            BendAlignment::Outer => "Outer",
            BendAlignment::Middle => "Middle",
        }
    }
}

/// A Bend's settings, resolved to the definition.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BendSpec {
    /// The wall to bend (the picked face's).
    pub wall: WallId,
    /// The bend line (any two points of it; projected onto the wall and extended across it).
    pub line: (P3, P3),
    /// Keep the other side still.
    pub hold_opposite: bool,
    pub alignment: BendAlignment,
    /// Bend angle (radians, 1°–359°; a bend of 180° or more folds flat over itself).
    pub angle: f64,
    /// Bend towards the wall's material side (else away from it).
    pub toward_material: bool,
    /// How far above the definition surface the line lies (0: on it; the thickness: on the other
    /// face). The folded alignments measure from here.
    pub line_height: f64,
    /// A custom inner radius; `None` uses the model's (SM9.6).
    pub radius: Option<f64>,
    /// A custom K factor; `None` uses the model's.
    pub k_factor: Option<f64>,
}

/// What a bend made.
#[derive(Clone, Debug, PartialEq)]
pub struct BendMade {
    /// The new wall: the side that moved.
    pub wall: WallId,
    /// The bends between the two (more than one where the line crosses cut-outs).
    pub joints: Vec<JointId>,
    /// Walls that turned with the moving side.
    pub moved: Vec<WallId>,
    /// The bend region's flat width.
    pub allowance: f64,
}

/// A fold resolved on a wall: the line through `a` (wall-local) with the unit normal `nn`
/// towards the side that moves, the bend region from `d1` to `d1 + allowance` along `nn`.
#[derive(Clone, Copy, Debug)]
struct Fold {
    a: P2,
    nn: V2,
    d1: f64,
    angle: f64,
    toward: bool,
    radius: f64,
    model_radius: bool,
    value: Option<BendValue>,
}

fn bend_of(f: &Fold) -> Bend {
    let s = Seg2::new(P2::origin(), P2::new(1.0, 0.0));
    Bend {
        on_a: s,
        on_b: s,
        angle: f.angle,
        toward_material: f.toward,
        radius: f.radius,
        model_radius: f.model_radius,
        value: f.value,
        hem: false,
    }
}

/// The turn a fold makes (about its axis by its angle), and the map of the moving side's points
/// (shifted back by the allowance first, then turned).
fn fold_maps(w: &Wall, f: &Fold, t: f64, allowance: f64) -> (Rigid, Rigid) {
    let n = w.surface.normal().expect("planar");
    let c3 = w.surface.direction_at(f.a, f.nn);
    let side = if f.toward { n } else { -n };
    let rad = if f.toward { f.radius + t } else { f.radius };
    let tp = w.surface.point(f.a + f.nn * f.d1);
    let axis_pt = tp + side * rad;
    let turn = Rigid::about(axis_pt, c3.cross(&side), f.angle);
    let moving = turn.after(&Rigid::translation(-c3 * allowance));
    (turn, moving)
}

/// Splits wall `wall` along a resolved fold: the held side keeps the wall, the moving side becomes
/// wall `new_wall`, turned with everything joined to it.
fn fold_wall(m: &mut Model, wall: WallId, f: &Fold, new_wall: WallId, joint_seed: (u64, u32)) -> Result<BendMade, EditError> {
    let t = m.params.thickness;
    let w = m.wall(wall).ok_or(EditError::NoWall)?.clone();
    if !matches!(w.surface, Surface::Planar { .. }) {
        return Err(EditError::NotPlanar);
    }
    if !(f.angle > 1e-6 && f.angle < std::f64::consts::TAU - 1e-6) {
        return Err(EditError::BadAngle);
    }
    let ba = bend_of(f).allowance(&m.params).ok_or(EditError::BadAngle)?;
    if !(ba.is_finite() && ba >= 0.0) {
        return Err(EditError::BadAngle);
    }
    let d = V2::new(f.nn.y, -f.nn.x);
    let p0 = f.a + f.nn * f.d1;
    let p1 = f.a + f.nn * (f.d1 + ba);
    let held = half(&w.outline, p0, -f.nn);
    let moving = half(&w.outline, p1, f.nn);
    if held.is_empty() && moving.is_empty() {
        return Err(EditError::LineMissesWall);
    }
    if held.is_empty() || moving.is_empty() {
        return Err(EditError::NoRoom);
    }
    if held.len() > 1 || moving.len() > 1 {
        return Err(EditError::TooManyPieces);
    }
    // The bend's stretches: where both tangent lines cross material.
    let spans = overlap(&line_intervals(&w.outline, p0, d), &line_intervals(&w.outline, p1, d));
    if spans.is_empty() {
        return Err(EditError::NoRoom);
    }
    let (turn, map) = fold_maps(&w, f, t, ba);
    // Joints on the wall: on the held side they stay, on the moving side they move to the new
    // wall (their walls turn too); across the band they are in the way.
    let tol = 1e-6 * size_of(&w.outline);
    let mut starts: Vec<WallId> = Vec::new();
    let mut moved_joints: Vec<JointId> = Vec::new();
    for j in &m.joints {
        let Some(seg) = j.segment_on(wall) else { continue };
        let s = |p: P2| (p - f.a).dot(&f.nn);
        let (sa, sb) = (s(seg.a), s(seg.b));
        if sa.max(sb) <= f.d1 + tol {
            continue;
        }
        if sa.min(sb) >= f.d1 + ba - tol {
            moved_joints.push(j.id);
            starts.push(j.other(wall));
            continue;
        }
        return Err(EditError::CrossesJoint);
    }
    let carried = reachable(m, &starts, &[wall], &moved_joints)?;
    for j in m.joints.iter_mut().filter(|j| moved_joints.contains(&j.id)) {
        let seg = segment_on_mut(j, wall).expect("on the wall");
        *seg = seg.offset(-f.nn * ba);
        if j.a == wall {
            j.a = new_wall;
        } else {
            j.b = new_wall;
        }
    }
    for wid in &carried {
        if let Some(cw) = m.walls.iter_mut().find(|x| x.id == *wid) {
            cw.surface = map_surface(cw.surface, &map);
        }
    }
    let wi = m.walls.iter().position(|x| x.id == wall).expect("wall");
    m.walls[wi].outline = held.into_iter().next().expect("one");
    m.walls.push(Wall {
        id: new_wall,
        surface: map_surface(w.surface, &turn),
        outline: moving.into_iter().next().expect("one").map(|p| p - f.nn * ba),
    });
    let mut names = namer(m);
    let mut joints = Vec::new();
    let mut k = joint_seed.1;
    for (lo, hi) in spans {
        let seg = Seg2::new(p0 + d * lo, p0 + d * hi);
        let id = free_joint(m, joint_seed.0, &mut k);
        let bend = Bend { on_a: seg, on_b: seg, ..bend_of(f) };
        let kind = JointKind::Bend(bend);
        let name = names.name(&kind);
        m.joints.push(Joint { id, name, a: wall, b: new_wall, kind });
        joints.push(id);
    }
    Ok(BendMade { wall: new_wall, joints, moved: carried, allowance: ba })
}

/// The line through `spec.line` on the wall (local point and unit direction).
fn line_on(w: &Wall, line: (P3, P3)) -> Result<(P2, V2), EditError> {
    let (a, b) = (w.surface.local(line.0), w.surface.local(line.1));
    let d = b - a;
    if d.norm() < 1e-9 {
        return Err(EditError::BadLine);
    }
    Ok((a, d / d.norm()))
}

/// Resolves a Bend's settings on its wall: the side that moves (the smaller side, unless Hold
/// opposite side), the radius, the value and where the bend region starts.
fn resolve(m: &Model, spec: &BendSpec) -> Result<Fold, EditError> {
    let t = m.params.thickness;
    let w = m.wall(spec.wall).ok_or(EditError::NoWall)?;
    if !matches!(w.surface, Surface::Planar { .. }) {
        return Err(EditError::NotPlanar);
    }
    if !(spec.angle > 1e-6 && spec.angle < std::f64::consts::TAU - 1e-6) {
        return Err(EditError::BadAngle);
    }
    let (a, d) = line_on(w, spec.line)?;
    let mut nn = perp(d);
    let area = |n: V2| half(&w.outline, a, n).iter().map(|p| p.area()).sum::<f64>();
    let (pos, neg) = (area(nn), area(-nn));
    if pos < 1e-9 || neg < 1e-9 {
        return Err(EditError::LineMissesWall);
    }
    // The smaller side moves (the larger stays put), unless Hold opposite side.
    if pos > neg {
        nn = -nn;
    }
    if spec.hold_opposite {
        nn = -nn;
    }
    let radius = spec.radius.unwrap_or(m.params.bend_radius);
    let mut f = Fold {
        a,
        nn,
        d1: 0.0,
        angle: spec.angle,
        toward: spec.toward_material,
        radius,
        model_radius: spec.radius.is_none(),
        value: spec.k_factor.map(BendValue::KFactor),
    };
    let ba = bend_of(&f).allowance(&m.params).ok_or(EditError::BadAngle)?;
    f.d1 = match spec.alignment {
        BendAlignment::BendLine => -ba / 2.0,
        BendAlignment::HoldLine => 0.0,
        BendAlignment::HoldOtherLine => -ba,
        al => {
            // Folded: with the band starting at the line, where does the chosen face of the bent
            // wall pass the line? Moving the band along the wall moves that face as much.
            let n = w.surface.normal().expect("planar");
            let (turn, _) = fold_maps(w, &f, t, ba);
            let c3 = w.surface.direction_at(a, nn);
            let p1 = turn.point(w.surface.point(a));
            let n2 = turn.vec(n);
            let o = match al {
                BendAlignment::Inner => {
                    if f.toward {
                        t
                    } else {
                        0.0
                    }
                }
                BendAlignment::Outer => {
                    if f.toward {
                        0.0
                    } else {
                        t
                    }
                }
                _ => t / 2.0,
            };
            let q = p1 + n2 * o;
            let l3 = w.surface.point(a) + n * spec.line_height;
            let denom = c3.dot(&n2);
            if denom.abs() < 1e-6 {
                -ba / 2.0
            } else {
                (l3 - q).dot(&n2) / denom
            }
        }
    };
    Ok(f)
}

/// A bend's frame on its wall (SM9.5, Align to geometry and Angle from direction): the unit
/// direction the moving side runs in, in the wall's plane away from the line, and the unit
/// direction it turns towards. Bent by θ, the wall runs along `cos θ·c + sin θ·side`.
pub fn bend_frame(m: &Model, spec: &BendSpec) -> Result<(V3, V3), EditError> {
    let f = resolve(m, &BendSpec { angle: std::f64::consts::FRAC_PI_2, alignment: BendAlignment::HoldLine, ..*spec })?;
    let w = m.wall(spec.wall).ok_or(EditError::NoWall)?;
    let n = w.surface.normal().ok_or(EditError::NotPlanar)?;
    Ok((w.surface.direction_at(f.a, f.nn), if f.toward { n } else { -n }))
}

/// **Bend** (SM9): folds wall `spec.wall` along the line. The new wall gets an id from `seed`,
/// and the bend joints too.
pub fn bend_wall(m: &mut Model, spec: &BendSpec, seed: u64) -> Result<BendMade, EditError> {
    let f = resolve(m, spec)?;
    let mut k = 0;
    let new_wall = free_wall(m, seed, &mut k);
    fold_wall(m, spec.wall, &f, new_wall, (seed ^ 0x4a4f_494e_5400_0000, 0))
}

// ---------------------------------------------------------------------------------------------
// Jog (SM19.1)

/// How the jog offset is measured (SM19.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum JogAnchor {
    /// From the face the jog moves towards to the far wall's inner face.
    #[default]
    Inside,
    /// From the other face to the far wall's outer face (the overall height).
    Nominal,
    /// From the face the jog moves towards to the far wall's outer face.
    Outside,
}

impl JogAnchor {
    pub const ALL: [JogAnchor; 3] = [JogAnchor::Inside, JogAnchor::Nominal, JogAnchor::Outside];

    pub fn label(self) -> &'static str {
        match self {
            JogAnchor::Inside => "Inside",
            JogAnchor::Nominal => "Nominal",
            JogAnchor::Outside => "Outside",
        }
    }
}

/// A Jog's settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct JogSpec {
    /// The first bend (its line, side, alignment, angle, radius and K factor; the second bend
    /// turns back by the same angle).
    pub bend: BendSpec,
    /// The jog offset, measured as `anchor` says.
    pub offset: f64,
    pub anchor: JogAnchor,
    /// The flat doesn't grow (else the sheet is stretched so its far end stays put).
    pub preserve_material: bool,
}

/// What a jog made.
#[derive(Clone, Debug, PartialEq)]
pub struct JogMade {
    pub first: BendMade,
    pub second: BendMade,
    /// The flat length of the wall between the bends.
    pub middle: f64,
    /// How much material was added (0 with Preserve material).
    pub added: f64,
}

/// Stretches wall `wall` by `by` across the line through `a` with normal `nn` at `s0`: the part
/// beyond moves out by `by` (and the walls joined to it with it), the gap filled.
fn stretch_wall(m: &mut Model, wall: WallId, a: P2, nn: V2, s0: f64, by: f64) -> Result<(), EditError> {
    if by.abs() < 1e-12 {
        return Ok(());
    }
    let w = m.wall(wall).ok_or(EditError::NoWall)?.clone();
    let tol = 1e-9 * size_of(&w.outline);
    let s = |p: P2| (p - a).dot(&nn) - s0;
    let mut starts = Vec::new();
    let mut moved_joints = Vec::new();
    for j in &m.joints {
        let Some(seg) = j.segment_on(wall) else { continue };
        let (sa, sb) = (s(seg.a), s(seg.b));
        if sa.max(sb) <= tol {
            continue;
        }
        if sa.min(sb) >= -tol {
            moved_joints.push(j.id);
            starts.push(j.other(wall));
            continue;
        }
        return Err(EditError::CrossesJoint);
    }
    let carried = reachable(m, &starts, &[wall], &moved_joints)?;
    let shift3 = w.surface.direction_at(a, nn) * by;
    for j in m.joints.iter_mut().filter(|j| moved_joints.contains(&j.id)) {
        let seg = segment_on_mut(j, wall).expect("on the wall");
        *seg = seg.offset(nn * by);
    }
    for wid in &carried {
        if let Some(cw) = m.walls.iter_mut().find(|x| x.id == *wid) {
            cw.surface = map_surface(cw.surface, &Rigid::translation(shift3));
        }
    }
    let stretch_loop = |l: &[P2]| -> Vec<P2> {
        let n = l.len();
        let cls = |p: P2| {
            let v = s(p);
            if v > tol {
                1
            } else if v < -tol {
                -1
            } else {
                0
            }
        };
        let mut out = Vec::with_capacity(n + 4);
        for i in 0..n {
            let (prev, p, next) = (l[(i + n - 1) % n], l[i], l[(i + 1) % n]);
            match cls(p) {
                1 => out.push(p + nn * by),
                -1 => out.push(p),
                _ => match (cls(prev) == 1, cls(next) == 1) {
                    (true, true) => out.push(p + nn * by),
                    (true, false) => {
                        out.push(p + nn * by);
                        out.push(p);
                    }
                    (false, true) => {
                        out.push(p);
                        out.push(p + nn * by);
                    }
                    (false, false) => out.push(p),
                },
            }
            // An edge crossing the line strictly: the gap's sides.
            let (cp, cq) = (cls(p), cls(next));
            if cp * cq == -1 {
                let (sp, sq) = (s(p), s(next));
                let c = p + (next - p) * (sp / (sp - sq));
                if cp < 0 {
                    out.push(c);
                    out.push(c + nn * by);
                } else {
                    out.push(c + nn * by);
                    out.push(c);
                }
            }
        }
        out.dedup_by(|x, y| (*x - *y).norm() < 1e-12);
        out
    };
    let wi = m.walls.iter().position(|x| x.id == wall).expect("wall");
    let o = &w.outline;
    m.walls[wi].outline = Polygon::with_holes(stretch_loop(&o.outer), o.holes.iter().map(|h| stretch_loop(h)).collect()).normalized();
    Ok(())
}

/// **Jog** (SM19.1): two opposite bends with a wall between them, the far wall parallel to the
/// first and `offset` away (measured from `anchor`) towards the side the first bend turns to.
pub fn jog_wall(m: &mut Model, spec: &JogSpec, seed: u64) -> Result<JogMade, EditError> {
    let t = m.params.thickness;
    let f1 = resolve(m, &spec.bend)?;
    if f1.angle.sin().abs() < 1e-3 {
        return Err(EditError::BadAngle);
    }
    let wall = spec.bend.wall;
    let w0 = m.wall(wall).ok_or(EditError::NoWall)?.clone();
    let n = w0.surface.normal().expect("planar");
    let side = if f1.toward { n } else { -n };
    // The far wall's definition surface this far over (along `side`).
    let target = match spec.anchor {
        JogAnchor::Inside => spec.offset + t,
        JogAnchor::Outside => spec.offset,
        JogAnchor::Nominal => spec.offset - t,
    };
    let mut ids = 0;
    let (mid_id, far_id) = (free_wall(m, seed, &mut ids), free_wall(m, seed, &mut ids));
    let build = |m: &Model, middle: f64, stretch: f64| -> Result<(Model, BendMade, BendMade), EditError> {
        let mut mm = m.clone();
        stretch_wall(&mut mm, wall, f1.a, f1.nn, f1.d1, stretch)?;
        let b1 = fold_wall(&mut mm, wall, &f1, mid_id, (seed ^ 0x4a4f_4731, 0))?;
        let f2 = Fold { d1: f1.d1 + middle, toward: !f1.toward, ..f1 };
        let b2 = fold_wall(&mut mm, mid_id, &f2, far_id, (seed ^ 0x4a4f_4732, 0))?;
        Ok((mm, b1, b2))
    };
    let over = |mm: &Model| -> f64 {
        let far = mm.wall(far_id).expect("far wall");
        (far.surface.point(P2::origin()) - w0.surface.point(P2::origin())).dot(&side)
    };
    // The far wall's offset grows linearly with the middle wall's length: probe two lengths.
    let (m1, ..) = build(m, 1.0, 0.0)?;
    let (m2, ..) = build(m, 2.0, 0.0)?;
    let (o1, slope) = (over(&m1), over(&m2) - over(&m1));
    if slope.abs() < 1e-6 {
        return Err(EditError::BadAngle);
    }
    let middle = 1.0 + (target - o1) / slope;
    if middle < 1e-6 {
        return Err(EditError::JogTooSmall);
    }
    let (m1, b1, b2) = build(m, middle, 0.0)?;
    let mut added = 0.0;
    let result = if spec.preserve_material {
        (m1, b1, b2)
    } else {
        // How far the far end came back: stretch the sheet by that much and jog again.
        let c3 = w0.surface.direction_at(f1.a, f1.nn);
        let x = f1.a + f1.nn * (f1.d1 + b1.allowance + middle + b2.allowance + 1.0);
        let far = m1.wall(far_id).expect("far wall");
        let now = far.surface.point(x - f1.nn * (b1.allowance + b2.allowance));
        let back = -(now - w0.surface.point(x)).dot(&c3);
        if back > 1e-9 {
            added = back;
            build(m, middle, back)?
        } else {
            (m1, b1, b2)
        }
    };
    let (mm, first, second) = result;
    *m = mm;
    Ok(JogMade { first, second, middle, added })
}

// ---------------------------------------------------------------------------------------------
// Profiles in space

/// A planar region in space: `polygon` in the frame `origin + x·px + y·py`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region3 {
    pub origin: P3,
    pub x: V3,
    pub y: V3,
    pub polygon: Polygon,
}

impl Region3 {
    pub fn point(&self, p: P2) -> P3 {
        self.origin + self.x * p.x + self.y * p.y
    }

    pub fn normal(&self) -> V3 {
        self.x.cross(&self.y).normalize()
    }

    /// The region projected (perpendicular) into a planar wall's local 2D.
    fn on_wall(&self, w: &Wall) -> Polygon {
        let s = w.surface;
        self.polygon.map(|p| s.local(self.point(p)))
    }
}

fn parallel(w: &Wall, n: V3) -> bool {
    w.surface.normal().is_some_and(|wn| wn.dot(&n).abs() > 1.0 - 1e-6)
}

/// A polygon grown by `by` all round, its corners mitred as an offset of the outline (so a
/// rectangle grows into a rectangle; a corner sharper than about 30° is rounded instead, so the
/// mitre doesn't run far out). `by ≤ 0` returns it as it is.
pub fn grow(p: &Polygon, by: f64) -> Polygon {
    if by <= 1e-12 {
        return p.clone();
    }
    let mut parts = vec![p.clone()];
    for l in std::iter::once(&p.outer).chain(p.holes.iter()) {
        let n = l.len();
        for i in 0..n {
            let (a, b) = (l[i], l[(i + 1) % n]);
            let d = b - a;
            if d.norm() < 1e-12 {
                continue;
            }
            let nn = perp(d.normalize()) * by;
            parts.push(Polygon::new(vec![a - nn, b - nn, b + nn, a + nn]));
            // The corner at `a`: material lies left of each edge (outer counter-clockwise, holes
            // clockwise), so away from it is the right-hand normal. A left turn is a convex
            // corner of the material: fill it with the mitre.
            let prev = l[(i + n - 1) % n];
            let e1 = a - prev;
            if e1.norm() < 1e-12 {
                continue;
            }
            let (u1, u2) = (e1.normalize(), d.normalize());
            if u1.perp(&u2) <= 1e-12 {
                continue;
            }
            let (n1, n2) = (-perp(u1), -perp(u2));
            let c = 1.0 + n1.dot(&n2);
            if c < 0.27 {
                // Sharper than ~30°: rounded.
                parts.push(poly::circle(a, by, 24));
                continue;
            }
            let tip = a + (n1 + n2) * (by / c);
            parts.push(Polygon::new(vec![a, a + n1 * by, tip, a + n2 * by]));
        }
    }
    let refs: Vec<&Polygon> = parts.iter().collect();
    exact(poly::union(&parts), &refs).into_iter().max_by(|a, b| a.area().total_cmp(&b.area())).unwrap_or_else(|| p.clone())
}

// ---------------------------------------------------------------------------------------------
// Tab (SM5)

/// **Tab** (SM5): each region added to the walls in `walls` it is parallel to and touches. Walls
/// of one plane that the tab bridges become one wall. Returns the walls that took it.
pub fn add_tab(m: &mut Model, regions: &[Region3], walls: &[WallId]) -> Result<Vec<WallId>, EditError> {
    let mut took: Vec<WallId> = Vec::new();
    for r in regions {
        let n = r.normal();
        let mut hit: Vec<WallId> = Vec::new();
        for wid in walls {
            let Some(wi) = m.walls.iter().position(|w| w.id == *wid) else { continue };
            if !parallel(&m.walls[wi], n) {
                continue;
            }
            // The profile must lie in (or on a face of) the wall's sheet.
            let w = &m.walls[wi];
            let off = (r.origin - w.surface.point(P2::origin())).dot(&w.surface.normal().expect("planar"));
            if off.abs() > m.params.thickness + 1e-6 {
                continue;
            }
            let tab = r.on_wall(w);
            let u = exact(poly::union(&[w.outline.clone(), tab.clone()]), &[&w.outline, &tab]);
            if u.len() != 1 {
                continue;
            }
            m.walls[wi].outline = u.into_iter().next().expect("one");
            hit.push(*wid);
        }
        // Bridged walls of one plane: merge them into the first.
        let mut i = 0;
        while i < hit.len() {
            let mut k = i + 1;
            while k < hit.len() {
                let (a, b) = (m.wall(hit[i]).expect("a").clone(), m.wall(hit[k]).expect("b").clone());
                let (na, nb) = (a.surface.normal().expect("planar"), b.surface.normal().expect("planar"));
                let coplanar = (b.surface.point(P2::origin()) - a.surface.point(P2::origin())).dot(&na).abs() < 1e-6;
                let b_on_a = b.outline.map(|p| a.surface.local(b.surface.point(p)));
                if coplanar && poly::overlap_area(&a.outline, &b_on_a) > 1e-9 {
                    if na.dot(&nb) < 0.0 {
                        return Err(EditError::OppositeSides);
                    }
                    merge_walls(m, a.id, b.id);
                    hit.remove(k);
                } else {
                    k += 1;
                }
            }
            i += 1;
        }
        for h in hit {
            if !took.contains(&h) {
                took.push(h);
            }
        }
    }
    if took.is_empty() {
        return Err(EditError::NoTab);
    }
    Ok(took)
}

/// The sheet a tab region lies in: the span of the material, measured along the region's normal
/// from its plane, of the parallel wall of `walls` its profile overlaps (the Tab's clearance
/// pocket runs through it, SM5.4). `None` if it is on none of them.
pub fn tab_sheet(m: &Model, r: &Region3, walls: &[WallId]) -> Option<(f64, f64)> {
    let n = r.normal();
    let t = m.params.thickness;
    let mut best: Option<((f64, f64), f64)> = None;
    for w in m.walls.iter().filter(|w| walls.contains(&w.id)) {
        let Some(wn) = w.surface.normal() else { continue };
        if wn.dot(&n).abs() < 1.0 - 1e-6 {
            continue;
        }
        let z0 = (w.surface.point(P2::origin()) - r.origin).dot(&n);
        let z1 = z0 + t * wn.dot(&n).signum();
        if z0.min(z1) > 1e-6 || z0.max(z1) < -1e-6 {
            continue;
        }
        let over = poly::overlap_area(&w.outline, &r.on_wall(w));
        if over > 1e-9 && best.is_none_or(|(_, a)| over > a) {
            best = Some(((z0.min(z1), z0.max(z1)), over));
        }
    }
    best.map(|(z, _)| z)
}

/// Wall `b` (coplanar with `a`, same material side) merged into `a`.
fn merge_walls(m: &mut Model, a: WallId, b: WallId) {
    let wa = m.wall(a).expect("a").clone();
    let wb = m.wall(b).expect("b").clone();
    let to_a = |p: P2| wa.surface.local(wb.surface.point(p));
    let bb = wb.outline.map(to_a);
    let u = exact(poly::union(&[wa.outline.clone(), bb.clone()]), &[&wa.outline, &bb]);
    if let Some(big) = u.into_iter().max_by(|x, y| x.area().total_cmp(&y.area())) {
        m.walls.iter_mut().find(|w| w.id == a).expect("a").outline = big;
    }
    for j in &mut m.joints {
        if j.a != b && j.b != b {
            continue;
        }
        let seg = segment_on_mut(j, b).expect("on b");
        *seg = Seg2::new(to_a(seg.a), to_a(seg.b));
        if j.a == b {
            j.a = a;
        } else {
            j.b = a;
        }
    }
    m.joints.retain(|j| j.a != j.b);
    m.walls.retain(|w| w.id != b);
    if m.fixed == Some(b) {
        m.fixed = Some(a);
    }
}

// ---------------------------------------------------------------------------------------------
// Cuts (SM1.6, SM12.1)

/// A cutting tool: a profile swept along `dir` (unit), over `z` along it from the profile's
/// plane (`None`: all the way through).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CutTool {
    pub region: Region3,
    pub dir: V3,
    pub z: Option<(f64, f64)>,
}

/// The tool's cross-section on a planar wall, in its local 2D (at the sheet's mid plane).
fn section(tool: &CutTool, w: &Wall, t: f64) -> Vec<Polygon> {
    let Some(nw) = w.surface.normal() else { return Vec::new() };
    let dn = tool.dir.dot(&nw);
    if dn.abs() < 1e-6 {
        return Vec::new();
    }
    let mid = w.surface.point(P2::origin()) + nw * (t / 2.0);
    let onto = |p: P3| {
        let tau = (mid - p).dot(&nw) / dn;
        w.surface.local(p + tool.dir * tau)
    };
    let mut pieces = vec![tool.region.polygon.map(|p| onto(tool.region.point(p)))];
    if let Some((z0, z1)) = tool.z {
        // How far along the tool a point of the wall is: affine in the wall's 2D.
        let nr = tool.region.normal();
        let k = tool.dir.dot(&nr);
        if k.abs() > 1e-9 {
            let at = |p: P2| (w.surface.point(p) + nw * (t / 2.0) - tool.region.origin).dot(&nr) / k;
            let o = at(P2::origin());
            let g = V2::new(at(P2::new(1.0, 0.0)) - o, at(P2::new(0.0, 1.0)) - o);
            if g.norm() < 1e-9 {
                if o < z0 - t || o > z1 + t {
                    return Vec::new();
                }
            } else {
                let gg = g.norm_squared();
                // τ ≥ z0 − t/2 and τ ≤ z1 + t/2 (the sheet's thickness counts).
                let lo = P2::from(g * ((z0 - t / 2.0 - o) / gg));
                let hi = P2::from(g * ((z1 + t / 2.0 - o) / gg));
                pieces = pieces.iter().flat_map(|p| half(p, lo, g)).collect();
                pieces = pieces.iter().flat_map(|p| half(p, hi, -g)).collect();
            }
        }
    }
    pieces.into_iter().filter(|p| p.area() > 1e-9).collect()
}

/// What a cut did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutMade {
    /// The walls cut.
    pub walls: Vec<WallId>,
    /// Walls the cut split: each extra piece is a wall of its own (the biggest keeps the id).
    pub split: Vec<WallId>,
    /// The bends whose regions the cut crossed (taken out in the flat, [`crate::flat_edit::FlatCut`]).
    pub bends: Vec<JointId>,
}

impl CutMade {
    pub fn is_empty(&self) -> bool {
        self.walls.is_empty() && self.bends.is_empty()
    }
}

/// Takes the tools' cross-sections out of the planar walls they cross (perpendicular to each
/// wall), and out of the bend regions between them (SM12.1): what a tool takes from a bend's mid
/// surface, in the bend's `(s, u)`, goes into the model as a flat cut ([`crate::flat_edit`]), so
/// the flat and the folded bend both lose it. A wall the cut splits keeps every piece: the
/// biggest keeps its id, each other piece becomes a wall (with the joints along its edges), and
/// pieces no joint holds become parts of their own.
pub fn cut_walls(m: &mut Model, tools: &[CutTool], only: Option<&[WallId]>) -> Result<CutMade, EditError> {
    let t = m.params.thickness;
    let mut made = CutMade::default();
    // The bends first, on the model as it is.
    let bend_cuts = bend_sections(m, tools, only);
    // A part whose bends the cut crosses is cut in the flat as a whole (its walls' outlines stay,
    // so their joints still run along their edges): the walls' sections and the bends' go into
    // one flat cut.
    let flat = (!bend_cuts.is_empty()).then(|| crate::flat::flatten(m));
    let mut in_flat: Vec<(usize, Vec<Polygon>)> = Vec::new();
    if let Some(flat) = &flat {
        for (pi, part) in flat.parts.iter().enumerate() {
            let mut shapes: Vec<Polygon> = Vec::new();
            for (j, su) in &bend_cuts {
                let Some(b) = part.bend(*j) else { continue };
                let e = b.tangent_a.dir();
                let n = {
                    let n = perp(e);
                    if (b.tangent_b.a - b.tangent_a.a).dot(&n) >= 0.0 { n } else { -n }
                };
                shapes.extend(su.iter().map(|p| p.map(|q| b.tangent_a.a + e * q.x + n * q.y)));
                if !made.bends.contains(j) {
                    made.bends.push(*j);
                }
            }
            if !shapes.is_empty() {
                in_flat.push((pi, shapes));
            }
        }
    }
    let via_flat = |w: WallId| -> Option<usize> {
        let flat = flat.as_ref()?;
        in_flat.iter().find(|(pi, _)| flat.parts[*pi].walls.contains(&w)).map(|(pi, _)| *pi)
    };
    let mut flat_walls: Vec<(usize, Polygon)> = Vec::new();
    let mut new_walls: Vec<Wall> = Vec::new();
    for wi in 0..m.walls.len() {
        let w = &m.walls[wi];
        if only.is_some_and(|o| !o.contains(&w.id)) {
            continue;
        }
        let shapes: Vec<Polygon> = tools.iter().flat_map(|tool| section(tool, w, t)).collect();
        if shapes.is_empty() || shapes.iter().all(|s| poly::overlap_area(&w.outline, s) < 1e-9) {
            continue;
        }
        if let (Some(pi), Some(flat)) = (via_flat(w.id), flat.as_ref()) {
            // Into the flat: the wall's own flat 2D placed in the part.
            let Some(pm) = flat.parts[pi].placement(w.id) else { continue };
            let params = m.params;
            // Only what lies on the wall (past its edges, the flat holds other pieces).
            for sh in shapes.iter().flat_map(|sh| poly::intersection(sh, &w.outline)) {
                flat_walls.push((pi, sh.map(|q| pm.apply(w.flat_local(&params, q)))));
            }
            made.walls.push(w.id);
            continue;
        }
        let mut inputs: Vec<&Polygon> = vec![&w.outline];
        inputs.extend(shapes.iter());
        let mut left: Vec<Polygon> = exact(poly::difference(std::slice::from_ref(&w.outline), &shapes), &inputs).into_iter().filter(|p| p.area() > 1e-9).collect();
        if left.is_empty() {
            return Err(EditError::WallCutAway);
        }
        left.sort_by(|a, b| b.area().total_cmp(&a.area()));
        let id = w.id;
        let surface = w.surface;
        let size = size_of(&w.outline);
        let mut pieces = left.into_iter();
        m.walls[wi].outline = pieces.next().expect("one");
        made.walls.push(id);
        // The other pieces: new walls, ids from the wall's own (the same at every replay).
        let mut k = 0;
        for piece in pieces {
            let nid = loop {
                let c = WallId(derived_id(0x5350_4c49_5400_0000 ^ u64::from(id.0), k));
                k += 1;
                if m.wall(c).is_none() && !new_walls.iter().any(|w| w.id == c) {
                    break c;
                }
            };
            // The joints along this piece's edges go with it.
            let tol = 1e-6 * size + 1e-9;
            for j in m.joints.iter_mut() {
                let Some(seg) = j.segment_on(id) else { continue };
                let mid = P2::from((seg.a.coords + seg.b.coords) / 2.0);
                if distance_to_outline(&piece, mid) <= tol {
                    if j.a == id {
                        j.a = nid;
                    } else {
                        j.b = nid;
                    }
                }
            }
            new_walls.push(Wall { id: nid, surface, outline: piece });
            made.split.push(nid);
        }
    }
    m.walls.extend(new_walls);
    if let Some(flat) = &flat {
        for (pi, mut shapes) in in_flat {
            shapes.extend(flat_walls.iter().filter(|(p, _)| *p == pi).map(|(_, s)| s.clone()));
            // One region: the pieces of the cut meet along the tangent lines.
            let shapes: Vec<Polygon> = poly::union(&shapes).into_iter().filter(|p| p.area() > 1e-9).collect();
            crate::flat_edit::remove(m, flat, pi, &shapes).map_err(|_| EditError::WallCutAway)?;
        }
    }
    Ok(made)
}

/// Slices across a bend region the cut through it is measured on (exact for cuts whose edges
/// run along or across the bend; others are a staircase this fine).
const BEND_CUT_SLICES: usize = 24;

/// What the tools take out of each bend region (both of whose walls are in `only`), in the bend's
/// `(s, u)`: `s` along the bend from its tangent lines' `a` end, `u` across from wall `a`'s
/// tangent line. Measured on the bend's mid surface, slice by slice across it.
fn bend_sections(m: &Model, tools: &[CutTool], only: Option<&[WallId]>) -> Vec<(JointId, Vec<Polygon>)> {
    let t = m.params.thickness;
    let mut out = Vec::new();
    for j in &m.joints {
        let Some(b) = j.bend() else { continue };
        if b.hem || only.is_some_and(|o| !o.contains(&j.a) || !o.contains(&j.b)) {
            continue;
        }
        let (Some(g), Some(ba)) = (m.bend_geometry(j.id), b.allowance(&m.params)) else { continue };
        if ba <= 1e-9 {
            continue;
        }
        let span = g.ends.1 - g.ends.0;
        let total = span.norm();
        if total <= 1e-9 {
            continue;
        }
        let es = span / total;
        let rm = g.inner_radius + t / 2.0;
        let mut rects: Vec<Polygon> = Vec::new();
        for k in 0..BEND_CUT_SLICES {
            let (u0, u1) = (ba * k as f64 / BEND_CUT_SLICES as f64, ba * (k + 1) as f64 / BEND_CUT_SLICES as f64);
            let a = (u0 + u1) / 2.0 / ba * g.sweep;
            let p0 = g.ends.0 + g.rotate_vec(g.start, a) * rm;
            for tool in tools {
                for (s0, s1) in tool_intervals(tool, p0, es, total, t) {
                    if s1 - s0 > 1e-9 {
                        rects.push(Polygon::rect(P2::new(s0, u0), P2::new(s1, u1)));
                    }
                }
            }
        }
        if rects.is_empty() {
            continue;
        }
        let shapes: Vec<Polygon> = poly::union(&rects).into_iter().filter(|p| p.area() > 1e-9).collect();
        if !shapes.is_empty() {
            out.push((j.id, shapes));
        }
    }
    out
}

/// Where the line `p0 + es·s` (`s` in `0..len`) runs inside a tool's prism (the sheet's
/// thickness counting at its depth limits, as for walls).
fn tool_intervals(tool: &CutTool, p0: P3, es: V3, len: f64, t: f64) -> Vec<(f64, f64)> {
    let r = &tool.region;
    let nr = r.normal();
    let dn = tool.dir.dot(&nr);
    if dn.abs() < 1e-9 {
        return Vec::new();
    }
    // Along the tool onto its profile's plane, in the profile's 2D.
    let onto = |p: P3| {
        let tau = (r.origin - p).dot(&nr) / dn;
        let q = p + tool.dir * tau - r.origin;
        P2::new(q.dot(&r.x), q.dot(&r.y))
    };
    let (q0, q1) = (onto(p0), onto(p0 + es * len));
    let mut ivs: Vec<(f64, f64)> = if (q1 - q0).norm() < 1e-9 * len.max(1.0) {
        if r.polygon.contains(q0) { vec![(0.0, len)] } else { Vec::new() }
    } else {
        let d = q1 - q0;
        poly::clip_segment(Seg2::new(q0, q1), std::slice::from_ref(&r.polygon))
            .into_iter()
            .map(|s| ((s.a - q0).dot(&d) / d.norm_squared() * len, (s.b - q0).dot(&d) / d.norm_squared() * len))
            .map(|(a, b)| (a.min(b).max(0.0), a.max(b).min(len)))
            .collect()
    };
    if let Some((z0, z1)) = tool.z {
        // How far along the tool: affine in s.
        let z = |s: f64| (p0 + es * s - r.origin).dot(&nr) / dn;
        let (za, zb) = (z(0.0), z(len));
        let (lo, hi) = (z0 - t / 2.0, z1 + t / 2.0);
        let clip = |(a, b): (f64, f64)| -> Option<(f64, f64)> {
            if (zb - za).abs() < 1e-12 {
                return (za >= lo && za <= hi).then_some((a, b));
            }
            let (sl, sh) = ((lo - za) / (zb - za) * len, (hi - za) / (zb - za) * len);
            let (sl, sh) = (sl.min(sh), sl.max(sh));
            let (x, y) = (a.max(sl), b.min(sh));
            (y > x).then_some((x, y))
        };
        ivs = ivs.into_iter().filter_map(clip).collect();
    }
    ivs
}

// ---------------------------------------------------------------------------------------------
// Corner break (SM11)

/// A corner break's shape (SM11.2, SM11.3).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CornerBreakKind {
    /// Rounded with this radius.
    Fillet { radius: f64 },
    /// Cut off `d1` along the edge into the corner and `d2` along the edge out of it (the
    /// setbacks from the corner, as Tangent measures them).
    Chamfer { d1: f64, d2: f64 },
    /// A round with radius `r1` on the edge into the corner and `r2` on the edge out of it
    /// (Asymmetric, SM11.2): it leaves the edges `rᵢ / tan(β/2)` from the corner and is the conic
    /// through them tangent to both edges (a quarter ellipse at a square corner); equal radii make
    /// the circular fillet. `overflow` (Allow edge overflow): a round longer than an edge runs on
    /// over the next corner and trims what it meets, instead of failing.
    Round { r1: f64, r2: f64, overflow: bool },
}

/// How far an arc's chord may stray from it (mm): sets how many segments a round gets.
const ARC_SAG: f64 = 0.002;

/// Segments for an arc of radius `r` turning by `turn` (radians).
fn arc_steps(r: f64, turn: f64) -> usize {
    let r = r.abs().max(1e-9);
    let step = if ARC_SAG >= r { std::f64::consts::FRAC_PI_4 } else { 2.0 * (1.0 - ARC_SAG / r).acos() };
    // At least 12 a quarter turn, at most 128.
    let step = step.clamp(std::f64::consts::FRAC_PI_2 / 128.0, std::f64::consts::FRAC_PI_2 / 12.0);
    (turn.abs() / step).ceil().max(2.0) as usize
}

/// The setbacks (along the edge into the corner and out of it) of an Offset chamfer: the faces
/// offset by `d1` (the edge in) and `d2` (the edge out) meet over the chamfer's ends (SM11.2). At
/// a square corner they are the distances themselves.
pub fn chamfer_offset_setbacks(d1: f64, d2: f64, beta: f64) -> (f64, f64) {
    let (s, c) = beta.sin_cos();
    let s = s.max(1e-9);
    ((d2 + d1 * c) / s, (d1 + d2 * c) / s)
}

/// The interior angle at outline vertex `i` of `l` (between the edges to its neighbours, 0–π),
/// with the unit directions in (from the previous vertex) and out (to the next).
fn corner_angle(l: &[P2], i: usize) -> Option<(f64, V2, V2)> {
    let n = l.len();
    let (p, v, q) = (l[(i + n - 1) % n], l[i], l[(i + 1) % n]);
    let (e1, e2) = (v - p, q - v);
    if e1.norm() < 1e-12 || e2.norm() < 1e-12 {
        return None;
    }
    let (e1, e2) = (e1.normalize(), e2.normalize());
    let beta = (-e1).dot(&e2).clamp(-1.0, 1.0).acos();
    (beta > 1e-6 && beta < std::f64::consts::PI - 1e-6).then_some((beta, e1, e2))
}

/// A Fillet's radius for a Width (the chord across the round) at a corner of interior angle
/// `beta`.
pub fn radius_for_width(width: f64, beta: f64) -> f64 {
    width / (2.0 * (beta / 2.0).cos()).max(1e-9)
}

/// The second distance of a Distance and angle chamfer: `d` along the edge into the corner, the
/// chamfer at `angle` to that edge.
pub fn chamfer_second(d: f64, angle: f64, beta: f64) -> f64 {
    d * angle.sin() / (beta + angle).sin().max(1e-9)
}

/// The vertex of wall `wall`'s outline nearest `at` (local 2D), as (loop, index): 0 the outer
/// loop, k the k-th hole.
fn vertex_index(w: &Wall, at: P2, tol: f64) -> Option<(usize, usize)> {
    let loops: Vec<&Vec<P2>> = std::iter::once(&w.outline.outer).chain(w.outline.holes.iter()).collect();
    let mut best: Option<((usize, usize), f64)> = None;
    for (li, l) in loops.iter().enumerate() {
        for (i, p) in l.iter().enumerate() {
            let d = (p - at).norm();
            if d <= tol && best.is_none_or(|(_, b)| d < b) {
                best = Some(((li, i), d));
            }
        }
    }
    best.map(|(x, _)| x)
}

/// The interior angle of wall `wall`'s corner at `at` (for Width and Distance and angle).
pub fn corner_beta(m: &Model, wall: WallId, at: P2) -> Option<f64> {
    let w = m.wall(wall)?;
    let (li, i) = vertex_index(w, at, 1e-6 * size_of(&w.outline).max(1.0))?;
    let l = if li == 0 { &w.outline.outer } else { &w.outline.holes[li - 1] };
    corner_angle(l, i).map(|(b, _, _)| b)
}

/// The points of a round at corner `v` (edge directions `e1` in, `e2` out, interior angle
/// `beta`), from its end on the edge in to its end on the edge out. Fails if it is longer than
/// either edge (unless `overflow`).
#[allow(clippy::too_many_arguments)]
fn round_points(v: P2, e1: V2, e2: V2, beta: f64, r1: f64, r2: f64, len_in: f64, len_out: f64, overflow: bool) -> Result<Vec<P2>, EditError> {
    if !(r1 > 0.0 && r2 > 0.0) {
        return Err(EditError::CornerTooBig);
    }
    let tb = (beta / 2.0).tan();
    let (s1, s2) = (r1 / tb, r2 / tb);
    if !overflow && (s1 > len_in + 1e-9 || s2 > len_out + 1e-9) {
        return Err(EditError::CornerTooBig);
    }
    let (a, b) = (v - e1 * s1, v + e2 * s2);
    let turn = std::f64::consts::PI - beta;
    if (r1 - r2).abs() <= 1e-12 {
        // The circular fillet.
        let radius = r1;
        let bis = (e2 - e1).normalize();
        let c = v + bis * (radius / (beta / 2.0).sin());
        let (a0, a1) = ((a - c).y.atan2((a - c).x), (b - c).y.atan2((b - c).x));
        let mut sweep = a1 - a0;
        while sweep > std::f64::consts::PI {
            sweep -= std::f64::consts::TAU;
        }
        while sweep < -std::f64::consts::PI {
            sweep += std::f64::consts::TAU;
        }
        let steps = arc_steps(radius, turn);
        let mut pts: Vec<P2> = (0..=steps).map(|k| c + V2::new((a0 + sweep * k as f64 / steps as f64).cos(), (a0 + sweep * k as f64 / steps as f64).sin()) * radius).collect();
        // The ends exactly on the edges.
        pts[0] = a;
        pts[steps] = b;
        return Ok(pts);
    }
    // The conic: a rational quadratic Bézier from `a` to `b` with control point `v` and weight
    // sin(β/2) (the circle's when the setbacks are equal), sampled evenly in its turning.
    let w = (beta / 2.0).sin();
    let at = |t: f64| {
        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * w * t * (1.0 - t), t * t);
        P2::from((a.coords * b0 + v.coords * b1 + b.coords * b2) / (b0 + b1 + b2))
    };
    let steps = arc_steps(r1.max(r2), turn) * 2;
    Ok((0..=steps).map(|k| at(k as f64 / steps as f64)).collect())
}

/// **Corner break** (SM11): wall `wall`'s outline corner at `at` (local 2D) rounded or cut.
pub fn break_corner(m: &mut Model, wall: WallId, at: P2, kind: CornerBreakKind) -> Result<(), EditError> {
    let wi = m.walls.iter().position(|w| w.id == wall).ok_or(EditError::NoWall)?;
    let w = &m.walls[wi];
    let (li, i) = vertex_index(w, at, 1e-6 * size_of(&w.outline).max(1.0)).ok_or(EditError::NoCorner)?;
    let mut l = if li == 0 { w.outline.outer.clone() } else { w.outline.holes[li - 1].clone() };
    let n = l.len();
    let (beta, e1, e2) = corner_angle(&l, i).ok_or(EditError::NoCorner)?;
    let (p, v, q) = (l[(i + n - 1) % n], l[i], l[(i + 1) % n]);
    let (len_in, len_out) = ((v - p).norm(), (q - v).norm());
    let pts: Vec<P2> = match kind {
        CornerBreakKind::Chamfer { d1, d2 } => {
            if d1 <= 0.0 || d2 <= 0.0 {
                return Err(EditError::CornerTooBig);
            }
            if d1 > len_in + 1e-9 || d2 > len_out + 1e-9 {
                return Err(EditError::CornerTooBig);
            }
            vec![v - e1 * d1, v + e2 * d2]
        }
        CornerBreakKind::Fillet { radius } => round_points(v, e1, e2, beta, radius, radius, len_in, len_out, false)?,
        CornerBreakKind::Round { r1, r2, overflow } => {
            let pts = round_points(v, e1, e2, beta, r1, r2, len_in, len_out, overflow)?;
            let (s1, s2) = ((pts[0] - v).norm(), (pts[pts.len() - 1] - v).norm());
            if s1 > len_in + 1e-9 || s2 > len_out + 1e-9 {
                // Overflow: the round past the corner's edges, cut from the whole outline.
                let mut cut = vec![v];
                cut.extend(pts);
                let cut = Polygon::new(cut);
                let w = &m.walls[wi];
                let left = exact(poly::difference(std::slice::from_ref(&w.outline), std::slice::from_ref(&cut)), &[&w.outline, &cut]);
                let big = left.into_iter().filter(|p| p.area() > 1e-9).max_by(|a, b| a.area().total_cmp(&b.area())).ok_or(EditError::CornerTooBig)?;
                m.walls[wi].outline = big;
                return Ok(());
            }
            pts
        }
    };
    l.splice(i..=i, pts);
    l.dedup_by(|x, y| (*x - *y).norm() < 1e-9);
    while l.len() > 3 && (l[0] - l[l.len() - 1]).norm() < 1e-9 {
        l.pop();
    }
    let w = &mut m.walls[wi];
    if li == 0 {
        w.outline.outer = l;
    } else {
        w.outline.holes[li - 1] = l;
    }
    w.outline = w.outline.clone().normalized();
    Ok(())
}

/// The wall corner a pick at `p` means: the outline vertex whose edge through the thickness
/// passes nearest `p` (within `tol`), as (wall, vertex in its local 2D).
pub fn corner_vertex_at(m: &Model, p: P3, tol: f64) -> Option<(WallId, P2)> {
    let t = m.params.thickness;
    let mut best: Option<((WallId, P2), f64)> = None;
    for w in &m.walls {
        let Some(n) = w.surface.normal() else { continue };
        for v in w.outline.outer.iter().chain(w.outline.holes.iter().flatten()) {
            let a = w.surface.point(*v);
            let s = (p - a).dot(&n).clamp(0.0, t);
            let d = (p - (a + n * s)).norm();
            if d <= tol && best.is_none_or(|(_, b)| d < b) {
                best = Some(((w.id, *v), d));
            }
        }
    }
    best.map(|(x, _)| x)
}

// ---------------------------------------------------------------------------------------------
// Picks

/// The planar wall a point on one of its two faces lies on, with whether it is on the face away
/// from the definition surface (the "other" face).
pub fn wall_at(m: &Model, p: P3, tol: f64) -> Option<(WallId, bool)> {
    let t = m.params.thickness;
    let mut best: Option<((WallId, bool), f64)> = None;
    for w in &m.walls {
        let Some(n) = w.surface.normal() else { continue };
        let z = (p - w.surface.point(P2::origin())).dot(&n);
        let (d, other) = if (z - t).abs() < z.abs() { ((z - t).abs(), true) } else { (z.abs(), false) };
        if d > tol {
            continue;
        }
        let local = w.surface.local(p);
        let inside = w.outline.contains(local) || w.outline.bounds().is_some_and(|_| distance_to_outline(&w.outline, local) <= tol);
        if inside && best.is_none_or(|(_, b)| d < b) {
            best = Some(((w.id, other), d));
        }
    }
    best.map(|(x, _)| x)
}

fn distance_to_outline(p: &Polygon, q: P2) -> f64 {
    std::iter::once(&p.outer)
        .chain(p.holes.iter())
        .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
        .map(|(a, b)| {
            let d = b - a;
            let s = ((q - a).dot(&d) / d.norm_squared().max(1e-300)).clamp(0.0, 1.0);
            (a + d * s - q).norm()
        })
        .fold(f64::INFINITY, f64::min)
}

/// The 3D point of a bend's end on its wall `a` and wall `b`.
fn bend_end_points(m: &Model, j: &Joint, end: BendEnd) -> Option<[P3; 2]> {
    let b = j.bend()?;
    let (wa, wb) = (m.wall(j.a)?, m.wall(j.b)?);
    let pick = |s: &Seg2| if end == BendEnd::Start { s.a } else { s.b };
    Some([wa.surface.point(pick(&b.on_a)), wb.surface.point(pick(&b.on_b))])
}

/// The corner (two bends meeting, SM7) nearest `p`, and how far it is.
pub fn corner_near(m: &Model, flat: &FlatPattern, p: P3) -> Option<((JointId, JointId), f64)> {
    let mut best: Option<((JointId, JointId), f64)> = None;
    for c in flat.parts.iter().flat_map(|part| part.corners.iter()) {
        let (Some(j0), Some(j1)) = (m.joint(c.bends.0), m.joint(c.bends.1)) else { continue };
        let (Some(a), Some(b)) = (bend_end_points(m, j0, c.ends.0), bend_end_points(m, j1, c.ends.1)) else { continue };
        let centre = P3::from((a[0].coords + a[1].coords + b[0].coords + b[1].coords) / 4.0);
        let d = a.iter().chain(b.iter()).map(|q| (q - p).norm()).fold((centre - p).norm(), f64::min);
        if best.is_none_or(|(_, x)| d < x) {
            best = Some((c.bends, d));
        }
    }
    best
}

/// The bend end (SM8) nearest `p`, and how far it is.
pub fn bend_end_near(m: &Model, p: P3) -> Option<((JointId, BendEnd), f64)> {
    let mut best: Option<((JointId, BendEnd), f64)> = None;
    for j in m.joints.iter().filter(|j| j.bend().is_some_and(|b| !b.hem)) {
        for end in [BendEnd::Start, BendEnd::End] {
            let Some(pts) = bend_end_points(m, j, end) else { continue };
            let mid = P3::from((pts[0].coords + pts[1].coords) / 2.0);
            let d = pts.iter().map(|q| (q - p).norm()).fold((mid - p).norm(), f64::min);
            if best.is_none_or(|(_, x)| d < x) {
                best = Some(((j.id, end), d));
            }
        }
    }
    best
}

// ---------------------------------------------------------------------------------------------
// Face pattern and mirror (SM12.2)

/// How copies are placed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Placement {
    Rigid(Rigid),
    /// Mirrored in the plane through `point` with unit normal `normal`.
    Mirror { point: P3, normal: V3 },
    /// Any rigid motion or reflection: `p ↦ linear·p + t` (`linear` orthogonal).
    Affine { linear: nalgebra::Matrix3<f64>, t: V3 },
}

impl Placement {
    fn point(&self, p: P3) -> P3 {
        match self {
            Placement::Rigid(r) => r.point(p),
            Placement::Mirror { point, normal } => p - normal * (2.0 * (p - point).dot(normal)),
            Placement::Affine { linear, t } => P3::from(linear * p.coords + t),
        }
    }

    fn vec(&self, v: V3) -> V3 {
        match self {
            Placement::Rigid(r) => r.vec(v),
            Placement::Mirror { normal, .. } => v - normal * (2.0 * v.dot(normal)),
            Placement::Affine { linear, .. } => linear * v,
        }
    }

    fn mirrors(&self) -> bool {
        match self {
            Placement::Rigid(_) => false,
            Placement::Mirror { .. } => true,
            Placement::Affine { linear, .. } => linear.determinant() < 0.0,
        }
    }
}

/// How far each wall is from the flat's root (over bends and tangent joints).
fn depths(m: &Model) -> Vec<(WallId, usize)> {
    let mut out: Vec<(WallId, usize)> = Vec::new();
    let mut order: Vec<WallId> = m.walls.iter().map(|w| w.id).collect();
    if let Some(f) = m.fixed {
        order.retain(|w| *w != f);
        order.insert(0, f);
    }
    for root in order {
        if out.iter().any(|(w, _)| *w == root) {
            continue;
        }
        out.push((root, 0));
        let mut q = VecDeque::from([(root, 0)]);
        while let Some((w, d)) = q.pop_front() {
            for j in m.joints.iter().filter(|j| j.connects() && (j.a == w || j.b == w)) {
                let o = j.other(w);
                if !out.iter().any(|(x, _)| *x == o) {
                    out.push((o, d + 1));
                    q.push_back((o, d + 1));
                }
            }
        }
    }
    out
}

/// **Face pattern / Face mirror** of walls (SM12.2): each wall in `seeds`, with everything
/// beyond it (away from the flat's root), copied by `place` and joined to the same wall its
/// bend joins (the copy's bend lands where `place` puts it). Returns the new walls.
pub fn copy_walls(m: &mut Model, seeds: &[WallId], place: &Placement, seed: u64) -> Result<Vec<WallId>, EditError> {
    let depth = depths(m);
    let depth_of = |w: WallId| depth.iter().find(|(x, _)| *x == w).map(|(_, d)| *d).unwrap_or(usize::MAX);
    let mut made = Vec::new();
    let mut kw = 0;
    let mut kj = 0;
    for s in seeds {
        let Some(sw) = m.wall(*s) else { return Err(EditError::NoWall) };
        if !matches!(sw.surface, Surface::Planar { .. }) {
            return Err(EditError::CantCopy);
        }
        // Its bend towards the root.
        let parent = m
            .joints
            .iter()
            .filter(|j| j.bend().is_some() && (j.a == *s || j.b == *s))
            .min_by_key(|j| depth_of(j.other(*s)))
            .ok_or(EditError::CantCopy)?
            .clone();
        let base = parent.other(*s);
        if depth_of(base) >= depth_of(*s) {
            return Err(EditError::CantCopy);
        }
        let subtree = reachable(m, &[*s], &[base], &[parent.id]).map_err(|_| EditError::CantCopy)?;
        let mut ids: Vec<(WallId, WallId)> = Vec::new();
        for w in &subtree {
            ids.push((*w, free_wall(m, seed, &mut kw)));
        }
        let new_id = |w: WallId| ids.iter().find(|(o, _)| *o == w).map(|(_, n)| *n);
        let swap = |p: P2| if place.mirrors() { P2::new(p.y, p.x) } else { p };
        let mut walls = Vec::new();
        for w in &subtree {
            let ow = m.wall(*w).expect("wall");
            let Surface::Planar { origin, u, v } = ow.surface else { return Err(EditError::CantCopy) };
            let surface = if place.mirrors() {
                Surface::Planar { origin: place.point(origin), u: place.vec(v), v: place.vec(u) }
            } else {
                Surface::Planar { origin: place.point(origin), u: place.vec(u), v: place.vec(v) }
            };
            walls.push(Wall { id: new_id(*w).expect("id"), surface, outline: ow.outline.map(swap) });
        }
        let mut joints = Vec::new();
        let mut trims: Vec<(Seg2, Seg2, Bend)> = Vec::new();
        let mut names = namer(m);
        let base_wall = m.wall(base).expect("base").clone();
        for j in m.joints.iter().filter(|j| subtree.contains(&j.a) || subtree.contains(&j.b)) {
            let mut c = j.clone();
            let onto_base = |p: P2| {
                let s = m.wall(base).expect("base").surface;
                base_wall.surface.local(place.point(s.point(p)))
            };
            let (a_in, b_in) = (subtree.contains(&j.a), subtree.contains(&j.b));
            if !(a_in && b_in) && j.id != parent.id {
                // A rip or joint to a wall outside the copied walls: not copied.
                continue;
            }
            let (sa, sb) = segments_mut(&mut c);
            *sa = if a_in { Seg2::new(swap(sa.a), swap(sa.b)) } else { Seg2::new(onto_base(sa.a), onto_base(sa.b)) };
            *sb = if b_in { Seg2::new(swap(sb.a), swap(sb.b)) } else { Seg2::new(onto_base(sb.a), onto_base(sb.b)) };
            c.a = if a_in { new_id(j.a).expect("id") } else { j.a };
            c.b = if b_in { new_id(j.b).expect("id") } else { j.b };
            c.id = free_joint(m, seed ^ 0x434f_5059, &mut kj);
            c.name = names.name(&c.kind);
            if j.id == parent.id {
                trims.push((c.segment_on(base).expect("on the base"), j.segment_on(base).expect("on the base"), *j.bend().expect("a bend")));
            }
            joints.push(c);
        }
        // The base gives up what lies past the copy's tangent line, as it did for the original
        // bend.
        for (seg, orig, _) in trims {
            let Some(into) = poly::inward_normal(&base_wall.outline, orig) else { continue };
            let s0 = m.wall(base).expect("base").surface;
            let into3 = s0.direction_at(orig.a, into);
            let new_into3 = place.vec(into3);
            let into_new = {
                let q = base_wall.surface.local(base_wall.surface.point(seg.a) + new_into3);
                (q - seg.a).normalize()
            };
            if poly::inward_normal(&base_wall.outline, seg).is_some() {
                continue;
            }
            // Everything of the base past the copy's tangent line, along its span, goes.
            let back = 4.0 * size_of(&base_wall.outline);
            let band = Polygon::new(vec![seg.a, seg.b, seg.b - into_new * back, seg.a - into_new * back]);
            let bw = m.walls.iter_mut().find(|w| w.id == base).expect("base");
            let left = exact(poly::difference(std::slice::from_ref(&bw.outline), std::slice::from_ref(&band)), &[&bw.outline, &band]);
            if let Some(big) = left.into_iter().max_by(|a, b| a.area().total_cmp(&b.area())) {
                bw.outline = big;
            }
        }
        m.walls.extend(walls);
        m.joints.extend(joints);
        made.extend(ids.iter().map(|(_, n)| *n));
    }
    Ok(made)
}

/// **Part pattern / Part mirror** of a sheet metal part (SM12.2): the walls `walls` (a whole
/// flat-pattern part), the joints between them, their flat cuts and relief overrides copied by
/// `place` as a part of their own in the same model (no joint joins it to the original), so the
/// copy is sheet metal too: it has its own flat pattern and takes later sheet metal features.
/// Returns the new walls.
pub fn copy_part(m: &mut Model, walls: &[WallId], place: &Placement, seed: u64) -> Result<Vec<WallId>, EditError> {
    let mut kw = 0;
    let mut kj = 0;
    let mut ids: Vec<(WallId, WallId)> = Vec::new();
    for w in walls {
        if m.wall(*w).is_none() {
            return Err(EditError::NoWall);
        }
        let mut id = free_wall(m, seed, &mut kw);
        while ids.iter().any(|(_, n)| *n == id) {
            id = free_wall(m, seed, &mut kw);
        }
        ids.push((*w, id));
    }
    let new_id = |w: WallId| ids.iter().find(|(o, _)| *o == w).map(|(_, n)| *n);
    let swap = |p: P2| if place.mirrors() { P2::new(p.y, p.x) } else { p };
    let mut new_walls = Vec::new();
    for (old, nid) in &ids {
        let ow = m.wall(*old).expect("checked");
        let surface = match ow.surface {
            Surface::Planar { origin, u, v } if place.mirrors() => Surface::Planar { origin: place.point(origin), u: place.vec(v), v: place.vec(u) },
            Surface::Planar { origin, u, v } => Surface::Planar { origin: place.point(origin), u: place.vec(u), v: place.vec(v) },
            Surface::Rolled { .. } if place.mirrors() => return Err(EditError::CantCopy),
            Surface::Rolled { axis_origin, axis, start, radius, material_outside } => Surface::Rolled {
                axis_origin: place.point(axis_origin),
                axis: place.vec(axis),
                start: place.vec(start),
                radius,
                material_outside,
            },
        };
        new_walls.push(Wall { id: *nid, surface, outline: ow.outline.map(swap) });
    }
    let mut names = namer(m);
    let mut new_joints: Vec<Joint> = Vec::new();
    let mut joint_ids: Vec<(JointId, JointId)> = Vec::new();
    for j in m.joints.iter().filter(|j| walls.contains(&j.a) && walls.contains(&j.b)) {
        let mut c = j.clone();
        let (sa, sb) = segments_mut(&mut c);
        *sa = Seg2::new(swap(sa.a), swap(sa.b));
        *sb = Seg2::new(swap(sb.a), swap(sb.b));
        c.a = new_id(j.a).expect("id");
        c.b = new_id(j.b).expect("id");
        let mut id = free_joint(m, seed ^ 0x5041_5254, &mut kj);
        while new_joints.iter().any(|x| x.id == id) {
            id = free_joint(m, seed ^ 0x5041_5254, &mut kj);
        }
        c.id = id;
        c.name = names.name(&c.kind);
        joint_ids.push((j.id, id));
        new_joints.push(c);
    }
    let nj = |j: JointId| joint_ids.iter().find(|(o, _)| *o == j).map(|(_, n)| *n);
    let cuts: Vec<crate::flat_edit::FlatCut> = m
        .flat_cuts
        .iter()
        .filter_map(|c| Some(crate::flat_edit::FlatCut { anchor: new_id(c.anchor)?, shapes: c.shapes.iter().map(|s| s.map(swap)).collect() }))
        .collect();
    let corners: Vec<crate::model::CornerOverride> = m.corner_overrides.iter().filter_map(|o| Some(crate::model::CornerOverride { bends: (nj(o.bends.0)?, nj(o.bends.1)?), ..*o })).collect();
    let ends: Vec<crate::model::BendReliefOverride> = m.bend_relief_overrides.iter().filter_map(|o| Some(crate::model::BendReliefOverride { bend: nj(o.bend)?, ..*o })).collect();
    m.walls.extend(new_walls);
    m.joints.extend(new_joints);
    m.flat_cuts.extend(cuts);
    m.corner_overrides.extend(corners);
    m.bend_relief_overrides.extend(ends);
    Ok(ids.into_iter().map(|(_, n)| n).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flatten;
    use crate::params::Params;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn params() -> Params {
        Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, ..Params::default() }
    }

    /// A 100 × 60 plate in the XY plane, material above.
    fn plate() -> Model {
        Model {
            params: params(),
            walls: vec![Wall {
                id: WallId(0),
                surface: Surface::Planar { origin: P3::origin(), u: V3::x(), v: V3::y() },
                outline: Polygon::rect(P2::new(0.0, 0.0), P2::new(100.0, 60.0)),
            }],
            ..Default::default()
        }
    }

    fn spec(x: f64, alignment: BendAlignment) -> BendSpec {
        BendSpec {
            wall: WallId(0),
            line: (P3::new(x, 0.0, 2.0), P3::new(x, 60.0, 2.0)),
            hold_opposite: false,
            alignment,
            angle: FRAC_PI_2,
            toward_material: true,
            line_height: 2.0,
            radius: None,
            k_factor: None,
        }
    }

    fn flat_bounds(m: &Model) -> (P2, P2) {
        let f = flatten(m);
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        f.parts[0].bounds().unwrap()
    }

    #[test]
    fn a_bend_keeps_the_flat_pattern_the_same_size() {
        for al in BendAlignment::ALL {
            let mut m = plate();
            let made = bend_wall(&mut m, &spec(70.0, al), 7).unwrap();
            assert!(m.validate().is_empty(), "{al:?}: {:?}", m.validate());
            assert_eq!(made.joints.len(), 1);
            let (lo, hi) = flat_bounds(&m);
            assert!((hi.x - lo.x - 100.0).abs() < 1e-5 && (hi.y - lo.y - 60.0).abs() < 1e-5, "{al:?}: {lo:?} {hi:?}");
            let area = flatten(&m).parts[0].area();
            assert!((area - 6000.0).abs() < 1e-3, "{al:?}: {area}");
        }
        // A custom K factor changes the folded lengths, not the flat.
        let mut m = plate();
        bend_wall(&mut m, &BendSpec { k_factor: Some(0.3), radius: Some(5.0), ..spec(70.0, BendAlignment::BendLine) }, 7).unwrap();
        let (lo, hi) = flat_bounds(&m);
        assert!((hi.x - lo.x - 100.0).abs() < 1e-5, "{lo:?} {hi:?}");
    }

    #[test]
    fn the_smaller_side_moves_unless_held_opposite() {
        let mut m = plate();
        let made = bend_wall(&mut m, &spec(70.0, BendAlignment::HoldLine), 1).unwrap();
        let w = m.wall(made.wall).unwrap();
        // The x > 70 side stood up: its points are above the plate.
        let (_, hi) = w.outline.bounds().unwrap();
        assert!(w.surface.point(hi).z > 10.0);
        assert!(m.wall(WallId(0)).unwrap().outline.bounds().unwrap().1.x <= 70.0 + 1e-9);
        let mut m = plate();
        let made = bend_wall(&mut m, &BendSpec { hold_opposite: true, ..spec(70.0, BendAlignment::HoldLine) }, 1).unwrap();
        assert!(m.wall(WallId(0)).unwrap().outline.bounds().unwrap().0.x >= 70.0 - 1e-9);
        assert!(m.wall(made.wall).is_some());
    }

    #[test]
    fn folded_alignments_put_the_bent_wall_on_the_line() {
        let t = 2.0;
        for (al, face) in [(BendAlignment::Inner, t), (BendAlignment::Outer, 0.0), (BendAlignment::Middle, t / 2.0)] {
            let mut m = plate();
            let made = bend_wall(&mut m, &spec(70.0, al), 3).unwrap();
            assert!(m.validate().is_empty());
            let w = m.wall(made.wall).unwrap();
            let n = w.surface.normal().unwrap();
            // Bent up (towards the material): the wall stands at x ≈ 70, its normal along −x.
            assert!((n + V3::x()).norm() < 1e-9, "{n:?}");
            // The material lies on the −x side of the definition surface; the inner face (the
            // bend's inside, facing the plate) is the far one.
            let def_x = w.surface.point(P2::origin()).x;
            let face_x = def_x - face;
            assert!((face_x - 70.0).abs() < 1e-6, "{al:?}: face at {face_x}");
        }
    }

    #[test]
    fn a_bend_turns_the_walls_beyond_it() {
        // An L: base with a flange on x = 50; bending the base at x = 30 (the flange side
        // moves) carries the flange round.
        let mut m = crate::samples::l_bracket(params(), true).unwrap();
        let base = m.walls[0].id;
        let s = BendSpec { wall: base, line: (P3::new(20.0, 0.0, 0.0), P3::new(20.0, 40.0, 0.0)), line_height: 0.0, ..spec(0.0, BendAlignment::BendLine) };
        let before = flatten(&m).parts[0].area();
        let made = bend_wall(&mut m, &BendSpec { toward_material: false, hold_opposite: true, ..s }, 9).unwrap();
        assert_eq!(made.moved.len(), 1);
        assert!(m.validate().is_empty(), "{:?}", m.validate());
        let f = flatten(&m);
        assert!(f.errors.is_empty());
        assert!((f.parts[0].area() - before).abs() < 1e-3, "{} {before}", f.parts[0].area());
    }

    #[test]
    fn a_bend_line_across_a_slot_makes_two_bends() {
        let mut m = plate();
        m.walls[0].outline.holes.push(vec![P2::new(65.0, 20.0), P2::new(65.0, 40.0), P2::new(75.0, 40.0), P2::new(75.0, 20.0)]);
        let made = bend_wall(&mut m, &spec(70.0, BendAlignment::BendLine), 4).unwrap();
        assert_eq!(made.joints.len(), 2);
        assert!(m.validate().is_empty());
        let (lo, hi) = flat_bounds(&m);
        assert!((hi.x - lo.x - 100.0).abs() < 1e-5, "{lo:?} {hi:?}");
    }

    #[test]
    fn a_jog_offsets_the_far_wall() {
        let t = 2.0;
        for (anchor, want) in [(JogAnchor::Inside, 20.0 + t), (JogAnchor::Outside, 20.0), (JogAnchor::Nominal, 20.0 - t)] {
            let mut m = plate();
            let s = JogSpec { bend: spec(60.0, BendAlignment::BendLine), offset: 20.0, anchor, preserve_material: true };
            let j = jog_wall(&mut m, &s, 5).unwrap();
            assert!(m.validate().is_empty(), "{:?}", m.validate());
            let far = m.wall(j.second.wall).unwrap();
            assert!((far.surface.normal().unwrap() - V3::z()).norm() < 1e-9);
            let dz = far.surface.point(P2::origin()).z;
            assert!((dz - want).abs() < 1e-6, "{anchor:?}: {dz}");
            let (lo, hi) = flat_bounds(&m);
            assert!((hi.x - lo.x - 100.0).abs() < 1e-5, "preserved: the flat keeps its size");
        }
        // Without Preserve material the far end stays where it was and the flat grows.
        let mut m = plate();
        let s = JogSpec { bend: spec(60.0, BendAlignment::BendLine), offset: 10.0, anchor: JogAnchor::Inside, preserve_material: false };
        let j = jog_wall(&mut m, &s, 5).unwrap();
        assert!(m.validate().is_empty());
        let far = m.wall(j.second.wall).unwrap();
        let x_max = far.outline.outer.iter().map(|p| far.surface.point(*p).x).fold(f64::MIN, f64::max);
        assert!((x_max - 100.0).abs() < 1e-6, "{x_max}");
        let (lo, hi) = flat_bounds(&m);
        assert!((hi.x - lo.x - 100.0 - j.added).abs() < 1e-5 && j.added > 1.0);
        // Too small an offset for the bends.
        let mut m = plate();
        let s = JogSpec { bend: spec(60.0, BendAlignment::BendLine), offset: 0.5, anchor: JogAnchor::Outside, preserve_material: true };
        assert_eq!(jog_wall(&mut m, &s, 5).unwrap_err(), EditError::JogTooSmall);
    }

    #[test]
    fn tabs_add_to_parallel_walls_and_bridge_them() {
        let mut m = crate::samples::l_bracket(params(), true).unwrap();
        let base = m.walls[0].id;
        let before = m.wall(base).unwrap().outline.area();
        let tab = Region3 { origin: P3::new(0.0, 0.0, 2.0), x: V3::x(), y: V3::y(), polygon: Polygon::rect(P2::new(10.0, -15.0), P2::new(30.0, 5.0)) };
        let flange = m.walls[1].id;
        let took = add_tab(&mut m, std::slice::from_ref(&tab), &[base, flange]).unwrap();
        assert_eq!(took, vec![base]);
        assert!((m.wall(base).unwrap().outline.area() - before - 300.0).abs() < 1e-6);
        assert!(m.validate().is_empty());
        // Two coplanar plates bridged by a tab become one wall.
        let mut m = plate();
        m.walls.push(Wall { id: WallId(1), surface: Surface::Planar { origin: P3::new(120.0, 0.0, 0.0), u: V3::x(), v: V3::y() }, outline: Polygon::rect(P2::new(0.0, 0.0), P2::new(30.0, 60.0)) });
        let bridge = Region3 { origin: P3::origin(), x: V3::x(), y: V3::y(), polygon: Polygon::rect(P2::new(90.0, 20.0), P2::new(130.0, 40.0)) };
        add_tab(&mut m, &[bridge], &[WallId(0), WallId(1)]).unwrap();
        assert_eq!(m.walls.len(), 1);
        assert!((m.walls[0].outline.area() - (6000.0 + 1800.0 + 400.0)).abs() < 1e-6);
        // Not touching: no tab.
        let mut m = plate();
        let far = Region3 { origin: P3::origin(), x: V3::x(), y: V3::y(), polygon: Polygon::rect(P2::new(200.0, 0.0), P2::new(210.0, 10.0)) };
        assert_eq!(add_tab(&mut m, &[far], &[WallId(0)]).unwrap_err(), EditError::NoTab);
    }

    #[test]
    fn a_slanted_cut_goes_through_perpendicular() {
        let mut m = plate();
        // A 10 × 10 square sketched 50 above the plate, cut straight down: a 10 × 10 hole.
        let sq = Region3 { origin: P3::new(0.0, 0.0, 50.0), x: V3::x(), y: V3::y(), polygon: Polygon::rect(P2::new(40.0, 20.0), P2::new(50.0, 30.0)) };
        cut_walls(&mut m, &[CutTool { region: sq.clone(), dir: -V3::z(), z: None }], None).unwrap();
        assert!((m.walls[0].outline.area() - 5900.0).abs() < 1e-6);
        assert_eq!(m.walls[0].outline.holes.len(), 1);
        // A blind cut that stops short of the plate cuts nothing.
        let mut m = plate();
        assert!(cut_walls(&mut m, &[CutTool { region: sq, dir: -V3::z(), z: Some((0.0, 20.0)) }], None).unwrap().is_empty());
    }

    #[test]
    fn corner_breaks_round_or_cut_a_corner() {
        let mut m = plate();
        break_corner(&mut m, WallId(0), P2::new(100.0, 60.0), CornerBreakKind::Fillet { radius: 10.0 }).unwrap();
        let a = m.walls[0].outline.area();
        let want = 6000.0 - (1.0 - PI / 4.0) * 100.0;
        // (The arc is a polyline within 0.002 of the circle.)
        assert!((a - want).abs() < 0.05, "{a} vs {want}");
        break_corner(&mut m, WallId(0), P2::new(0.0, 0.0), CornerBreakKind::Chamfer { d1: 5.0, d2: 8.0 }).unwrap();
        assert!((m.walls[0].outline.area() - a + 20.0).abs() < 1e-6);
        assert_eq!(break_corner(&mut m, WallId(0), P2::new(100.0, 0.0), CornerBreakKind::Fillet { radius: 70.0 }), Err(EditError::CornerTooBig));
        assert!(corner_vertex_at(&m, P3::new(100.0, 0.0, 1.0), 0.5).is_some());
        assert!((radius_for_width(10.0 * 2f64.sqrt(), FRAC_PI_2) - 10.0).abs() < 1e-9);
        assert!((chamfer_second(5.0, PI / 4.0, FRAC_PI_2) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn asymmetric_rounds_overflow_and_offset_chamfers() {
        // Asymmetric at a square corner: a quarter ellipse with semi-axes 10 and 6.
        let mut m = plate();
        break_corner(&mut m, WallId(0), P2::new(100.0, 60.0), CornerBreakKind::Round { r1: 10.0, r2: 6.0, overflow: false }).unwrap();
        let want = 6000.0 - 10.0 * 6.0 * (1.0 - PI / 4.0);
        let a = m.walls[0].outline.area();
        assert!((a - want).abs() < 0.02, "{a} vs {want}");
        // The 10 is on the edge into the corner (the right edge, going up), the 6 on the top edge.
        let (_, hi) = m.walls[0].outline.bounds().unwrap();
        assert!((hi.x - 100.0).abs() < 1e-9 && (hi.y - 60.0).abs() < 1e-9);
        assert!(m.walls[0].outline.outer.iter().any(|p| (p - P2::new(100.0, 50.0)).norm() < 1e-9));
        assert!(m.walls[0].outline.outer.iter().any(|p| (p - P2::new(94.0, 60.0)).norm() < 1e-9));
        // A round longer than an edge fails without overflow...
        let mut m = plate();
        let big = CornerBreakKind::Round { r1: 70.0, r2: 70.0, overflow: false };
        assert_eq!(break_corner(&mut m, WallId(0), P2::new(100.0, 60.0), big), Err(EditError::CornerTooBig));
        // ...and with it runs over the next corner: the circle about (30, −10), radius 70, trims
        // the plate's corner. Removed: the quarter round's corner less the part below y = 0.
        break_corner(&mut m, WallId(0), P2::new(100.0, 60.0), CornerBreakKind::Round { r1: 70.0, r2: 70.0, overflow: true }).unwrap();
        let below = 700.0 - (5.0 * 4800f64.sqrt() + 2450.0 * (1.0f64 / 7.0).asin());
        let want = 6000.0 - (4900.0 * (1.0 - PI / 4.0) - below);
        let a = m.walls[0].outline.area();
        assert!((a - want).abs() < 0.2, "{a} vs {want}");
        assert!(m.validate().is_empty());
        // Offset chamfers at a 60° corner: the faces offset by d meet over points d / tan 30°
        // from the corner (Tangent would be d).
        let (s1, s2) = chamfer_offset_setbacks(5.0, 5.0, PI / 3.0);
        assert!((s1 - 5.0 / (PI / 6.0).tan()).abs() < 1e-9 && (s1 - s2).abs() < 1e-12);
        let (s1, s2) = chamfer_offset_setbacks(5.0, 8.0, FRAC_PI_2);
        assert!((s1 - 8.0).abs() < 1e-9 && (s2 - 5.0).abs() < 1e-9);
        let mut tri = plate();
        tri.walls[0].outline = Polygon::new(vec![P2::new(0.0, 0.0), P2::new(100.0, 0.0), P2::new(50.0, 50.0 * 3f64.sqrt())]);
        let before = tri.walls[0].outline.area();
        let (s1, s2) = chamfer_offset_setbacks(5.0, 5.0, PI / 3.0);
        break_corner(&mut tri, WallId(0), P2::new(0.0, 0.0), CornerBreakKind::Chamfer { d1: s1, d2: s2 }).unwrap();
        let cut = before - tri.walls[0].outline.area();
        assert!((cut - 0.5 * s1 * s2 * (PI / 3.0).sin()).abs() < 1e-6, "{cut}");
    }

    #[test]
    fn a_cut_that_splits_a_wall_keeps_both_pieces() {
        let mut m = plate();
        // A 4 wide slot right across the plate.
        let slot = Region3 { origin: P3::new(0.0, 0.0, 10.0), x: V3::x(), y: V3::y(), polygon: Polygon::rect(P2::new(48.0, -10.0), P2::new(52.0, 70.0)) };
        let made = cut_walls(&mut m, &[CutTool { region: slot, dir: -V3::z(), z: None }], None).unwrap();
        assert_eq!(made.walls, vec![WallId(0)]);
        assert_eq!(made.split.len(), 1);
        assert_eq!(m.walls.len(), 2);
        for w in &m.walls {
            assert!((w.outline.area() - 48.0 * 60.0).abs() < 1e-6, "{}", w.outline.area());
        }
        let f = flatten(&m);
        assert!(f.errors.is_empty());
        assert_eq!(f.parts.len(), 2, "two parts now");
        // On a bent plate, the piece beyond the slot keeps its bend.
        let mut m = plate();
        bend_wall(&mut m, &spec(80.0, BendAlignment::HoldLine), 3).unwrap();
        let slot = Region3 { origin: P3::new(0.0, 0.0, 10.0), x: V3::x(), y: V3::y(), polygon: Polygon::rect(P2::new(38.0, -10.0), P2::new(42.0, 70.0)) };
        cut_walls(&mut m, &[CutTool { region: slot, dir: -V3::z(), z: None }], None).unwrap();
        assert!(m.validate().is_empty(), "{:?}", m.validate());
        let f = flatten(&m);
        assert!(f.errors.is_empty());
        assert_eq!(f.parts.len(), 2);
        assert!(f.parts.iter().any(|p| p.bends.len() == 1), "the bend went with its piece");
    }

    #[test]
    fn a_slot_across_a_bend_is_cut_from_the_bend_region_too() {
        // Bent up at x = 70 (Hold line): axis at x = 70, z = 5; the mid surface (radius 4) runs
        // x = 70 + 4 sin a. A slot from x = 60 to 72 (y 25..35) cut straight down takes 10 of the
        // base and the bend region up to a = 30°: a third of its allowance.
        let mut m = plate();
        bend_wall(&mut m, &spec(70.0, BendAlignment::HoldLine), 3).unwrap();
        let before = flatten(&m).parts[0].area();
        let slot = Region3 { origin: P3::new(0.0, 0.0, 50.0), x: V3::x(), y: V3::y(), polygon: Polygon::rect(P2::new(60.0, 25.0), P2::new(72.0, 35.0)) };
        let made = cut_walls(&mut m, &[CutTool { region: slot, dir: -V3::z(), z: None }], None).unwrap();
        assert_eq!(made.bends.len(), 1);
        let ba = m.joints[0].bend().unwrap().allowance(&m.params).unwrap();
        let f = flatten(&m);
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        let part = &f.parts[0];
        let removed = before - part.area();
        let want = 10.0 * (10.0 + ba / 3.0);
        // (The booleans work on a 1 nm grid.)
        assert!((removed - want).abs() < 1e-5, "{removed} vs {want}");
        // One hole: the slot's flat outline, 10 wide, 10 + ba/3 long.
        let holes: Vec<&Vec<P2>> = part.outline.iter().flat_map(|o| o.holes.iter()).collect();
        assert_eq!(holes.len(), 1);
        let (lo, hi) = Polygon::new(holes[0].clone()).bounds().unwrap();
        let (w, h) = (hi.x - lo.x, hi.y - lo.y);
        let (long, short) = (w.max(h), w.min(h));
        assert!((short - 10.0).abs() < 1e-6 && (long - (10.0 + ba / 3.0)).abs() < 1e-5, "{w} × {h}");
    }

    #[test]
    fn a_part_copy_is_sheet_metal_of_its_own() {
        let mut m = crate::samples::l_bracket(params(), true).unwrap();
        let walls: Vec<WallId> = m.walls.iter().map(|w| w.id).collect();
        let one = flatten(&m).parts[0].area();
        let made = copy_part(&mut m, &walls, &Placement::Rigid(Rigid::translation(V3::new(0.0, 100.0, 0.0))), 21).unwrap();
        assert_eq!(made.len(), walls.len());
        assert!(m.validate().is_empty(), "{:?}", m.validate());
        let f = flatten(&m);
        assert_eq!(f.parts.len(), 2);
        assert!(f.parts.iter().all(|p| (p.area() - one).abs() < 1e-6));
        // Mirrored too.
        let mut m = crate::samples::l_bracket(params(), true).unwrap();
        copy_part(&mut m, &walls, &Placement::Mirror { point: P3::new(-10.0, 0.0, 0.0), normal: V3::x() }, 22).unwrap();
        assert!(m.validate().is_empty(), "{:?}", m.validate());
        let f = flatten(&m);
        assert_eq!(f.parts.len(), 2);
        assert!(f.parts.iter().all(|p| (p.area() - one).abs() < 1e-6));
    }

    #[test]
    fn a_grown_rectangle_stays_a_rectangle() {
        let g = grow(&Polygon::rect(P2::new(0.0, 0.0), P2::new(20.0, 10.0)), 0.5);
        assert!((g.area() - 21.0 * 11.0).abs() < 1e-9, "{}", g.area());
        // A concave L grows by its offset, its inner corner staying square.
        let l = Polygon::new(vec![P2::new(0.0, 0.0), P2::new(10.0, 0.0), P2::new(10.0, 4.0), P2::new(4.0, 4.0), P2::new(4.0, 10.0), P2::new(0.0, 10.0)]);
        let g = grow(&l, 1.0);
        let want = Polygon::new(vec![P2::new(-1.0, -1.0), P2::new(11.0, -1.0), P2::new(11.0, 5.0), P2::new(5.0, 5.0), P2::new(5.0, 11.0), P2::new(-1.0, 11.0)]).area();
        assert!((g.area() - want).abs() < 1e-9, "{} vs {want}", g.area());
    }

    #[test]
    fn a_mirrored_flange_lands_on_the_other_edge() {
        // A flange on x = 50 of a 50 × 40 base, mirrored in x = 25: a flange on x = 0 too.
        let mut m = crate::samples::l_bracket(params(), true).unwrap();
        let flange = m.walls[1].id;
        let made = copy_walls(&mut m, &[flange], &Placement::Mirror { point: P3::new(25.0, 0.0, 0.0), normal: V3::x() }, 11).unwrap();
        assert_eq!(made.len(), 1);
        assert!(m.validate().is_empty(), "{:?}", m.validate());
        let f = flatten(&m);
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        assert_eq!(m.joints.iter().filter(|j| j.bend().is_some()).count(), 2);
        // A U-channel's flat: twice the flange.
        let one = flatten(&crate::samples::l_bracket(params(), true).unwrap()).parts[0].area();
        let base = 50.0 * 40.0;
        assert!((f.parts[0].area() - (2.0 * one - base)).abs() < 1e-3, "{}", f.parts[0].area());
    }

    #[test]
    fn picks_find_walls_corners_and_bend_ends() {
        let m = crate::samples::partial_flange(params()).unwrap();
        assert_eq!(wall_at(&m, P3::new(10.0, 10.0, 2.0), 1e-6).map(|x| x.1), Some(true));
        assert_eq!(wall_at(&m, P3::new(10.0, 10.0, 0.0), 1e-6).map(|x| x.1), Some(false));
        let j = m.joints[0].id;
        let ((jj, end), _) = bend_end_near(&m, P3::new(50.0, 30.0, 0.0)).unwrap();
        assert_eq!(jj, j);
        let b = m.joints[0].bend().unwrap();
        let near = if (b.on_a.b.y - 30.0).abs() < 5.0 { BendEnd::End } else { BendEnd::Start };
        assert_eq!(end, near);
        let bx = crate::samples::open_box(params(), crate::model::RipStyle::EdgeJoint).unwrap();
        let f = flatten(&bx);
        let (c, d) = corner_near(&bx, &f, P3::new(100.0, 60.0, 0.0)).unwrap();
        assert!(d < 10.0, "{d}");
        assert_ne!(c.0, c.1);
    }
}
