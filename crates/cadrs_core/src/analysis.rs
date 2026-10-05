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
//! - **Zebra stripes** ([`zebra_phase`]): the view ray reflected off the surface, its angle out
//!   of a plane across a slanted screen axis cut into bands. The stripes depend on the reflected
//!   direction only, so a flat face under an orthographic view is one shade (straight bands in
//!   perspective, where the ray changes across it) and a curved face shows the bands, which
//!   kink where faces meet without curvature continuity.
//! - **Curvature** ([`vertex_mean_curvature`], [`curvature_band`]): the faces coloured by their
//!   mean curvature (the mean of the two principal curvatures, per mm: 1/R on a sphere of
//!   radius R, 1/(2R) on a cylinder, 0 on a plane), in six bands from flat (blue) to the most
//!   curved shown (red), with a legend.
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

/// The tolerance of the draft bands' edges (degrees): a face drafted exactly by the required
/// angle passes, and a vertical wall (|draft| below it, either sign) is in the 0…a band whatever
/// its tessellation's noise. `part_shading.wgsl` gets it as a uniform.
pub const DRAFT_EPS: f64 = 1e-4;

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
        let eps = DRAFT_EPS;
        if draft >= 2.0 * a - eps {
            DraftBand::PositiveSteep
        } else if draft >= a - eps {
            DraftBand::Positive
        } else if draft > -eps {
            // Zero draft within the tolerance either side (a vertical wall) is here.
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

/// The screen axis (right, up, back) the zebra stripes run across: slanted so a vertical and a
/// horizontal cylinder both show bands.
pub const ZEBRA_AXIS: Vec3 = [0.5, 0.85, 0.0];

/// The number of zebra bands over half a turn of the reflected ray.
pub const ZEBRA_BANDS: f64 = 10.0;

/// The direction `d` reflected off a surface with unit normal `n`.
pub fn reflect(d: Vec3, n: Vec3) -> Vec3 {
    sub(d, scale(n, 2.0 * dot(d, n)))
}

/// Where the reflected view ray `reflected` (in the view's frame: x right, y up, z back) falls
/// in the zebra pattern: 0…1 across one band pair, the white band where
/// [`zebra_white`]. `part_shading.wgsl` does the same per pixel.
pub fn zebra_phase(reflected: Vec3, bands: f64) -> f64 {
    let phi = dot(normalize(reflected), normalize(ZEBRA_AXIS)).clamp(-1.0, 1.0).asin();
    (phi * bands / std::f64::consts::PI).rem_euclid(1.0)
}

/// [`zebra_phase`] at a point: the view's frame `view` (its right, up and back axes, world),
/// the eye's position `eye`, the point `point` and the surface's unit normal `n` there. An
/// orthographic view looks along −back everywhere; a perspective one along the ray from the
/// eye. `part_shading.wgsl` does exactly this per pixel.
pub fn zebra_phase_at(view: [Vec3; 3], eye: Vec3, point: Vec3, n: Vec3, ortho: bool) -> f64 {
    let [right, up, back] = view;
    let e = if ortho { back } else { normalize(sub(eye, point)) };
    let r = reflect(scale(e, -1.0), n);
    zebra_phase([dot(r, right), dot(r, up), dot(r, back)], ZEBRA_BANDS)
}

/// The white half of a band pair.
pub fn zebra_white(phase: f64) -> bool {
    (phase - 0.5).abs() * 2.0 > 0.5
}

/// The mean curvature (per mm, ≥ 0: its size) at each vertex of a triangle mesh with vertex
/// normals: the normal curvature towards each neighbour, `(n_j − n_i)·(p_j − p_i) / |p_j − p_i|²`
/// (exact for a sphere and along a circle), fitted with the second fundamental form in the
/// vertex's tangent plane (least squares), whose trace / 2 is the mean curvature. With too few
/// neighbours to fit, their mean. A face's vertices are its own (the kernel's tessellation), so
/// a face's edges don't mix in its neighbours' normals.
pub fn vertex_mean_curvature(positions: &[Vec3], normals: &[Vec3], indices: &[u32]) -> Vec<f64> {
    let n = positions.len().min(normals.len());
    let mut nb: Vec<Vec<u32>> = vec![Vec::new(); n];
    for t in indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if (a as usize) < n && (b as usize) < n && a != b {
                if !nb[a as usize].contains(&b) {
                    nb[a as usize].push(b);
                }
                if !nb[b as usize].contains(&a) {
                    nb[b as usize].push(a);
                }
            }
        }
    }
    (0..n)
        .map(|i| {
            let ni = normalize(normals[i]);
            // A tangent basis.
            let helper = if ni[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let u = normalize(cross(ni, helper));
            let v = cross(ni, u);
            let mut ata = [[0.0f64; 3]; 3];
            let mut atb = [0.0f64; 3];
            let (mut sum, mut count) = (0.0, 0usize);
            for &j in &nb[i] {
                let d = sub(positions[j as usize], positions[i]);
                let dd = dot(d, d);
                if dd < 1e-18 {
                    continue;
                }
                let k = dot(sub(normalize(normals[j as usize]), ni), d) / dd;
                let (du, dv) = (dot(d, u), dot(d, v));
                let l = (du * du + dv * dv).sqrt();
                if l < 1e-12 {
                    continue;
                }
                let (x, y) = (du / l, dv / l);
                let row = [x * x, 2.0 * x * y, y * y];
                for r in 0..3 {
                    for c in 0..3 {
                        ata[r][c] += row[r] * row[c];
                    }
                    atb[r] += row[r] * k;
                }
                sum += k;
                count += 1;
            }
            if count == 0 {
                return 0.0;
            }
            let h = match solve3(ata, atb) {
                Some([a, _, c]) if count >= 3 => 0.5 * (a + c),
                _ => sum / count as f64,
            };
            h.abs()
        })
        .collect()
}

/// Solves a 3×3 linear system (Cramer's rule); `None` when it is (nearly) singular.
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d = det(m);
    let scale = m.iter().flatten().fold(0.0f64, |a, x| a.max(x.abs()));
    if d.abs() <= 1e-9 * scale.powi(3).max(1e-300) {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut mk = m;
        for r in 0..3 {
            mk[r][k] = b[r];
        }
        *o = det(mk) / d;
    }
    Some(out)
}

/// The number of curvature bands.
pub const CURVATURE_BANDS: usize = 6;

/// A mean curvature at most this (per mm: a radius of 10 m) is flat.
pub const CURVATURE_FLAT: f64 = 1e-4;

/// The band (0 the most curved … 5 flat) of a mean curvature `k` when the most curved shown is
/// `max`: flat on its own, the rest in five equal steps up to `max`. `part_shading.wgsl` does
/// the same per pixel.
pub fn curvature_band(k: f64, max: f64) -> usize {
    let k = k.abs();
    if k <= CURVATURE_FLAT || max <= CURVATURE_FLAT {
        return CURVATURE_BANDS - 1;
    }
    let steps = (CURVATURE_BANDS - 1) as f64;
    let i = ((k / max) * steps).floor().clamp(0.0, steps - 1.0) as usize;
    CURVATURE_BANDS - 2 - i
}

/// A curvature band's range (per mm, low to high) when the most curved shown is `max`.
pub fn curvature_band_range(band: usize, max: f64) -> (f64, f64) {
    if band >= CURVATURE_BANDS - 1 {
        return (0.0, 0.0);
    }
    let step = max / (CURVATURE_BANDS - 1) as f64;
    let i = (CURVATURE_BANDS - 2 - band) as f64;
    (i * step, (i + 1.0) * step)
}

/// A curvature band's legend text: "0.033 to 0.042 /mm", and "0 (flat)" for the last.
pub fn curvature_band_label(band: usize, max: f64) -> String {
    if band >= CURVATURE_BANDS - 1 {
        return "0 (flat)".into();
    }
    let (lo, hi) = curvature_band_range(band, max);
    format!("{lo:.3} to {hi:.3} /mm")
}

/// The curvature bands' colours (sRGB), most curved first: red through yellow and green to
/// blue for flat.
pub const CURVATURE_COLORS: [[u8; 3]; CURVATURE_BANDS] = [[0xd7, 0x30, 0x27], [0xf4, 0x8c, 0x2c], [0xf2, 0xd4, 0x3a], [0x7c, 0xc4, 0x5a], [0x3a, 0xa8, 0xb8], [0x4a, 0x72, 0xc8]];

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
    fn a_cylinder_runs_through_the_zebra_bands() {
        let view = [0.0, 0.0, -1.0];
        // A cylinder standing up (its normal turning about the up axis over the half facing the
        // viewer) runs through several bands.
        let mut changes = 0;
        let mut last = None;
        for k in 0..=180 {
            let t = (k as f64 - 90.0).to_radians();
            let white = zebra_white(zebra_phase(reflect(view, [t.sin(), 0.0, t.cos()]), ZEBRA_BANDS));
            if last.is_some_and(|l| l != white) {
                changes += 1;
            }
            last = Some(white);
        }
        assert!(changes >= 6, "{changes}");
        // Head-on, the reflected ray is the axis-free back direction: the first band's middle.
        close(zebra_phase(reflect(view, [0.0, 0.0, 1.0]), ZEBRA_BANDS), 0.0, 1e-12);
    }

    #[test]
    fn zebra_at_points_of_a_plane_is_constant_for_ortho_and_varies_in_perspective() {
        // A view looking down on the Top plane obliquely (right +X, up tipped, back towards the
        // eye), the eye 300 mm away from the origin along back.
        let back = normalize([0.0, -1.0, 1.0]);
        let right = [1.0, 0.0, 0.0];
        let up = cross(back, right);
        let view = [right, up, back];
        let eye = scale(back, 300.0);
        let n = [0.0, 0.0, 1.0];
        let points = [[0.0, 0.0, 0.0], [40.0, 0.0, 0.0], [0.0, 60.0, 0.0], [-50.0, -30.0, 0.0], [80.0, 80.0, 0.0]];
        let ortho: Vec<f64> = points.iter().map(|p| zebra_phase_at(view, eye, *p, n, true)).collect();
        for p in &ortho {
            close(*p, ortho[0], 1e-12);
        }
        // The orthographic phase is that of the view's back direction reflected, in the view's
        // frame.
        let r = reflect(scale(back, -1.0), n);
        close(ortho[0], zebra_phase([dot(r, right), dot(r, up), dot(r, back)], ZEBRA_BANDS), 1e-12);
        let persp: Vec<f64> = points.iter().map(|p| zebra_phase_at(view, eye, *p, n, false)).collect();
        let spread = persp.iter().fold(0.0f64, |m, p| m.max((p - persp[0]).abs()));
        assert!(spread > 0.05, "perspective phases vary over the plane: {persp:?}");
        // At the point straight below the eye's ray through the origin, both agree.
        close(persp[0], ortho[0], 1e-9);
    }

    /// A UV sphere of radius `r`: positions, outward normals and triangles.
    fn sphere(r: f64, rings: usize, segs: usize) -> (Vec<Vec3>, Vec<Vec3>, Vec<u32>) {
        let (mut p, mut nn, mut idx) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..=rings {
            let th = std::f64::consts::PI * i as f64 / rings as f64;
            for j in 0..segs {
                let ph = std::f64::consts::TAU * j as f64 / segs as f64;
                let d = [th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()];
                p.push(scale(d, r));
                nn.push(d);
            }
        }
        for i in 0..rings {
            for j in 0..segs {
                let a = (i * segs + j) as u32;
                let b = (i * segs + (j + 1) % segs) as u32;
                let (c, d) = (a + segs as u32, b + segs as u32);
                idx.extend([a, c, b, b, c, d]);
            }
        }
        (p, nn, idx)
    }

    #[test]
    fn a_sphere_reads_one_over_r_and_a_plane_zero() {
        let r = 25.0;
        let (p, n, idx) = sphere(r, 24, 48);
        let h = vertex_mean_curvature(&p, &n, &idx);
        // Away from the poles (whose duplicated vertices have no width).
        for k in h.iter().skip(48).take(p.len() - 96) {
            close(*k, 1.0 / r, 1e-9);
        }
        // A plane, as a grid.
        let mut pp = Vec::new();
        let mut idx = Vec::new();
        for y in 0..5 {
            for x in 0..5 {
                pp.push([x as f64 * 10.0, y as f64 * 7.0, 3.0]);
            }
        }
        for y in 0..4u32 {
            for x in 0..4u32 {
                let a = y * 5 + x;
                idx.extend([a, a + 1, a + 5, a + 1, a + 6, a + 5]);
            }
        }
        let nn = vec![[0.0, 0.0, 1.0]; pp.len()];
        assert!(vertex_mean_curvature(&pp, &nn, &idx).iter().all(|k| k.abs() < 1e-12));
        // A cylinder of radius 10: the mean of 1/10 and 0.
        let (mut cp, mut cn, mut ci) = (Vec::new(), Vec::new(), Vec::new());
        let segs = 36;
        for z in 0..4 {
            for j in 0..segs {
                let t = std::f64::consts::TAU * j as f64 / segs as f64;
                cp.push([10.0 * t.cos(), 10.0 * t.sin(), z as f64 * 4.0]);
                cn.push([t.cos(), t.sin(), 0.0]);
            }
        }
        for z in 0..3u32 {
            for j in 0..segs as u32 {
                let a = z * segs as u32 + j;
                let b = z * segs as u32 + (j + 1) % segs as u32;
                ci.extend([a, b, a + segs as u32, b, b + segs as u32, a + segs as u32]);
            }
        }
        let hc = vertex_mean_curvature(&cp, &cn, &ci);
        for k in &hc[segs..2 * segs] {
            close(*k, 0.05, 1e-3);
        }
        // The bands: flat on its own, the most curved in band 0.
        assert_eq!(curvature_band(0.0, 0.1), CURVATURE_BANDS - 1);
        assert_eq!(curvature_band(0.1, 0.1), 0);
        assert_eq!(curvature_band(0.05, 0.1), 2);
        assert_eq!(curvature_band(0.001, 0.1), 4);
        assert_eq!(curvature_band_label(0, 0.1), "0.080 to 0.100 /mm");
        assert_eq!(curvature_band_label(5, 0.1), "0 (flat)");
    }

    #[test]
    fn a_vertical_wall_is_in_the_zero_to_a_band_either_side_of_zero() {
        for d in [0.0, 1e-6, -1e-6, -5e-5, 5e-5] {
            assert_eq!(DraftBand::of(d, 3.0), DraftBand::InsufficientPositive, "{d}");
        }
        assert_eq!(DraftBand::of(-2.0 * DRAFT_EPS, 3.0), DraftBand::InsufficientNegative);
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
