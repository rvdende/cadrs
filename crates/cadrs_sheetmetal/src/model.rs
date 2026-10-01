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

    /// The bend region's flat width (`None` when the value can't apply: a deduction on a bend of
    /// 180° or more).
    pub fn allowance(&self, p: &Params) -> Option<f64> {
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

    /// Move a joint up (`-1`) or down (`+1`) in the table (SM13.4). Returns whether it moved.
    pub fn move_joint(&mut self, id: JointId, by: isize) -> bool {
        let Some(i) = self.joints.iter().position(|j| j.id == id) else {
            return false;
        };
        let to = i as isize + by;
        if to < 0 || to as usize >= self.joints.len() {
            return false;
        }
        self.joints.swap(i, to as usize);
        true
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
#[derive(Clone, Debug, PartialEq)]
pub struct SharpWall {
    pub origin: P3,
    pub u: V3,
    pub v: V3,
    pub outline: Polygon,
}

/// A joint along the virtual sharp `edge` (a 3D segment on both walls' outlines).
#[derive(Clone, Debug, PartialEq)]
pub struct SharpJoint {
    pub a: usize,
    pub b: usize,
    pub edge: (P3, P3),
    pub kind: SharpJointKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SharpJointKind {
    /// `radius: None` uses the model's; `value: None` the model's K factor/allowance/deduction.
    Bend { radius: Option<f64>, value: Option<BendValue> },
    Rip { style: RipStyle },
}

/// Why a sharp definition couldn't be built.
#[derive(Clone, Debug, PartialEq)]
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
}

/// Builds a [`Model`] from walls meeting at virtual sharps.
#[derive(Clone, Debug, Default)]
pub struct SharpBuilder {
    pub params: Params,
    pub walls: Vec<SharpWall>,
    pub joints: Vec<SharpJoint>,
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
        self.walls.push(SharpWall { origin, u, v, outline });
        self.walls.len() - 1
    }

    pub fn bend(&mut self, a: usize, b: usize, edge: (P3, P3)) -> usize {
        self.joint(a, b, edge, SharpJointKind::Bend { radius: None, value: None })
    }

    pub fn rip(&mut self, a: usize, b: usize, edge: (P3, P3), style: RipStyle) -> usize {
        self.joint(a, b, edge, SharpJointKind::Rip { style })
    }

    pub fn joint(&mut self, a: usize, b: usize, edge: (P3, P3), kind: SharpJointKind) -> usize {
        self.joints.push(SharpJoint { a, b, edge, kind });
        self.joints.len() - 1
    }

    fn side(&self, joint: usize, wall: usize) -> Result<Side, BuildError> {
        let w = &self.walls[wall];
        let s = Surface::Planar {
            origin: w.origin,
            u: w.u,
            v: w.v,
        };
        let (p0, p1) = self.joints[joint].edge;
        let edge = Seg2::new(s.local(p0), s.local(p1));
        let size = w.outline.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0).max(1e-9);
        let tol = 1e-6 * size;
        // Both ends must lie in the wall's plane and on its outline's boundary.
        let n = s.normal().expect("planar");
        if ((p0 - w.origin).dot(&n)).abs() > tol || ((p1 - w.origin).dot(&n)).abs() > tol || edge.len() < tol {
            return Err(BuildError::EdgeNotOnWall { joint, wall });
        }
        let mid = P2::from((edge.a.coords + edge.b.coords) / 2.0);
        let nrm = perp(edge.dir());
        let step = 1e-4 * size;
        let into = if w.outline.contains(mid + nrm * step) && !w.outline.contains(mid - nrm * step) {
            nrm
        } else if w.outline.contains(mid - nrm * step) && !w.outline.contains(mid + nrm * step) {
            -nrm
        } else {
            return Err(BuildError::EdgeNotOnWall { joint, wall });
        };
        let into3 = (w.u * into.x + w.v * into.y).normalize();
        Ok(Side { edge, into, into3 })
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

        // Bends: the extent where both walls still reach the sharp, then trim by the setback.
        for (ji, j) in self.joints.iter().enumerate() {
            let SharpJointKind::Bend { radius, value } = j.kind else { continue };
            let g = &geos[ji];
            let r = radius.unwrap_or(p.bend_radius);
            let setback = if g.toward { bend::outside_setback(r, t, g.theta) } else { bend::inside_setback(r, g.theta) };
            let sb = setback.ok_or(BuildError::TooSharp { joint: ji })?;
            let ia = on_line_interval(&outlines[j.a], &g.sa.edge);
            let ib = on_line_interval(&outlines[j.b], &g.sb.edge);
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

        let walls = self
            .walls
            .iter()
            .zip(outlines)
            .enumerate()
            .map(|(i, (w, outline))| Wall {
                id: WallId(i as u32),
                surface: Surface::Planar {
                    origin: w.origin,
                    u: w.u,
                    v: w.v,
                },
                outline,
            })
            .collect();
        let (mut nb, mut nr) = (0, 0);
        let joints = self
            .joints
            .iter()
            .zip(joints)
            .enumerate()
            .map(|(i, (j, kind))| {
                let kind = kind.expect("every joint resolved");
                let name = match kind {
                    JointKind::Bend(_) => {
                        nb += 1;
                        format!("Bend {}", letters(nb - 1))
                    }
                    JointKind::Rip { .. } => {
                        nr += 1;
                        format!("Rip {nr}")
                    }
                    JointKind::Tangent { .. } => format!("Tangent {}", i + 1),
                };
                Joint {
                    id: JointId(i as u32),
                    name,
                    a: WallId(j.a as u32),
                    b: WallId(j.b as u32),
                    kind,
                }
            })
            .collect();
        Ok(Model {
            params: p,
            walls,
            joints,
            fixed: None,
            corner_overrides: Vec::new(),
            bend_relief_overrides: Vec::new(),
        })
    }
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
        crate::poly::difference(std::slice::from_ref(outline), &[band])
    } else {
        crate::poly::union(&[outline.clone(), band])
    };
    // Keep the biggest piece (a trim never splits a sane wall; slivers can appear).
    out.into_iter().filter(|p| p.area() > 1e-12).max_by(|a, b| a.area().total_cmp(&b.area()))
}

/// The parameter interval `[t0, t1]` (along `edge`, 0 at `edge.a`, 1 at `edge.b`) of the
/// outline's boundary lying on the edge's line.
fn on_line_interval(outline: &Polygon, edge: &Seg2) -> Option<(f64, f64)> {
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
