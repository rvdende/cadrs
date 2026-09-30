//! **Relations** (P3B.9, `intro-to-assemblies.md` A1.3, A1.4, X16): constraints between the
//! motions of mates, not between instances. A relation is a feature of the Mate Features list
//! ([`super::mate::MateKind::Relation`]) that couples one degree of freedom of one mate with one
//! of another (or, for Screw, the two of one Cylindrical mate):
//!
//! | type | mates | coupling (v₁ of the first mate, v₂ of the second) |
//! |---|---|---|
//! | **Gear** | two rotating mates (Revolute, Cylindrical, Pin slot) | a·θ₂ = −b·θ₁ (Reverse: +): the first turns `a` while the second turns `b` (ratio `a : b`), the other way round, as meshing gears do |
//! | **Rack and pinion** | a rotating mate (the pinion) and a sliding one (Slider, Cylindrical, Pin slot; the rack) | z = d·θ / 2π (Reverse: −), `d` the distance per revolution (2πr for a pinion of pitch radius r) |
//! | **Screw** | one Cylindrical mate | z = p·θ / 2π (Reverse: −, a left-hand thread), `p` the pitch |
//! | **Linear** | two sliding mates | z₂ = r·z₁ (Reverse: −) |
//!
//! The values are the mates' positions ([`super::mate::dof_value`]): zero where each mate was
//! made, so a relation made on mates at their zero holds there, and **Reset** of either mate
//! brings the other back too.
//!
//! **Turns.** A mate's angle is only known up to whole turns (a pinion looks the same after a
//! turn), so the solver holds the coupling up to the whole turns of its angles: the residual is
//! reduced modulo `2π·g` ([`Coupling::period`]), `g` the largest common step of the angle
//! coefficients (1 for a 2 : 1 gear, `d/2π` for a rack: the rack's position is held up to a whole
//! turn's travel). A drag or an animation moves in small steps, so it stays on its branch: the
//! driven gear keeps turning half as fast, the rack keeps travelling `d` per turn.

use serde::{Deserialize, Serialize};

use super::mate::{Dof, MateFeature, MateId, MateType};

/// The relation types (the toolbar's relation buttons, A1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationType {
    Gear,
    RackPinion,
    Screw,
    Linear,
}

impl RelationType {
    pub const ALL: [RelationType; 4] = [RelationType::Gear, RelationType::RackPinion, RelationType::Screw, RelationType::Linear];

    /// "Gear", "Rack and pinion", … (the dialog's type dropdown and feature names).
    pub fn label(self) -> &'static str {
        match self {
            RelationType::Gear => "Gear",
            RelationType::RackPinion => "Rack and pinion",
            RelationType::Screw => "Screw",
            RelationType::Linear => "Linear",
        }
    }

    /// How many mates it takes.
    pub fn mate_count(self) -> usize {
        match self {
            RelationType::Screw => 1,
            _ => 2,
        }
    }

    /// Whether the `k`-th mate (0 or 1) may be of type `t`.
    pub fn accepts(self, k: usize, t: MateType) -> bool {
        match self {
            RelationType::Gear => angle_dof(t).is_some(),
            RelationType::RackPinion => {
                if k == 0 {
                    angle_dof(t).is_some()
                } else {
                    linear_dof(t).is_some()
                }
            }
            RelationType::Screw => t == MateType::Cylindrical,
            RelationType::Linear => linear_dof(t).is_some(),
        }
    }

    /// The toolbar button (and icon) of the type.
    pub fn icon(self) -> &'static str {
        match self {
            RelationType::Gear => "gear-relation",
            RelationType::RackPinion => "rack-pinion",
            RelationType::Screw => "screw-relation",
            RelationType::Linear => "linear-relation",
        }
    }
}

/// The turning DOF of a mate type a relation couples: Revolute's, Cylindrical's and Pin slot's
/// Z angle.
pub fn angle_dof(t: MateType) -> Option<Dof> {
    matches!(t, MateType::Revolute | MateType::Cylindrical | MateType::PinSlot).then_some(Dof::Angle)
}

/// The sliding DOF of a mate type a relation couples: Slider's and Cylindrical's Z, Pin slot's
/// X.
pub fn linear_dof(t: MateType) -> Option<Dof> {
    match t {
        MateType::Slider | MateType::Cylindrical => Some(Dof::Z),
        MateType::PinSlot => Some(Dof::X),
        _ => None,
    }
}

/// A relation between mates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub relation_type: RelationType,
    /// The mates, in the dialog's order (one for Screw).
    pub mates: Vec<MateId>,
    /// Gear: the ratio `a : b` (the first mate turns `a` while the second turns `b`); Linear:
    /// `(1, r)`, the second moves `r` times as far as the first.
    #[serde(default = "one_one")]
    pub ratio: (f64, f64),
    /// Rack and pinion: the distance per revolution; Screw: the pitch (mm).
    #[serde(default)]
    pub distance: f64,
    /// **Reverse direction**.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
}

fn one_one() -> (f64, f64) {
    (1.0, 1.0)
}

impl Relation {
    /// A relation of `t` between `mates` with its defaults: ratio 1 : 1, 1 in per revolution.
    pub fn new(t: RelationType, mates: Vec<MateId>) -> Self {
        Self { relation_type: t, mates, ratio: (1.0, 1.0), distance: 25.4, reverse: false }
    }

    /// Whether its values make sense (non-zero ratio, distance).
    pub fn values_ok(&self) -> bool {
        match self.relation_type {
            RelationType::Gear => self.ratio.0.abs() > 1e-12 && self.ratio.1.abs() > 1e-12,
            RelationType::Linear => self.ratio.1.is_finite() && self.ratio.0.abs() > 1e-12,
            RelationType::RackPinion | RelationType::Screw => self.distance.is_finite() && self.distance.abs() > 1e-9,
        }
    }
}

/// A relation as the solver holds it: `k₁·v₁ + k₂·v₂ ≡ 0`, `v₁` the DOF `dofs[0]` of the first
/// mate and `v₂` the DOF `dofs[1]` of the second (the same mate for Screw), reduced modulo
/// [`Coupling::period`] when that is not zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coupling {
    pub k: [f64; 2],
    pub dofs: [Dof; 2],
    /// Residuals are taken modulo this (0: not at all): the whole turns of the angles.
    pub period: f64,
    /// The residual is an angle (Gear): the solver scales it like its other angle rows.
    pub angular: bool,
}

impl Coupling {
    /// The residual for the values `v₁`, `v₂` (mm or radians).
    pub fn residual(&self, v1: f64, v2: f64) -> f64 {
        let r = self.k[0] * v1 + self.k[1] * v2;
        if self.period > 0.0 { reduce(r, self.period) } else { r }
    }
}

/// `x` reduced to (−p/2, p/2].
pub fn reduce(x: f64, p: f64) -> f64 {
    let r = (x + p / 2.0).rem_euclid(p) - p / 2.0;
    if r <= -p / 2.0 { r + p } else { r }
}

/// The largest `g` with `a` and `b` whole multiples of it (to 1e-6, denominators up to 1000):
/// a 2 : 1 gear gives 1, 1.5 : 1 gives 0.5.
pub fn common_step(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    if a < 1e-12 {
        return b;
    }
    if b < 1e-12 {
        return a;
    }
    // As fractions over a common denominator.
    for q in 1..=1000u64 {
        let (x, y) = (a * q as f64, b * q as f64);
        if (x - x.round()).abs() < 1e-6 * x.max(1.0) && (y - y.round()).abs() < 1e-6 * y.max(1.0) {
            let (mut m, mut n) = (x.round() as u64, y.round() as u64);
            while n != 0 {
                (m, n) = (n, m % n);
            }
            return m.max(1) as f64 / q as f64;
        }
    }
    1e-3
}

/// The coupling of `r` given its mates' types (in its order), or `None` if a mate doesn't fit
/// the type.
pub fn coupling(r: &Relation, types: &[MateType]) -> Option<Coupling> {
    let t = r.relation_type;
    let s = if r.reverse { -1.0 } else { 1.0 };
    let tau = std::f64::consts::TAU;
    match t {
        RelationType::Gear => {
            let (a, b) = r.ratio;
            let (d1, d2) = (angle_dof(*types.first()?)?, angle_dof(*types.get(1)?)?);
            // a·θ₂ + b·θ₁ = 0 (reversed: a·θ₂ − b·θ₁ = 0).
            let k = [s * b, a];
            Some(Coupling { k, dofs: [d1, d2], period: tau * common_step(k[0], k[1]), angular: true })
        }
        RelationType::RackPinion => {
            let (d1, d2) = (angle_dof(*types.first()?)?, linear_dof(*types.get(1)?)?);
            // z − s·d·θ/2π = 0: whole turns of θ move z by d.
            let k = [-s * r.distance / tau, 1.0];
            Some(Coupling { k, dofs: [d1, d2], period: r.distance.abs(), angular: false })
        }
        RelationType::Screw => {
            let m = *types.first()?;
            (m == MateType::Cylindrical).then_some(())?;
            let k = [-s * r.distance / tau, 1.0];
            Some(Coupling { k, dofs: [Dof::Angle, Dof::Z], period: r.distance.abs(), angular: false })
        }
        RelationType::Linear => {
            let (d1, d2) = (linear_dof(*types.first()?)?, linear_dof(*types.get(1)?)?);
            let ratio = r.ratio.1 / r.ratio.0;
            Some(Coupling { k: [-s * ratio, 1.0], dofs: [d1, d2], period: 0.0, angular: false })
        }
    }
}

/// Why `r` can't be made on the mate features `mates` (the dialog's error), or `None`.
pub fn check(r: &Relation, mates: &[MateFeature]) -> Option<String> {
    let t = r.relation_type;
    if r.mates.len() != t.mate_count() {
        return Some(if t.mate_count() == 1 { "Select a Cylindrical mate".into() } else { "Select two mates".into() });
    }
    if r.mates.len() == 2 && r.mates[0] == r.mates[1] {
        return Some("Select two different mates".into());
    }
    for (k, id) in r.mates.iter().enumerate() {
        let Some(f) = mates.iter().find(|f| f.id == *id) else {
            return Some("A mate of the relation is gone".into());
        };
        let Some(m) = f.mate() else {
            return Some(format!("{} is not a mate", f.name));
        };
        if !t.accepts(k, m.mate_type) {
            let want = match (t, k) {
                (RelationType::Gear, _) | (RelationType::RackPinion, 0) => "a Revolute, Cylindrical or Pin slot mate",
                (RelationType::Screw, _) => "a Cylindrical mate",
                _ => "a Slider, Cylindrical or Pin slot mate",
            };
            return Some(format!("{} is a {} mate; select {want}", f.name, m.mate_type.label()));
        }
    }
    if !r.values_ok() {
        return Some(match t {
            RelationType::RackPinion => "Enter a distance per revolution".into(),
            RelationType::Screw => "Enter a pitch".into(),
            _ => "Enter a ratio".into(),
        });
    }
    None
}

/// The mate types of a relation's mates, if they are all there and mates.
pub fn mate_types(r: &Relation, mates: &[MateFeature]) -> Option<Vec<MateType>> {
    r.mates.iter().map(|id| mates.iter().find(|f| f.id == *id).and_then(|f| f.mate()).map(|m| m.mate_type)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_steps() {
        assert_eq!(common_step(2.0, 1.0), 1.0);
        assert!((common_step(1.5, 1.0) - 0.5).abs() < 1e-12);
        assert!((common_step(12.0, 18.0) - 6.0).abs() < 1e-12);
        assert!((reduce(7.0, 2.0) - 1.0).abs() < 1e-12);
        assert!((reduce(-0.75, 1.0) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn a_gear_holds_modulo_whole_turns() {
        let r = Relation { ratio: (2.0, 1.0), ..Relation::new(RelationType::Gear, vec![MateId::from_u128(1), MateId::from_u128(2)]) };
        let c = coupling(&r, &[MateType::Revolute, MateType::Revolute]).unwrap();
        let pi = std::f64::consts::PI;
        assert!(c.residual(pi / 2.0, -pi / 4.0).abs() < 1e-12);
        // A whole turn of the driver: the driven gear half a turn back; the wrapped angles agree.
        assert!(c.residual(0.0, -pi).abs() < 1e-12);
        assert!(c.residual(pi / 2.0, pi / 4.0).abs() > 0.1);
    }
}
