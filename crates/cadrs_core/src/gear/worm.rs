//! A worm drive: a cylindrical worm and the wheel it drives, on shafts square and apart.
//!
//! - **The worm** is turned as a lathe cuts one (ZA): its thread's axial section is a rack's,
//!   straight-sided at the pressure angle, carried along its helix. Its axial module m is the
//!   wheel's transverse module; its pitch diameter is q m (q the diameter quotient), its lead
//!   z₁ π m for z₁ starts, so its lead angle is atan(z₁ / q). Addendum m, dedendum 1.25 m. Its
//!   threads run its length; its core a module further at each end.
//! - **The wheel** is hobbed by its worm: a hob the worm's own, its thread a quarter module
//!   longer to cut the wheel's roots and at its full thickness (the backlash is taken off the
//!   worm's thread), turns whole turns while the wheel turns in step (z₁ teeth
//!   for each of the worm's turns), and every tooth space is generated exactly
//!   ([`crate::gear::Generated`]). Its blank is a cylinder over its tips, z₂ m + 2 m across.
//! - The centre distance is (q + z₂) m / 2.
//!
//! Frame: the wheel's axis +z through the origin, its middle plane z = 0; the worm's axis along
//! +x through (0, a, 0).

use super::{Frame, Region2, Seg};
use super::{Generated, Piece, Revolved, Thread, Vec3};
use std::f64::consts::{PI, TAU};

/// A worm drive as a designer states it. Lengths in mm, angles in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Design {
    /// The worm's starts and the wheel's teeth.
    pub starts: u32,
    pub teeth: u32,
    /// The axial module.
    pub module: f64,
    /// The worm's pitch diameter over the module.
    pub quotient: f64,
    pub pressure: f64,
    /// +1 a right-hand worm, −1 a left-hand one.
    pub hand: f64,
    /// The thread thinner by this, along the normal at the pitch line.
    pub backlash: f64,
    /// The worm's length and the wheel's face width.
    pub length: f64,
    pub width: f64,
    /// The bores' diameters (0 for none).
    pub worm_bore: f64,
    pub wheel_bore: f64,
}

impl Design {
    pub fn centre_distance(&self) -> f64 {
        (self.quotient + self.teeth as f64) * self.module / 2.0
    }

    /// What isn't so.
    pub fn problem(&self) -> Option<&'static str> {
        let m = self.module;
        if !(self.starts >= 1 && self.starts <= 8) {
            return Some("A worm has between 1 and 8 starts");
        }
        if !(self.quotient >= 4.0 && self.quotient <= 30.0) {
            return Some("The worm's diameter quotient must be between 4 and 30");
        }
        if self.worm_bore / 2.0 >= self.quotient * m / 2.0 - 1.25 * m - 0.5 * m {
            return Some("The worm's bore is too large for its roots");
        }
        if self.wheel_bore / 2.0 >= self.teeth as f64 * m / 2.0 - 1.25 * m - 0.5 * m {
            return Some("The wheel's bore is too large for its roots");
        }
        if !(self.length > 0.0 && self.width > 0.0) {
            return Some("The worm's length and the wheel's width must be greater than zero");
        }
        None
    }
}

/// A frame whose y is `axis`, its x `x`, at `origin`.
fn frame(origin: Vec3, axis: Vec3, x: Vec3) -> Frame {
    Frame {
        origin,
        x,
        normal: super::cross(x, axis),
    }
}

fn polygon(pts: &[[f64; 2]]) -> Region2 {
    Region2 {
        outer: (0..pts.len())
            .map(|i| Seg::Line {
                a: pts[i],
                b: pts[(i + 1) % pts.len()],
            })
            .collect(),
        holes: Vec::new(),
    }
}

/// One of the worm's threads (start `k`), from `base` to its tip `tip` from the axis, thinner
/// by `backlash`: its axial section carried along its helix past both ends, and the cylinder it
/// is cut to.
fn thread(d: &Design, k: u32, base: f64, tip: f64, length: f64, backlash: f64) -> Vec<Piece> {
    let m = d.module;
    let a = d.centre_distance();
    let r1 = d.quotient * m / 2.0;
    let pitch = PI * m;
    let lead = d.starts as f64 * pitch;
    let lead_angle = (d.starts as f64 / d.quotient).atan();
    let root = base;
    // The section's half width along the axis at `r`.
    let half =
        |r: f64| pitch / 4.0 - (r - r1) * d.pressure.tan() - backlash / (2.0 * lead_angle.cos());
    let section = vec![
        [root, -half(root)],
        [tip, -half(tip)],
        [tip, half(tip)],
        [root, half(root)],
    ];
    // From two leads before the worm's start to two past its end, starting toward −y (where
    // the wheel is). Each start is the first moved an axial pitch along the axis (the same
    // helicoid as turned a whole turn over the starts about it).
    let from = -length / 2.0 - 2.0 * lead + k as f64 * pitch;
    let turns = ((length + 4.0 * lead) / lead).ceil();
    let x = [0.0, -1.0, 0.0];
    let axis = [1.0, 0.0, 0.0];
    vec![
        Piece::Thread(Thread {
            origin: [from, a, 0.0],
            dir: axis,
            x,
            section,
            radius: r1,
            lead,
            turns: d.hand * turns,
        }),
        Piece::Revolved(Revolved {
            frame: frame([0.0, a, 0.0], axis, [0.0, 1.0, 0.0]),
            region: polygon(&[
                [0.0, -length / 2.0],
                [tip + m, -length / 2.0],
                [tip + m, length / 2.0],
                [0.0, length / 2.0],
            ]),
            turn: None,
        }),
    ]
}

/// The worm: its core, and each thread cut to its length; all one solid.
pub(super) fn worm(d: &Design) -> Vec<Vec<Piece>> {
    let m = d.module;
    let a = d.centre_distance();
    let r1 = d.quotient * m / 2.0;
    let root = r1 - 1.25 * m;
    let axis = [1.0, 0.0, 0.0];
    let l = d.length;
    // The core runs a module past the threads at each end (a shoulder, and no end face the
    // threads' ends lie in).
    let core_l = l + 2.0 * m;
    let core = Piece::Revolved(Revolved {
        frame: frame([0.0, a, 0.0], axis, [0.0, 1.0, 0.0]),
        region: polygon(&[
            [d.worm_bore / 2.0, -core_l / 2.0],
            [root, -core_l / 2.0],
            [root, core_l / 2.0],
            [d.worm_bore / 2.0, core_l / 2.0],
        ]),
        turn: None,
    });
    let mut out = Vec::new();
    for k in 0..d.starts {
        // (Each thread from half a module inside the core, so its flanks cross the core's
        // cylinder and join it there.)
        out.push(thread(d, k, root - 0.5 * m, r1 + m, l, d.backlash));
    }
    // (The threads first, the core last: the core's cylinder cut once, by them all.)
    out.push(vec![core]);
    out
}

/// The wheel: its blank hobbed by the worm's first start, turned a start at a time for the
/// others.
pub(super) fn wheel(d: &Design) -> Generated {
    let m = d.module;
    let a = d.centre_distance();
    let r1 = d.quotient * m / 2.0;
    let r2 = d.teeth as f64 * m / 2.0;
    let tip2 = r2 + m;
    let b = d.width;
    let blank = Revolved {
        frame: frame([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        region: polygon(&[
            [d.wheel_bore / 2.0, -b / 2.0],
            [tip2, -b / 2.0],
            [tip2, b / 2.0],
            [d.wheel_bore / 2.0, b / 2.0],
        ]),
        turn: None,
    };
    // The hob: long enough to reach past the wheel's rim wherever its thread meets it.
    let reach = 2.0 * (tip2 * tip2 - (a - r1 - 1.25 * m).powi(2)).max(0.0).sqrt() + 4.0 * m;
    // (The hob is the worm's thread at its full thickness: the backlash is the worm's.)
    let tool = thread(d, 0, r1 - 1.25 * m, r1 + 1.25 * m, reach.max(d.length), 0.0);
    // Whole turns of the hob, the wheel turning z₁ / z₂ of each, back the other way seen from
    // the wheel (its own turn's inverse).
    let g = gcd(d.starts, d.teeth);
    let (hob_turns, wheel_turns) = ((d.teeth / g) as f64, (d.starts / g) as f64);
    let worm_axis = [1.0, 0.0, 0.0];
    Generated {
        blank,
        tool,
        turns: [
            ([0.0, a, 0.0], worm_axis, 0.0, TAU * hob_turns),
            ([0.0; 3], [0.0, 0.0, 1.0], 0.0, -d.hand * TAU * wheel_turns),
        ],
        copies: 1,
        starts: d.starts as usize,
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}
