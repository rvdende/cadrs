//! The assembly solver (P3B.2, P3B.3; `intro-to-assemblies.md` A6.2, A6.7, A6.11, A7, A9–A13,
//! A16.4, X6).
//!
//! **Bodies.** Every instance is a rigid body, except that the instances of a **Group** (and of
//! groups that share instances) form one body, keeping their placements relative to each other
//! (A13.3). A body with a fixed instance is **ground**: it doesn't move (A3.7, A13.4).
//!
//! **Unknowns.** Each free body has six: a translation δ and a small rotation ω about its pivot
//! `c` (the middle of its connectors), applied as `p ↦ exp(ω)(p − c) + c + δ` to its current
//! placement. Rotations are measured in "millimetres" through the length scale [`L`], so a turn
//! of 1/L rad weighs like a 1 mm move.
//!
//! **Residuals.** A connector mate compares the first connector's world frame F₁ (flip and
//! reorient applied) with the target G = F₂ · Offset (see [`super::mate`]), giving only the rows
//! its type removes, so the rank of the constraint Jacobian is exactly 6 − DOF (A7); `d` is
//! p₁ − p_G:
//!
//! | type | rows |
//! |---|---|
//! | Fastened | d (3), ½(x_G × x₁ + y_G × y₁ + z_G × z₁)·L (3) |
//! | Revolute | d (3), (z₁·x_G, z₁·y_G)·L (2) |
//! | Slider | d·x_G, d·y_G (2), the rotation rows (3) |
//! | Cylindrical | d·x_G, d·y_G (2), the axis rows (2) |
//! | Pin slot | d·y₁, d·z₁ (2), the axis rows (2): free along the slot connector's X and about Z |
//! | Planar | d·z_G (1), the axis rows (2) |
//! | Ball | d (3) |
//! | Parallel | the axis rows (2) |
//!
//! **Tangent** (A11) compares two surfaces ([`SurfaceKind`], each a frame and a size) instead:
//! a cylinder on a plane keeps its axis at its radius from the plane and parallel to it; a sphere
//! at its radius; two cylinders keep parallel axes r₁ + r₂ apart (|r₁ − r₂| flipped, inside); a
//! vertex or straight edge lies on the plane or the cylinder, … Flip puts the other side of a
//! plane. **Width** (A12) takes the two width connectors W₁, W₂ and one or two tabs T: the tabs'
//! middle lies on the centre plane, `n·(t̄ − (w₁ + w₂)/2) = 0` with `n = z_W₁`, and each tab's Z
//! stays parallel to `n`. One instance's tabs so stay centred; two instances' tabs stay
//! mirror-symmetric about the centre plane (their distances to it equal and opposite).
//!
//! Limits and drives add one row on the mate's position along a DOF (its X, Y or Z travel, or
//! its angle about Z × L), see [`super::mate::dof_value`].
//!
//! **Method.** Damped minimum-norm Gauss–Newton (Levenberg–Marquardt on the constraint rows):
//! each step solves `(J M Jᵀ + λI) y = r`, `Δ = −M Jᵀ y`, the smallest step (in the mobility
//! metric `M`) that cancels the residuals to first order. Starting from the current placements,
//! it converges to the solution nearest to them: this is the **minimum-motion regulariser**. The
//! mobility of the instances being mated ("movers": the first movable connector's body, when a
//! mate is picked) is 1, the others' 10⁻³, so the picked instance moves and the rest stay put
//! unless they must follow. λ starts tiny and grows ×10 when a step makes the residual worse (it
//! shrinks ÷10 after good steps), so redundant or conflicting rows don't blow up. The Jacobian
//! is taken by central differences, per constraint, over only its bodies' unknowns; `J M Jᵀ` is
//! assembled from the rows that share a body, then factored by Cholesky.
//!
//! **Robustness.**
//! - **Snap on pick** (A6.7): before iterating, the mover's body is placed so the new mate's
//!   frames coincide exactly (F₁ = G, or at the mate's driven position). So a pick never starts
//!   Newton half a turn away, where the rotation rows have saddle points. Animation snaps with
//!   the mate's other free DOF held where they are ([`SolveOptions::hold_free`]).
//! - After converging, each mate is checked for a false solution (axes opposite: the rotation
//!   rows also vanish for half turns). If one is found, its body is snapped and the solve runs
//!   once more.
//! - **Limits** are inequalities handled by an active set: solve the equalities, then for each
//!   limit that is violated, add a row holding the position at the nearest bound, and solve
//!   again (a few rounds). So a drag past a slider's end stops at the end.
//! - Mates between instances of one body (all ground, or one group) have no unknowns and are
//!   skipped, as are **suppressed** mates.
//! - It stops after [`MAX_ITERATIONS`] steps and reports whether it converged (|r|∞ < 10⁻⁸ mm).
//!
//! **Drag** ([`drag`], A16.4): points of instances are pulled toward targets (the pointer, or a
//! triad handle's placement) within what the mates allow. Each step moves along the null space
//! N of the mate rows (so the mates hold to first order), by the least-squares step toward the
//! targets, `Δ = N (AᵀA + μI)⁻¹ Aᵀ (−e)` with `A = J_pull N`, turns weighing ten times more than
//! moves (a free instance slides rather than spins); then it projects back onto the mates (the
//! solve above) and clamps the limits. A fully constrained instance has no null space: it
//! doesn't move. A revolute instance only turns.
//!
//! **When** (A6.11): the app never solves continuously. It solves when the second connector of
//! a mate is picked (with that mate's snap), on the dialog's **Solve**, on ✓, for **Reset** /
//! **Apply limit position** (a drive on that mate), per frame of an animation, and while
//! dragging. The result goes into the document through a command, as one undo step.
//!
//! **DOF** ([`dof_counts`], A16.1): the null space of the mate rows' Jacobian at the solution;
//! each body's degrees of freedom are the rank of its six rows of that null space.

use std::collections::HashMap;

use cadrs_sketch::Vec3;
use nalgebra::{DMatrix, DVector, Matrix3, Vector3};

use super::connector::{ConnectorFrame, MateConnector, SurfaceKind};
use super::mate::{Dof, Mate, MateId, MateKind, MateType, dof_value};
use super::{Assembly, InstanceId, Pose};

/// The length (mm) a radian weighs like.
pub const L: f64 = 10.0;
/// Converged when every residual is below this (mm).
pub const TOLERANCE: f64 = 1e-8;
pub const MAX_ITERATIONS: usize = 60;

/// A mate's position to hold: a DOF at a value (mm or radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drive {
    pub mate: MateId,
    pub dof: Dof,
    pub value: f64,
}

/// What to solve for.
#[derive(Debug, Clone, Default)]
pub struct SolveOptions {
    /// Instances that should do the moving (the others move only if they must).
    pub movers: Vec<InstanceId>,
    /// A mate to snap before iterating (the one just picked, Reset, Apply limit): its first
    /// movable connector's body is placed so the mate holds at its driven position.
    pub snap: Option<MateId>,
    /// Mate positions to hold (Reset: 0; Apply limit position: the limit; Animate: the frame's
    /// value).
    pub drives: Vec<Drive>,
    /// Only place the snapped mate's body; don't solve the other mates (while a mate dialog is
    /// being edited, A6.11: the rest of the assembly is solved on Solve and on ✓).
    pub snap_only: bool,
    /// The snap keeps the mate's undriven free DOF where they are (Animate), instead of at 0
    /// (a pick, Reset).
    pub hold_free: bool,
}

/// The instances that can't move: fixed, or grouped with a fixed instance.
pub fn grounded(asm: &Assembly) -> std::collections::HashSet<InstanceId> {
    let (bodies, _) = bodies_of(asm);
    bodies.iter().filter(|b| b.ground).flat_map(|b| b.members.iter().map(|(m, _)| *m)).collect()
}

/// The solved placements.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    /// Every instance's placement (in the assembly's order).
    pub poses: Vec<(InstanceId, Pose)>,
    pub converged: bool,
    /// The largest residual left (mm).
    pub residual: f64,
    pub iterations: usize,
}

impl Solution {
    /// Only the placements that differ from the assembly's.
    pub fn changed(&self, asm: &Assembly) -> Vec<(InstanceId, Pose)> {
        self.poses
            .iter()
            .filter(|(id, p)| asm.instance(*id).is_some_and(|i| !same_pose(&i.pose, p)))
            .copied()
            .collect()
    }
}

fn same_pose(a: &Pose, b: &Pose) -> bool {
    (0..3).all(|i| (a.translation[i] - b.translation[i]).abs() < 1e-9 && (0..3).all(|j| (a.rotation[i][j] - b.rotation[i][j]).abs() < 1e-12))
}

// ---------------------------------------------------------------------------------------------
// The problem

#[derive(Debug, Clone)]
struct Body {
    /// Each member and its placement relative to the body.
    members: Vec<(InstanceId, Pose)>,
    pose: Pose,
    ground: bool,
    /// The index of its first unknown, if it is free.
    var: Option<usize>,
    mobility: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Row {
    Trans3,
    Trans2,
    /// d·y_G, d·z_G (Pin slot).
    TransYZ,
    /// d·z_G (Planar).
    TransZ,
    Rot3,
    Axis2,
    Drive(Dof, f64),
    /// Two surfaces kept tangent; `true` flips the side.
    Tangent(SurfaceKind, SurfaceKind, bool),
    /// Tabs centred between the width pair; `true` when the second tab is on another body (its
    /// axis is held too).
    Width(bool),
    /// P3B.9: a relation between two mates' positions; the ends are the first mate's (F₁, G)
    /// then the second's.
    Relation(super::relation::Coupling),
}

/// The order of surface kinds in a Tangent pair (the lower first).
fn surface_rank(k: SurfaceKind) -> u8 {
    match k {
        SurfaceKind::Plane => 0,
        SurfaceKind::Cylinder { .. } => 1,
        SurfaceKind::Sphere { .. } => 2,
        SurfaceKind::Line => 3,
        SurfaceKind::Point => 4,
    }
}

/// How many rows a Tangent between `a` and `b` has (0: not supported).
fn tangent_len(a: SurfaceKind, b: SurfaceKind) -> usize {
    use SurfaceKind::*;
    let (a, b) = if surface_rank(a) <= surface_rank(b) { (a, b) } else { (b, a) };
    match (a, b) {
        (Plane, Plane) => 4,
        (Plane, Cylinder { .. }) | (Plane, Line) => 2,
        (Plane, Sphere { .. }) | (Plane, Point) => 1,
        (Cylinder { .. }, Cylinder { .. }) | (Cylinder { .. }, Line) => 4,
        (Cylinder { .. }, Sphere { .. }) | (Cylinder { .. }, Point) => 1,
        (Sphere { .. }, Sphere { .. }) | (Sphere { .. }, Line) | (Sphere { .. }, Point) => 1,
        _ => 0,
    }
}

/// Whether a Tangent mate between these two surfaces is supported.
pub fn tangent_supported(a: SurfaceKind, b: SurfaceKind) -> bool {
    tangent_len(a, b) > 0
}

/// How far two surfaces (world frames) are from tangent: the largest Tangent row (mm).
pub fn tangent_error(ka: SurfaceKind, a: &ConnectorFrame, kb: SurfaceKind, b: &ConnectorFrame, flip: bool) -> f64 {
    let mut out = Vec::new();
    tangent_rows(ka, a, kb, b, flip, &mut out);
    out.iter().fold(if out.is_empty() { f64::INFINITY } else { 0.0 }, |m, x| m.max(x.abs()))
}

impl Row {
    fn len(self) -> usize {
        match self {
            Row::Trans3 | Row::Rot3 => 3,
            Row::Trans2 | Row::TransYZ | Row::Axis2 => 2,
            Row::TransZ | Row::Drive(..) | Row::Relation(_) => 1,
            Row::Tangent(a, b, _) => tangent_len(a, b),
            Row::Width(two) => 3 + if two { 2 } else { 0 },
        }
    }
}

#[derive(Debug, Clone)]
struct Con {
    mate: MateId,
    /// The frames the rows compare, each in its body's coordinates: for connector mates the
    /// first connector's (adjusted) and the target; Tangent's two surfaces; Width's pair, then
    /// its tabs.
    ends: Vec<(usize, ConnectorFrame)>,
    rows: Vec<Row>,
    /// A mate row (not a limit or a drive): counts for DOF.
    is_mate: bool,
    /// The ends are (F₁, G) of a connector mate (it can be snapped, driven and limited).
    pair: bool,
}

fn rows_of(t: MateType) -> Vec<Row> {
    match t {
        MateType::Fastened => vec![Row::Trans3, Row::Rot3],
        MateType::Revolute => vec![Row::Trans3, Row::Axis2],
        MateType::Slider => vec![Row::Trans2, Row::Rot3],
        MateType::Cylindrical => vec![Row::Trans2, Row::Axis2],
        MateType::PinSlot => vec![Row::TransYZ, Row::Axis2],
        MateType::Planar => vec![Row::TransZ, Row::Axis2],
        MateType::Ball => vec![Row::Trans3],
        MateType::Parallel => vec![Row::Axis2],
        // Built from the mate's data in `Problem::new`.
        MateType::Tangent | MateType::Width => Vec::new(),
    }
}

type V3 = Vector3<f64>;

fn v(a: [f64; 3]) -> V3 {
    V3::from(a)
}

fn wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let r = (a + std::f64::consts::PI).rem_euclid(t) - std::f64::consts::PI;
    if r <= -std::f64::consts::PI { r + t } else { r }
}

/// The distance from `p` to the line through `o` along the unit `z`.
fn from_axis(p: V3, o: V3, z: V3) -> f64 {
    let d = p - o;
    (d - z * d.dot(&z)).norm()
}

/// The rows of a Tangent between surfaces `ka` at `a` and `kb` at `b` (world frames).
fn tangent_rows(ka: SurfaceKind, a: &ConnectorFrame, kb: SurfaceKind, b: &ConnectorFrame, flip: bool, out: &mut Vec<f64>) {
    use SurfaceKind::*;
    let (ka, a, kb, b) = if surface_rank(ka) <= surface_rank(kb) { (ka, a, kb, b) } else { (kb, b, ka, a) };
    let (oa, za, ob, zb) = (v(a.origin), v(a.z), v(b.origin), v(b.z));
    let s = if flip { -1.0 } else { 1.0 };
    let sum = |ra: f64, rb: f64| if flip { (ra - rb).abs() } else { ra + rb };
    match (ka, kb) {
        (Plane, Plane) => {
            // Coincident, facing each other (flipped: the same way).
            out.push((ob - oa).dot(&za));
            let e = (zb + za * s) * L;
            out.extend_from_slice(&[e.x, e.y, e.z]);
        }
        (Plane, Cylinder { radius }) => {
            out.push((ob - oa).dot(&za) - s * radius);
            out.push(zb.dot(&za) * L);
        }
        (Plane, Sphere { radius }) => out.push((ob - oa).dot(&za) - s * radius),
        (Plane, Line) => {
            out.push((ob - oa).dot(&za));
            out.push(zb.dot(&za) * L);
        }
        (Plane, Point) => out.push((ob - oa).dot(&za)),
        (Cylinder { radius: ra }, Cylinder { radius: rb }) => {
            let e = za.cross(&zb) * L;
            out.extend_from_slice(&[e.x, e.y, e.z]);
            out.push(from_axis(ob, oa, za) - sum(ra, rb));
        }
        (Cylinder { radius: ra }, Sphere { radius: rb }) => out.push(from_axis(ob, oa, za) - sum(ra, rb)),
        (Cylinder { radius }, Line) => {
            let e = za.cross(&zb) * L;
            out.extend_from_slice(&[e.x, e.y, e.z]);
            out.push(from_axis(ob, oa, za) - radius);
        }
        (Cylinder { radius }, Point) => out.push(from_axis(ob, oa, za) - radius),
        (Sphere { radius: ra }, Sphere { radius: rb }) => out.push((ob - oa).norm() - sum(ra, rb)),
        (Sphere { radius }, Line) => out.push(from_axis(oa, ob, zb) - radius),
        (Sphere { radius }, Point) => out.push((ob - oa).norm() - radius),
        _ => {}
    }
}

/// The direction across a pair of connectors: from the first to the second when they are apart
/// and roughly along the first's Z (or its Z isn't across at all, an edge's), else the first's
/// Z. So two picks on opposite faces, or on the edges of opposite faces, give the pair's normal.
fn across(a: &ConnectorFrame, b: Option<&ConnectorFrame>) -> V3 {
    let z = v(a.z);
    let Some(b) = b else { return z };
    let d = v(b.origin) - v(a.origin);
    let len = d.norm();
    if len < 1e-6 {
        return z;
    }
    let along = z.dot(&d).abs() / len;
    if along > 0.5 {
        return z;
    }
    // Two picks on parallel edges (the same Z, X along the edges, as an edge's implicit points
    // are): only the part of the step across them counts, so picking one edge at its end and the
    // other at its middle doesn't tilt the normal (Final part 3: `course_asm_mates_tangent_width`
    // 14–16, a Width pair picked at (160, −15) and (140, 15) turned the jaws about 34°).
    let (xa, zb, xb) = (v(a.x), v(b.z), v(b.x));
    if z.cross(&zb).norm() < 1e-6 && xa.cross(&xb).norm() < 1e-6 {
        let across = d - z * z.dot(&d) - xa * xa.dot(&d);
        let l = across.norm();
        if l > 1e-6 {
            return across / l;
        }
    }
    d / len
}

/// Width's centre-plane normal `n` (across the width pair) and two directions in the plane.
fn width_axes(a: &ConnectorFrame, g: &ConnectorFrame) -> (V3, V3, V3) {
    let n = across(a, Some(g));
    let x = v(super::connector::x_for([n.x, n.y, n.z]));
    (n, x, n.cross(&x))
}

/// The residual rows of `rows` for world frames `f` (see [`Con::ends`]).
fn residuals(rows: &[Row], f: &[ConnectorFrame], out: &mut Vec<f64>) {
    let (a, g) = (&f[0], &f[1]);
    let (pa, xa, za) = (v(a.origin), v(a.x), v(a.z));
    let ya = za.cross(&xa);
    let (pg, xg, zg) = (v(g.origin), v(g.x), v(g.z));
    let yg = zg.cross(&xg);
    let d = pa - pg;
    for r in rows {
        match *r {
            Row::Trans3 => out.extend_from_slice(&[d.x, d.y, d.z]),
            Row::Trans2 => out.extend_from_slice(&[d.dot(&xg), d.dot(&yg)]),
            // The slot is the first connector: the pin slides along its X (P3B.7: so realigning
            // the slot connector's X to the slot's edge, A23.3, sets the direction).
            Row::TransYZ => out.extend_from_slice(&[d.dot(&ya), d.dot(&za)]),
            Row::TransZ => out.push(d.dot(&zg)),
            Row::Rot3 => {
                let e = (xg.cross(&xa) + yg.cross(&ya) + zg.cross(&za)) * (0.5 * L);
                out.extend_from_slice(&[e.x, e.y, e.z]);
            }
            Row::Axis2 => out.extend_from_slice(&[za.dot(&xg) * L, za.dot(&yg) * L]),
            Row::Drive(dof, target) => {
                let val = dof_value(a, g, dof);
                out.push(if dof.is_angle() { wrap(val - target) * L } else { val - target });
            }
            Row::Tangent(ka, kb, flip) => tangent_rows(ka, a, kb, g, flip, out),
            Row::Relation(c) => {
                let v1 = dof_value(a, g, c.dofs[0]);
                let v2 = dof_value(&f[2], &f[3], c.dofs[1]);
                let r = c.residual(v1, v2);
                out.push(if c.angular { r * L } else { r });
            }
            Row::Width(two) => {
                // a, g: the width pair; the rest: the tabs.
                let (n, x1, y1) = width_axes(a, g);
                let centre = (pa + pg) / 2.0;
                let tabs = &f[2..];
                let mid = tabs.iter().map(|t| v(t.origin)).sum::<V3>() / tabs.len().max(1) as f64;
                out.push(n.dot(&(mid - centre)));
                let axis = |z: V3, out: &mut Vec<f64>| out.extend_from_slice(&[z.dot(&x1) * L, z.dot(&y1) * L]);
                if two && tabs.len() > 1 {
                    axis(v(tabs[0].z), out);
                    axis(v(tabs[1].z), out);
                } else {
                    axis(across(&tabs[0], tabs.get(1)), out);
                }
            }
        }
    }
}

/// A rotation by the vector `w` (Rodrigues; exact for any size).
fn exp(w: V3) -> Matrix3<f64> {
    let t = w.norm();
    if t < 1e-300 {
        return Matrix3::identity();
    }
    let k = w / t;
    let kx = Matrix3::new(0.0, -k.z, k.y, k.z, 0.0, -k.x, -k.y, k.x, 0.0);
    Matrix3::identity() + kx * t.sin() + kx * kx * (1.0 - t.cos())
}

fn mat(p: &Pose) -> Matrix3<f64> {
    p.rotation_matrix()
}

fn pose_of(r: Matrix3<f64>, t: V3) -> Pose {
    Pose {
        rotation: [[r[(0, 0)], r[(0, 1)], r[(0, 2)]], [r[(1, 0)], r[(1, 1)], r[(1, 2)]], [r[(2, 0)], r[(2, 1)], r[(2, 2)]]],
        translation: [t.x, t.y, t.z],
    }
}

/// `p` moved by the unknowns `x` (δ, ω) about the pivot `c`.
fn step(p: &Pose, x: &[f64], c: V3) -> Pose {
    let e = exp(V3::new(x[3], x[4], x[5]));
    let t = e * (v(p.translation) - c) + c + V3::new(x[0], x[1], x[2]);
    pose_of(e * mat(p), t)
}

/// Keeps a rotation orthonormal (after many small steps).
fn orthonormalize(p: &Pose) -> Pose {
    let r = mat(p);
    let x = r.column(0).normalize();
    let y = r.column(1) - x * x.dot(&r.column(1));
    let y = y.normalize();
    let z = x.cross(&y);
    pose_of(Matrix3::from_columns(&[x, y, z]), v(p.translation))
}

struct Problem {
    bodies: Vec<Body>,
    cons: Vec<Con>,
    /// The mates by id (for drives and limits).
    mates: HashMap<MateId, Mate>,
    n: usize,
}

/// The bodies of an assembly: instances joined by groups (union–find), ground if any member is
/// fixed.
fn bodies_of(asm: &Assembly) -> (Vec<Body>, HashMap<InstanceId, usize>) {
    let ids: Vec<InstanceId> = asm.instances.iter().map(|i| i.id).collect();
    let index: HashMap<InstanceId, usize> = ids.iter().enumerate().map(|(k, i)| (*i, k)).collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for f in asm.mates.iter().filter(|f| !f.suppressed) {
        if let MateKind::Group { instances } = &f.kind {
            let members: Vec<usize> = instances.iter().filter_map(|i| index.get(i).copied()).collect();
            for w in members.windows(2) {
                let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
                if a != b {
                    parent[b.max(a)] = a.min(b);
                }
            }
        }
    }
    let mut bodies: Vec<Body> = Vec::new();
    let mut root_body: HashMap<usize, usize> = HashMap::new();
    let mut of: HashMap<InstanceId, usize> = HashMap::new();
    for (k, inst) in asm.instances.iter().enumerate() {
        let r = find(&mut parent, k);
        let b = *root_body.entry(r).or_insert_with(|| {
            bodies.push(Body { members: Vec::new(), pose: inst.pose, ground: false, var: None, mobility: 1.0 });
            bodies.len() - 1
        });
        let body = &mut bodies[b];
        let rel = inst.pose.then(&body.pose.inverse());
        body.members.push((inst.id, rel));
        body.ground |= inst.fixed;
        of.insert(inst.id, b);
    }
    // P3B.7 (A16.3): the assembly's Origin, ground at the identity, when a mate uses it.
    let uses_origin = asm.mates.iter().filter(|f| !f.suppressed).any(|f| f.mate().is_some_and(|m| m.all_connectors().any(|c| c.is_origin())));
    if uses_origin && !of.contains_key(&InstanceId::ORIGIN) {
        bodies.push(Body { members: vec![(InstanceId::ORIGIN, Pose::IDENTITY)], pose: Pose::IDENTITY, ground: true, var: None, mobility: 1.0 });
        of.insert(InstanceId::ORIGIN, bodies.len() - 1);
    }
    (bodies, of)
}

impl Problem {
    fn new(asm: &Assembly, frame: &dyn Fn(&MateConnector) -> ConnectorFrame, opts: &SolveOptions) -> Self {
        let (mut bodies, of) = bodies_of(asm);
        let rel = |bodies: &[Body], b: usize, i: InstanceId| -> Pose {
            bodies[b].members.iter().find(|(m, _)| *m == i).map(|(_, p)| *p).unwrap_or(Pose::IDENTITY)
        };
        let mut cons = Vec::new();
        let mut mates = HashMap::new();
        // P3B.9: each connector mate's (F₁, G) ends, for the relations.
        let mut pair_ends: HashMap<MateId, Vec<(usize, ConnectorFrame)>> = HashMap::new();
        for f in asm.mates.iter().filter(|f| !f.suppressed) {
            let MateKind::Mate(m) = &f.kind else { continue };
            let conns: Vec<&MateConnector> = if m.mate_type == MateType::Width {
                if m.tabs.is_empty() {
                    continue;
                }
                m.all_connectors().collect()
            } else {
                m.connectors.iter().collect()
            };
            let Some(bs) = conns.iter().map(|c| of.get(&c.instance).copied()).collect::<Option<Vec<usize>>>() else { continue };
            mates.insert(f.id, m.clone());
            let local: Vec<ConnectorFrame> = conns.iter().zip(&bs).map(|(c, b)| frame(c).moved(&rel(&bodies, *b, c.instance))).collect();
            if !matches!(m.mate_type, MateType::Tangent | MateType::Width) {
                let mut ends: Vec<(usize, ConnectorFrame)> = bs.iter().copied().zip(local.iter().copied()).collect();
                ends[1].1 = m.target(&ends[1].1);
                pair_ends.insert(f.id, ends);
            }
            if bs.iter().all(|b| *b == bs[0]) || bs.iter().all(|b| bodies[*b].ground) {
                continue;
            }
            let (rows, pair) = match m.mate_type {
                MateType::Tangent => {
                    let (Some(ka), Some(kb)) = (m.connectors[0].surface_kind(), m.connectors[1].surface_kind()) else { continue };
                    (vec![Row::Tangent(ka, kb, m.flip)], false)
                }
                MateType::Width => (vec![Row::Width(bs.len() > 3 && bs[3] != bs[2])], false),
                t => (rows_of(t), true),
            };
            let mut ends: Vec<(usize, ConnectorFrame)> = bs.iter().copied().zip(local).collect();
            if pair {
                ends[1].1 = m.target(&ends[1].1);
                let drives: Vec<Row> = opts.drives.iter().filter(|d| d.mate == f.id).map(|d| Row::Drive(d.dof, d.value)).collect();
                if !drives.is_empty() {
                    cons.push(Con { mate: f.id, ends: ends.clone(), rows: drives, is_mate: false, pair });
                }
            }
            let is_mate = !rows.is_empty();
            cons.push(Con { mate: f.id, ends, rows, is_mate, pair });
        }
        // P3B.9: relations couple two mates' positions (Screw: one mate's two).
        for f in asm.mates.iter().filter(|f| !f.suppressed) {
            let MateKind::Relation(r) = &f.kind else { continue };
            let types: Option<Vec<MateType>> = r.mates.iter().map(|id| mates.get(id).map(|m: &Mate| m.mate_type)).collect();
            let Some(types) = types else { continue };
            let Some(c) = super::relation::coupling(r, &types) else { continue };
            let first = r.mates.first().and_then(|m| pair_ends.get(m));
            let second = r.mates.get(1).or(r.mates.first()).and_then(|m| pair_ends.get(m));
            let (Some(a), Some(b)) = (first, second) else { continue };
            let ends: Vec<(usize, ConnectorFrame)> = a.iter().chain(b.iter()).copied().collect();
            if ends.iter().all(|e| bodies[e.0].ground) {
                continue;
            }
            cons.push(Con { mate: f.id, ends, rows: vec![Row::Relation(c)], is_mate: true, pair: false });
        }
        let movers: Vec<usize> = opts.movers.iter().filter_map(|i| of.get(i).copied()).collect();
        let mut n = 0;
        for (k, b) in bodies.iter_mut().enumerate() {
            if !b.ground {
                b.var = Some(n);
                n += 6;
                b.mobility = if movers.is_empty() || movers.contains(&k) { 1.0 } else { 1e-3 };
            }
        }
        Self { bodies, cons, mates, n }
    }

    fn world(&self, (b, f): &(usize, ConnectorFrame)) -> ConnectorFrame {
        f.moved(&self.bodies[*b].pose)
    }

    fn worlds(&self, c: &Con) -> Vec<ConnectorFrame> {
        c.ends.iter().map(|e| self.world(e)).collect()
    }

    fn residual_vec(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for c in &self.cons {
            residuals(&c.rows, &self.worlds(c), &mut out);
        }
        out
    }

    /// The pivot of each body: the middle of its connector origins (world).
    fn pivots(&self) -> Vec<V3> {
        let mut sum = vec![(V3::zeros(), 0usize); self.bodies.len()];
        for c in &self.cons {
            for end in &c.ends {
                let w = self.world(end);
                sum[end.0].0 += v(w.origin);
                sum[end.0].1 += 1;
            }
        }
        sum.iter()
            .enumerate()
            .map(|(k, (s, n))| if *n > 0 { s / *n as f64 } else { v(self.bodies[k].pose.translation) })
            .collect()
    }

    /// Each constraint's rows' Jacobian blocks: per row, one (body, 6 values) per body it
    /// involves.
    fn jacobian(&self, pivots: &[V3], only_mates: bool) -> Vec<Vec<(usize, [f64; 6])>> {
        let h = 1e-6;
        let mut rows = Vec::new();
        for c in &self.cons {
            if only_mates && !c.is_mate {
                continue;
            }
            let len: usize = c.rows.iter().map(|r| r.len()).sum();
            let mut blocks: Vec<Vec<(usize, [f64; 6])>> = vec![Vec::new(); len];
            let mut bodies: Vec<usize> = Vec::new();
            for e in &c.ends {
                if !bodies.contains(&e.0) {
                    bodies.push(e.0);
                }
            }
            for &b in &bodies {
                if self.bodies[b].var.is_none() {
                    continue;
                }
                let mut cols = vec![[0.0; 6]; len];
                for k in 0..6 {
                    let eval = |s: f64| -> Vec<f64> {
                        let mut x = [0.0; 6];
                        x[k] = s * if k >= 3 { h / L } else { h };
                        let p = step(&self.bodies[b].pose, &x, pivots[b]);
                        let frames: Vec<ConnectorFrame> =
                            c.ends.iter().map(|e| if e.0 == b { e.1.moved(&p) } else { self.world(e) }).collect();
                        let mut out = Vec::new();
                        residuals(&c.rows, &frames, &mut out);
                        out
                    };
                    let (plus, minus) = (eval(1.0), eval(-1.0));
                    let hk = if k >= 3 { h / L } else { h };
                    for r in 0..len {
                        let mut dr = plus[r] - minus[r];
                        // Angles wrap: a drive row near ±π.
                        if dr.abs() > std::f64::consts::PI * L {
                            dr -= (std::f64::consts::TAU * L).copysign(dr);
                        }
                        cols[r][k] = dr / (2.0 * hk);
                    }
                }
                for r in 0..len {
                    blocks[r].push((b, cols[r]));
                }
            }
            rows.extend(blocks);
        }
        rows
    }

    /// The mobility of body `b`'s unknown `k`: translations by the body's mobility, rotations
    /// by it over L².
    fn mobility(&self, b: usize, k: usize) -> f64 {
        let m = self.bodies[b].mobility;
        if k >= 3 { m / (L * L) } else { m }
    }

    /// One damped minimum-norm step for the residuals `r`.
    fn step_for(&self, jac: &[Vec<(usize, [f64; 6])>], r: &[f64], lambda: f64) -> Option<Vec<f64>> {
        let m = r.len();
        let mut a = DMatrix::<f64>::zeros(m, m);
        // Rows by body.
        let mut by_body: Vec<Vec<(usize, [f64; 6])>> = vec![Vec::new(); self.bodies.len()];
        for (i, row) in jac.iter().enumerate() {
            for (b, vals) in row {
                by_body[*b].push((i, *vals));
            }
        }
        for (b, list) in by_body.iter().enumerate() {
            let w: [f64; 6] = std::array::from_fn(|k| self.mobility(b, k));
            for (i, vi) in list {
                for (j, vj) in list {
                    if j < i {
                        continue;
                    }
                    let s: f64 = (0..6).map(|k| vi[k] * w[k] * vj[k]).sum();
                    a[(*i, *j)] += s;
                    if i != j {
                        a[(*j, *i)] += s;
                    }
                }
            }
        }
        let scale = (0..m).map(|i| a[(i, i)]).fold(0.0f64, f64::max).max(1e-12);
        for i in 0..m {
            a[(i, i)] += lambda * scale + 1e-14 * scale;
        }
        let y = a.cholesky()?.solve(&DVector::from_column_slice(r));
        let mut dx = vec![0.0; self.n];
        for (i, row) in jac.iter().enumerate() {
            for (b, vals) in row {
                let Some(o) = self.bodies[*b].var else { continue };
                for k in 0..6 {
                    dx[o + k] -= self.mobility(*b, k) * vals[k] * y[i];
                }
            }
        }
        Some(dx)
    }

    fn apply(&mut self, dx: &[f64], pivots: &[V3]) {
        for (k, b) in self.bodies.iter_mut().enumerate() {
            if let Some(o) = b.var {
                b.pose = orthonormalize(&step(&b.pose, &dx[o..o + 6], pivots[k]));
            }
        }
    }

    /// Iterates until the residuals vanish. Returns (converged, residual, iterations).
    fn iterate(&mut self, budget: usize) -> (bool, f64, usize) {
        let norm = |r: &[f64]| r.iter().map(|x| x * x).sum::<f64>();
        let max = |r: &[f64]| r.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        let mut r = self.residual_vec();
        let mut lambda = 1e-9;
        let mut it = 0;
        while it < budget {
            if max(&r) < TOLERANCE || self.n == 0 {
                return (true, max(&r), it);
            }
            it += 1;
            let pivots = self.pivots();
            let jac = self.jacobian(&pivots, false);
            let mut improved = false;
            for _ in 0..12 {
                let Some(dx) = self.step_for(&jac, &r, lambda) else {
                    lambda *= 10.0;
                    continue;
                };
                let saved: Vec<Pose> = self.bodies.iter().map(|b| b.pose).collect();
                self.apply(&dx, &pivots);
                let r2 = self.residual_vec();
                if norm(&r2) < norm(&r) {
                    r = r2;
                    lambda = (lambda / 10.0).max(1e-15);
                    improved = true;
                    break;
                }
                for (b, p) in self.bodies.iter_mut().zip(saved) {
                    b.pose = p;
                }
                lambda *= 10.0;
            }
            if !improved {
                break;
            }
        }
        (max(&r) < TOLERANCE, max(&r), it)
    }

    /// The driven position of connector-mate constraint `c`: each DOF at its drive, the other
    /// free DOF where they are (`hold_free`) or at 0, and the removed DOF at 0.
    fn snap_values(&self, ci: usize, drives: &[Drive], hold_free: bool) -> [f64; 4] {
        let c = &self.cons[ci];
        let (a, g) = (self.world(&c.ends[0]), self.world(&c.ends[1]));
        let Some(m) = self.mates.get(&c.mate) else { return [0.0; 4] };
        let mut vals = [0.0; 4];
        for (k, dof) in [Dof::X, Dof::Y, Dof::Z, Dof::Angle].into_iter().enumerate() {
            if !m.mate_type.dof().contains(&dof) {
                continue;
            }
            if let Some(d) = drives.iter().find(|d| d.mate == c.mate && d.dof == dof) {
                vals[k] = d.value;
            } else if hold_free {
                vals[k] = dof_value(&a, &g, dof);
            }
        }
        vals
    }

    /// Places the first movable body of connector-mate constraint `ci` so the constraint holds
    /// exactly at the position `[x, y, z, angle]`.
    fn snap(&mut self, ci: usize, at: [f64; 4]) -> Option<usize> {
        let c = self.cons[ci].clone();
        if !c.pair {
            return None;
        }
        let (a, g) = (c.ends[0], c.ends[1]);
        // D = F₁⁻¹ G at the drive: G = F₁ · D.
        let d = Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], at[3]).then(&Pose::translation([at[0], at[1], at[2]]));
        let place = |local: &ConnectorFrame, want: &ConnectorFrame| -> Pose { local.pose().inverse().then(&want.pose()) };
        if !self.bodies[a.0].ground {
            // F₁ = G · D⁻¹.
            let gw = self.world(&g);
            let want = gw.then_local(&d.inverse());
            self.bodies[a.0].pose = place(&a.1, &want);
            Some(a.0)
        } else if !self.bodies[g.0].ground {
            let fa = self.world(&a);
            let want = fa.then_local(&d);
            self.bodies[g.0].pose = place(&g.1, &want);
            Some(g.0)
        } else {
            None
        }
    }

    /// The constraints whose axes ended opposite (a false solution of the rotation rows).
    fn false_solutions(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for (k, c) in self.cons.iter().enumerate() {
            if !c.is_mate || !c.pair {
                continue;
            }
            let (a, g) = (self.world(&c.ends[0]), self.world(&c.ends[1]));
            let zz = v(a.z).dot(&v(g.z));
            let xx = v(a.x).dot(&v(g.x));
            let rot3 = c.rows.contains(&Row::Rot3);
            let axis = rot3 || c.rows.contains(&Row::Axis2);
            if (axis && zz < 0.0) || (rot3 && xx < 0.0) {
                out.push(k);
            }
        }
        out
    }

    fn limit_rows(&self) -> Vec<Con> {
        let mut out = Vec::new();
        for c in &self.cons {
            if !c.is_mate || !c.pair {
                continue;
            }
            let Some(m) = self.mates.get(&c.mate) else { continue };
            let (a, g) = (self.world(&c.ends[0]), self.world(&c.ends[1]));
            for dof in m.mate_type.limit_dofs() {
                let Some((lo, hi)) = m.limit(*dof) else { continue };
                let val = dof_value(&a, &g, *dof);
                let eps = if dof.is_angle() { 1e-9 } else { 1e-7 };
                if val < lo - eps || val > hi + eps {
                    let at = val.clamp(lo, hi);
                    out.push(Con { mate: c.mate, ends: c.ends.clone(), rows: vec![Row::Drive(*dof, at)], is_mate: false, pair: true });
                }
            }
        }
        out
    }

    /// Solves the equalities, then the limits (an active set, a few rounds). Returns
    /// (converged, residual, iterations).
    fn solve_with_limits(&mut self) -> (bool, f64, usize) {
        let base = self.cons.len();
        let (mut ok, mut res, mut its) = self.iterate(MAX_ITERATIONS);
        for _ in 0..4 {
            let extra = self.limit_rows();
            if extra.is_empty() {
                break;
            }
            self.cons.extend(extra);
            let (a, b, c) = self.iterate(MAX_ITERATIONS);
            (ok, res, its) = (a, b, its + c);
        }
        self.cons.truncate(base);
        (ok, res, its)
    }

    fn poses(&self, asm: &Assembly) -> Vec<(InstanceId, Pose)> {
        asm.instances
            .iter()
            .filter_map(|i| {
                let b = self.bodies.iter().find(|b| b.members.iter().any(|(m, _)| *m == i.id))?;
                let rel = b.members.iter().find(|(m, _)| *m == i.id)?.1;
                Some((i.id, if b.ground || b.var.is_none() { i.pose } else { rel.then(&b.pose) }))
            })
            .collect()
    }
}

/// Solves the assembly's mates (see the module doc). `frame` gives a connector's frame in its
/// instance's coordinates, flip and reorient applied ([`MateConnector::local_frame`] on the
/// source part as it is now).
pub fn solve(asm: &Assembly, frame: &dyn Fn(&MateConnector) -> ConnectorFrame, opts: &SolveOptions) -> Solution {
    let mut p = Problem::new(asm, frame, opts);
    if let Some(id) = opts.snap
        && let Some(ci) = p.cons.iter().position(|c| c.mate == id && c.is_mate)
    {
        let at = p.snap_values(ci, &opts.drives, opts.hold_free);
        if let Some(b) = p.snap(ci, at) {
            // The snapped body does the moving.
            for (k, body) in p.bodies.iter_mut().enumerate() {
                if body.var.is_some() {
                    body.mobility = if k == b { 1.0 } else { 1e-3 };
                }
            }
        }
    }
    if opts.snap_only {
        return Solution { poses: p.poses(asm), converged: true, residual: 0.0, iterations: 0 };
    }
    let (_, _, mut its) = p.iterate(MAX_ITERATIONS);
    // A false solution (axes opposite): snap that mate and try again, once.
    let wrong = p.false_solutions();
    if !wrong.is_empty() {
        for k in wrong {
            let at = p.snap_values(k, &opts.drives, true);
            p.snap(k, at);
        }
        its += p.iterate(MAX_ITERATIONS).2;
    }
    // Limits: an active set, a few rounds (it first finishes the equalities).
    let (ok, res, c) = p.solve_with_limits();
    Solution { poses: p.poses(asm), converged: ok, residual: res, iterations: its + c }
}

/// A point of an instance pulled toward a target while dragging (A16.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pull {
    pub instance: InstanceId,
    /// In the instance's coordinates.
    pub point: Vec3,
    /// In the assembly's.
    pub target: Vec3,
    /// A direction the distance to the target isn't measured along (the view direction, for a
    /// drag under the pointer: the point goes where it looks nearest the pointer).
    pub view: Option<Vec3>,
}

/// How much more a turn weighs than a move while dragging (so a free instance slides).
const DRAG_TURN_WEIGHT: f64 = 10.0;

/// Moves the assembly toward `pulls` as far as the mates and limits allow (see the module doc):
/// the placements after the drag. Starts from the assembly's placements (which should satisfy
/// the mates: a drag continues from the last frame's result).
pub fn drag(asm: &Assembly, frame: &dyn Fn(&MateConnector) -> ConnectorFrame, pulls: &[Pull]) -> Solution {
    let mut p = Problem::new(asm, frame, &SolveOptions::default());
    let (_, of) = bodies_of(asm);
    let pulls: Vec<(usize, Pose, Pull)> = pulls
        .iter()
        .filter_map(|q| {
            let b = *of.get(&q.instance)?;
            let rel = p.bodies[b].members.iter().find(|(m, _)| *m == q.instance)?.1;
            p.bodies[b].var.is_some().then_some((b, rel, *q))
        })
        .collect();
    let mut its = 0;
    if p.n > 0 && !pulls.is_empty() {
        let n = p.n;
        let base = p.cons.len();
        // Scaled unknowns: moves in mm, turns in mm × L × weight.
        let scale = |k: usize| if k % 6 >= 3 { 1.0 / (L * DRAG_TURN_WEIGHT) } else { 1.0 };
        // Each pull's projector: the identity, or off its view direction.
        let proj: Vec<Matrix3<f64>> = pulls
            .iter()
            .map(|(_, _, q)| match q.view.and_then(|n| v(n).try_normalize(1e-12)) {
                Some(n) => Matrix3::identity() - n * n.transpose(),
                None => Matrix3::identity(),
            })
            .collect();
        let error = |p: &Problem| -> Vec<f64> {
            let mut e = Vec::new();
            for (k, (b, rel, q)) in pulls.iter().enumerate() {
                let w = rel.then(&p.bodies[*b].pose).apply(q.point);
                let d = proj[k] * V3::new(w[0] - q.target[0], w[1] - q.target[1], w[2] - q.target[2]);
                e.extend_from_slice(&[d.x, d.y, d.z]);
            }
            e
        };
        let norm = |e: &[f64]| e.iter().map(|x| x * x).sum::<f64>().sqrt();
        for _ in 0..40 {
            its += 1;
            let e = error(&p);
            let e0 = norm(&e);
            if e0 < 1e-6 {
                break;
            }
            let pivots = p.pivots();
            // The mate rows' Jacobian (dense, scaled columns) and its null space.
            let jac = p.jacobian(&pivots, true);
            let mut jc = DMatrix::<f64>::zeros(jac.len(), n);
            for (i, row) in jac.iter().enumerate() {
                for (b, vals) in row {
                    if let Some(o) = p.bodies[*b].var {
                        for k in 0..6 {
                            jc[(i, o + k)] = vals[k] * scale(k);
                        }
                    }
                }
            }
            let null = if jac.is_empty() { DMatrix::identity(n, n) } else { null_space(&jc) };
            if null.ncols() == 0 {
                break;
            }
            // The pulls' Jacobian: a point w of body b moves by δ + ω × (w − c).
            let mut jd = DMatrix::<f64>::zeros(3 * pulls.len(), n);
            for (i, (b, rel, q)) in pulls.iter().enumerate() {
                let Some(o) = p.bodies[*b].var else { continue };
                let w = v(rel.then(&p.bodies[*b].pose).apply(q.point));
                let r = w - pivots[*b];
                // ∂(ω × r)/∂ω = −[r]×.
                let rx = Matrix3::new(0.0, -r.z, r.y, r.z, 0.0, -r.x, -r.y, r.x, 0.0);
                let pm = proj[i];
                let rot = pm * (-rx);
                for a in 0..3 {
                    for k in 0..3 {
                        jd[(3 * i + a, o + k)] = pm[(a, k)];
                        jd[(3 * i + a, o + 3 + k)] = rot[(a, k)] * scale(3 + k);
                    }
                }
            }
            let am = &jd * &null;
            let ata = am.transpose() * &am;
            let mu = 1e-9 * ata.diagonal().iter().fold(1.0f64, |m, x| m.max(*x));
            let rhs = -(am.transpose() * DVector::from_column_slice(&e));
            let Some(z) = (ata + DMatrix::identity(null.ncols(), null.ncols()) * mu).cholesky().map(|c| c.solve(&rhs)) else { break };
            let mut dx: Vec<f64> = (&null * z).iter().copied().collect();
            for (k, x) in dx.iter_mut().enumerate() {
                *x *= scale(k);
            }
            // At most ~0.3 rad of turn per step.
            let turn = (0..n / 6).map(|b| V3::new(dx[6 * b + 3], dx[6 * b + 4], dx[6 * b + 5]).norm()).fold(0.0f64, f64::max);
            if turn > 0.3 {
                let s = 0.3 / turn;
                dx.iter_mut().for_each(|x| *x *= s);
            }
            let saved: Vec<Pose> = p.bodies.iter().map(|b| b.pose).collect();
            let mut accepted = false;
            for _ in 0..6 {
                p.apply(&dx, &pivots);
                p.cons.truncate(base);
                p.solve_with_limits();
                if norm(&error(&p)) < e0 - 1e-9 {
                    accepted = true;
                    break;
                }
                for (b, s) in p.bodies.iter_mut().zip(&saved) {
                    b.pose = *s;
                }
                dx.iter_mut().for_each(|x| *x *= 0.5);
            }
            if !accepted {
                break;
            }
        }
    }
    let r = p.residual_vec();
    let res = r.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    Solution { poses: p.poses(asm), converged: res < 1e-6, residual: res, iterations: its }
}

/// The degrees of freedom each instance has left under the mates (A16.1; 0 for grounded and
/// fully constrained ones): the rank of its body's rows of the null space of the mate rows'
/// Jacobian at the current placements.
pub fn dof_counts(asm: &Assembly, frame: &dyn Fn(&MateConnector) -> ConnectorFrame) -> HashMap<InstanceId, u32> {
    let p = Problem::new(asm, frame, &SolveOptions::default());
    let mut out: HashMap<InstanceId, u32> = asm.instances.iter().map(|i| (i.id, 0)).collect();
    if p.n == 0 {
        return out;
    }
    let pivots = p.pivots();
    let jac = p.jacobian(&pivots, true);
    // The free bodies the mates couple, in groups (bodies sharing a row): a body in no mate keeps
    // its 6 DOF, and each group's DOF come from its own Jacobian. One matrix over every body cost
    // O(n³) in all the assembly's bodies (thousands with flexible subassemblies: it never
    // finished).
    let free: Vec<usize> = (0..p.bodies.len()).filter(|b| p.bodies[*b].var.is_some()).collect();
    let mut group: HashMap<usize, usize> = free.iter().map(|b| (*b, *b)).collect();
    fn find(g: &mut HashMap<usize, usize>, b: usize) -> usize {
        let parent = g[&b];
        if parent == b {
            return b;
        }
        let root = find(g, parent);
        g.insert(b, root);
        root
    }
    for row in &jac {
        let bodies: Vec<usize> = row.iter().map(|(b, _)| *b).filter(|b| group.contains_key(b)).collect();
        for w in bodies.windows(2) {
            let (a, c) = (find(&mut group, w[0]), find(&mut group, w[1]));
            if a != c {
                group.insert(a, c);
            }
        }
    }
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for &b in &free {
        let r = find(&mut group, b);
        members.entry(r).or_default().push(b);
    }
    let mut rows_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, row) in jac.iter().enumerate() {
        if let Some((b, _)) = row.iter().find(|(b, _)| group.contains_key(b)) {
            let r = find(&mut group, *b);
            rows_of.entry(r).or_default().push(i);
        }
    }
    for (root, bodies) in &members {
        let rows = rows_of.get(root).map(Vec::as_slice).unwrap_or(&[]);
        let dofs: Vec<u32> = if rows.is_empty() {
            vec![6; bodies.len()]
        } else {
            // Columns scaled so rotations weigh like L mm (the same metric as the solve).
            let col: HashMap<usize, usize> = bodies.iter().enumerate().map(|(k, b)| (*b, k * 6)).collect();
            let mut j = DMatrix::<f64>::zeros(rows.len(), bodies.len() * 6);
            for (i, r) in rows.iter().enumerate() {
                for (b, vals) in &jac[*r] {
                    if let Some(o) = col.get(b) {
                        for k in 0..6 {
                            j[(i, o + k)] = vals[k];
                        }
                    }
                }
            }
            let null = null_space(&j);
            (0..bodies.len()).map(|k| if null.ncols() == 0 { 0 } else { rank(&null.rows(k * 6, 6).into_owned()) as u32 }).collect()
        };
        for (b, dof) in bodies.iter().zip(dofs) {
            for (m, _) in &p.bodies[*b].members {
                out.insert(*m, dof);
            }
        }
    }
    out
}

/// An orthonormal basis of the null space of `j` (columns).
fn null_space(j: &DMatrix<f64>) -> DMatrix<f64> {
    let n = j.ncols();
    let jtj = j.transpose() * j;
    let eig = nalgebra::SymmetricEigen::new(jtj);
    let top = eig.eigenvalues.iter().fold(0.0f64, |m, x| m.max(x.abs())).max(1e-300);
    let cols: Vec<DVector<f64>> = (0..n)
        .filter(|&k| eig.eigenvalues[k].abs() <= 1e-10 * top.max(1.0))
        .map(|k| eig.eigenvectors.column(k).into_owned())
        .collect();
    if cols.is_empty() { DMatrix::zeros(n, 0) } else { DMatrix::from_columns(&cols) }
}

fn rank(m: &DMatrix<f64>) -> usize {
    if m.ncols() == 0 || m.nrows() == 0 {
        return 0;
    }
    let s = m.clone().svd(false, false).singular_values;
    let top = s.iter().fold(0.0f64, |a, x| a.max(*x));
    s.iter().filter(|x| **x > 1e-6 * top.max(1e-300) && **x > 1e-9).count()
}

/// The rank of one connector mate's constraint Jacobian, with its first connector's instance
/// free and the second's fixed: 6 minus the DOF the type leaves (A7).
pub fn mate_rank(mate: &Mate, f1: ConnectorFrame, f2: ConnectorFrame) -> usize {
    let a = InstanceId::from_u128(1);
    let b = InstanceId::from_u128(2);
    let src = super::InstanceSource::Part { element: crate::ids::ElementId::from_u128(0), part: crate::ids::PartId::new(crate::ids::FeatureId::from_u128(0), 0) };
    let mut ia = super::Instance::new(a, src, Pose::IDENTITY);
    ia.index = 1;
    let mut ib = super::Instance::new(b, src, Pose::IDENTITY);
    ib.index = 2;
    ib.fixed = true;
    let mut m = mate.clone();
    m.connectors = [MateConnector { instance: a, frame: f1, ..mate.connectors[0] }, MateConnector { instance: b, frame: f2, ..mate.connectors[1] }];
    m.tabs.clear();
    let asm = Assembly { instances: vec![ia, ib], mates: vec![super::mate::MateFeature::new(MateId::from_u128(1), "m", MateKind::Mate(m))], ..Default::default() };
    let p = Problem::new(&asm, &|c: &MateConnector| c.adjust(c.frame), &SolveOptions::default());
    let pivots = p.pivots();
    let jac = p.jacobian(&pivots, true);
    let mut j = DMatrix::<f64>::zeros(jac.len().max(1), 6);
    for (i, row) in jac.iter().enumerate() {
        for (_, vals) in row {
            for k in 0..6 {
                j[(i, k)] = vals[k];
            }
        }
    }
    rank(&j)
}
