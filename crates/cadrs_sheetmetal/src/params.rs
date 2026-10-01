//! The Sheet metal model feature's General, Material and Relief settings (SM2.5–SM2.7), which
//! every later feature on the model uses unless it overrides them ("Use model bend radius",
//! "Use model K Factor", a Corner or Bend relief feature).
//!
//! Lengths are millimetres. Defaults follow Onshape's help where it gives them (K factor 0.45,
//! rolled K factor 0.5, the relief scale ranges); the lengths Onshape shows in its screenshots are
//! inch examples, so cadrs picks round millimetre values for those.

use serde::{Deserialize, Serialize};

/// How bends are laid out flat (SM2.6). It also names the last column of the Bends table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BendCalc {
    /// The neutral axis lies at `K × thickness` from the inside of the bend.
    #[default]
    KFactor,
    /// The neutral arc's length between the bend's tangent lines.
    BendAllowance,
    /// Twice the outside setback minus the allowance: what is taken off the sum of the outside
    /// flange lengths (to the virtual sharp).
    BendDeduction,
}

impl BendCalc {
    pub const ALL: [BendCalc; 3] = [BendCalc::KFactor, BendCalc::BendAllowance, BendCalc::BendDeduction];

    /// The dialog and table label ("K Factor", "Bend allowance", "Bend deduction").
    pub fn label(self) -> &'static str {
        match self {
            BendCalc::KFactor => "K Factor",
            BendCalc::BendAllowance => "Bend allowance",
            BendCalc::BendDeduction => "Bend deduction",
        }
    }
}

/// The cut where two bends meet at a corner (SM2.7, SM7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CornerReliefKind {
    SquareSized,
    RectangleScaled,
    RoundSized,
    RoundScaled,
    Closed,
    /// Onshape's default: the corner left as the bends leave it, nothing extra cut.
    #[default]
    Simple,
}

impl CornerReliefKind {
    pub const ALL: [CornerReliefKind; 6] = [
        CornerReliefKind::SquareSized,
        CornerReliefKind::RectangleScaled,
        CornerReliefKind::RoundSized,
        CornerReliefKind::RoundScaled,
        CornerReliefKind::Closed,
        CornerReliefKind::Simple,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CornerReliefKind::SquareSized => "Square - Sized",
            CornerReliefKind::RectangleScaled => "Rectangle - Scaled",
            CornerReliefKind::RoundSized => "Round - Sized",
            CornerReliefKind::RoundScaled => "Round - Scaled",
            CornerReliefKind::Closed => "Closed",
            CornerReliefKind::Simple => "Simple",
        }
    }

    pub fn is_sized(self) -> bool {
        matches!(self, CornerReliefKind::SquareSized | CornerReliefKind::RoundSized)
    }

    pub fn is_scaled(self) -> bool {
        matches!(self, CornerReliefKind::RectangleScaled | CornerReliefKind::RoundScaled)
    }
}

/// The cut where a bend ends at a free edge (SM2.7, SM8). The model feature offers the scaled
/// types and Tear; the Bend relief feature adds the sized ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BendReliefKind {
    SquareSized,
    RectangleScaled,
    #[default]
    ObroundScaled,
    ObroundSized,
    Tear,
}

impl BendReliefKind {
    pub const ALL: [BendReliefKind; 5] = [
        BendReliefKind::SquareSized,
        BendReliefKind::RectangleScaled,
        BendReliefKind::ObroundScaled,
        BendReliefKind::ObroundSized,
        BendReliefKind::Tear,
    ];

    /// The types the Sheet metal model feature offers (the Bend relief feature offers all).
    pub const MODEL: [BendReliefKind; 3] = [BendReliefKind::RectangleScaled, BendReliefKind::ObroundScaled, BendReliefKind::Tear];

    pub fn label(self) -> &'static str {
        match self {
            BendReliefKind::SquareSized => "Square - Sized",
            BendReliefKind::RectangleScaled => "Rectangle - Scaled",
            BendReliefKind::ObroundScaled => "Obround - Scaled",
            BendReliefKind::ObroundSized => "Obround - Sized",
            BendReliefKind::Tear => "Tear",
        }
    }

    pub fn is_sized(self) -> bool {
        matches!(self, BendReliefKind::SquareSized | BendReliefKind::ObroundSized)
    }

    pub fn is_scaled(self) -> bool {
        matches!(self, BendReliefKind::RectangleScaled | BendReliefKind::ObroundScaled)
    }
}

/// A corner relief: its type, the scale (scaled types) and the size (sized types: the square's
/// side or the circle's diameter).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CornerRelief {
    pub kind: CornerReliefKind,
    pub scale: f64,
    pub size: f64,
}

impl Default for CornerRelief {
    fn default() -> Self {
        CornerRelief {
            kind: CornerReliefKind::Simple,
            scale: 1.5,
            size: 3.0,
        }
    }
}

/// A bend relief: its type, the depth and width scales (scaled types) and the depth (sized
/// types).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BendRelief {
    pub kind: BendReliefKind,
    pub depth_scale: f64,
    pub width_scale: f64,
    pub depth: f64,
    /// Bend relief feature only (SM8.3): run the cut the other way, to the end of the sheet.
    pub extend: bool,
}

impl Default for BendRelief {
    fn default() -> Self {
        BendRelief {
            kind: BendReliefKind::ObroundScaled,
            depth_scale: 2.0,
            width_scale: 1.0,
            depth: 6.0,
            extend: false,
        }
    }
}

/// The Sheet metal model's General, Material and Relief sections.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// Sheet thickness (mm).
    pub thickness: f64,
    /// Inner bend radius (mm).
    pub bend_radius: f64,
    /// Show the other side up in the flat view and drawings (flips Up/Down).
    pub flip_direction_up: bool,
    pub bend_calc: BendCalc,
    /// Default bend K factor (Onshape: 0.45).
    pub k_factor: f64,
    /// K factor of rolled walls (Onshape: 0.5).
    pub rolled_k_factor: f64,
    /// Default bend allowance (mm) when `bend_calc` is BendAllowance.
    pub bend_allowance: f64,
    /// Default bend deduction (mm) when `bend_calc` is BendDeduction.
    pub bend_deduction: f64,
    /// The smallest gap between the edges at a rip (mm).
    pub minimal_gap: f64,
    pub corner_relief: CornerRelief,
    pub bend_relief: BendRelief,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            thickness: 1.0,
            bend_radius: 1.0,
            flip_direction_up: false,
            bend_calc: BendCalc::KFactor,
            k_factor: 0.45,
            rolled_k_factor: 0.5,
            bend_allowance: 2.0,
            bend_deduction: 2.0,
            minimal_gap: 0.2,
            corner_relief: CornerRelief::default(),
            bend_relief: BendRelief::default(),
        }
    }
}

/// A field outside its range: the dialog or table cell turns red with this as its tooltip
/// (SM13.3, X5).
#[derive(Clone, Debug, PartialEq)]
pub struct RangeError {
    pub field: &'static str,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    /// The value must be strictly greater than `min` (a thickness, a size).
    pub min_exclusive: bool,
}

impl RangeError {
    pub fn message(&self) -> String {
        if self.max.is_finite() {
            format!("{} must be between {} and {}", self.field, fmt_num(self.min), fmt_num(self.max))
        } else if self.min_exclusive {
            format!("{} must be greater than {}", self.field, fmt_num(self.min))
        } else {
            format!("{} must be at least {}", self.field, fmt_num(self.min))
        }
    }
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.into() }
}

/// The valid ranges (inclusive), as Onshape's help gives them.
pub mod range {
    /// Model K factor and rolled K factor.
    pub const K_FACTOR: (f64, f64) = (0.0, 1.0);
    /// A per-bend K factor in Modify joint or the table (SM6.4).
    pub const JOINT_K_FACTOR: (f64, f64) = (-1.5, 1.0);
    pub const CORNER_RELIEF_SCALE: (f64, f64) = (1.0, 2.0);
    pub const BEND_RELIEF_DEPTH_SCALE: (f64, f64) = (1.0, 5.0);
    pub const BEND_RELIEF_WIDTH_SCALE: (f64, f64) = (0.0625, 2.0);
}

fn check(out: &mut Vec<RangeError>, field: &'static str, value: f64, (min, max): (f64, f64)) {
    if !(value >= min && value <= max) {
        out.push(RangeError {
            field,
            value,
            min,
            max,
            min_exclusive: false,
        });
    }
}

fn check_positive(out: &mut Vec<RangeError>, field: &'static str, value: f64) {
    if !(value > 0.0 && value.is_finite()) {
        out.push(RangeError {
            field,
            value,
            min: 0.0,
            max: f64::INFINITY,
            min_exclusive: true,
        });
    }
}

fn check_non_negative(out: &mut Vec<RangeError>, field: &'static str, value: f64) {
    if !(value >= 0.0 && value.is_finite()) {
        out.push(RangeError {
            field,
            value,
            min: 0.0,
            max: f64::INFINITY,
            min_exclusive: false,
        });
    }
}

impl Params {
    /// Every field outside its range (empty when the settings are valid).
    pub fn validate(&self) -> Vec<RangeError> {
        let mut out = Vec::new();
        check_positive(&mut out, "Thickness", self.thickness);
        check_non_negative(&mut out, "Bend radius", self.bend_radius);
        match self.bend_calc {
            BendCalc::KFactor => {
                check(&mut out, "Default bend K Factor", self.k_factor, range::K_FACTOR);
            }
            BendCalc::BendAllowance => check_positive(&mut out, "Bend allowance", self.bend_allowance),
            BendCalc::BendDeduction => check_non_negative(&mut out, "Bend deduction", self.bend_deduction),
        }
        check(&mut out, "Rolled K Factor", self.rolled_k_factor, range::K_FACTOR);
        check_non_negative(&mut out, "Minimal gap", self.minimal_gap);
        let c = &self.corner_relief;
        if c.kind.is_scaled() {
            check(&mut out, "Corner relief scale", c.scale, range::CORNER_RELIEF_SCALE);
        }
        if c.kind.is_sized() {
            check_positive(&mut out, "Corner relief size", c.size);
        }
        let b = &self.bend_relief;
        if b.kind.is_scaled() {
            check(&mut out, "Bend relief depth scale", b.depth_scale, range::BEND_RELIEF_DEPTH_SCALE);
            check(&mut out, "Bend relief width scale", b.width_scale, range::BEND_RELIEF_WIDTH_SCALE);
        }
        if b.kind.is_sized() {
            check_positive(&mut out, "Bend relief depth", b.depth);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_onshape_and_are_valid() {
        let p = Params::default();
        assert_eq!(p.k_factor, 0.45);
        assert_eq!(p.rolled_k_factor, 0.5);
        assert_eq!(p.bend_calc, BendCalc::KFactor);
        assert_eq!(p.corner_relief.kind, CornerReliefKind::Simple);
        assert_eq!(p.bend_relief.kind, BendReliefKind::ObroundScaled);
        assert!(p.validate().is_empty());
    }

    #[test]
    fn out_of_range_fields_are_reported_with_their_range() {
        let mut p = Params {
            thickness: 0.0,
            k_factor: 1.2,
            ..Default::default()
        };
        p.corner_relief.kind = CornerReliefKind::RoundScaled;
        p.corner_relief.scale = 2.5;
        p.bend_relief.width_scale = 0.01;
        let errs = p.validate();
        let fields: Vec<_> = errs.iter().map(|e| e.field).collect();
        assert_eq!(fields, ["Thickness", "Default bend K Factor", "Corner relief scale", "Bend relief width scale"]);
        assert_eq!(errs[1].message(), "Default bend K Factor must be between 0 and 1");
        assert_eq!(errs[3].message(), "Bend relief width scale must be between 0.0625 and 2");
        assert_eq!(errs[0].message(), "Thickness must be greater than 0");
    }

    #[test]
    fn only_the_fields_of_the_chosen_types_are_checked() {
        let mut p = Params::default();
        p.corner_relief.scale = 9.0; // Simple: no scale
        p.bend_relief.kind = BendReliefKind::Tear;
        p.bend_relief.depth_scale = 9.0; // Tear: no scale
        p.k_factor = 7.0;
        p.bend_calc = BendCalc::BendAllowance; // K unused
        assert!(p.validate().is_empty());
    }
}
