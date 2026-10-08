//! Lengths and points. Every length is an `i64` count of nanometres ([`Nm`]): grids snap
//! exactly, sums never drift, and six-decimal millimetres (what other tools write) convert both
//! ways without loss. The app shows millimetres. The Y axis points up.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Neg, Sub};

/// A length in nanometres.
pub type Nm = i64;

pub const NM_PER_MM: i64 = 1_000_000;
/// 1 mil (a thousandth of an inch).
pub const MIL: Nm = 25_400;
/// The schematic grid: 50 mil, 1.27 mm. Pin ends and wires sit on it.
pub const SCHEMATIC_GRID: Nm = 50 * MIL;

/// Millimetres to nanometres, rounded to the nearest.
pub fn mm(v: f64) -> Nm {
    (v * NM_PER_MM as f64).round() as Nm
}

/// Nanometres to millimetres.
pub fn to_mm(v: Nm) -> f64 {
    v as f64 / NM_PER_MM as f64
}

/// A point or vector in nanometres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Pt {
    pub x: Nm,
    pub y: Nm,
}

impl Pt {
    pub const ZERO: Pt = Pt { x: 0, y: 0 };

    pub const fn new(x: Nm, y: Nm) -> Pt {
        Pt { x, y }
    }

    /// From millimetres.
    pub fn mm(x: f64, y: f64) -> Pt {
        Pt { x: mm(x), y: mm(y) }
    }

    /// In millimetres.
    pub fn to_mm(self) -> [f64; 2] {
        [to_mm(self.x), to_mm(self.y)]
    }

    /// Rotated counter-clockwise by `deg` degrees about the origin. Quarter turns are exact.
    pub fn rotated(self, deg: f64) -> Pt {
        let d = normalize_deg(deg);
        if d == 0.0 {
            self
        } else if d == 90.0 {
            Pt::new(-self.y, self.x)
        } else if d == 180.0 {
            Pt::new(-self.x, -self.y)
        } else if d == 270.0 {
            Pt::new(self.y, -self.x)
        } else {
            let (s, c) = d.to_radians().sin_cos();
            let (x, y) = (self.x as f64, self.y as f64);
            Pt::new((x * c - y * s).round() as Nm, (x * s + y * c).round() as Nm)
        }
    }

    /// Mirrored across the X axis (Y negated).
    pub fn flip_y(self) -> Pt {
        Pt::new(self.x, -self.y)
    }

    pub fn length(self) -> f64 {
        (self.x as f64).hypot(self.y as f64)
    }

    pub fn dist(self, o: Pt) -> f64 {
        (self - o).length()
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}

impl Neg for Pt {
    type Output = Pt;
    fn neg(self) -> Pt {
        Pt::new(-self.x, -self.y)
    }
}

impl Mul<i64> for Pt {
    type Output = Pt;
    fn mul(self, k: i64) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
}

/// A width and height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    pub w: Nm,
    pub h: Nm,
}

impl Size {
    pub const fn new(w: Nm, h: Nm) -> Size {
        Size { w, h }
    }

    pub fn mm(w: f64, h: f64) -> Size {
        Size { w: mm(w), h: mm(h) }
    }
}

/// An angle in degrees, normalised to [0, 360). Values within 1e-9 of a whole degree snap to it.
pub fn normalize_deg(deg: f64) -> f64 {
    let mut d = deg % 360.0;
    if d < 0.0 {
        d += 360.0;
    }
    let r = d.round();
    if (d - r).abs() < 1e-9 { r % 360.0 } else { d }
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: Pt,
    pub max: Pt,
}

impl Bounds {
    pub fn of(p: Pt) -> Bounds {
        Bounds { min: p, max: p }
    }

    pub fn add(&mut self, p: Pt) {
        self.min = Pt::new(self.min.x.min(p.x), self.min.y.min(p.y));
        self.max = Pt::new(self.max.x.max(p.x), self.max.y.max(p.y));
    }

    pub fn union(a: Option<Bounds>, b: Bounds) -> Bounds {
        match a {
            None => b,
            Some(mut a) => {
                a.add(b.min);
                a.add(b.max);
                a
            }
        }
    }

    pub fn grow(self, by: Nm) -> Bounds {
        Bounds { min: Pt::new(self.min.x - by, self.min.y - by), max: Pt::new(self.max.x + by, self.max.y + by) }
    }

    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn size(&self) -> Size {
        Size::new(self.max.x - self.min.x, self.max.y - self.min.y)
    }

    pub fn center(&self) -> Pt {
        Pt::new((self.min.x + self.max.x) / 2, (self.min.y + self.max.y) / 2)
    }
}

/// For `#[serde(skip_serializing_if)]`: leaves default values out of saved files.
pub fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mm_round_trips_six_decimals() {
        for v in [0.0, 1.27, -88.648737, 99.605041, 2.54, 1e-6, -1e-6] {
            assert_eq!(to_mm(mm(v)), v);
        }
    }

    #[test]
    fn quarter_turns_are_exact() {
        let p = Pt::mm(1.27, -2.54);
        assert_eq!(p.rotated(90.0), Pt::mm(2.54, 1.27));
        assert_eq!(p.rotated(-90.0), Pt::mm(-2.54, -1.27));
        assert_eq!(p.rotated(180.0).rotated(180.0), p);
        assert_eq!(p.rotated(450.0), p.rotated(90.0));
    }

    #[test]
    fn normalizes_degrees() {
        assert_eq!(normalize_deg(-90.0), 270.0);
        assert_eq!(normalize_deg(360.0), 0.0);
        assert_eq!(normalize_deg(359.9999999999), 0.0);
        assert_eq!(normalize_deg(725.5), 5.5);
    }
}
