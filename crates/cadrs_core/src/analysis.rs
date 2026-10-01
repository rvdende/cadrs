//! The view's analysis tools (P3E.3, TD6.6, PS2.11, A1.9), like Onshape's: the geometry behind
//! them, free of any rendering.
//!
//! - **Draft analysis**: a face's draft is the angle it leans from the pull direction:
//!   90° − the angle between its outward normal and the pull direction, so a wall parallel to
//!   the pull has 0°, a face turned towards the pull a positive draft and one turned away a
//!   negative draft. [`DraftBand`] sorts drafts into six bands by the required angle `a`:
//!   ≥ 2a and a…2a (enough positive draft, greens), 0…a and −a…0 (not enough: yellows),
//!   −2a…−a and ≤ −2a (negative draft, reds). The view colours each point of a face by its
//!   band, so a curved face shows where its draft runs out.
//! - **Curvature combs** ([`curvature_comb`]): along an edge, a tooth at each point of its
//!   polyline, pointing away from the centre of curvature, as long as the curvature (1 / radius)
//!   times a scale. A straight run has none.

use crate::solid::{add, cross, dot, len, normalize, scale, sub};
use cadrs_sketch::Vec3;

/// The draft of a face whose outward normal is `normal`, against the pull direction `pull`,
/// in degrees (−90…90).
pub fn draft_angle(normal: Vec3, pull: Vec3) -> f64 {
    let (n, d) = (normalize(normal), normalize(pull));
    dot(n, d).clamp(-1.0, 1.0).asin().to_degrees()
}

/// A band of the draft analysis's legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DraftBand {
    /// At least twice the required angle.
    PositiveSteep,
    /// The required angle up to twice it.
    Positive,
    /// Positive, but less than the required angle.
    InsufficientPositive,
    /// Negative, less than the required angle (vertical walls fall here or just above).
    InsufficientNegative,
    /// Negative, the required angle up to twice it.
    Negative,
    /// Negative, at least twice the required angle.
    NegativeSteep,
}

impl DraftBand {
    /// Top to bottom, as the legend lists them.
    pub const ALL: [DraftBand; 6] = [
        DraftBand::PositiveSteep,
        DraftBand::Positive,
        DraftBand::InsufficientPositive,
        DraftBand::InsufficientNegative,
        DraftBand::Negative,
        DraftBand::NegativeSteep,
    ];

    /// The band of a draft of `draft` degrees when `required` degrees are needed.
    pub fn of(draft: f64, required: f64) -> DraftBand {
        let a = required.abs();
        // A hair of tolerance, so a face drafted exactly by the required angle passes.
        let eps = 1e-9;
        if draft >= 2.0 * a - eps {
            DraftBand::PositiveSteep
        } else if draft >= a - eps {
            DraftBand::Positive
        } else if draft >= 0.0 {
            DraftBand::InsufficientPositive
        } else if draft > -a + eps {
            DraftBand::InsufficientNegative
        } else if draft > -2.0 * a + eps {
            DraftBand::Negative
        } else {
            DraftBand::NegativeSteep
        }
    }

    /// The band's range in degrees, low to high, for a required angle.
    pub fn range(self, required: f64) -> (f64, f64) {
        let a = required.abs();
        match self {
            DraftBand::PositiveSteep => (2.0 * a, 90.0),
            DraftBand::Positive => (a, 2.0 * a),
            DraftBand::InsufficientPositive => (0.0, a),
            DraftBand::InsufficientNegative => (-a, 0.0),
            DraftBand::Negative => (-2.0 * a, -a),
            DraftBand::NegativeSteep => (-90.0, -2.0 * a),
        }
    }

    /// The legend's text for the band: "3° to 6°".
    pub fn label(self, required: f64) -> String {
        let (lo, hi) = self.range(required);
        let f = |v: f64| {
            let s = format!("{v:.1}");
            let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            if s == "-0" { "0".into() } else { s.replace('-', "−") }
        };
        format!("{}° to {}°", f(lo), f(hi))
    }

    /// The band's colour (sRGB): greens for enough positive draft, yellows for too little,
    /// reds for negative draft.
    pub fn color(self) -> [u8; 3] {
        match self {
            DraftBand::PositiveSteep => [0x1f, 0x8a, 0x3c],
            DraftBand::Positive => [0x5c, 0xc4, 0x52],
            DraftBand::InsufficientPositive => [0xf2, 0xdc, 0x3a],
            DraftBand::InsufficientNegative => [0xf0, 0xb0, 0x2c],
            DraftBand::Negative => [0xe8, 0x6a, 0x58],
            DraftBand::NegativeSteep => [0xc4, 0x24, 0x2a],
        }
    }

    /// The band's place in [`Self::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|b| *b == self).unwrap_or(0)
    }
}

/// One tooth of a curvature comb.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombTooth {
    /// The point on the curve.
    pub at: Vec3,
    /// Unit direction away from the centre of curvature (zero on a straight run).
    pub dir: Vec3,
    /// The curvature there (1 / radius, per mm).
    pub curvature: f64,
}

impl CombTooth {
    /// The tooth's tip for a comb scale (mm of tooth per unit of curvature).
    pub fn tip(&self, scale_mm: f64) -> Vec3 {
        add(self.at, scale(self.dir, self.curvature * scale_mm))
    }
}

/// The curvature comb of a polyline (an edge's points): a tooth at every point, from the circle
/// through it and its neighbours. A closed polyline (first point = last) wraps round; an open
/// one's ends take their neighbour's curvature.
pub fn curvature_comb(points: &[Vec3]) -> Vec<CombTooth> {
    let n = points.len();
    if n < 3 {
        return points.iter().map(|p| CombTooth { at: *p, dir: [0.0; 3], curvature: 0.0 }).collect();
    }
    let closed = len(sub(points[0], points[n - 1])) < 1e-9;
    let at = |i: usize| -> Option<(Vec3, f64)> {
        let (a, b, c) = if i == 0 || i == n - 1 {
            if !closed {
                return None;
            }
            // The seam: the points either side of it.
            (points[n - 2], points[0], points[1])
        } else {
            (points[i - 1], points[i], points[i + 1])
        };
        circle_through(a, b, c).map(|(center, r)| (normalize(sub(b, center)), 1.0 / r))
    };
    let mut teeth: Vec<CombTooth> = (0..n)
        .map(|i| match at(i) {
            Some((dir, k)) => CombTooth { at: points[i], dir, curvature: k },
            None => CombTooth { at: points[i], dir: [0.0; 3], curvature: 0.0 },
        })
        .collect();
    if !closed {
        // An open curve's ends: the neighbouring tooth's curvature, pointing the same way
        // round.
        for (end, next) in [(0, 1), (n - 1, n - 2)] {
            let t = teeth[next];
            if t.curvature > 0.0 {
                teeth[end] = CombTooth { at: points[end], dir: t.dir, curvature: t.curvature };
            }
        }
    }
    teeth
}

/// The circle through three points: its centre and radius; `None` when they are (nearly) in a
/// line.
fn circle_through(a: Vec3, b: Vec3, c: Vec3) -> Option<(Vec3, f64)> {
    let (ab, ac) = (sub(b, a), sub(c, a));
    let n = cross(ab, ac);
    let nn = dot(n, n);
    let size = dot(ab, ab).max(dot(ac, ac));
    if nn <= 1e-12 * size * size {
        return None;
    }
    // The circumcentre: a + (|ac|²(n × ab)... ) / 2|n|² (the usual formula).
    let t1 = scale(cross(n, ab), dot(ac, ac));
    let t2 = scale(cross(ac, n), dot(ab, ab));
    let center = add(a, scale(add(t1, t2), 0.5 / nn));
    Some((center, len(sub(b, center))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
    }

    #[test]
    fn a_5_degree_drafted_face_falls_in_the_3_to_6_band() {
        // Pull +Z. A side wall leaning 5° towards the pull: its normal is tipped 5° up from
        // horizontal.
        let t = 5f64.to_radians();
        let wall = [t.cos(), 0.0, t.sin()];
        let d = draft_angle(wall, [0.0, 0.0, 1.0]);
        close(d, 5.0, 1e-9);
        assert_eq!(DraftBand::of(d, 3.0), DraftBand::Positive);
        assert_eq!(DraftBand::Positive.label(3.0), "3° to 6°");
        // The same wall against the opposite pull: negative, −6…−3.
        let d = draft_angle(wall, [0.0, 0.0, -1.0]);
        assert_eq!(DraftBand::of(d, 3.0), DraftBand::Negative);
        assert_eq!(DraftBand::Negative.label(3.0), "−6° to −3°");
    }

    #[test]
    fn bands_cover_every_draft() {
        let a = 3.0;
        assert_eq!(DraftBand::of(90.0, a), DraftBand::PositiveSteep);
        assert_eq!(DraftBand::of(6.0, a), DraftBand::PositiveSteep);
        assert_eq!(DraftBand::of(3.0, a), DraftBand::Positive);
        assert_eq!(DraftBand::of(2.0, a), DraftBand::InsufficientPositive);
        // A vertical wall has no draft: not enough.
        assert_eq!(DraftBand::of(draft_angle([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), a), DraftBand::InsufficientPositive);
        assert_eq!(DraftBand::of(-1.0, a), DraftBand::InsufficientNegative);
        assert_eq!(DraftBand::of(-3.0, a), DraftBand::Negative);
        assert_eq!(DraftBand::of(-90.0, a), DraftBand::NegativeSteep);
        // The top of a part faces the pull (+90), its bottom away (−90).
        close(draft_angle([0.0, 0.0, 1.0], [0.0, 0.0, 2.0]), 90.0, 1e-9);
        close(draft_angle([0.0, 0.0, -1.0], [0.0, 0.0, 1.0]), -90.0, 1e-9);
        // The labels read top to bottom.
        let labels: Vec<String> = DraftBand::ALL.iter().map(|b| b.label(a)).collect();
        assert_eq!(labels, ["6° to 90°", "3° to 6°", "0° to 3°", "−3° to 0°", "−6° to −3°", "−90° to −6°"]);
        for (i, b) in DraftBand::ALL.iter().enumerate() {
            assert_eq!(b.index(), i);
        }
    }

    #[test]
    fn a_circle_has_a_constant_comb() {
        // A Ø40 circle every 5°: every tooth reads 1/20, pointing outward.
        let pts: Vec<Vec3> = (0..=72)
            .map(|k| {
                let t = std::f64::consts::TAU * k as f64 / 72.0;
                [20.0 * t.cos(), 20.0 * t.sin(), 3.0]
            })
            .collect();
        let comb = curvature_comb(&pts);
        assert_eq!(comb.len(), pts.len());
        for t in &comb {
            close(t.curvature, 1.0 / 20.0, 1e-9);
            let out = normalize([t.at[0], t.at[1], 0.0]);
            close(dot(out, t.dir), 1.0, 1e-9);
        }
        // A tooth's tip with a scale of 200 mm per 1/mm: 10 mm out.
        let tip = comb[0].tip(200.0);
        close(tip[0], 30.0, 1e-9);
    }

    #[test]
    fn a_straight_run_has_no_teeth_and_an_arc_has_its_ends() {
        let line = [[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
        assert!(curvature_comb(&line).iter().all(|t| t.curvature == 0.0));
        // An open quarter arc of radius 10: its ends read like the middle.
        let arc: Vec<Vec3> = (0..=9)
            .map(|k| {
                let t = std::f64::consts::FRAC_PI_2 * k as f64 / 9.0;
                [10.0 * t.cos(), 10.0 * t.sin(), 0.0]
            })
            .collect();
        let comb = curvature_comb(&arc);
        for t in &comb {
            close(t.curvature, 0.1, 1e-9);
        }
    }
}
