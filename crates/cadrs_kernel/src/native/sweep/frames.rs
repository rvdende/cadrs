//! Sweeps along general path curves (B-splines, ellipses, arcs the profile isn't square to,
//! curves out of a plane) with sampled frames: the profile is carried along the segment by
//! rotation-minimizing frames (double reflection; or no rotation, or a locked direction), each
//! profile piece is placed at sections along the segment and the sections are skinned (cubic
//! interpolation of the homogeneous poles, so conics stay exact in each section). The rails
//! (the paths of the profile's corners) are interpolated with the same parameters and degree,
//! so they are exactly the side surfaces' boundary iso-curves. The generic sweep builder
//! assembles the faces and caps.

use super::path::Seg;
use super::{Frames, SideGeo, SweepKind};
use crate::native::build::Built;
use crate::native::curve::Curve;
use crate::native::math::*;
use crate::native::nurbs::{H, NurbsCurve, NurbsSurface};
use crate::native::profile::{MergedFace, Piece};
use crate::native::surface::Surface;
use crate::native::tol;
use crate::*;
use std::sync::Arc;

/// Rotation-minimizing steps per segment (the frames between them are interpolated).
const FINE: usize = 256;

/// The most sections a run is skinned through.
const MAX_SECTIONS: usize = 1024;

/// How far (mm, per mm of the profile's reach from the path, plus one) the skinned sections may
/// stray from the true sweep between them: the gap a loop may have ([`crate::native::tol::LOOP_GAP`]).
const SECTION_TOL: f64 = crate::native::tol::LOOP_GAP;

/// The most a path's curvature may vary along it (max / min) for the profile to follow its
/// principal normal (Frenet frames, for threads); else frames are rotation-minimizing.
const FRENET_SPREAD: f64 = 2.0;

/// A sweep along a run of path segments meeting tangentially; s ∈ [0, 1] runs along them by
/// length.
pub struct PathSweep {
    plane: Plane,
    segs: Vec<Seg>,
    /// The segments' starts in s (and 1 at the end).
    cum: Vec<f64>,
    /// Each segment's length from its start at travel fractions i / [`ARC_STEPS`]: s runs along
    /// the path by length, so the sections' motion has no jump in speed where segments meet.
    arc: Vec<ArcTable>,
    /// The path point and frame rotation (relative to the start frame) at the fine steps.
    pts: Vec<P3>,
    rots: Vec<nalgebra::UnitQuaternion<f64>>,
    /// How the frame between the fine steps is found: exactly at s (a locked direction, or one
    /// more double-reflection step from the fine step before s), else by interpolating `rots`.
    exact: Option<ExactFrames>,
    /// The sections' parameters.
    v: Vec<f64>,
    deg: usize,
    /// How far the profile reaches from the path (for the section count).
    reach: f64,
    /// The helix the path follows (within its approximation), swept by its exact screw motion.
    helix: Option<Helix>,
    /// The helix's angle at the fine steps (unwrapped along the path).
    angles: Vec<f64>,
    /// How the profile twists and scales along the path (in proportion to s).
    law: SweepLaw,
}

/// Frames found exactly at any s, so the profile's motion is smooth between the fine steps (a
/// rotation interpolated between them turns at a rate that jumps at each step: the rails then
/// have kinks no cubic through sections finer than the steps follows).
struct ExactFrames {
    /// The locked direction, or None for rotation-minimizing frames.
    lock: Option<V3>,
    /// The reference vectors and tangents at the fine steps (rotation-minimizing).
    refs: Vec<V3>,
    tans: Vec<V3>,
    /// The start frame's inverse.
    start: nalgebra::Rotation3<f64>,
}

impl ExactFrames {
    /// The frame at s (point `p` of the path there, after fine step `i`) relative to the start's.
    fn frame(
        &self,
        sweep: &PathSweep,
        i: usize,
        p: &P3,
        s: f64,
    ) -> Option<nalgebra::Rotation3<f64>> {
        let t = sweep.dir(s);
        let r = match self.lock {
            Some(d) => d - t * d.dot(&t),
            // One double-reflection step (Wang, Jüttler, Zheng & Liu 2008, Table 1) from the fine
            // step before s: it reaches the next step's frame exactly at it, and for a planar path
            // it is exact everywhere.
            None => {
                let (r0, t0) = (self.refs[i], self.tans[i]);
                let v1 = p - sweep.pts[i];
                let c1 = v1.norm_squared();
                if c1 < 1e-24 {
                    r0
                } else {
                    let rl = r0 - v1 * (2.0 / c1) * v1.dot(&r0);
                    let tl = t0 - v1 * (2.0 / c1) * v1.dot(&t0);
                    let v2 = t - tl;
                    let c2 = v2.norm_squared();
                    if c2 < 1e-24 {
                        rl
                    } else {
                        rl - v2 * (2.0 / c2) * v2.dot(&rl)
                    }
                }
            }
        };
        (r.norm() > tol::ANGULAR).then(|| frame_of(&t, &r) * self.start)
    }
}

/// A helix: axis through `o` along unit `a`, radius `r`, rising `c` per radian, `e1`/`e2`
/// square to `a`; angle `th0` where the path starts.
#[derive(Debug, Clone, Copy)]
pub struct Helix {
    o: P3,
    a: V3,
    e1: V3,
    e2: V3,
    r: f64,
    c: f64,
    th0: f64,
}

impl Helix {
    /// The helix about the axis through `o` along the unit `a`, of radius `r` from it, its point
    /// at angle θ (from `e1` toward `e2`, square to `a` and each other) rising `c` θ along `a`,
    /// starting at θ = `th0`.
    pub fn new(o: P3, a: V3, e1: V3, e2: V3, r: f64, c: f64, th0: f64) -> Self {
        Helix {
            o,
            a,
            e1,
            e2,
            r,
            c,
            th0,
        }
    }

    fn point(&self, th: f64) -> P3 {
        self.o + (self.e1 * th.cos() + self.e2 * th.sin()) * self.r + self.a * (self.c * th)
    }
    fn tangent(&self, th: f64) -> V3 {
        ((-self.e1 * th.sin() + self.e2 * th.cos()) * self.r + self.a * self.c).normalize()
    }
    /// The angle of a point near the helix, unwrapped next to `near`.
    fn angle(&self, p: &P3, near: f64) -> f64 {
        let w = p - self.o;
        let a = w.dot(&self.e2).atan2(w.dot(&self.e1));
        a + ((near - a) / TAU).round() * TAU
    }
    /// The angle of a point on the helix, its turn told by its height (when the helix rises).
    fn angle_of(&self, p: &P3, near: f64) -> f64 {
        let w = p - self.o;
        let a = w.dot(&self.e2).atan2(w.dot(&self.e1));
        let guess = if self.c.abs() > tol::DEGENERATE {
            w.dot(&self.a) / self.c
        } else {
            near
        };
        a + ((guess - a) / TAU).round() * TAU
    }
    /// The screw motion that turns the helix by `phi` along itself.
    fn screw(&self, phi: f64) -> Affine {
        let m = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(self.a), phi)
            .into_inner();
        Affine {
            m,
            t: self.o.coords - m * self.o.coords + self.a * (self.c * phi),
        }
    }
}

/// The helix a chain of path segments follows (within its approximation), if any.
pub fn fit_helix_segs(segs: &[Seg]) -> Option<Helix> {
    let mut pts = Vec::new();
    let mut tans = Vec::new();
    for g in segs {
        for i in 0..64 {
            let f = i as f64 / 64.0;
            pts.push(g.c.point(travel_t(g, f)));
            tans.push(g.dir(f));
        }
    }
    let last = segs.last()?;
    pts.push(last.end());
    tans.push(last.dir(1.0));
    let size = pts.iter().map(|p| (p - pts[0]).norm()).fold(0.0, f64::max);
    // The segments' ends: a Bézier approximation of a helix passes through it there and strays
    // between (a rational arc's height isn't linear in its angle), so its rise is fitted there.
    let nodes: Vec<usize> = (0..=segs.len()).map(|k| k * 64).collect();
    let h = fit_helix(&pts, &tans, Some(&nodes), 1e-4 * (1.0 + size));
    if std::env::var("CADRS_SWEEP_DEBUG").is_ok() {
        eprintln!("helix fit over {} segments: {h:?}", segs.len());
    }
    h
}

/// The eigenvalues of a symmetric 3 × 3 matrix, least first, in closed form (Smith's: the
/// matrix shifted by its mean eigenvalue and scaled, whose determinant gives the angle).
fn symmetric_eigenvalues(m: &nalgebra::Matrix3<f64>) -> [f64; 3] {
    let p1 = m[(0, 1)].powi(2) + m[(0, 2)].powi(2) + m[(1, 2)].powi(2);
    let q = m.trace() / 3.0;
    let p2 = (m[(0, 0)] - q).powi(2) + (m[(1, 1)] - q).powi(2) + (m[(2, 2)] - q).powi(2) + 2.0 * p1;
    if !(p2 > 0.0) {
        return [q; 3];
    }
    let p = (p2 / 6.0).sqrt();
    let b = (m - nalgebra::Matrix3::identity() * q) / p;
    let r = (b.determinant() / 2.0).clamp(-1.0, 1.0);
    let phi = r.acos() / 3.0;
    let hi = q + 2.0 * p * phi.cos();
    let lo = q + 2.0 * p * (phi + 2.0 * PI / 3.0).cos();
    [lo, 3.0 * q - hi - lo, hi]
}

/// The helix through points of a path (with their travel fractions), when they all lie on one
/// within `tol` (an approximated helix: Bézier pieces through points of it). Its rise is fitted
/// to the points `nodes` names, where the approximation meets the helix, else to them all.
fn fit_helix(pts: &[P3], tans: &[V3], nodes: Option<&[usize]>, tol: f64) -> Option<Helix> {
    let n = pts.len();
    if n < 8 {
        return None;
    }
    // Each point weighs the stretch of path about it (half at the two ends, as the trapezoid
    // rule weighs them): a run of whole turns, sampled with both ends, otherwise counts its first
    // angle twice and tilts the axis it finds (10⁻⁵ over a turn, 10⁻⁴ of radius at its top).
    let w = |i: usize| if i == 0 || i + 1 == n { 0.5 } else { 1.0 };
    let total: f64 = (0..n).map(w).sum();
    // The axis: the tangents make one angle with it, so it is square to their differences.
    let mean = tans
        .iter()
        .enumerate()
        .fold(V3::zeros(), |acc, (i, t)| acc + t * w(i))
        / total;
    let mut m = nalgebra::Matrix3::zeros();
    for (i, t) in tans.iter().enumerate() {
        let d = t - mean;
        m += d * d.transpose() * w(i);
    }
    // Its eigenvalues in closed form (not an iteration, whose result moved with the build: an
    // axis 10⁻⁸ of a turn off one build, and a helix missed the next), and the axis the
    // eigenvector of the least: M − λI has rank 2, and its null vector is the largest cross
    // product of two of its rows, polished by a step of inverse iteration.
    let eigen = symmetric_eigenvalues(&m);
    let lam = eigen[0];
    let shifted = m - nalgebra::Matrix3::identity() * lam;
    let rows = [
        shifted.row(0).transpose(),
        shifted.row(1).transpose(),
        shifted.row(2).transpose(),
    ];
    let null = [
        rows[0].cross(&rows[1]),
        rows[1].cross(&rows[2]),
        rows[2].cross(&rows[0]),
    ]
    .into_iter()
    .max_by(|x, y| x.norm_squared().total_cmp(&y.norm_squared()))
    .unwrap();
    let mut a = null.try_normalize(tol::TINY).map(|n| {
        // (Shifted a hair off λ, so the solve is regular; the null direction dominates.)
        let off = (m - nalgebra::Matrix3::identity() * (lam - 1e-9 * (1.0 + m.norm())))
            .lu()
            .solve(&n);
        off.and_then(|x| x.try_normalize(tol::TINY)).unwrap_or(n)
    })?;
    // The tangents must turn round it (not a straight line or a plane curve's single direction).
    if eigen
        .iter()
        .filter(|e| **e > tol::REL_POINT * n as f64)
        .count()
        < 2
    {
        return None;
    }
    if mean.dot(&a) < 0.0 {
        a = -a;
    }
    // The circle of the points projected square to the axis.
    let fr = Frame::from_z(P3::origin(), a);
    let mut mm = nalgebra::Matrix3::zeros();
    let mut rhs = V3::zeros();
    for (i, p) in pts.iter().enumerate() {
        let (x, y) = (p.coords.dot(&fr.x), p.coords.dot(&fr.y));
        let row = V3::new(x, y, 1.0);
        mm += row * row.transpose() * w(i);
        rhs += row * (x * x + y * y) * w(i);
    }
    let sol = mm.lu().solve(&rhs)?;
    let (cx, cy) = (sol.x / 2.0, sol.y / 2.0);
    let r = (sol.z + cx * cx + cy * cy).sqrt();
    if !(r.is_finite() && r > tol::ROUNDOFF) {
        return None;
    }
    let o0 = P3::from(fr.x * cx + fr.y * cy);
    let h0 = Helix {
        o: o0,
        a,
        e1: fr.x,
        e2: fr.y,
        r,
        c: 0.0,
        th0: 0.0,
    };
    // Angles (unwrapped) and heights: z = c θ + z0 by least squares.
    let mut ths = Vec::with_capacity(n);
    let mut prev = h0.angle(&pts[0], 0.0);
    for p in pts {
        prev = h0.angle(p, prev);
        ths.push(prev);
    }
    let zs: Vec<f64> = pts.iter().map(|p| (p - o0).dot(&a)).collect();
    let fit: Vec<(usize, f64)> = match nodes {
        Some(k) if k.len() >= 2 && k.iter().all(|&i| i < n) => {
            k.iter().map(|&i| (i, 1.0)).collect()
        }
        _ => (0..n).map(|i| (i, w(i))).collect(),
    };
    let wsum: f64 = fit.iter().map(|f| f.1).sum();
    let (st, sz) = (
        fit.iter().map(|&(i, wi)| ths[i] * wi).sum::<f64>() / wsum,
        fit.iter().map(|&(i, wi)| zs[i] * wi).sum::<f64>() / wsum,
    );
    let (mut num, mut den) = (0.0, 0.0);
    for &(i, wi) in &fit {
        let (t, z) = (ths[i], zs[i]);
        num += (t - st) * (z - sz) * wi;
        den += (t - st) * (t - st) * wi;
    }
    if den < tol::DEGENERATE {
        return None;
    }
    let c = num / den;
    let z0 = sz - c * st;
    let o = o0 + a * z0;
    let h = Helix {
        o,
        a,
        e1: fr.x,
        e2: fr.y,
        r,
        c,
        th0: ths[0],
    };
    for (p, t) in pts.iter().zip(&ths) {
        if (h.point(*t) - p).norm() > tol {
            return None;
        }
    }
    Some(h)
}

fn frame_of(t: &V3, r: &V3) -> nalgebra::Rotation3<f64> {
    let t = t.normalize();
    let r = (r - t * r.dot(&t)).normalize();
    nalgebra::Rotation3::from_matrix_unchecked(nalgebra::Matrix3::from_columns(&[
        t,
        r,
        t.cross(&r),
    ]))
}

/// Any unit vector square to `t`.
fn square_to(t: &V3) -> V3 {
    let a = if t.x.abs() < 0.9 { V3::x() } else { V3::y() };
    t.cross(&a).normalize()
}

impl PathSweep {
    pub fn new(
        plane: Plane,
        segs: &[Seg],
        frames: Frames,
        reach: f64,
        helix: Option<Helix>,
        law: SweepLaw,
    ) -> Result<Self> {
        let control = frames.control();
        let any_spread = frames == Frames::Frenet;
        let arc: Vec<ArcTable> = segs.iter().map(ArcTable::new).collect();
        let lens: Vec<f64> = arc.iter().map(ArcTable::total).collect();
        let total: f64 = lens.iter().sum();
        let mut at = 0.0;
        let mut cum: Vec<f64> = std::iter::once(0.0)
            .chain(lens.iter().map(|l| {
                at += l / total;
                at
            }))
            .collect();
        if let Some(end) = cum.last_mut() {
            *end = 1.0;
        }
        let fine = FINE * segs.len();
        let mut me = Self {
            plane,
            segs: segs.to_vec(),
            cum,
            arc,
            pts: vec![],
            rots: vec![],
            exact: None,
            v: vec![],
            deg: 3,
            reach,
            helix: None,
            angles: vec![],
            law,
        };
        let pts: Vec<P3> = (0..=fine).map(|i| me.at(i as f64 / fine as f64)).collect();
        let tans: Vec<V3> = (0..=fine).map(|i| me.dir(i as f64 / fine as f64)).collect();
        if tans.iter().any(|t| !t.iter().all(|x| x.is_finite())) {
            return Err(KernelError::InvalidParameter(
                "The sweep path has a point with no direction".into(),
            ));
        }
        let mut frames: Vec<nalgebra::Rotation3<f64>> = Vec::with_capacity(fine + 1);
        // A helix (within its Bézier approximation): the exact screw motion, of the helix the
        // caller fitted to the whole chain (so neighbouring pieces share it), else this run's.
        if control == SweepControl::None {
            me.helix = match helix {
                Some(h) => {
                    let th0 = h.angle_of(&pts[0], h.th0);
                    if std::env::var("CADRS_SWEEP_DEBUG").is_ok() {
                        let end = &pts[fine];
                        eprintln!(
                            "run of {} segments: th0 {th0} (end angle {}), start {:?}",
                            segs.len(),
                            h.angle_of(end, th0),
                            pts[0]
                        );
                    }
                    Some(Helix { th0, ..h })
                }
                None => {
                    let size = pts.iter().map(|p| (p - pts[0]).norm()).fold(0.0, f64::max);
                    fit_helix(&pts, &tans, None, 1e-4 * (1.0 + size))
                }
            };
        }
        if let Some(h) = me.helix {
            let mut th = h.th0;
            me.angles = (0..=fine)
                .map(|i| {
                    th = h.angle(&me.path_at(i as f64 / fine as f64), th);
                    th
                })
                .collect();
        }
        let (pts, tans) = match me.helix {
            Some(_) => {
                let p: Vec<P3> = (0..=fine).map(|i| me.at(i as f64 / fine as f64)).collect();
                let t: Vec<V3> = (0..=fine).map(|i| me.dir(i as f64 / fine as f64)).collect();
                (p, t)
            }
            None => (pts, tans),
        };
        match (control, me.helix) {
            (SweepControl::None, Some(h)) => {
                let mut prev = h.th0;
                for p in &pts {
                    prev = h.angle(p, prev);
                    frames.push(nalgebra::Rotation3::from_axis_angle(
                        &nalgebra::Unit::new_normalize(h.a),
                        prev - h.th0,
                    ));
                }
            }
            (SweepControl::KeepOrientation, _) => {}
            (SweepControl::LockDirection(d), _) => {
                me.exact = Some(ExactFrames {
                    lock: Some(d.into_inner()),
                    refs: Vec::new(),
                    tans: Vec::new(),
                    start: nalgebra::Rotation3::identity(),
                });
                for t in &tans {
                    let r = d.into_inner() - t * d.dot(t);
                    if r.norm() < tol::ANGULAR {
                        return Err(KernelError::InvalidParameter(
                            "The sweep path runs along the locked direction".into(),
                        ));
                    }
                    frames.push(frame_of(t, &r));
                }
            }
            (SweepControl::None, None) => match me.frenet(&tans, any_spread) {
                // A path that bends everywhere (a helix): its principal normal, so the profile
                // keeps its place relative to the curve (a thread stays on its cylinder).
                Some(normals) => {
                    for (t, n) in tans.iter().zip(normals) {
                        frames.push(frame_of(t, &n));
                    }
                }
                None => {
                    // Double reflection (Wang, Jüttler, Zheng & Liu 2008).
                    let mut r = square_to(&tans[0]);
                    frames.push(frame_of(&tans[0], &r));
                    for i in 0..fine {
                        let v1 = pts[i + 1] - pts[i];
                        let c1 = v1.norm_squared();
                        if c1 < 1e-24 {
                            frames.push(frame_of(&tans[i + 1], &r));
                            continue;
                        }
                        let rl = r - v1 * (2.0 / c1) * v1.dot(&r);
                        let tl = tans[i] - v1 * (2.0 / c1) * v1.dot(&tans[i]);
                        let v2 = tans[i + 1] - tl;
                        let c2 = v2.norm_squared();
                        r = if c2 < 1e-24 {
                            rl
                        } else {
                            rl - v2 * (2.0 / c2) * v2.dot(&rl)
                        };
                        frames.push(frame_of(&tans[i + 1], &r));
                    }
                    let refs = frames
                        .iter()
                        .map(|f| f.matrix().column(1).into_owned())
                        .collect();
                    me.exact = Some(ExactFrames {
                        lock: None,
                        refs,
                        tans: tans.clone(),
                        start: nalgebra::Rotation3::identity(),
                    });
                }
            },
        }
        let rots: Vec<nalgebra::UnitQuaternion<f64>> = if frames.is_empty() {
            vec![nalgebra::UnitQuaternion::identity(); fine + 1]
        } else {
            let f0 = frames[0].inverse();
            if let Some(x) = me.exact.as_mut() {
                x.start = f0;
            }
            frames
                .iter()
                .map(|f| nalgebra::UnitQuaternion::from_rotation_matrix(&(f * f0)))
                .collect()
        };
        // Sections spaced by the local bend (research/algorithms/offsets_sweeps_import.md card 1):
        // cubic interpolation of a rail of curvature κ (the path's, grown by the profile's reach)
        // through sections h apart strays about h⁴κ³/384 from it, so a stretch of the path needs
        // ds·(κ³/384·tol)^¼ sections to hold `tol`, and at least one per π/24 it turns or twists.
        // The sections are spread by that density (cumulated over the fine steps), not evenly: one
        // tight bend on a long run no longer sets the spacing of all of it.
        let reach = me.profile_reach();
        let tol = SECTION_TOL * (1.0 + reach);
        let mut density = Vec::with_capacity(fine + 1);
        density.push(0.0);
        for i in 0..fine {
            let ds = (pts[i + 1] - pts[i]).norm();
            let bend = tans[i].angle(&tans[i + 1]);
            let by_err = if ds > tol::ROUNDOFF_FINE {
                let k = bend / ds;
                let k_rail = k * (1.0 + reach * k);
                if k_rail > tol::ROUNDOFF_FINE {
                    ds / (384.0 * tol / k_rail.powi(3)).powf(0.25)
                } else {
                    0.0
                }
            } else {
                0.0
            };
            // (A law's twist turns the profile too, and its scale moves the profile's far
            // points as a turn of that many radians would.)
            let law_turn = (me.law.twist.abs() + (me.law.scale - 1.0).abs()) / fine as f64;
            let by_turn = (rots[i].angle_to(&rots[i + 1]) + bend + law_turn) / (PI / 24.0);
            density.push(density[i] + by_err.max(by_turn));
        }
        // Eight sections on top, spread evenly, so no stretch goes without (a straight one between
        // bends would otherwise be one interval long beside short ones).
        let even = 8.0;
        let total = density[fine] + even;
        let n = total.ceil().clamp(8.0, MAX_SECTIONS as f64) as usize;
        let mut v = Vec::with_capacity(n + 1);
        v.push(0.0);
        let mut i = 0;
        for k in 1..n {
            let target = total * k as f64 / n as f64;
            let at = |j: usize| density[j] + even * j as f64 / fine as f64;
            while i + 1 < fine && at(i + 1) < target {
                i += 1;
            }
            let (a, b) = (at(i), at(i + 1));
            let f = if b > a {
                ((target - a) / (b - a)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            v.push((i as f64 + f) / fine as f64);
        }
        v.push(1.0);
        me.pts = pts;
        me.rots = rots;
        // The estimate assumes rails parametrized by length; a path whose speed varies along its
        // parameter (a B-spline's) bends them in s more. Check the rails of the profile's reach
        // midway between the sections and add a section in each interval that strays more than
        // `tol` until they hold (each halving cuts the cubic's error about 16 times, 4 where the
        // curvature jumps; a pass that cuts the worst less than 2 times is following frames that
        // aren't smooth, where more sections only cost, so the refinement stops there and keeps
        // the sections before it). Neighbouring intervals differ at most twofold: the
        // interpolating cubic overshoots in a long interval beside short ones.
        graded(&mut v);
        let mut errs = me.rail_errors(&v);
        loop {
            let worst = errs.iter().copied().fold(0.0, f64::max);
            if worst <= tol {
                break;
            }
            let mut more = Vec::with_capacity(2 * v.len());
            more.push(v[0]);
            for (w, e) in v.windows(2).zip(&errs) {
                if *e > tol {
                    more.push(0.5 * (w[0] + w[1]));
                }
                more.push(w[1]);
            }
            graded(&mut more);
            if more.len() > MAX_SECTIONS + 1 {
                break;
            }
            let e = me.rail_errors(&more);
            if e.iter().copied().fold(0.0, f64::max) > worst / 2.0 {
                break;
            }
            (v, errs) = (more, e);
        }
        me.v = v;
        Ok(me)
    }

    /// How far the cubic interpolation through sections at `v` strays from the true sweep midway
    /// between each two of them: the path point's rail, and the rails of points `reach` from it
    /// in the profile's plane along the path (how far the profile tilts off square to the
    /// path; a turn within its plane only twists the sampled frames, which are approximate
    /// anyway).
    fn rail_errors(&self, v: &[f64]) -> Vec<f64> {
        let (x, y) = (
            self.plane.x_dir.into_inner(),
            self.plane.y_dir().into_inner(),
        );
        let p0 = self.pts[0];
        let r = self.reach.max(tol::ROUNDOFF);
        let deg = self.deg.min(v.len() - 1);
        let mut worst = vec![0.0f64; v.len() - 1];
        for (k, q) in [p0, p0 + x * r, p0 - x * r, p0 + y * r, p0 - y * r]
            .into_iter()
            .enumerate()
        {
            let at: Vec<P3> = v.iter().map(|&s| self.motion(s).point(&q)).collect();
            let rail = NurbsCurve::interpolate(&at, v, deg, None, None);
            for (w, e) in v.windows(2).zip(worst.iter_mut()) {
                let m = 0.5 * (w[0] + w[1]);
                let d = rail.point(m) - self.motion(m).point(&q);
                // (A law twists and scales the profile as it goes: its rails' every stray
                // counts, not only along the path.)
                *e = e.max(if k == 0 || !self.law.is_identity() {
                    d.norm()
                } else {
                    d.dot(&self.dir(m)).abs()
                });
            }
        }
        worst
    }

    /// How far the profile reaches from the path's start (an upper bound, from the profile
    /// plane's frame: set by the caller through `reach`).
    fn profile_reach(&self) -> f64 {
        self.reach
    }

    /// The principal normals at the fine steps when the path bends about as much everywhere
    /// along it (its curvature never drops near zero or, unless `any_spread`, varies more than
    /// [`FRENET_SPREAD`], and the normal turns smoothly), else None.
    fn frenet(&self, tans: &[V3], any_spread: bool) -> Option<Vec<V3>> {
        let fine = tans.len() - 1;
        let mut size = 0.0f64;
        for g in &self.segs {
            size = size.max((g.end() - g.start()).norm()).max(g.length() / TAU);
        }
        let mut out: Vec<V3> = Vec::with_capacity(fine + 1);
        let (mut kmin, mut kmax) = (f64::INFINITY, 0.0f64);
        for i in 0..=fine {
            let (k, f) = self.seg_at(i as f64 / fine as f64);
            let g = &self.segs[k];
            let (_, d1, d2) = g.c.d2(travel_t(g, f));
            let l2 = d1.norm_squared();
            if l2 < tol::TINY {
                return None;
            }
            let kn = (d2 - d1 * (d1.dot(&d2) / l2)) / l2;
            if kn.norm() * size.max(tol::ROUNDOFF) < 1e-3 {
                return None;
            }
            (kmin, kmax) = (kmin.min(kn.norm()), kmax.max(kn.norm()));
            let n = kn.normalize();
            if out.last().is_some_and(|m: &V3| m.dot(&n) < 0.5) {
                return None;
            }
            out.push(n);
        }
        // Only a path that bends about as much everywhere (a thread's): where the curvature
        // drops the principal normal spins, and the profile would twist with it.
        (any_spread || kmax <= FRENET_SPREAD * kmin).then_some(out)
    }

    /// The segment and its travel fraction at s (s by length along the run).
    fn seg_at(&self, s: f64) -> (usize, f64) {
        let s = s.clamp(0.0, 1.0);
        let k = self
            .cum
            .partition_point(|c| *c <= s)
            .saturating_sub(1)
            .min(self.segs.len() - 1);
        let (a, b) = (self.cum[k], self.cum[k + 1]);
        let frac = if b > a {
            ((s - a) / (b - a)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (k, self.arc[k].frac_at(frac * self.arc[k].total()))
    }
    fn path_at(&self, s: f64) -> P3 {
        let (k, f) = self.seg_at(s);
        self.segs[k].c.point(travel_t(&self.segs[k], f))
    }
    /// The helix's angle at s (from the path's point there).
    fn helix_angle(&self, h: &Helix, s: f64) -> f64 {
        // Unwrapped next to the angle at the fine step before s.
        let s = s.clamp(0.0, 1.0);
        let fine = self.angles.len().saturating_sub(1);
        if fine == 0 {
            return h.angle(&self.path_at(s), h.th0);
        }
        let i = ((s * fine as f64).floor() as usize).min(fine);
        h.angle(&self.path_at(s), self.angles[i])
    }
    fn at(&self, s: f64) -> P3 {
        match &self.helix {
            Some(h) => h.point(self.helix_angle(h, s)),
            None => self.path_at(s),
        }
    }
    fn dir(&self, s: f64) -> V3 {
        match &self.helix {
            Some(h) => {
                let (k, f) = self.seg_at(s);
                let t = h.tangent(self.helix_angle(h, s));
                // In the direction of travel.
                if t.dot(&self.segs[k].dir(f)) < 0.0 {
                    -t
                } else {
                    t
                }
            }
            None => {
                let (k, f) = self.seg_at(s);
                self.segs[k].dir(f)
            }
        }
    }

    /// The motion of the profile from the start of the segment to s: rigid, then twisted and
    /// scaled about the path's point by the law.
    fn motion(&self, s: f64) -> Affine {
        let a = self.rigid(s);
        if self.law.is_identity() {
            return a;
        }
        let p = a.point(&self.pts[0]);
        let s = s.clamp(0.0, 1.0);
        let k = 1.0 + (self.law.scale - 1.0) * s;
        let turn = nalgebra::Rotation3::from_axis_angle(
            &nalgebra::Unit::new_normalize(self.dir(s)),
            self.law.twist * s,
        )
        .into_inner();
        let kr = turn * k;
        Affine {
            m: kr * a.m,
            t: kr * a.t + p.coords - kr * p.coords,
        }
    }

    /// The rigid motion of the profile from the start of the segment to s.
    fn rigid(&self, s: f64) -> Affine {
        if let Some(h) = &self.helix {
            return h.screw(self.helix_angle(h, s) - h.th0);
        }
        let fine = self.pts.len() - 1;
        let x = s.clamp(0.0, 1.0) * fine as f64;
        let i = (x.floor() as usize).min(fine - 1);
        let f = x - i as f64;
        let p = self.at(s);
        let m = match self.exact.as_ref().and_then(|e| e.frame(self, i, &p, s)) {
            Some(r) => r.into_inner(),
            None => self.rots[i]
                .slerp(&self.rots[i + 1], f)
                .to_rotation_matrix()
                .into_inner(),
        };
        Affine {
            m,
            t: p.coords - m * self.pts[0].coords,
        }
    }

    /// The profile's plane where the segment ends.
    pub fn end_plane(&self) -> Plane {
        let m = self.motion(1.0);
        Plane {
            origin: m.point(&self.plane.origin),
            x_dir: nalgebra::Unit::new_normalize(m.vector(&self.plane.x_dir)),
            normal: nalgebra::Unit::new_normalize(m.vector(&self.plane.normal)),
        }
    }

    fn rail_nurbs(&self, q: P2) -> NurbsCurve {
        let pts: Vec<P3> = self
            .v
            .iter()
            .map(|&s| self.place(s).point(&P3::new(q.x, q.y, 0.0)))
            .collect();
        NurbsCurve::interpolate(&pts, &self.v, self.deg.min(pts.len() - 1), None, None)
    }

    /// The side surface of a piece: its NURBS placed at the sections, skinned.
    fn skin(&self, piece: &Piece) -> NurbsSurface {
        let n0 = piece.c.to_nurbs(piece.t0, piece.t1);
        let curves: Vec<NurbsCurve> = self
            .v
            .iter()
            .map(|&s| n0.transformed(&self.place(s)))
            .collect();
        let q = self.deg.min(curves.len() - 1);
        let ncp = n0.cps.len();
        let mut cols: Vec<Vec<H>> = Vec::with_capacity(ncp);
        let mut kv = Vec::new();
        for i in 0..ncp {
            let xyz: Vec<P3> = curves
                .iter()
                .map(|c| P3::new(c.cps[i].x, c.cps[i].y, c.cps[i].z))
                .collect();
            let a = NurbsCurve::interpolate(&xyz, &self.v, q, None, None);
            let w = n0.cps[i].w;
            kv = a.knots.clone();
            cols.push(
                a.cps
                    .iter()
                    .map(|p| H::new(p.x, p.y, p.z, p.w * w))
                    .collect(),
            );
        }
        NurbsSurface {
            du: n0.degree,
            dv: q,
            ku: n0.knots.clone(),
            kv,
            cps: cols,
        }
    }
}

/// Splits section intervals in halves until each is at most twice as long as its neighbours (or
/// there are [`MAX_SECTIONS`]).
fn graded(v: &mut Vec<f64>) {
    loop {
        let n = v.len() - 1;
        let h = |k: usize| v[k + 1] - v[k];
        let mut out = Vec::with_capacity(v.len() + 8);
        out.push(v[0]);
        for k in 0..n {
            let left = if k > 0 { h(k - 1) } else { f64::INFINITY };
            let right = if k + 1 < n { h(k + 1) } else { f64::INFINITY };
            if h(k) > 2.0 * left.min(right) {
                out.push(0.5 * (v[k] + v[k + 1]));
            }
            out.push(v[k + 1]);
        }
        let done = out.len() == v.len() || out.len() > MAX_SECTIONS + 1;
        if out.len() <= MAX_SECTIONS + 1 {
            *v = out;
        }
        if done {
            return;
        }
    }
}

/// Steps of a segment's length table.
const ARC_STEPS: usize = 64;

/// How fast a segment's point moves with its travel fraction.
fn speed(seg: &Seg, f: f64) -> f64 {
    seg.c.d2(travel_t(seg, f)).1.norm() * (seg.t1 - seg.t0)
}

/// A segment's length from its start to fraction `f1` from fraction `f0` (8-point Gauss).
fn length_between(seg: &Seg, f0: f64, f1: f64) -> f64 {
    let (m, r) = ((f0 + f1) * 0.5, (f1 - f0) * 0.5);
    GAUSS8
        .x
        .iter()
        .zip(GAUSS8.w)
        .map(|(x, w)| w * speed(seg, m + r * x))
        .sum::<f64>()
        * r
}

/// A segment's length table: its length from its start and its speed at travel fractions
/// i / [`ARC_STEPS`] (lengths by 8-point Gauss over each step), computed once per segment.
#[derive(Debug, Clone)]
struct ArcTable {
    len: Vec<f64>,
    speed: Vec<f64>,
}

impl ArcTable {
    fn new(seg: &Seg) -> Self {
        let mut len = vec![0.0];
        for i in 0..ARC_STEPS {
            let (f0, f1) = (
                i as f64 / ARC_STEPS as f64,
                (i + 1) as f64 / ARC_STEPS as f64,
            );
            len.push(len[i] + length_between(seg, f0, f1));
        }
        let speed = (0..=ARC_STEPS)
            .map(|i| speed(seg, i as f64 / ARC_STEPS as f64))
            .collect();
        Self { len, speed }
    }

    fn total(&self) -> f64 {
        self.len[ARC_STEPS]
    }

    /// The travel fraction where the segment has run `len`: the cubic Hermite interpolation of
    /// the fraction against length through the table (the fraction's slope is 1 / speed), so
    /// the motion stays smooth, O(h⁴) from the exact inverse and with no curve evaluations.
    fn frac_at(&self, len: f64) -> f64 {
        let i = self
            .len
            .partition_point(|l| *l <= len)
            .saturating_sub(1)
            .min(ARC_STEPS - 1);
        let (la, lb) = (self.len[i], self.len[i + 1]);
        let h = lb - la;
        let (fa, fb) = (
            i as f64 / ARC_STEPS as f64,
            (i + 1) as f64 / ARC_STEPS as f64,
        );
        if !(h > tol::TINY) {
            return fa;
        }
        let x = ((len - la) / h).clamp(0.0, 1.0);
        let slope = |v: f64| if v > tol::TINY { h / v } else { fb - fa };
        let (ma, mb) = (slope(self.speed[i]), slope(self.speed[i + 1]));
        let (x2, x3) = (x * x, x * x * x);
        let f = (2.0 * x3 - 3.0 * x2 + 1.0) * fa
            + (x3 - 2.0 * x2 + x) * ma
            + (-2.0 * x3 + 3.0 * x2) * fb
            + (x3 - x2) * mb;
        f.clamp(fa, fb)
    }
}

/// The curve parameter at travel fraction s of a segment.
fn travel_t(seg: &Seg, s: f64) -> f64 {
    if seg.rev {
        seg.t1 + (seg.t0 - seg.t1) * s
    } else {
        seg.t0 + (seg.t1 - seg.t0) * s
    }
}

impl SweepKind for PathSweep {
    fn place(&self, s: f64) -> Affine {
        let base = crate::native::build::plane_map(&self.plane, &V3::zeros(), 0.0);
        let m = self.motion(s);
        Affine {
            m: m.m * base.m,
            t: m.m * base.t + m.t,
        }
    }
    fn rail(&self, q: P2) -> Option<Curve> {
        Some(Curve::Nurbs(Arc::new(self.rail_nurbs(q))))
    }
    fn side(&self, piece: &Piece, _c3: &Curve) -> Option<SideGeo> {
        let sk = self.skin(piece);
        let (ua, ub) = (sk.ku[0], sk.ku[sk.ku.len() - 1]);
        // The surface's u runs from the piece's t0 to its t1.
        let n0 = piece.c.to_nurbs(piece.t0, piece.t1);
        let (ua, ub) = if (n0.point(n0.range().0) - piece.c.point(piece.t0)).norm()
            <= (n0.point(n0.range().0) - piece.c.point(piece.t1)).norm()
        {
            (ua, ub)
        } else {
            (ub, ua)
        };
        let linear = n0.degree == 1 && !n0.is_rational() && n0.cps.len() == 2;
        let surf = Surface::Nurbs(Arc::new(sk));
        let c = piece.c.clone();
        let (t0, t1) = (piece.t0, piece.t1);
        let n0c = n0.clone();
        Some(SideGeo {
            surf,
            along: Box::new(move |s| {
                let v = if s < 0.5 { 0.0 } else { 1.0 };
                if linear {
                    let k = (ub - ua) / (t1 - t0);
                    return super::line2(P2::new(ua - t0 * k, v), V2::new(k, 0.0));
                }
                iso_pcurve(&n0c, &c, t0, t1, ua, ub, v)
            }),
            across: Box::new(move |t| {
                let u = if (t - t0).abs() <= (t - t1).abs() {
                    ua
                } else {
                    ub
                };
                super::line2(P2::new(u, 0.0), V2::new(0.0, 1.0))
            }),
        })
    }
    fn up(&self) -> bool {
        // Which way the profile leaves its plane at the start.
        let d = self.pts[1] - self.pts[0];
        d.dot(&self.plane.normal) > 0.0
    }
}

/// The p-curve of a profile piece's iso-line v of its skin: u(t) the parameter of the piece's
/// own B-spline `n0` (the skin's u) at the piece's point c(t), u(t0) = ua and u(t1) = ub (a whole
/// circle's ends are its seam), interpolated until it maps onto the piece within 1e-12.
fn iso_pcurve(n0: &NurbsCurve, c: &Curve, t0: f64, t1: f64, ua: f64, ub: f64, v: f64) -> Curve {
    let invert = |t: f64, guess: f64| -> f64 {
        let p = c.point(t);
        let (lo, hi) = (ua.min(ub), ua.max(ub));
        let mut u = guess.clamp(lo, hi);
        for _ in 0..40 {
            let d = n0.ders(u, 2);
            let (q, d1, d2) = (P3::from(d[0]), d[1], d[2]);
            let r = q - p;
            let f = r.dot(&d1);
            let df = d1.norm_squared() + r.dot(&d2);
            if df.abs() < tol::TINY {
                break;
            }
            let step = f / df;
            u = (u - step).clamp(lo, hi);
            if step.abs() < 1e-15 * (1.0 + u.abs()) {
                break;
            }
        }
        u
    };
    // The samples (t, u): the ends given, the rest inverted from a guess (the chord's at first,
    // the fit's so far for those added), each found once.
    let mut ts: Vec<f64> = (0..=32).map(|i| t0 + (t1 - t0) * i as f64 / 32.0).collect();
    let n = ts.len();
    let mut us: Vec<f64> = ts
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            if i == 0 {
                ua
            } else if i + 1 == n {
                ub
            } else {
                invert(t, ua + (ub - ua) * (t - t0) / (t1 - t0))
            }
        })
        .collect();
    let fit_of = |ts: &[f64], us: &[f64]| {
        let pts: Vec<P3> = us.iter().map(|&u| P3::new(u, v, 0.0)).collect();
        NurbsCurve::interpolate(&pts, ts, 3, None, None)
    };
    for _ in 0..10 {
        let fit = fit_of(&ts, &us);
        let mut ins: Vec<(f64, f64)> = Vec::new();
        for k in 0..ts.len() - 1 {
            let tm = 0.5 * (ts[k] + ts[k + 1]);
            let q = fit.point(tm);
            if (n0.point(q.x) - c.point(tm)).norm()
                > tol::REL_FINE * (1.0 + c.point(tm).coords.norm())
            {
                ins.push((tm, invert(tm, q.x)));
            }
        }
        if ins.is_empty() || ts.len() > 4096 {
            return Curve::Nurbs(Arc::new(fit));
        }
        let mut both: Vec<(f64, f64)> = ts.iter().copied().zip(us.iter().copied()).collect();
        both.extend(ins);
        both.sort_by(|a, b| a.0.total_cmp(&b.0));
        ts = both.iter().map(|x| x.0).collect();
        us = both.iter().map(|x| x.1).collect();
    }
    let pts: Vec<P3> = us.iter().map(|&u| P3::new(u, v, 0.0)).collect();
    Curve::Nurbs(Arc::new(NurbsCurve::interpolate(&pts, &ts, 3, None, None)))
}

/// Sweeps the profile along a run of tangent-continuous segments; the profile's plane where it
/// ends.
pub fn sweep_seg(
    faces: &[MergedFace],
    chains: &[Vec<Piece>],
    plane: Plane,
    segs: &[Seg],
    frames: Frames,
    solid: bool,
    helix: Option<Helix>,
) -> Result<(Built, Plane)> {
    sweep_seg_with(
        faces,
        chains,
        plane,
        segs,
        frames,
        solid,
        helix,
        SweepLaw::default(),
    )
}

/// [`sweep_seg`] twisting and scaling the profile by `law` along the segments.
pub fn sweep_seg_law(
    faces: &[MergedFace],
    chains: &[Vec<Piece>],
    plane: Plane,
    segs: &[Seg],
    frames: Frames,
    solid: bool,
    law: SweepLaw,
) -> Result<(Built, Plane)> {
    sweep_seg_with(faces, chains, plane, segs, frames, solid, None, law)
}

#[allow(clippy::too_many_arguments)]
fn sweep_seg_with(
    faces: &[MergedFace],
    chains: &[Vec<Piece>],
    plane: Plane,
    segs: &[Seg],
    frames: Frames,
    solid: bool,
    helix: Option<Helix>,
    law: SweepLaw,
) -> Result<(Built, Plane)> {
    // The profile's reach from the path's start.
    let m = crate::native::build::plane_map(&plane, &V3::zeros(), 0.0);
    let start = segs[0].start();
    let mut reach: f64 = 0.0;
    for p in faces
        .iter()
        .flat_map(|f| std::iter::once(&f.outer).chain(f.holes.iter()))
        .flatten()
        .chain(chains.iter().flatten())
    {
        for k in 0..=4 {
            let q = p.at(k as f64 / 4.0);
            reach = reach.max((m.point(&P3::new(q.x, q.y, 0.0)) - start).norm());
        }
    }
    let t0 = web_time::Instant::now();
    let kind = PathSweep::new(plane, segs, frames, reach, helix, law)?;
    let t1 = t0.elapsed();
    if (kind.pts[1] - kind.pts[0]).dot(&plane.normal).abs()
        < tol::REL * (kind.pts[1] - kind.pts[0]).norm().max(tol::TINY)
    {
        return Err(KernelError::InvalidParameter(
            "The sweep path runs in the profile's plane".into(),
        ));
    }
    let mut b = super::sweep_full(faces, chains, &kind, 0.0, 1.0, false, solid)?;
    let t2 = t0.elapsed();
    // Sides that came out flat or on a cylinder (a profile edge along a helix's axis) as those
    // surfaces, so booleans see them coincide with faces of the same surface.
    crate::native::unify::planarize(&mut b.body);
    crate::native::unify::cylindrize(&mut b.body);
    if std::env::var("CADRS_PROFILE").is_ok() {
        eprintln!(
            "[profile] sweep piece: frames {t1:?}, faces {:?}, recognise {:?} ({} sections)",
            t2 - t1,
            t0.elapsed() - t2,
            kind.v.len()
        );
    }
    Ok((b, kind.end_plane()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    /// A helix drawn as Helix features draw one (rational quadratic arcs, the height rising
    /// with each arc's parameter, so it strays from the helix between their ends), one whole turn
    /// from its start: the helix fitted to it is the true one, axis, radius and rise
    /// (brep-bench's twisted-thin-blade, swept by a fit 10⁻⁵ off, came out 3·10⁻⁶ too big).
    #[test]
    fn a_turn_of_helix_arcs_fits_the_true_helix() {
        use std::f64::consts::TAU;
        let (r, pitch, start) = (20.0f64.hypot(0.2), 60.0, 0.2f64.atan2(20.0));
        let n = 16;
        let at = |t: f64, r: f64, z: f64| P3::new(r * t.cos(), r * t.sin(), z);
        let segs: Vec<Seg> = (0..n)
            .map(|i| {
                let (s0, s1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
                let (t0, t1) = (start + TAU * s0, start + TAU * s1);
                let half = (t1 - t0) / 2.0;
                let (z0, z1) = (pitch * s0, pitch * s1);
                let poles = [
                    at(t0, r, z0),
                    at(t0 + half, r / half.cos(), (z0 + z1) / 2.0),
                    at(t1, r, z1),
                ];
                let c = Curve::nurbs(crate::native::nurbs::NurbsCurve::bezier(
                    &poles,
                    &[1.0, half.cos(), 1.0],
                ));
                Seg {
                    c,
                    t0: 0.0,
                    t1: 1.0,
                    rev: false,
                }
            })
            .collect();
        let h = fit_helix_segs(&segs).expect("a helix");
        assert!(h.a.z > 1.0 - 1e-15, "axis {:?}", h.a);
        assert!(
            h.o.x.abs() < 1e-12 && h.o.y.abs() < 1e-12,
            "centre {:?}",
            h.o
        );
        assert!((h.r - r).abs() < 1e-12, "radius {} against {r}", h.r);
        let rise = pitch / TAU;
        assert!(
            (h.c - rise).abs() < 1e-12 * rise,
            "rise {} against {rise}",
            h.c
        );
    }

    /// A quarter ellipse, bending tightly at one end and hardly at the other: the sections gather
    /// where it bends (spaced by the local curvature; spaced by the tightest bend everywhere, it
    /// took about 950), and the rails between them stay within the section tolerance.
    #[test]
    fn sections_gather_where_the_path_bends() {
        let ellipse = Curve::Ellipse {
            c: P3::origin(),
            x: V3::x(),
            y: V3::y(),
            a: 40.0,
            b: 8.0,
        };
        let seg = Seg {
            c: ellipse,
            t0: 0.0,
            t1: FRAC_PI_2,
            rev: false,
        };
        let plane = Plane {
            origin: P3::new(40.0, 0.0, 0.0),
            x_dir: nalgebra::Unit::new_normalize(V3::x()),
            normal: nalgebra::Unit::new_normalize(V3::y()),
        };
        let sweep = PathSweep::new(
            plane,
            &[seg],
            Frames::Control(SweepControl::None),
            1.0,
            None,
            SweepLaw::default(),
        )
        .unwrap();
        let n = sweep.v.len() - 1;
        assert!((8..300).contains(&n), "{n} sections");
        // Its first fifth (by length) bends the most and holds most of the sections.
        let tight = sweep.v.iter().filter(|&&s| s < 0.2).count();
        assert!(tight * 2 > n, "{tight} of {n} sections where it bends most");
        let tol = SECTION_TOL * 2.0;
        let worst = sweep.rail_errors(&sweep.v).into_iter().fold(0.0, f64::max);
        assert!(worst <= tol, "rails stray {worst:e} (tolerance {tol:e})");
    }
}
