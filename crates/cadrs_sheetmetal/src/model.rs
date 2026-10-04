//! The sheet metal **definition** of one Sheet metal model (SM1.1): its walls and the joints
//! between them. The folded solid and the flat pattern are both made from it.
//!
//! - A [`Wall`] is one face of the **definition surface**: one side of the sheet, with the
//!   material on its `+normal` side, `thickness` deep. A planar wall has a frame and a 2D
//!   outline; a rolled wall is a piece of a cylinder, with an outline in (arc length, height).
//! - A [`Joint`] connects two walls: a **bend** (a cylindrical bend region between two tangent
//!   lines), a **rip** (a cut with a gap, nothing joins) or a **tangent** joint (a planar wall
//!   running smoothly into a rolled one).
//!
//! Walls are stored as they really are, trimmed back to the bends' tangent lines. Most features
//! think in **virtual sharps** instead (a flange's depth is measured from where the outside
//! faces would meet): [`SharpBuilder`] takes walls meeting at sharp edges, computes each bend's
//! angle, direction and setback, and trims the walls (and the rips) for you.

use nalgebra::{Point3, Vector3};
use serde::{Deserialize, Serialize};

use crate::bend::{self, BendValue};
use crate::params::{BendRelief, CornerRelief, Params};
use crate::poly::{P2, Polygon, Seg2, V2, perp};

pub type P3 = Point3<f64>;
pub type V3 = Vector3<f64>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WallId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct JointId(pub u32);

/// Where a wall lies in 3D.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Surface {
    /// Local 2D `(x, y)` ↦ `origin + x·u + y·v`; the material is on the `u × v` side.
    Planar { origin: P3, u: V3, v: V3 },
    /// A cylinder about `axis` through `axis_origin`, of radius `radius` (the definition surface's).
    /// Local 2D `(s, z)`: `s` is the arc length on the definition surface from the `start`
    /// direction, turning right-handed about `axis`; `z` runs along `axis`.
    Rolled {
        axis_origin: P3,
        axis: V3,
        start: V3,
        radius: f64,
        /// The material is outside the definition cylinder (the definition surface is the inner
        /// one).
        material_outside: bool,
    },
}

impl Surface {
    /// The unit normal on the material side of a planar surface.
    pub fn normal(&self) -> Option<V3> {
        match self {
            Surface::Planar { u, v, .. } => Some(u.cross(v).normalize()),
            Surface::Rolled { .. } => None,
        }
    }

    /// The unit normal on the material side at local point `p`.
    pub fn normal_at(&self, p: P2) -> V3 {
        match *self {
            Surface::Planar { u, v, .. } => u.cross(&v).normalize(),
            Surface::Rolled {
                axis, start, radius, material_outside, ..
            } => {
                let a = axis.normalize();
                let x = start.normalize();
                let y = a.cross(&x);
                let phi = p.x / radius;
                let radial = x * phi.cos() + y * phi.sin();
                if material_outside { radial } else { -radial }
            }
        }
    }

    /// A local 2D direction `d` at local point `p`, in 3D (unit length).
    pub fn direction_at(&self, p: P2, d: V2) -> V3 {
        match *self {
            Surface::Planar { u, v, .. } => (u * d.x + v * d.y).normalize(),
            Surface::Rolled { axis, start, radius, .. } => {
                let a = axis.normalize();
                let x = start.normalize();
                let y = a.cross(&x);
                let phi = p.x / radius;
                let tangent = -x * phi.sin() + y * phi.cos();
                (tangent * d.x + a * d.y).normalize()
            }
        }
    }

    /// Local 2D → 3D.
    pub fn point(&self, p: P2) -> P3 {
        match *self {
            Surface::Planar { origin, u, v } => origin + u * p.x + v * p.y,
            Surface::Rolled {
                axis_origin,
                axis,
                start,
                radius,
                ..
            } => {
                let a = axis.normalize();
                let x = start.normalize();
                let y = a.cross(&x);
                let phi = p.x / radius;
                axis_origin + a * p.y + (x * phi.cos() + y * phi.sin()) * radius
            }
        }
    }

    /// 3D → local 2D (planar surfaces; the point is projected onto the plane).
    pub fn local(&self, p: P3) -> P2 {
        match *self {
            Surface::Planar { origin, u, v } => P2::new((p - origin).dot(&u) / u.norm_squared(), (p - origin).dot(&v) / v.norm_squared()),
            Surface::Rolled {
                axis_origin,
                axis,
                start,
                radius,
                ..
            } => {
                let a = axis.normalize();
                let x = start.normalize();
                let y = a.cross(&x);
                let d = p - axis_origin;
                let phi = d.dot(&y).atan2(d.dot(&x)).rem_euclid(std::f64::consts::TAU);
                P2::new(phi * radius, d.dot(&a))
            }
        }
    }
}

/// One wall of the definition surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wall {
    pub id: WallId,
    pub surface: Surface,
    /// The wall's real outline (trimmed back to its bends) in its local 2D coordinates.
    pub outline: Polygon,
}

impl Wall {
    /// How a local `x` (arc length on the definition surface for a rolled wall) stretches when
    /// laid flat: 1 for a planar wall; for a rolled wall, the neutral radius over the definition
    /// radius, the neutral radius being `inner + rolled K × thickness` (SM2.3).
    pub fn flat_scale(&self, p: &Params) -> f64 {
        match self.surface {
            Surface::Planar { .. } => 1.0,
            Surface::Rolled {
                radius,
                material_outside,
                ..
            } => {
                let inner = if material_outside { radius } else { radius - p.thickness };
                (inner + p.rolled_k_factor * p.thickness) / radius
            }
        }
    }

    /// Local 2D → the wall's own flat 2D (before it is placed in the flat pattern).
    pub fn flat_local(&self, p: &Params, q: P2) -> P2 {
        P2::new(q.x * self.flat_scale(p), q.y)
    }
}

/// Rip styles (SM6.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RipStyle {
    /// Both walls stop where their inner faces meet, the minimal gap apart.
    #[default]
    EdgeJoint,
    /// The first wall stops short; the second runs on to cover its end (90° only).
    ButtDirection1,
    /// The other way round (90° only).
    ButtDirection2,
}

impl RipStyle {
    pub const ALL: [RipStyle; 3] = [RipStyle::EdgeJoint, RipStyle::ButtDirection1, RipStyle::ButtDirection2];

    pub fn label(self) -> &'static str {
        match self {
            RipStyle::EdgeJoint => "Edge joint",
            RipStyle::ButtDirection1 => "Butt joint - Direction 1",
            RipStyle::ButtDirection2 => "Butt joint - Direction 2",
        }
    }
}

/// A bend between two walls. `on_a` and `on_b` are its tangent lines on each wall (in their
/// local 2D), of the same length, `on_a.a` matching `on_b.a`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bend {
    pub on_a: Seg2,
    pub on_b: Seg2,
    /// Bend angle (radians): how far wall `b` turns from wall `a`'s continuation. Hems are 180°
    /// and more.
    pub angle: f64,
    /// Wall `b` turns towards wall `a`'s material side (the definition surface is then on the
    /// outside of the bend).
    pub toward_material: bool,
    /// Inner radius (mm).
    pub radius: f64,
    /// The radius came from the model ("Use model bend radius").
    pub model_radius: bool,
    /// This bend's own K factor, allowance or deduction; `None` uses the model's (SM6.4).
    pub value: Option<BendValue>,
    /// The bend is a hem's (SM4.5: listed, reorderable, not editable).
    #[serde(default)]
    pub hem: bool,
}

impl Bend {
    pub fn value_or_model(&self, p: &Params) -> BendValue {
        self.value.unwrap_or_else(|| BendValue::from_params(p))
    }

    /// The bend region's flat width. The model's allowance or deduction is stated for its
    /// ordinary bends: a deduction has no meaning from 180° on and an allowance meant for 90°
    /// bends would be far off for a hem, so hems (and other bends of 180° or more) without their
    /// own value use the model's K factor. (`None` only for a bend's own deduction there.)
    /// An assumption until checked against Onshape, which doesn't document hems in these modes.
    pub fn allowance(&self, p: &Params) -> Option<f64> {
        if self.value.is_none() && self.angle >= std::f64::consts::PI - 1e-9 && p.bend_calc != crate::params::BendCalc::KFactor {
            return Some(bend::bend_allowance(self.radius, p.thickness, self.angle, p.k_factor));
        }
        self.value_or_model(p).allowance(self.radius, p.thickness, self.angle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum JointKind {
    Bend(Bend),
    /// A cut between two walls; `on_a`/`on_b` are the walls' edges at the rip.
    Rip { on_a: Seg2, on_b: Seg2, style: RipStyle },
    /// A planar wall running into a rolled one (or two rolled walls); their common edge.
    Tangent { on_a: Seg2, on_b: Seg2 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub id: JointId,
    /// "Bend A", "Rip 1", … (the table's Name column and the labels in the views).
    pub name: String,
    pub a: WallId,
    pub b: WallId,
    pub kind: JointKind,
}

impl Joint {
    pub fn bend(&self) -> Option<&Bend> {
        match &self.kind {
            JointKind::Bend(b) => Some(b),
            _ => None,
        }
    }

    /// Whether the joint keeps its walls together in the flat pattern (bends and tangent joints).
    pub fn connects(&self) -> bool {
        !matches!(self.kind, JointKind::Rip { .. })
    }

    /// The joint's edge on wall `w` (`None` if `w` is neither of its walls).
    pub fn segment_on(&self, w: WallId) -> Option<Seg2> {
        let (sa, sb) = match &self.kind {
            JointKind::Bend(b) => (b.on_a, b.on_b),
            JointKind::Rip { on_a, on_b, .. } | JointKind::Tangent { on_a, on_b } => (*on_a, *on_b),
        };
        if w == self.a {
            Some(sa)
        } else if w == self.b {
            Some(sb)
        } else {
            None
        }
    }

    pub fn other(&self, w: WallId) -> WallId {
        if w == self.a { self.b } else { self.a }
    }
}

/// Which end of a bend (its tangent lines' `a` or `b` end).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BendEnd {
    Start,
    End,
}

/// A Corner feature's override of one corner (SM7): the corner where two bends meet.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerOverride {
    pub bends: (JointId, JointId),
    pub relief: CornerRelief,
}

/// A Bend relief feature's override of one bend end (SM8).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BendReliefOverride {
    pub bend: JointId,
    pub end: BendEnd,
    pub relief: BendRelief,
}

/// A bend in 3D (for the folded solid, highlighting and drawings). The bend region is the part of
/// the cylinders about the axis from `ends.0` to `ends.1`, between `inner_radius` and
/// `outer_radius`, from the direction `start` (axis to the first wall's tangent line) turning
/// right-handed about `axis` by `sweep`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BendGeom {
    /// The axis points level with the tangent lines' two ends.
    pub ends: (P3, P3),
    /// Unit axis; positive rotation about it turns the first wall into the second.
    pub axis: V3,
    pub inner_radius: f64,
    pub outer_radius: f64,
    /// The definition surface's radius (outer when the bend turns towards the material).
    pub def_radius: f64,
    /// Unit direction from the axis to the first wall's tangent line.
    pub start: V3,
    pub sweep: f64,
}

impl BendGeom {
    /// Rodrigues rotation of a vector about the axis.
    pub fn rotate_vec(&self, v: V3, angle: f64) -> V3 {
        let k = self.axis;
        v * angle.cos() + k.cross(&v) * angle.sin() + k * k.dot(&v) * (1.0 - angle.cos())
    }

    /// A point turned about the axis.
    pub fn rotate(&self, p: P3, angle: f64) -> P3 {
        let o = self.ends.0;
        o + self.rotate_vec(p - o, angle)
    }

    /// A tool that takes `cut` (a polygon in the bend's `(s, u)`: `s` along it from `ends.0`,
    /// `u` across, 0 to `allowance`) out of the bend's shell, through the thickness: the
    /// polygon's outline swept radially (each edge in steps of at most 2° of the bend), closed
    /// by caps inside the inner radius and outside the outer one. A closed triangle mesh, for
    /// cuts whose sides aren't along or across the bend (a round relief, a slanted slot): their
    /// faces follow the outline instead of a staircase. `None` for polygons with holes or
    /// spanning more than 170° of the bend.
    pub fn cut_mesh(&self, cut: &crate::poly::Polygon, allowance: f64, thickness: f64) -> Option<Vec<[P3; 3]>> {
        use crate::poly::P2;
        if !cut.holes.is_empty() || cut.outer.len() < 3 || allowance <= 1e-9 || self.sweep.abs() <= 1e-9 {
            return None;
        }
        let span = self.ends.1 - self.ends.0;
        let total = span.norm();
        if total <= 1e-9 {
            return None;
        }
        let es = span / total;
        // Past the bend's ends and sides a little, so no skin is left there.
        let (ds, du) = (1e-3 * thickness.max(0.01), 1e-4 / self.sweep.abs() * allowance);
        let out = |q: P2| {
            let s = if q.x <= 1e-9 { q.x - ds } else if q.x >= total - 1e-9 { q.x + ds } else { q.x };
            let u = if q.y <= 1e-9 { q.y - du } else if q.y >= allowance - 1e-9 { q.y + du } else { q.y };
            (s, u / allowance * self.sweep)
        };
        // The outline, each edge in steps of at most 2°.
        let n = cut.outer.len();
        let mut lp: Vec<(f64, f64)> = Vec::new();
        for i in 0..n {
            let (a, b) = (out(cut.outer[i]), out(cut.outer[(i + 1) % n]));
            let k = ((b.1 - a.1).abs() / 2f64.to_radians()).ceil().max(1.0) as usize;
            for j in 0..k {
                let t = j as f64 / k as f64;
                lp.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
        let (lo, hi) = lp.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.1), h.max(p.1)));
        if hi - lo > 170f64.to_radians() {
            return None;
        }
        // Just inside the inner surface (an inner cap's chords dip towards the axis, away from the
        // material); the sides run through the sheet to just outside it, then on out to the outer
        // cap, far enough that its chords stay clear of the material.
        let gap = 0.05 * thickness.max(0.01);
        let rin = (self.inner_radius - gap).max(0.5 * self.inner_radius).max(1e-4);
        let rmid = self.outer_radius + gap;
        let rout = (self.outer_radius + 0.5 * thickness + 0.1) / ((hi - lo) / 2.0).cos();
        let rm = (self.inner_radius + self.outer_radius) / 2.0;
        let flat: Vec<f64> = lp.iter().flat_map(|(s, a)| [*s, *a * rm]).collect();
        let idx = earcutr::earcut(&flat, &[], 2).ok()?;
        if idx.len() < 3 {
            return None;
        }
        let at = |(s, a): (f64, f64), r: f64| self.ends.0 + es * s + self.rotate_vec(self.start, a) * r;
        // Every triangle wound to face out of the tool (its faces then come out of the boolean
        // facing the right way, not dark): the caps away from the bend's surfaces, the sides
        // away from the outline's inside.
        let ccw = {
            let m = lp.len();
            (0..m).map(|i| lp[i].0 * lp[(i + 1) % m].1 - lp[(i + 1) % m].0 * lp[i].1).sum::<f64>() > 0.0
        };
        let wound = |t: [P3; 3], out: V3| {
            let n = (t[1] - t[0]).cross(&(t[2] - t[0]));
            if n.dot(&out) >= 0.0 { t } else { [t[0], t[2], t[1]] }
        };
        let radial = |(_, a): (f64, f64)| self.rotate_vec(self.start, a);
        let mut tris: Vec<[P3; 3]> = Vec::with_capacity(idx.len() * 2 / 3 + lp.len() * 2);
        for t in idx.chunks(3) {
            let (a, b, c) = (lp[t[0]], lp[t[1]], lp[t[2]]);
            let mid = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
            tris.push(wound([at(a, rin), at(b, rin), at(c, rin)], -radial(mid)));
            tris.push(wound([at(a, rout), at(b, rout), at(c, rout)], radial(mid)));
        }
        let m = lp.len();
        for i in 0..m {
            let (a, b) = (lp[i], lp[(i + 1) % m]);
            // Away from the inside: right of the edge for a counter-clockwise outline in (s, θ).
            let (ds, da) = (b.0 - a.0, (b.1 - a.1) * rm);
            let (os, oa) = if ccw { (da, -ds) } else { (-da, ds) };
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let tangent = self.axis.cross(&radial(mid));
            let out = es * os + tangent * oa;
            // Through the sheet in eight rings: the true side (radial lines at the outline) is
            // twisted, so each ring's two triangles stay nearly flat to each other.
            const RINGS: usize = 8;
            let ring = |k: usize| if k < RINGS { rin + (rmid - rin) * k as f64 / RINGS as f64 } else if k == RINGS { rmid } else { rout };
            for (r0, r1) in (0..=RINGS).map(|k| (ring(k), ring(k + 1))) {
                tris.push(wound([at(a, r0), at(b, r0), at(b, r1)], out));
                tris.push(wound([at(a, r0), at(b, r1), at(a, r1)], out));
            }
        }
        Some(tris)
    }
}

/// A joint that doesn't agree with its walls in 3D ([`Model::validate`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelError {
    pub joint: JointId,
    pub kind: ModelErrorKind,
}

/// What is wrong with a joint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelErrorKind {
    MissingWall,
    /// The bend's two tangent lines differ in length.
    LengthMismatch,
    /// The bend's first wall isn't planar.
    NotPlanar,
    /// The joint's segment doesn't run along its wall's edge (`wall` is the joint's `a` or `b`).
    NotOnEdge { second: bool },
    /// The second wall's tangent line isn't where the bend puts it.
    Misplaced,
    /// The second wall's material side doesn't follow the bend.
    MaterialSide,
    /// The second wall doesn't run on from the bend.
    NotRunningOn,
    /// A tangent joint's edges don't meet.
    TangentApart,
    /// A tangent joint's walls don't meet smoothly.
    NotSmooth,
}

impl ModelError {
    pub fn message(&self) -> &'static str {
        match self.kind {
            ModelErrorKind::MissingWall => "A wall of this joint doesn't exist",
            ModelErrorKind::LengthMismatch => "The bend's tangent lines differ in length",
            ModelErrorKind::NotPlanar => "The bend's first wall isn't flat",
            ModelErrorKind::NotOnEdge { .. } => "The joint doesn't run along its wall's edge",
            ModelErrorKind::Misplaced => "The second wall isn't where the bend puts it",
            ModelErrorKind::MaterialSide => "The second wall's material side doesn't follow the bend",
            ModelErrorKind::NotRunningOn => "The second wall doesn't run on from the bend",
            ModelErrorKind::TangentApart => "The tangent joint's edges don't meet",
            ModelErrorKind::NotSmooth => "The walls don't meet smoothly at the tangent joint",
        }
    }
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ModelError {}

/// One Sheet metal model's definition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub params: Params,
    pub walls: Vec<Wall>,
    /// In table (bend) order: Move up / Move down reorder this list (SM13.4).
    pub joints: Vec<Joint>,
    /// The wall that stays put when flattening (the first wall of each part otherwise).
    pub fixed: Option<WallId>,
    #[serde(default)]
    pub corner_overrides: Vec<CornerOverride>,
    #[serde(default)]
    pub bend_relief_overrides: Vec<BendReliefOverride>,
    /// P3I.6: material removed in the flat (SM14, [`crate::flat_edit`]).
    #[serde(default)]
    pub flat_cuts: Vec<crate::flat_edit::FlatCut>,
}

impl Model {
    pub fn wall(&self, id: WallId) -> Option<&Wall> {
        self.walls.iter().find(|w| w.id == id)
    }

    pub fn joint(&self, id: JointId) -> Option<&Joint> {
        self.joints.iter().find(|j| j.id == id)
    }

    pub fn joint_mut(&mut self, id: JointId) -> Option<&mut Joint> {
        self.joints.iter_mut().find(|j| j.id == id)
    }

    /// Move a joint up (`by < 0`) or down (`by > 0`) past the previous or next joint of the same
    /// table (bends among bends, rips and tangent joints among themselves; SM13.4). Returns
    /// whether it moved.
    pub fn move_joint(&mut self, id: JointId, by: isize) -> bool {
        let Some(i) = self.joints.iter().position(|j| j.id == id) else {
            return false;
        };
        let is_bend = self.joints[i].bend().is_some();
        let same = |j: &Joint| j.bend().is_some() == is_bend;
        let to = if by < 0 {
            self.joints[..i].iter().rposition(same)
        } else if by > 0 {
            self.joints[i + 1..].iter().position(same).map(|k| i + 1 + k)
        } else {
            None
        };
        let Some(to) = to else { return false };
        let j = self.joints.remove(i);
        self.joints.insert(to, j);
        true
    }

    /// A bend's 3D geometry: its axis, radii and sweep (`None` for a joint that isn't a bend or
    /// whose first wall isn't planar).
    pub fn bend_geometry(&self, id: JointId) -> Option<BendGeom> {
        let j = self.joint(id)?;
        let b = j.bend()?;
        let wa = self.wall(j.a)?;
        if !matches!(wa.surface, Surface::Planar { .. }) {
            return None;
        }
        let t = self.params.thickness;
        let n = wa.surface.normal_at(b.on_a.a);
        let into_a = crate::poly::inward_normal(&wa.outline, b.on_a)?;
        let c = -wa.surface.direction_at(b.on_a.a, into_a); // a's continuation past the bend
        let (side, def_radius) = if b.toward_material { (n, b.radius + t) } else { (-n, b.radius) };
        let shift = side * if b.toward_material { b.radius + t } else { b.radius };
        let (pa, pb) = (wa.surface.point(b.on_a.a), wa.surface.point(b.on_a.b));
        Some(BendGeom {
            ends: (pa + shift, pb + shift),
            axis: c.cross(&side).normalize(),
            inner_radius: b.radius,
            outer_radius: b.radius + t,
            def_radius,
            start: -side,
            sweep: b.angle,
        })
    }

    /// Checks that the walls and joints agree in 3D: each joint's segments run along their
    /// walls' edges over their whole length, each bend's second wall is where its first wall's
    /// tangent line lands when turned about the bend axis by the bend angle, tangent joints meet
    /// smoothly, and every joint's walls exist. Empty when consistent.
    pub fn validate(&self) -> Vec<ModelError> {
        let mut out = Vec::new();
        let size = self
            .walls
            .iter()
            .filter_map(|w| w.outline.bounds())
            .map(|(lo, hi)| (hi - lo).norm())
            .fold(1.0, f64::max);
        let tol = 1e-6 * size;
        let t = self.params.thickness;
        for j in &self.joints {
            // How far this joint may run past its wall near a corner: the setback of another
            // (non-hem) bend on the same walls, which took the wall away there, plus a rip's trim.
            let reach = self
                .joints
                .iter()
                .filter(|k| k.id != j.id && [k.a, k.b].iter().any(|w| *w == j.a || *w == j.b))
                .filter_map(|k| k.bend().filter(|b| !b.hem && b.angle < std::f64::consts::PI - 1e-9))
                .map(|b| (b.radius + t) * (b.angle / 2.0).tan())
                .fold(0.0, f64::max)
                + t
                + self.params.minimal_gap
                + tol;
            let err = |kind| ModelError { joint: j.id, kind };
            let (Some(wa), Some(wb)) = (self.wall(j.a), self.wall(j.b)) else {
                out.push(err(ModelErrorKind::MissingWall));
                continue;
            };
            if !matches!(j.kind, JointKind::Rip { .. }) {
                let (sa, sb) = (j.segment_on(j.a).expect("a"), j.segment_on(j.b).expect("b"));
                if !along_edge(&wa.outline, sa, size, reach) {
                    out.push(err(ModelErrorKind::NotOnEdge { second: false }));
                    continue;
                }
                if !along_edge(&wb.outline, sb, size, reach) {
                    out.push(err(ModelErrorKind::NotOnEdge { second: true }));
                    continue;
                }
            }
            match &j.kind {
                JointKind::Bend(b) => {
                    if (b.on_a.len() - b.on_b.len()).abs() > tol {
                        out.push(err(ModelErrorKind::LengthMismatch));
                        continue;
                    }
                    let Some(g) = self.bend_geometry(j.id) else {
                        out.push(err(ModelErrorKind::NotPlanar));
                        continue;
                    };
                    let turn = |p: P3| g.rotate(p, g.sweep);
                    let (pa, pb) = (wa.surface.point(b.on_a.a), wa.surface.point(b.on_a.b));
                    let (qa, qb) = (wb.surface.point(b.on_b.a), wb.surface.point(b.on_b.b));
                    if (turn(pa) - qa).norm() > tol || (turn(pb) - qb).norm() > tol {
                        out.push(err(ModelErrorKind::Misplaced));
                        continue;
                    }
                    let na = wa.surface.normal_at(b.on_a.a);
                    let nb = wb.surface.normal_at(b.on_b.a);
                    if (g.rotate_vec(na, g.sweep) - nb).norm() > 1e-6 {
                        out.push(err(ModelErrorKind::MaterialSide));
                        continue;
                    }
                    let ia = crate::poly::inward_normal(&wa.outline, b.on_a).expect("checked above");
                    let ib = crate::poly::inward_normal(&wb.outline, b.on_b).expect("checked above");
                    let c = -wa.surface.direction_at(b.on_a.a, ia);
                    if (g.rotate_vec(c, g.sweep) - wb.surface.direction_at(b.on_b.a, ib)).norm() > 1e-6 {
                        out.push(err(ModelErrorKind::NotRunningOn));
                    }
                }
                JointKind::Tangent { on_a, on_b } => {
                    let close = |p: P2, q: P2| (wa.surface.point(p) - wb.surface.point(q)).norm() <= tol;
                    if !close(on_a.a, on_b.a) || !close(on_a.b, on_b.b) {
                        out.push(err(ModelErrorKind::TangentApart));
                    } else if (wa.surface.normal_at(on_a.a) - wb.surface.normal_at(on_b.a)).norm() > 1e-6 {
                        out.push(err(ModelErrorKind::NotSmooth));
                    }
                }
                JointKind::Rip { .. } => {}
            }
        }
        out
    }

    pub fn corner_relief(&self, a: JointId, b: JointId) -> CornerRelief {
        self.corner_overrides
            .iter()
            .rev()
            .find(|o| o.bends == (a, b) || o.bends == (b, a))
            .map(|o| o.relief)
            .unwrap_or(self.params.corner_relief)
    }

    pub fn bend_relief(&self, bend: JointId, end: BendEnd) -> BendRelief {
        self.bend_relief_overrides
            .iter()
            .rev()
            .find(|o| o.bend == bend && o.end == end)
            .map(|o| o.relief)
            .unwrap_or(self.params.bend_relief)
    }
}

/// Table names: "A", "B", …, "Z", "AA", "AB", … for the `i`-th (from 0).
pub fn letters(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).expect("ascii")
}

// ---------------------------------------------------------------------------------------------
// Building from virtual sharps

/// A planar wall as features describe it: its outline reaching the virtual sharps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharpWall {
    pub origin: P3,
    pub u: V3,
    pub v: V3,
    pub outline: Polygon,
    /// A persistent id from the feature that made the wall (`None`: numbered automatically).
    pub id: Option<WallId>,
}

/// A joint along the virtual sharp `edge` (a 3D segment on both walls' outlines).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharpJoint {
    pub a: usize,
    pub b: usize,
    pub edge: (P3, P3),
    pub kind: SharpJointKind,
    /// A persistent id and table name from the feature that made the joint (`None`: numbered
    /// and named automatically, "Bend A", "Rip 1", …).
    pub id: Option<JointId>,
    pub name: Option<String>,
    /// Creation order among joints and hems (the table order).
    pub seq: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SharpJointKind {
    /// `radius: None` uses the model's; `value: None` the model's K factor/allowance/deduction.
    Bend { radius: Option<f64>, value: Option<BendValue> },
    Rip { style: RipStyle },
}

/// Why a sharp definition couldn't be built. `joint` and `wall` index the builder's lists (hems
/// count after the joints).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BuildError {
    /// The joint's edge isn't on the edge of one of its walls.
    EdgeNotOnWall { joint: usize, wall: usize },
    /// The two walls of a joint are coplanar (no bend) or fold flat onto each other.
    NoAngle { joint: usize },
    /// The walls disagree on which side the material is (one wall's normal is flipped).
    InconsistentSide { joint: usize },
    /// A butt joint between walls that aren't at 90°.
    ButtNot90 { joint: usize },
    /// The bend doesn't fit: its setback trims a wall away entirely.
    WallTrimmedAway { joint: usize, wall: usize },
    /// A bend of 180° or more: build hems with [`SharpBuilder::hem`] instead.
    TooSharp { joint: usize },
    /// A hem with no length, or a hem edge that isn't on its wall.
    BadHem { hem: usize },
    /// Two walls or two joints were given the same id.
    DuplicateWallId { id: WallId },
    DuplicateJointId { id: JointId },
}

impl BuildError {
    /// The feature error text.
    pub fn message(&self) -> String {
        match self {
            BuildError::EdgeNotOnWall { .. } => "The joint's edge isn't on the edge of its wall".into(),
            BuildError::NoAngle { .. } => "The walls are in line: there is nothing to bend".into(),
            BuildError::InconsistentSide { .. } => "The walls' material sides don't match across the joint".into(),
            BuildError::ButtNot90 { .. } => "Only 90° joints can be butt joints".into(),
            BuildError::WallTrimmedAway { .. } => "The bend is too large for its wall".into(),
            BuildError::TooSharp { .. } => "Bends of 180° or more must be hems".into(),
            BuildError::BadHem { .. } => "The hem has no length or isn't on an edge of its wall".into(),
            BuildError::DuplicateWallId { id } => format!("Two walls have the id {}", id.0),
            BuildError::DuplicateJointId { id } => format!("Two joints have the id {}", id.0),
        }
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for BuildError {}

/// Where a hem's bend goes relative to the edge it is made on (SM4.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HemAlignment {
    /// The hem's outside lies on the original edge (the wall gets shorter).
    #[default]
    Outer,
    /// The hem's bend starts at the original edge.
    InPlace,
}

/// A hem folded back from a wall's edge: 180° (a straight hem), or further (a rolled or tear
/// drop hem, P3I.4: `angle`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharpHem {
    pub wall: usize,
    pub edge: (P3, P3),
    /// Length of the folded-back part, from its tangent line to its end.
    pub length: f64,
    /// Inner radius; `None` uses the model's.
    pub radius: Option<f64>,
    /// Fold over the material side (onto the wall) rather than the other side.
    pub toward_material: bool,
    pub alignment: HemAlignment,
    /// Persistent ids of the hem's bend and of its folded-back wall, and the bend's name.
    pub id: Option<JointId>,
    pub wall_id: Option<WallId>,
    pub name: Option<String>,
    /// Creation order among joints and hems (the table order).
    pub seq: usize,
    /// The hem's bend angle (radians): π for a straight hem, more for rolled and tear drop hems
    /// (P3I.4, SM4.2; less than a full turn).
    #[serde(default = "half_turn")]
    pub angle: f64,
    /// Where hems meet at a corner (P3I.4, SM4.4): the folded-back wall's end there is first
    /// carried on by `extend`, then cut back to the side of the plane through `point` that
    /// `normal` points to.
    #[serde(default)]
    pub clips: Vec<HemClip>,
}

/// A cut across the end of a hem's folded-back wall (see [`SharpHem::clips`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HemClip {
    pub point: P3,
    pub normal: V3,
    pub extend: f64,
}

fn half_turn() -> f64 {
    std::f64::consts::PI
}

/// The builder's hem length (from the hem's tangent line to its end) for Onshape's **Total
/// length** of a straight hem, which is measured from the hem's outermost edge: the bend sticks
/// out `radius + thickness` beyond its tangent line.
pub fn hem_length_from_total(total: f64, radius: f64, thickness: f64) -> f64 {
    total - (radius + thickness)
}

/// Builds a [`Model`] from walls meeting at virtual sharps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SharpBuilder {
    pub params: Params,
    pub walls: Vec<SharpWall>,
    pub joints: Vec<SharpJoint>,
    pub hems: Vec<SharpHem>,
    /// The next creation-order number.
    pub seq: usize,
}

struct Side {
    /// The edge in the wall's local 2D.
    edge: Seg2,
    /// Unit normal of the edge pointing into the wall, local 2D.
    into: V2,
    /// The same direction in 3D.
    into3: V3,
}

impl SharpBuilder {
    pub fn new(params: Params) -> SharpBuilder {
        SharpBuilder {
            params,
            ..Default::default()
        }
    }

    pub fn wall(&mut self, origin: P3, u: V3, v: V3, outline: Polygon) -> usize {
        self.walls.push(SharpWall {
            origin,
            u,
            v,
            outline,
            id: None,
        });
        self.walls.len() - 1
    }

    /// Gives wall `i` a persistent id (features key their walls so ids survive edits).
    pub fn set_wall_id(&mut self, i: usize, id: WallId) {
        self.walls[i].id = Some(id);
    }

    /// Gives joint `i` a persistent id and, optionally, its table name.
    pub fn set_joint_id(&mut self, i: usize, id: JointId, name: Option<String>) {
        self.joints[i].id = Some(id);
        self.joints[i].name = name;
    }

    /// Gives hem `i` persistent ids for its bend and its wall and, optionally, the bend's name.
    pub fn set_hem_id(&mut self, i: usize, id: JointId, wall_id: WallId, name: Option<String>) {
        let h = &mut self.hems[i];
        h.id = Some(id);
        h.wall_id = Some(wall_id);
        h.name = name;
    }

    pub fn bend(&mut self, a: usize, b: usize, edge: (P3, P3)) -> usize {
        self.joint(a, b, edge, SharpJointKind::Bend { radius: None, value: None })
    }

    pub fn rip(&mut self, a: usize, b: usize, edge: (P3, P3), style: RipStyle) -> usize {
        self.joint(a, b, edge, SharpJointKind::Rip { style })
    }

    pub fn joint(&mut self, a: usize, b: usize, edge: (P3, P3), kind: SharpJointKind) -> usize {
        self.joints.push(SharpJoint {
            a,
            b,
            edge,
            kind,
            id: None,
            name: None,
            seq: self.seq,
        });
        self.seq += 1;
        self.joints.len() - 1
    }

    /// A hem on `wall`'s edge `edge` (SM4): the edge folded back 180°, `length` long.
    pub fn hem(&mut self, wall: usize, edge: (P3, P3), length: f64, toward_material: bool, alignment: HemAlignment) -> usize {
        self.hems.push(SharpHem {
            wall,
            edge,
            length,
            radius: None,
            toward_material,
            alignment,
            id: None,
            wall_id: None,
            name: None,
            seq: self.seq,
            angle: std::f64::consts::PI,
            clips: Vec::new(),
        });
        self.seq += 1;
        self.hems.len() - 1
    }

    fn side(&self, joint: usize, wall: usize) -> Result<Side, BuildError> {
        self.side_of(self.joints[joint].edge, wall).ok_or(BuildError::EdgeNotOnWall { joint, wall })
    }

    fn side_of(&self, edge3: (P3, P3), wall: usize) -> Option<Side> {
        let w = &self.walls[wall];
        let s = Surface::Planar {
            origin: w.origin,
            u: w.u,
            v: w.v,
        };
        let (p0, p1) = edge3;
        let edge = Seg2::new(s.local(p0), s.local(p1));
        let size = w.outline.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0).max(1e-9);
        let tol = 1e-6 * size;
        // Both ends must lie in the wall's plane and on its outline's boundary.
        let n = s.normal().expect("planar");
        if ((p0 - w.origin).dot(&n)).abs() > tol || ((p1 - w.origin).dot(&n)).abs() > tol || edge.len() < tol {
            return None;
        }
        let mid = P2::from((edge.a.coords + edge.b.coords) / 2.0);
        let nrm = perp(edge.dir());
        let step = 1e-4 * size;
        let into = if w.outline.contains(mid + nrm * step) && !w.outline.contains(mid - nrm * step) {
            nrm
        } else if w.outline.contains(mid - nrm * step) && !w.outline.contains(mid + nrm * step) {
            -nrm
        } else {
            return None;
        };
        let into3 = (w.u * into.x + w.v * into.y).normalize();
        Some(Side { edge, into, into3 })
    }

    /// The walls trimmed and the joints resolved. Rips are applied first, then bends in order.
    pub fn build(&self) -> Result<Model, BuildError> {
        let p = self.params;
        let t = p.thickness;
        let mut outlines: Vec<Polygon> = self.walls.iter().map(|w| w.outline.clone()).collect();
        let mut joints: Vec<Option<JointKind>> = vec![None; self.joints.len()];

        // Per joint: its sides, angle and direction.
        struct Geo {
            sa: Side,
            sb: Side,
            theta: f64,
            toward: bool,
        }
        let mut geos = Vec::with_capacity(self.joints.len());
        for (ji, j) in self.joints.iter().enumerate() {
            let sa = self.side(ji, j.a)?;
            let sb = self.side(ji, j.b)?;
            let na = self.walls[j.a].u.cross(&self.walls[j.a].v).normalize();
            let nb = self.walls[j.b].u.cross(&self.walls[j.b].v).normalize();
            let cos_alpha = sa.into3.dot(&sb.into3).clamp(-1.0, 1.0);
            let theta = std::f64::consts::PI - cos_alpha.acos();
            let lift = sb.into3.dot(&na);
            if theta < 1e-6 || theta > std::f64::consts::PI - 1e-6 || lift.abs() < 1e-9 {
                return Err(BuildError::NoAngle { joint: ji });
            }
            let toward = lift > 0.0;
            // The definition surface must run on round the bend: b's material normal is a's
            // turned with it.
            let expected = if toward {
                na * theta.cos() + sa.into3 * theta.sin()
            } else {
                na * theta.cos() - sa.into3 * theta.sin()
            };
            if expected.dot(&nb) < 0.5 {
                return Err(BuildError::InconsistentSide { joint: ji });
            }
            geos.push(Geo { sa, sb, theta, toward });
        }

        // Rips: trim (or extend) each wall at its edge.
        for (ji, j) in self.joints.iter().enumerate() {
            let SharpJointKind::Rip { style } = j.kind else { continue };
            let g = &geos[ji];
            let alpha = std::f64::consts::PI - g.theta; // interior angle between the walls
            let gap = p.minimal_gap;
            // Where the inner faces meet, measured along each wall from the sharp: 0 when the
            // definition surface is the inside, T·cot(α/2) when it is the outside.
            let inner = if g.toward { t / (alpha / 2.0).tan() } else { 0.0 };
            let ninety = (g.theta - std::f64::consts::FRAC_PI_2).abs() < 1e-6;
            let (ta, tb) = match style {
                RipStyle::EdgeJoint => (inner + gap / 2.0, inner + gap / 2.0),
                RipStyle::ButtDirection1 | RipStyle::ButtDirection2 if !ninety => return Err(BuildError::ButtNot90 { joint: ji }),
                // Direction 1: wall a stops short of b's face; b runs on over a's end.
                RipStyle::ButtDirection1 => {
                    if g.toward {
                        (t + gap, 0.0)
                    } else {
                        (gap, -t)
                    }
                }
                RipStyle::ButtDirection2 => {
                    if g.toward {
                        (0.0, t + gap)
                    } else {
                        (-t, gap)
                    }
                }
            };
            outlines[j.a] = trim_band(&outlines[j.a], &g.sa, ta).ok_or(BuildError::WallTrimmedAway { joint: ji, wall: j.a })?;
            outlines[j.b] = trim_band(&outlines[j.b], &g.sb, tb).ok_or(BuildError::WallTrimmedAway { joint: ji, wall: j.b })?;
            joints[ji] = Some(JointKind::Rip {
                on_a: g.sa.edge.offset(g.sa.into * ta),
                on_b: g.sb.edge.offset(g.sb.into * tb),
                style,
            });
        }

        // Bends: the extent where both walls reach the sharp (after the rips, before any bend
        // trims, so it doesn't depend on the order of the bends), then trim by the setback.
        let after_rips = outlines.clone();
        for (ji, j) in self.joints.iter().enumerate() {
            let SharpJointKind::Bend { radius, value } = j.kind else { continue };
            let g = &geos[ji];
            let r = radius.unwrap_or(p.bend_radius);
            let setback = if g.toward { bend::outside_setback(r, t, g.theta) } else { bend::inside_setback(r, g.theta) };
            let sb = setback.ok_or(BuildError::TooSharp { joint: ji })?;
            let ia = on_line_interval(&after_rips[j.a], &g.sa.edge);
            let ib = on_line_interval(&after_rips[j.b], &g.sb.edge);
            let (Some((a0, a1)), Some((b0, b1))) = (ia, ib) else {
                return Err(BuildError::EdgeNotOnWall { joint: ji, wall: j.a });
            };
            // Both walls' edges are parametrised the same way (same 3D end points), so the
            // intervals compare directly.
            let (lo, hi) = (a0.max(b0), a1.min(b1));
            if hi - lo < 1e-9 {
                return Err(BuildError::EdgeNotOnWall { joint: ji, wall: j.b });
            }
            let sub = |s: &Seg2| Seg2::new(s.a + (s.b - s.a) * lo, s.a + (s.b - s.a) * hi);
            let (ea, eb) = (sub(&g.sa.edge), sub(&g.sb.edge));
            let side_a = Side {
                edge: ea,
                into: g.sa.into,
                into3: g.sa.into3,
            };
            let side_b = Side {
                edge: eb,
                into: g.sb.into,
                into3: g.sb.into3,
            };
            outlines[j.a] = trim_band(&outlines[j.a], &side_a, sb).ok_or(BuildError::WallTrimmedAway { joint: ji, wall: j.a })?;
            outlines[j.b] = trim_band(&outlines[j.b], &side_b, sb).ok_or(BuildError::WallTrimmedAway { joint: ji, wall: j.b })?;
            joints[ji] = Some(JointKind::Bend(Bend {
                on_a: ea.offset(g.sa.into * sb),
                on_b: eb.offset(g.sb.into * sb),
                angle: g.theta,
                toward_material: g.toward,
                radius: r,
                model_radius: radius.is_none(),
                value,
                hem: false,
            }));
        }

        // Hems: a new wall folded back from the (trimmed) edge. First every hem's trim (Outer:
        // the hem's outside, r + t beyond its tangent line, lands on the edge), so hems meeting
        // at a corner each stop where the other's trim begins; then each hem runs along what is
        // left of its tangent line.
        let mut tangents: Vec<Seg2> = Vec::with_capacity(self.hems.len());
        for (hi, h) in self.hems.iter().enumerate() {
            let bad = BuildError::BadHem { hem: hi };
            let side = self.side_of(h.edge, h.wall).ok_or(bad.clone())?;
            let r = h.radius.unwrap_or(p.bend_radius);
            let (lo, hi_) = on_line_interval(&outlines[h.wall], &side.edge).ok_or(bad.clone())?;
            let sub = Seg2::new(side.edge.a + (side.edge.b - side.edge.a) * lo, side.edge.a + (side.edge.b - side.edge.a) * hi_);
            let trim = match h.alignment {
                HemAlignment::Outer => r + t,
                HemAlignment::InPlace => 0.0,
            };
            let ts = Side {
                edge: sub,
                into: side.into,
                into3: side.into3,
            };
            outlines[h.wall] = trim_band(&outlines[h.wall], &ts, trim).ok_or(BuildError::WallTrimmedAway { joint: self.joints.len() + hi, wall: h.wall })?;
            tangents.push(sub.offset(side.into * trim));
        }
        let mut hem_walls: Vec<(Surface, Polygon)> = Vec::new();
        let mut hem_joints: Vec<(usize, usize, Bend)> = Vec::new();
        for (hi, h) in self.hems.iter().enumerate() {
            let bad = BuildError::BadHem { hem: hi };
            let side = self.side_of(h.edge, h.wall).ok_or(bad.clone())?;
            if h.length.is_nan() || h.length <= 0.0 {
                return Err(bad);
            }
            let w = &self.walls[h.wall];
            let n = w.u.cross(&w.v).normalize();
            let r = h.radius.unwrap_or(p.bend_radius);
            let line = tangents[hi];
            let (lo, hi_) = on_line_interval(&outlines[h.wall], &line).ok_or(bad.clone())?;
            let on_a = Seg2::new(line.a + (line.b - line.a) * lo, line.a + (line.b - line.a) * hi_);
            if on_a.len() < 1e-9 {
                return Err(bad);
            }
            let surf_a = Surface::Planar {
                origin: w.origin,
                u: w.u,
                v: w.v,
            };
            if !(h.angle >= std::f64::consts::PI - 1e-9 && h.angle < std::f64::consts::TAU - 1e-6) {
                return Err(bad);
            }
            // The definition surface turns about an axis on the material side (radius r + t) or
            // the other side (radius r), by the hem's angle (as `Model::bend_geometry` turns it).
            let side_n = if h.toward_material { n } else { -n };
            let rdef = if h.toward_material { r + t } else { r };
            let (pa, pb) = (surf_a.point(on_a.a), surf_a.point(on_a.b));
            let axis_pt = pa + side_n * rdef;
            let c = -side.into3; // the wall's continuation past the edge
            let k = c.cross(&side_n).normalize();
            let rot = |v: V3| v * h.angle.cos() + k.cross(&v) * h.angle.sin() + k * k.dot(&v) * (1.0 - h.angle.cos());
            let (qa, qb) = (axis_pt + rot(pa - axis_pt), axis_pt + rot(pb - axis_pt));
            let nb = rot(n);
            let u = rot(c); // the hem runs on from its bend
            let v = nb.cross(&u);
            let surf_b = Surface::Planar { origin: qa, u, v };
            let on_b = Seg2::new(surf_b.local(qa), surf_b.local(qb));
            let (y0, y1) = (on_b.a.y.min(on_b.b.y), on_b.a.y.max(on_b.b.y));
            // Ends carried on where hems meet, then cut back.
            let (mut e0, mut e1) = (0.0, 0.0);
            for cl in &h.clips {
                let y = surf_b.local(cl.point).y;
                if (y - y0).abs() <= (y - y1).abs() {
                    e0 = f64::max(e0, cl.extend);
                } else {
                    e1 = f64::max(e1, cl.extend);
                }
            }
            let mut outline = Polygon::rect(P2::new(0.0, y0 - e0), P2::new(h.length, y1 + e1));
            for cl in &h.clips {
                // Where the cut's plane crosses the wall's plane, moved (for a cut leaning across
                // the wall) so the wall's whole thickness stays on its side.
                let m2 = V2::new(cl.normal.dot(&u), cl.normal.dot(&v));
                if m2.norm() < 1e-12 {
                    continue;
                }
                let lift = (-t * nb.dot(&cl.normal)).max(0.0);
                let level = (cl.point + cl.normal * lift - qa).dot(&cl.normal);
                outline = outline.clip_half_plane(P2::from(m2 * (level / m2.norm_squared())), m2);
            }
            if outline.is_empty() || outline.area() < 1e-12 {
                return Err(bad);
            }
            hem_joints.push((
                h.wall,
                self.walls.len() + hem_walls.len(),
                Bend {
                    on_a,
                    on_b,
                    angle: h.angle,
                    toward_material: h.toward_material,
                    radius: r,
                    model_radius: h.radius.is_none(),
                    value: None,
                    hem: true,
                },
            ));
            hem_walls.push((surf_b, outline));
        }

        // Walls: given ids, the rest numbered from the first free id.
        let given_walls: Vec<WallId> = self.walls.iter().filter_map(|w| w.id).chain(self.hems.iter().filter_map(|h| h.wall_id)).collect();
        if let Some(d) = first_duplicate(&given_walls) {
            return Err(BuildError::DuplicateWallId { id: d });
        }
        let mut free_wall = (0u32..).map(WallId).filter(|w| !given_walls.contains(w));
        let mut walls: Vec<Wall> = Vec::new();
        for (w, outline) in self.walls.iter().zip(outlines) {
            walls.push(Wall {
                id: w.id.unwrap_or_else(|| free_wall.next().expect("ids")),
                surface: Surface::Planar {
                    origin: w.origin,
                    u: w.u,
                    v: w.v,
                },
                outline,
            });
        }
        let hem_wall_ids: Vec<WallId> = self.hems.iter().map(|h| h.wall_id.unwrap_or_else(|| free_wall.next().expect("ids"))).collect();
        for ((surface, outline), id) in hem_walls.into_iter().zip(&hem_wall_ids) {
            walls.push(Wall { id: *id, surface, outline });
        }
        let wall_id = |i: usize| walls[i].id;

        // Joints and hems in creation order, with given ids and names or automatic ones.
        struct Entry {
            seq: usize,
            a: WallId,
            b: WallId,
            kind: JointKind,
            id: Option<JointId>,
            name: Option<String>,
        }
        let mut entries: Vec<Entry> = self
            .joints
            .iter()
            .zip(joints)
            .map(|(j, kind)| Entry {
                seq: j.seq,
                a: wall_id(j.a),
                b: wall_id(j.b),
                kind: kind.expect("every joint resolved"),
                id: j.id,
                name: j.name.clone(),
            })
            .collect();
        for ((h, (a, _, bend)), bw) in self.hems.iter().zip(hem_joints).zip(&hem_wall_ids) {
            entries.push(Entry {
                seq: h.seq,
                a: wall_id(a),
                b: *bw,
                kind: JointKind::Bend(bend),
                id: h.id,
                name: h.name.clone(),
            });
        }
        entries.sort_by_key(|e| e.seq);
        let given: Vec<JointId> = entries.iter().filter_map(|e| e.id).collect();
        if let Some(d) = first_duplicate(&given) {
            return Err(BuildError::DuplicateJointId { id: d });
        }
        let mut free = (0u32..).map(JointId).filter(|j| !given.contains(j));
        let mut names = JointNamer {
            taken: entries.iter().filter_map(|e| e.name.clone()).collect(),
            ..Default::default()
        };
        let joints: Vec<Joint> = entries
            .into_iter()
            .map(|e| Joint {
                id: e.id.unwrap_or_else(|| free.next().expect("ids")),
                name: e.name.unwrap_or_else(|| names.name(&e.kind)),
                a: e.a,
                b: e.b,
                kind: e.kind,
            })
            .collect();
        Ok(Model {
            params: p,
            walls,
            joints,
            fixed: None,
            corner_overrides: Vec::new(),
            bend_relief_overrides: Vec::new(),
            flat_cuts: Vec::new(),
        })
    }
}

/// Whether segment `s` runs along the boundary of `outline`: somewhere along it, just off it on
/// one side is inside the wall and just off it on the other side outside; and none of it is more
/// than `reach` from the wall. (Near a corner a bend can run on past its wall's edge, where the
/// other bend at the corner took the wall away: the corner relief settles that, so the rest only
/// has to be close.)
fn along_edge(outline: &Polygon, s: Seg2, size: f64, reach: f64) -> bool {
    let Some(n) = crate::poly::inward_normal(outline, s) else { return false };
    let d = 1e-5 * size.max(s.len());
    let samples: Vec<P2> = (0..=20).map(|i| s.a + (s.b - s.a) * (i as f64 / 20.0)).collect();
    let on = samples.iter().any(|p| outline.contains(p + n * d) && !outline.contains(p - n * d));
    on && samples.iter().all(|p| distance_to(outline, *p) <= reach)
}

/// Distance from `p` to the polygon (0 inside).
fn distance_to(poly: &Polygon, p: P2) -> f64 {
    if poly.contains(p) {
        return 0.0;
    }
    std::iter::once(&poly.outer)
        .chain(poly.holes.iter())
        .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
        .map(|(a, b)| {
            let d = b - a;
            let t = ((p - a).dot(&d) / d.norm_squared().max(1e-300)).clamp(0.0, 1.0);
            (a + d * t - p).norm()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Moves the stretch `edge` of a wall's outline (`into`: its unit normal pointing into the wall)
/// `depth` into the wall, over the stretch's length only; a negative depth carries the wall on
/// past it. `None` if nothing is left. (P3I.4: features that move a wall's edge to a new
/// virtual sharp.)
pub fn shift_edge(outline: &Polygon, edge: Seg2, into: V2, depth: f64) -> Option<Polygon> {
    trim_band(outline, &Side { edge, into, into3: V3::zeros() }, depth)
}

/// Removes the band `[0, depth)` into the wall along the side's edge (or, for a negative depth,
/// adds a band of `−depth` outside it). `None` if nothing is left.
fn trim_band(outline: &Polygon, side: &Side, depth: f64) -> Option<Polygon> {
    if depth.abs() < 1e-12 {
        return Some(outline.clone());
    }
    let e = side.edge;
    let band = Polygon::new(vec![e.a, e.b, e.b + side.into * depth, e.a + side.into * depth]);
    let out = if depth > 0.0 {
        crate::poly::difference(std::slice::from_ref(outline), std::slice::from_ref(&band))
    } else {
        crate::poly::union(&[outline.clone(), band.clone()])
    };
    // Keep the biggest piece (a trim never splits a sane wall; slivers can appear), with its
    // vertices put back on the exact inputs the booleans rounded.
    let mut exact: Vec<P2> = outline.outer.iter().chain(outline.holes.iter().flatten()).copied().collect();
    exact.extend(band.outer.iter().copied());
    // Where the band's sides cross the outline's edges.
    let (n, d) = (side.into, e.b - e.a);
    let l = &outline.outer;
    for i in 0..l.len() {
        let (p, q) = (l[i], l[(i + 1) % l.len()]);
        for (o, dir) in [(e.a, side.into), (e.b, side.into), (e.a + n * depth, d), (e.a, d)] {
            let den = (q - p).perp(&dir);
            if den.abs() > 1e-15 {
                let t = (o - p).perp(&dir) / den;
                if (0.0..=1.0).contains(&t) {
                    exact.push(p + (q - p) * t);
                }
            }
        }
    }
    out.into_iter()
        .filter(|p| p.area() > 1e-12)
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .map(|p| crate::poly::snap_to(&p, &exact, 10.0 * crate::poly::GRID))
}

/// Table names in order of creation: "Bend A", "Bend B", …; "Rip 1", …; "Tangent 1", …,
/// skipping names already taken.
#[derive(Clone, Debug, Default)]
pub struct JointNamer {
    bends: usize,
    rips: usize,
    tangents: usize,
    pub taken: Vec<String>,
}

impl JointNamer {
    pub fn name(&mut self, kind: &JointKind) -> String {
        loop {
            let n = match kind {
                JointKind::Bend(_) => {
                    self.bends += 1;
                    format!("Bend {}", letters(self.bends - 1))
                }
                JointKind::Rip { .. } => {
                    self.rips += 1;
                    format!("Rip {}", self.rips)
                }
                JointKind::Tangent { .. } => {
                    self.tangents += 1;
                    format!("Tangent {}", self.tangents)
                }
            };
            if !self.taken.contains(&n) {
                self.taken.push(n.clone());
                return n;
            }
        }
    }
}

fn first_duplicate<T: PartialEq + Copy>(ids: &[T]) -> Option<T> {
    ids.iter().enumerate().find(|(i, a)| ids[..*i].contains(a)).map(|(_, a)| *a)
}

/// The parameter interval `[t0, t1]` (along `edge`, 0 at `edge.a`, 1 at `edge.b`) of the
/// outline's boundary lying on the edge's line.
pub(crate) fn on_line_interval(outline: &Polygon, edge: &Seg2) -> Option<(f64, f64)> {
    let d = edge.b - edge.a;
    let len2 = d.norm_squared();
    let n = perp(d.normalize());
    let tol = 1e-7 * len2.sqrt().max(1.0);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let l = &outline.outer;
    for i in 0..l.len() {
        let (p, q) = (l[i], l[(i + 1) % l.len()]);
        if (p - edge.a).dot(&n).abs() < tol && (q - edge.a).dot(&n).abs() < tol {
            for x in [p, q] {
                let s = (x - edge.a).dot(&d) / len2;
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
    }
    (lo <= hi).then(|| (lo.max(0.0), hi.min(1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_count_like_spreadsheet_columns() {
        assert_eq!(letters(0), "A");
        assert_eq!(letters(25), "Z");
        assert_eq!(letters(26), "AA");
        assert_eq!(letters(27), "AB");
        assert_eq!(letters(701), "ZZ");
        assert_eq!(letters(702), "AAA");
    }

    #[test]
    fn rolled_surface_round_trips() {
        let s = Surface::Rolled {
            axis_origin: P3::origin(),
            axis: V3::z(),
            start: V3::x(),
            radius: 10.0,
            material_outside: true,
        };
        let q = P2::new(7.5, 3.0);
        let p = s.point(q);
        assert!((p.coords.xy().norm() - 10.0).abs() < 1e-12);
        assert!((s.local(p) - q).norm() < 1e-9);
    }
}
