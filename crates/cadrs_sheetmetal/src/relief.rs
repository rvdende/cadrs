//! Relief cut-outs in the flat pattern (SM2.7, SM7, SM8).
//!
//! **Corner reliefs.** Where two bends of the same wall meet, their bend regions would cross:
//! the **corner zone** `Q` is the rectangle where the two bend regions, extended along their
//! bends, overlap (each side as wide as that bend's allowance). Every type except Closed removes
//! `Q` and whatever of the shared wall still pokes past both tangent lines into the corner; on top
//! of that, centred on `Q`:
//! - Square – Sized: a square of side `size`;
//! - Round – Sized: a circle of diameter `size`;
//! - Rectangle – Scaled: `Q` scaled by `scale` (1.00–2.00) about its centre;
//! - Round – Scaled: a circle of diameter `scale × max(allowances)`;
//! - Simple: nothing more (the corner left as the bends leave it);
//! - Closed: nothing removed; the two bend regions are mitred along `Q`'s diagonal so the
//!   corner closes as far as it can.
//!
//! **Bend reliefs.** Where a bend ends but one of its walls carries on along the bend line (a
//! partial flange, a bend across part of a face), a slot is cut just past the bend's end, across
//! the bend region and `extra` deep into the wall that carries on:
//! - width: `thickness × width scale` for the scaled types, the thickness for the sized ones, the
//!   minimal gap for Tear (a rip, almost no material removed);
//! - extra depth: `(depth scale − 1) × bend radius` for the scaled types (1 makes an obround's
//!   round end just touch the bend), the given depth for the sized ones, none for Tear;
//! - Rectangle and Square end square; Obround ends in a half circle reaching the same depth;
//! - "Extend bend relief" runs the cut on along the bend line to the end of the sheet.

use crate::params::{BendRelief, BendReliefKind, CornerRelief, CornerReliefKind, Params};
use crate::poly::{P2, Polygon, V2, circle};

/// Points on round reliefs.
const ROUND_SEGMENTS: usize = 48;

/// A local frame in the flat: `origin`, unit `x` and unit `y` (`y` need not be `x`'s left normal).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub origin: P2,
    pub x: V2,
    pub y: V2,
}

impl Frame {
    pub fn at(&self, x: f64, y: f64) -> P2 {
        self.origin + self.x * x + self.y * y
    }

    /// The rectangle `[x0, x1] × [y0, y1]` in this frame.
    pub fn rect(&self, x0: f64, x1: f64, y0: f64, y1: f64) -> Polygon {
        Polygon::new(vec![self.at(x0, y0), self.at(x1, y0), self.at(x1, y1), self.at(x0, y1)])
    }
}

/// The extra shape cut at a corner on top of the zone itself (`None` for Simple and Closed).
/// `q` is the corner zone, `allowances` the two bends' flat widths.
pub fn corner_shape(relief: &CornerRelief, q: &Polygon, allowances: (f64, f64)) -> Option<Polygon> {
    if q.is_empty() {
        return None;
    }
    // Q's centre from its vertices (Q is a parallelogram).
    let c = P2::from(q.outer.iter().fold(V2::zeros(), |s, p| s + p.coords) / q.outer.len() as f64);
    match relief.kind {
        CornerReliefKind::Simple | CornerReliefKind::Closed => None,
        CornerReliefKind::SquareSized => {
            // Aligned with Q's first side.
            let e = (q.outer[1] - q.outer[0]).normalize();
            let f = V2::new(-e.y, e.x);
            let h = relief.size / 2.0;
            Some(Polygon::new(vec![c - e * h - f * h, c + e * h - f * h, c + e * h + f * h, c - e * h + f * h]))
        }
        CornerReliefKind::RoundSized => Some(circle(c, relief.size / 2.0, ROUND_SEGMENTS)),
        CornerReliefKind::RectangleScaled => Some(q.map(|p| c + (p - c) * relief.scale)),
        CornerReliefKind::RoundScaled => Some(circle(c, relief.scale * allowances.0.max(allowances.1) / 2.0, ROUND_SEGMENTS)),
    }
}

/// The slot's width and extra depth for a bend relief.
pub fn bend_relief_size(relief: &BendRelief, p: &Params, bend_radius: f64) -> (f64, f64) {
    let t = p.thickness;
    match relief.kind {
        BendReliefKind::RectangleScaled | BendReliefKind::ObroundScaled => {
            (t * relief.width_scale, (relief.depth_scale - 1.0).max(0.0) * bend_radius)
        }
        BendReliefKind::SquareSized | BendReliefKind::ObroundSized => (t, relief.depth),
        BendReliefKind::Tear => (p.minimal_gap.max(1e-3), 0.0),
    }
}

/// A bend relief in `frame`: `x` along the bend, pointing away from the bend past its end
/// (x = 0 at the end); `y` across the bend, from the tangent line on the wall that carries on
/// (y = 0) towards the other (y = `allowance`). `reach` is how far an extended relief runs.
pub fn bend_relief_shape(relief: &BendRelief, p: &Params, bend_radius: f64, allowance: f64, frame: &Frame, reach: f64) -> Polygon {
    let (w, extra) = bend_relief_size(relief, p, bend_radius);
    let len = if relief.extend { reach.max(w) } else { w };
    let deep = -extra;
    match relief.kind {
        BendReliefKind::ObroundScaled | BendReliefKind::ObroundSized if !relief.extend => {
            // A rectangle ending in a half circle whose tip is at `deep`.
            let r = w / 2.0;
            let mut pts = vec![frame.at(0.0, allowance), frame.at(0.0, deep + r)];
            for i in 1..ROUND_SEGMENTS / 2 {
                let a = std::f64::consts::PI + std::f64::consts::PI * i as f64 / (ROUND_SEGMENTS / 2) as f64;
                pts.push(frame.at(r + r * a.cos(), deep + r + r * a.sin()));
            }
            pts.push(frame.at(w, deep + r));
            pts.push(frame.at(w, allowance));
            Polygon::new(pts)
        }
        _ => frame.rect(0.0, len, deep, allowance),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame {
            origin: P2::origin(),
            x: V2::x(),
            y: V2::y(),
        }
    }

    #[test]
    fn scaled_bend_relief_sizes() {
        let p = Params {
            thickness: 2.0,
            ..Default::default()
        };
        let r = BendRelief {
            kind: BendReliefKind::RectangleScaled,
            depth_scale: 2.0,
            width_scale: 0.5,
            ..Default::default()
        };
        assert_eq!(bend_relief_size(&r, &p, 3.0), (1.0, 3.0));
        let s = bend_relief_shape(&r, &p, 3.0, 4.0, &frame(), 100.0);
        // 1 wide, from −3 to 4 across.
        assert!((s.area() - 7.0).abs() < 1e-9);
    }

    #[test]
    fn obround_matches_the_rectangle_depth() {
        let p = Params {
            thickness: 2.0,
            ..Default::default()
        };
        let r = BendRelief {
            kind: BendReliefKind::ObroundScaled,
            depth_scale: 1.0,
            width_scale: 1.0,
            ..Default::default()
        };
        let s = bend_relief_shape(&r, &p, 3.0, 4.0, &frame(), 100.0);
        let (lo, hi) = s.bounds().unwrap();
        assert!(lo.y.abs() < 1e-9, "depth scale 1 just touches the bend: {lo:?}");
        assert!((hi.y - 4.0).abs() < 1e-9);
        assert!((hi.x - lo.x - 2.0).abs() < 1e-9);
    }

    #[test]
    fn extended_relief_runs_to_reach() {
        let p = Params::default();
        let r = BendRelief {
            kind: BendReliefKind::RectangleScaled,
            extend: true,
            ..Default::default()
        };
        let s = bend_relief_shape(&r, &p, 1.0, 2.0, &frame(), 50.0);
        assert!((s.bounds().unwrap().1.x - 50.0).abs() < 1e-9);
    }

    #[test]
    fn corner_shapes_are_centred_on_the_zone() {
        let q = Polygon::rect(P2::new(0.0, 0.0), P2::new(2.0, 2.0));
        for kind in CornerReliefKind::ALL {
            let r = CornerRelief {
                kind,
                scale: 1.5,
                size: 4.0,
            };
            match corner_shape(&r, &q, (2.0, 2.0)) {
                None => assert!(matches!(kind, CornerReliefKind::Simple | CornerReliefKind::Closed)),
                Some(s) => {
                    let (lo, hi) = s.bounds().unwrap();
                    let c = P2::from((lo.coords + hi.coords) / 2.0);
                    assert!((c - P2::new(1.0, 1.0)).norm() < 1e-6, "{kind:?}");
                    let expect = match kind {
                        CornerReliefKind::SquareSized => 16.0,
                        CornerReliefKind::RoundSized => std::f64::consts::PI * 4.0,
                        CornerReliefKind::RectangleScaled => 9.0,
                        CornerReliefKind::RoundScaled => std::f64::consts::PI * 2.25,
                        _ => unreachable!(),
                    };
                    assert!((s.area() - expect).abs() / expect < 0.01, "{kind:?}: {}", s.area());
                }
            }
        }
    }
}
