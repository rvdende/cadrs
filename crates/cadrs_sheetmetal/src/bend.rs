//! Bend arithmetic (SM2.6, SM13.1): the flat length of a bend region and the three ways of
//! stating it.
//!
//! For a bend of inner radius `r`, thickness `t` and bend angle `θ` (radians; 90° for an L, 180°
//! for a closed hem: how far the second wall turns away from the first wall's continuation):
//! - **K factor** `k`: the neutral axis lies at `r + k·t`;
//! - **bend allowance** `BA = θ·(r + k·t)`: the neutral arc, the width of the bend region in the
//!   flat pattern, between its two tangent lines;
//! - **outside setback** `OSSB = tan(θ/2)·(r + t)` and **inside setback** `ISSB = tan(θ/2)·r`: how
//!   far each tangent line sits from the virtual sharp, measured on the outside or inside faces;
//! - **bend deduction** `BD = 2·OSSB − BA`: what comes off the sum of the two outside flange
//!   lengths (to the virtual sharp). Undefined from 180° on, where the virtual sharp is at
//!   infinity.

use serde::{Deserialize, Serialize};

use crate::params::{BendCalc, Params};

/// The neutral arc length: the flat width of the bend region.
pub fn bend_allowance(r: f64, t: f64, theta: f64, k: f64) -> f64 {
    theta * (r + k * t)
}

/// Distance from the virtual sharp to a tangent line, on the outside face (`None` from 180° on).
pub fn outside_setback(r: f64, t: f64, theta: f64) -> Option<f64> {
    setback(r + t, theta)
}

/// Distance from the virtual sharp to a tangent line, on the inside face (`None` from 180° on).
pub fn inside_setback(r: f64, theta: f64) -> Option<f64> {
    setback(r, theta)
}

fn setback(radius: f64, theta: f64) -> Option<f64> {
    (theta > 0.0 && theta < std::f64::consts::PI - 1e-9).then(|| (theta / 2.0).tan() * radius)
}

/// `2·OSSB − BA` (`None` from 180° on).
pub fn bend_deduction(r: f64, t: f64, theta: f64, k: f64) -> Option<f64> {
    outside_setback(r, t, theta).map(|ossb| 2.0 * ossb - bend_allowance(r, t, theta, k))
}

/// One bend's own value, in whichever form the model or the table states it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum BendValue {
    KFactor(f64),
    Allowance(f64),
    Deduction(f64),
}

impl BendValue {
    pub fn calc(self) -> BendCalc {
        match self {
            BendValue::KFactor(_) => BendCalc::KFactor,
            BendValue::Allowance(_) => BendCalc::BendAllowance,
            BendValue::Deduction(_) => BendCalc::BendDeduction,
        }
    }

    /// The number shown in the table cell.
    pub fn value(self) -> f64 {
        match self {
            BendValue::KFactor(v) | BendValue::Allowance(v) | BendValue::Deduction(v) => v,
        }
    }

    /// The model's default for its calculation type.
    pub fn from_params(p: &Params) -> BendValue {
        match p.bend_calc {
            BendCalc::KFactor => BendValue::KFactor(p.k_factor),
            BendCalc::BendAllowance => BendValue::Allowance(p.bend_allowance),
            BendCalc::BendDeduction => BendValue::Deduction(p.bend_deduction),
        }
    }

    /// The bend allowance this value gives a bend of radius `r`, thickness `t`, angle `theta`.
    /// A deduction on a bend of 180° or more has no meaning: `None`.
    pub fn allowance(self, r: f64, t: f64, theta: f64) -> Option<f64> {
        match self {
            BendValue::KFactor(k) => Some(bend_allowance(r, t, theta, k)),
            BendValue::Allowance(ba) => Some(ba),
            BendValue::Deduction(bd) => outside_setback(r, t, theta).map(|ossb| 2.0 * ossb - bd),
        }
    }

    /// The same bend stated as `calc` (`None` where that form is undefined: a deduction from
    /// 180° on, or a K factor of a bend with no angle).
    pub fn to_calc(self, calc: BendCalc, r: f64, t: f64, theta: f64) -> Option<BendValue> {
        let ba = self.allowance(r, t, theta)?;
        match calc {
            BendCalc::BendAllowance => Some(BendValue::Allowance(ba)),
            BendCalc::KFactor => (theta.abs() > 1e-12 && t > 0.0).then(|| BendValue::KFactor((ba / theta - r) / t)),
            BendCalc::BendDeduction => outside_setback(r, t, theta).map(|ossb| BendValue::Deduction(2.0 * ossb - ba)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    const EPS: f64 = 1e-9;

    #[test]
    fn ninety_degree_bend_by_hand() {
        // r = 3, t = 2, k = 0.45: BA = π/2·3.9, OSSB = 5, ISSB = 3, BD = 10 − BA.
        let ba = bend_allowance(3.0, 2.0, FRAC_PI_2, 0.45);
        assert!((ba - FRAC_PI_2 * 3.9).abs() < EPS);
        assert!((outside_setback(3.0, 2.0, FRAC_PI_2).unwrap() - 5.0).abs() < EPS);
        assert!((inside_setback(3.0, FRAC_PI_2).unwrap() - 3.0).abs() < EPS);
        assert!((bend_deduction(3.0, 2.0, FRAC_PI_2, 0.45).unwrap() - (10.0 - ba)).abs() < EPS);
    }

    #[test]
    fn no_setback_or_deduction_from_180_degrees() {
        assert!(outside_setback(1.0, 1.0, PI).is_none());
        assert!(bend_deduction(1.0, 1.0, 1.5 * PI, 0.5).is_none());
        assert!(BendValue::Deduction(1.0).allowance(1.0, 1.0, PI).is_none());
        // The allowance of a hem is still defined.
        assert!((bend_allowance(1.0, 1.0, PI, 0.5) - 1.5 * PI).abs() < EPS);
    }

    #[test]
    fn the_three_forms_agree() {
        let (r, t, th) = (2.0, 1.5, 1.2);
        let k = BendValue::KFactor(0.4);
        let ba = k.to_calc(BendCalc::BendAllowance, r, t, th).unwrap();
        let bd = k.to_calc(BendCalc::BendDeduction, r, t, th).unwrap();
        for v in [k, ba, bd] {
            assert!((v.allowance(r, t, th).unwrap() - bend_allowance(r, t, th, 0.4)).abs() < EPS);
            let back = v.to_calc(BendCalc::KFactor, r, t, th).unwrap();
            assert!((back.value() - 0.4).abs() < EPS);
        }
    }

    proptest::proptest! {
        #[test]
        fn conversions_round_trip(r in 0.0f64..20.0, t in 0.1f64..10.0, th in 0.05f64..3.0, k in -1.5f64..1.0) {
            let v = BendValue::KFactor(k);
            for calc in BendCalc::ALL {
                let w = v.to_calc(calc, r, t, th).unwrap();
                proptest::prop_assert_eq!(w.calc(), calc);
                let back = w.to_calc(BendCalc::KFactor, r, t, th).unwrap();
                proptest::prop_assert!((back.value() - k).abs() < 1e-7);
            }
        }
    }
}
