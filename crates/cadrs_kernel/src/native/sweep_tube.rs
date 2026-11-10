//! What a torus fills under any rigid motion, exactly (a torus tumbling about two axes at once).
//!
//! A solid torus is the set of points within its tube radius r of its core circle k, so what it
//! fills as it moves is the set of points within r of Σ(t, ψ) = M(t) k(ψ), the surface the core
//! circle sweeps. Where the circle at t is at its nearest to a point, the point lies along the
//! circle's normal there, square to the circle's velocity too, so the swept solid's boundary is
//! made of the two offset sheets Σ ± r N (N the unit normal of Σ, where Σ_t × Σ_ψ isn't zero),
//! the half of the torus at the start that trails (where they meet it: the circles k ± r N at
//! t = 0) and the half at the end that leads. Those four make a closed surface K, a torus again,
//! which this module parametrizes by (σ, ψ), σ running round it in four charts of one each: the
//! leading sheet Σ + r N (σ = t), the end torus's leading half, the trailing sheet back from the
//! end, and the start torus's trailing half.
//!
//! K bounds the swept solid only where the motion never carries the torus back over it: where it
//! does, K runs inside the solid (where a sheet folds over, where r is beyond a radius of
//! curvature of Σ, and where the torus tumbles back over where it has been). Whether a point is
//! swept is decided exactly: p is in the solid when some instant has its distance from the core
//! circle placed then at most r (the inside test [`super::sweep_moving::Oracle`] is given).
//!
//! K is cut into patches (the sheets in chunks of turn and quarters of ψ, the tori's halves in
//! two by four), each fitted by a quintic B-spline through K's exact points on a grid, to a
//! billionth of the size; one that won't fit is cut in four and each quarter tried, and one
//! the solid covers deeply is left out. Where a sheet folds over (past a radius of curvature of
//! Σ, where it has a cuspidal edge) the patch is cut along the fold (the part folded over lies
//! inside the solid, under the sheet beyond the fold, and is judged so). The patches go to
//! [`super::sweep_moving::boundary_of`], which keeps what the inside test says is boundary, cuts
//! the faces along the curves where it changes (a sheet crossing itself, a torus's end, another
//! sheet) and joins them.

use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::topo::Body;
use crate::*;
use nalgebra::Matrix3;
use std::collections::HashMap;

type M3 = Matrix3<f64>;

/// The motion at t with its first two derivatives in t: x ↦ r[0] x + d[0], r[k] and d[k] the
/// k-th derivatives.
#[derive(Clone, Copy)]
pub(super) struct Jet {
    pub(super) r: [M3; 3],
    pub(super) d: [V3; 3],
}

/// The steps' motion at t (as [`super::sweep_solid`] places a rigid motion: each turn or slide
/// after the ones before) with its derivatives.
fn jet(steps: &[RigidStep], t: f64) -> Jet {
    let mut r = [M3::identity(), M3::zeros(), M3::zeros()];
    let mut d = [V3::zeros(); 3];
    for s in steps {
        match *s {
            RigidStep::Turn { axis, from, to } => {
                let w = to - from;
                let k = axis.dir.into_inner().cross_matrix();
                let q0 = *nalgebra::Rotation3::from_axis_angle(&axis.dir, from + w * t).matrix();
                let q1 = k * q0 * w;
                let q2 = k * q1 * w;
                let c = axis.origin.coords;
                // x ↦ q x + (c − q c).
                let b = [c - q0 * c, -(q1 * c), -(q2 * c)];
                r = [
                    q0 * r[0],
                    q1 * r[0] + q0 * r[1],
                    q2 * r[0] + q1 * r[1] * 2.0 + q0 * r[2],
                ];
                d = [
                    q0 * d[0] + b[0],
                    q1 * d[0] + q0 * d[1] + b[1],
                    q2 * d[0] + q1 * d[1] * 2.0 + q0 * d[2] + b[2],
                ];
            }
            RigidStep::Slide { from, to } => {
                d[0] += from + (to - from) * t;
                d[1] += to - from;
            }
        }
    }
    Jet { r, d }
}

/// The steps' place at t (the motion only).
fn place(steps: &[RigidStep], t: f64) -> (M3, V3) {
    let (mut r, mut d) = (M3::identity(), V3::zeros());
    for s in steps {
        match *s {
            RigidStep::Turn { axis, from, to } => {
                let q = *nalgebra::Rotation3::from_axis_angle(&axis.dir, from + (to - from) * t)
                    .matrix();
                let c = axis.origin.coords;
                r = q * r;
                d = q * (d - c) + c;
            }
            RigidStep::Slide { from, to } => d += from + (to - from) * t,
        }
    }
    (r, d)
}

/// A rigid motion over t from 0 to 1: where it places a body at t.
pub(crate) enum Motion<'a> {
    /// Turns and slides, each after the ones before, their derivatives known.
    Steps(&'a [RigidStep]),
    /// Any other, by its place alone (smooth in t, and given a little past either end): its
    /// derivative by differences.
    Place(&'a (dyn Fn(f64) -> (M3, V3) + Sync)),
}

impl Motion<'_> {
    pub(super) fn place(&self, t: f64) -> (M3, V3) {
        match self {
            Motion::Steps(steps) => place(steps, t),
            Motion::Place(at) => at(t),
        }
    }

    /// The place at t with its first derivative (and, of turns and slides, its second: nothing
    /// here reads another motion's).
    pub(super) fn jet(&self, t: f64) -> Jet {
        match self {
            Motion::Steps(steps) => jet(steps, t),
            Motion::Place(at) => {
                // (A five-point difference: good to twelve places of a smooth motion.)
                let h = 1e-3;
                let (p2, p1, m1, m2) = (at(t + 2.0 * h), at(t + h), at(t - h), at(t - 2.0 * h));
                let (r0, d0) = at(t);
                let r1 = (m2.0 - p2.0 + (p1.0 - m1.0) * 8.0) / (12.0 * h);
                let d1 = (m2.1 - p2.1 + (p1.1 - m1.1) * 8.0) / (12.0 * h);
                Jet {
                    r: [r0, r1, M3::zeros()],
                    d: [d0, d1, V3::zeros()],
                }
            }
        }
    }
}

/// The core circle's sweep at a point (t, ψ): Σ and its derivatives, the unit normal
/// N = Σ_t × Σ_ψ / |…| and N's derivatives.
struct SheetPt {
    /// Σ, Σ_t, Σ_ψ, Σ_tt, Σ_tψ, Σ_ψψ.
    s: [V3; 6],
    n: V3,
    npsi: V3,
    /// |Σ_t × Σ_ψ| / (|Σ_t| |Σ_ψ|): how far Σ is from folding there.
    sin: f64,
}

/// The charts of K: the leading sheet (σ = t), the end torus's leading half, the trailing
/// sheet (from the end back), the start torus's trailing half.
const LEAD: usize = 0;
const END: usize = 1;
const TRAIL: usize = 2;
const START: usize = 3;
/// A pinch's own chart, the first's (the k-th's is `PINCH + k`): ψ = the angle θ round the tube
/// whose normal the core circle's point's velocity across the circle is square to, a = how far
/// along that the velocity is, as a part of the way to the seam (−1 to 1 between the seams).
const PINCH: usize = 4;

/// A pinch of the core circle's sweep: a (t, ψ) where the circle's point moves along the circle,
/// so that the torus touches what it sweeps all round its tube there. About it the sheets'
/// normal turns once round, and they are no surfaces of (t, ψ); what the torus sweeps is smooth
/// all the same, a band round that section of the tube, which the pinch's own chart covers out
/// to its seam: an ellipse about the pinch in (t, ψ), `reach` wide each way (in the measure of
/// [`Tube::across`], which the chart is laid out by). The sheets are left off inside it.
struct Pinch {
    c: P2,
    /// The inverse of the derivative of [`Tube::across`] there.
    inv: nalgebra::Matrix2<f64>,
    reach: V2,
    /// [`Tube::across`] to (t, ψ) from the pinch as parts of `reach`, to the first order.
    shape: nalgebra::Matrix2<f64>,
}

impl Pinch {
    /// How large [`Tube::across`] is at the seam where it is square to the tube's normal at θ.
    fn rho(&self, th: f64) -> f64 {
        1.0 / (self.shape * V2::new(-th.sin(), th.cos())).norm()
    }
}

/// A pinch's chart reaches this much past its seam, the band beyond being cut off along it.
const PINCH_REACH: f64 = 1.25;

/// A near pinch of the core circle's sweep: a (t, ψ) where the circle's point all but moves
/// along the circle ([`Tube::across`] least there, and its derivative singular: a line of what
/// it takes passing by nothing). Across that line the sheets' normal turns half round the
/// tube within a hair of (t, ψ): surfaces still, but ones no patch laid out in (t, ψ) follows.
/// Each sheet has its own chart of the band about it instead, the first band's leading sheet's
/// being the chart after the pinches', its trailing sheet's the next: ψ = η, how far along
/// the line, a = τ, with w sinh τ how far across it (so that the turn, as wide as w, is
/// spread over τ). The sheets are left off inside its seam, an ellipse in (τ, η).
struct Band {
    c: P2,
    /// Across the line and along it, in (t, ψ).
    ex: V2,
    ey: V2,
    w: f64,
    /// The seam's reach in τ and in η.
    reach: V2,
    /// How far across the line bends by η along it: a polynomial's coefficients, the constant
    /// first.
    bend: [f64; 5],
}

impl Band {
    /// How far across the line lies at η along it.
    fn mid(&self, eta: f64) -> f64 {
        self.bend.iter().rev().fold(0.0, |a, c| a * eta + c)
    }

    /// How far out a place (τ, η) of the band's charts is, as a part of the way to the seam.
    fn out(&self, w: P2) -> f64 {
        (w.x / self.reach.x).hypot(w.y / self.reach.y)
    }
}

/// A place on K: its chart, the coordinate a ∈ [0, 1] across it (σ less the chart), and ψ.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    chart: usize,
    a: f64,
    psi: f64,
}

/// A point of K: where it is, and its own instant.
struct KPt {
    p: P3,
    own: f64,
}

/// A tube's inside test reads every so many of its samples first, and those between only
/// where the circle's travel could bring a point near.
const STRIDE: usize = 8;

/// How near a line of pinches a sheet's membership is read ([`Tube::inward`]), in ψ (and a
/// pipe's by its sides, in half turns round it): the sheet's stretch from beyond the fold lies
/// under this one by the cube of the way from the line (a hundred-millionth of the size a
/// hundredth of a radian off), and at the line itself by nothing that can be told.
const LINE_BY: f64 = 0.02;

/// A torus and its motion, with the samples of the motion its inside test uses.
pub(crate) struct Tube<'a> {
    motion: &'a Motion<'a>,
    /// The core circle in the body's frame: its centre, axes (z square to it) and radius.
    c: P3,
    x: V3,
    y: V3,
    z: V3,
    big: f64,
    /// The tube's radius.
    r: f64,
    size: f64,
    ts: Vec<f64>,
    places: Vec<(M3, V3)>,
    /// How far the core circle's points move at most over each STRIDE samples.
    spans: Vec<f64>,
    pinches: Vec<Pinch>,
    bands: Vec<Band>,
    /// The lines of pinches: each ψ at which the core circle's point moves along the circle
    /// the whole motion through (a wheel's top and bottom, carried along a level path). The
    /// core circle's sweep folds along each, and the sheets with it; the tube's whole section
    /// there sweeps a pipe. In order round the circle, an even number of them: the sheets'
    /// normal, which would turn about at each, is taken one way on every other stretch
    /// between them and the other way on the rest, and so runs on smoothly across them.
    lines: Vec<f64>,
}

/// A local least of the distance over the motion, less r: (instant, value).
type Least = (f64, f64);

impl<'a> Tube<'a> {
    fn new(motion: &'a Motion<'a>, fr: &Frame, big: f64, r: f64, size: f64) -> Tube<'a> {
        // Samples a few thousandths of a turn apart.
        let (r0, _) = motion.place(0.0);
        let mut turn = 0.0;
        let mut prev = r0;
        for i in 1..=256 {
            let (ri, _) = motion.place(i as f64 / 256.0);
            turn += nalgebra::Rotation3::from_matrix_unchecked(prev.transpose() * ri).angle();
            prev = ri;
        }
        let n = ((turn / 0.003).ceil() as usize)
            .clamp(1024, 8192)
            .next_multiple_of(STRIDE);
        let ts: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
        let places: Vec<(M3, V3)> = ts.iter().map(|&t| motion.place(t)).collect();
        // (How far the core circle's points move at most over each stretch of STRIDE samples:
        // by no more does a point's distance from it change.)
        let spans: Vec<f64> = places
            .chunks(STRIDE)
            .zip(places[1..].chunks(STRIDE))
            .map(|(a, b)| {
                a.iter()
                    .zip(b)
                    .map(|(p, q)| {
                        let dr = q.0 - p.0;
                        (dr * fr.o.coords + (q.1 - p.1)).norm() + big * dr.norm()
                    })
                    .sum::<f64>()
            })
            .collect();
        // ψ measured from a direction the motion itself picks out, so that the faces' seams
        // (at quarters of ψ) fall evenly on what is swept where the motion is even: the way the
        // core circle's speed leans round the circle midway through the motion (its first
        // harmonic there, or its second where it leans no way), which a motion that mirrors
        // itself about its middle has on its mirror.
        let (x, y) = {
            let j = motion.jet(0.5);
            let n = 360;
            let (mut h1, mut h2, mut mean) = (V2::zeros(), V2::zeros(), 0.0);
            for i in 0..n {
                let psi = TAU * i as f64 / n as f64;
                let k = fr.o.coords + (fr.x * psi.cos() + fr.y * psi.sin()) * big;
                let s = (j.r[1] * k + j.d[1]).norm_squared();
                h1 += V2::new(psi.cos(), psi.sin()) * s;
                h2 += V2::new((2.0 * psi).cos(), (2.0 * psi).sin()) * s;
                mean += s;
            }
            let psi0 = if h1.norm() > 1e-6 * mean {
                h1.y.atan2(h1.x)
            } else if h2.norm() > 1e-6 * mean {
                0.5 * h2.y.atan2(h2.x)
            } else {
                0.0
            };
            (
                fr.x * psi0.cos() + fr.y * psi0.sin(),
                fr.y * psi0.cos() - fr.x * psi0.sin(),
            )
        };
        Tube {
            motion,
            c: fr.o,
            x,
            y,
            z: fr.z,
            big,
            r,
            size,
            ts,
            places,
            spans,
            pinches: Vec::new(),
            bands: Vec::new(),
            lines: Vec::new(),
        }
    }

    fn e1(&self, psi: f64) -> V3 {
        self.x * psi.cos() + self.y * psi.sin()
    }

    fn e1d(&self, psi: f64) -> V3 {
        self.y * psi.cos() - self.x * psi.sin()
    }

    /// The core circle's point at ψ (body frame).
    fn k(&self, psi: f64) -> V3 {
        self.c.coords + self.e1(psi) * self.big
    }

    fn sheet_pt(&self, j: &Jet, psi: f64) -> SheetPt {
        // (The sweep's point and its derivatives, and the cross product of its two tangents
        // with that's derivative in ψ.)
        let at = |psi: f64| {
            let k = self.k(psi);
            let kd = self.e1d(psi) * self.big;
            let kdd = -self.e1(psi) * self.big;
            let s = [
                j.r[0] * k + j.d[0],
                j.r[1] * k + j.d[1],
                j.r[0] * kd,
                j.r[2] * k + j.d[2],
                j.r[1] * kd,
                j.r[0] * kdd,
            ];
            let m = s[1].cross(&s[2]);
            let mp = s[4].cross(&s[2]) + s[1].cross(&s[5]);
            (s, m, mp)
        };
        let (s, m, mp) = at(psi);
        let len = m.norm();
        let sin = len / (s[1].norm() * s[2].norm());
        // Hard by a line of pinches the cross product is nothing but its own slope across
        // the line, to rounding: the normal from that slope there and its change, one way
        // right across the line.
        if let Some((line, off)) = self.line_by(psi, 1e-5) {
            let h = 1e-4;
            let (_, _, m1) = at(line);
            let m2 = (at(line + h).2 - at(line - h).2) / (2.0 * h);
            let v = m1 + m2 * (0.5 * off);
            let side = self.side(line + 1e-3);
            let n = v.normalize() * side;
            let npsi = (m2 - n * n.dot(&m2)) * (0.5 * side / v.norm());
            return SheetPt { s, n, npsi, sin };
        }
        let side = self.side(psi);
        let n = m * (side / len);
        let npsi = (mp * side - n * n.dot(&mp) * side) / len;
        SheetPt { s, n, npsi, sin }
    }

    /// [`Self::sheet_cover`] of a place of a patch, read beside a line of pinches where it is
    /// hard by one ([`Self::inward`]).
    fn cover_in(&self, patch: &Patch, w: P2) -> (bool, f64) {
        self.sheet_cover(patch.0, self.inward(patch, w))
    }

    /// [`Self::sheet_cover_near`] of a place of a patch, as [`Self::cover_in`] reads it.
    fn cover_near_in(&self, patch: &Patch, w: P2, tc: f64) -> (bool, f64) {
        self.sheet_cover_near(patch.0, self.inward(patch, w), tc)
    }

    /// [`Self::margin`] of a place of a patch, as [`Self::cover_in`] reads it: from an end's
    /// torus where it is, from any other instant beside it.
    fn margin_in(&self, patch: &Patch, w: P2, tau: f64) -> (f64, f64) {
        if (0.0..=1e-9).contains(&tau) || (1.0 - 1e-9..=1.0).contains(&tau) {
            self.margin(patch.0, w, tau)
        } else {
            self.margin(patch.0, self.inward(patch, w), tau)
        }
    }

    /// A place of a sheet's patch as its membership is read: no nearer a side of the patch
    /// that is a line of pinches than LINE_BY. The sheet folds along the line, its stretch
    /// from the far side of the fold coming to lie on this one, the two a hair apart hard by
    /// it: which of them is the outer is read a little way in, where they stand clear.
    fn inward(&self, patch: &Patch, w: P2) -> P2 {
        if self.lines.is_empty() {
            return w;
        }
        // (A pipe's halves end at the sheets' joins, where they touch them as the sheets
        // touch one another.)
        if self.pipe(patch.0).is_some() {
            return P2::new(w.x, w.y.clamp(0.5 * LINE_BY, 1.0 - 0.5 * LINE_BY));
        }
        if patch.0 >= PINCH {
            return w;
        }
        // (Along the fold, not straight across to it: by the line the sheet's point moves
        // along the fold as ψ changes, as it does with the instant, so that the place the
        // membership is read at is the one beside this on the fold, and a curve that runs
        // into the fold's zone runs on in it to the fold as it would.)
        let mut to = w;
        for (side, inside) in [(patch.3, 1.0), (patch.4, -1.0)] {
            if self.line_by(side, 1e-9).is_none() || (w.y - side) * inside >= LINE_BY {
                continue;
            }
            let at = |a: f64, psi: f64| {
                self.eval(&Pos {
                    chart: patch.0,
                    a,
                    psi,
                })
                .p
            };
            let h = 1e-6;
            let along = (at((w.x + h).min(1.0), side) - at((w.x - h).max(0.0), side))
                / ((w.x + h).min(1.0) - (w.x - h).max(0.0));
            let across = (at(w.x, side + h) - at(w.x, side - h)) / (2.0 * h);
            let slide = across.dot(&along) / along.norm_squared();
            let y = side + inside * LINE_BY;
            to = P2::new((w.x + slide * (w.y - y)).clamp(0.0, 1.0), y);
        }
        to
    }

    /// The line of pinches within `near` of ψ, with how far past it ψ is.
    fn line_by(&self, psi: f64, near: f64) -> Option<(f64, f64)> {
        self.lines.iter().find_map(|&l| {
            let off = wrap(psi - l, -PI, TAU);
            (off.abs() <= near).then_some((l, off))
        })
    }

    /// Which way the sheets' normal is taken at ψ: one way from the first line of pinches to
    /// the second, the other way to the third, and so on round (one way everywhere without
    /// lines).
    fn side(&self, psi: f64) -> f64 {
        let Some(&first) = self.lines.first() else {
            return 1.0;
        };
        let at = wrap(psi - first, 0.0, TAU);
        let past = self
            .lines
            .iter()
            .filter(|&&l| wrap(l - first, 0.0, TAU) <= at)
            .count();
        if past % 2 == 1 { 1.0 } else { -1.0 }
    }

    /// The lines of pinches: each ψ at which [`Self::across`] is nothing at every instant.
    fn find_lines(&self) -> Vec<f64> {
        let n = 1440;
        let size = |t: f64, psi: f64| self.across(t, psi).norm();
        let scale = (0..n)
            .map(|k| size(0.37, TAU * k as f64 / n as f64))
            .fold(0.0, f64::max);
        let mut out: Vec<f64> = Vec::new();
        for k in 0..n {
            let at = |k: usize| size(0.37, TAU * (k % n) as f64 / n as f64);
            if !(at(k) <= at(k + 1) && at(k) <= at(k + n - 1)) {
                continue;
            }
            // (The least there, by golden section.)
            let g = 0.5 * (5f64.sqrt() - 1.0);
            let (mut lo, mut hi) = (
                TAU * (k as f64 - 1.0) / n as f64,
                TAU * (k as f64 + 1.0) / n as f64,
            );
            for _ in 0..80 {
                let (c, d) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if size(0.37, c) < size(0.37, d) {
                    hi = d;
                } else {
                    lo = c;
                }
            }
            let psi = wrap(0.5 * (lo + hi), 0.0, TAU);
            let through = (0..=64).all(|i| size(i as f64 / 64.0, psi) <= 1e-9 * scale);
            if through && !out.iter().any(|o| wrap(o - psi, -PI, TAU).abs() < 1e-6) {
                out.push(psi);
            }
        }
        for l in &mut out {
            if (*l - TAU).abs() < 1e-9 {
                *l = 0.0;
            }
        }
        out.sort_by(f64::total_cmp);
        out
    }

    /// The angle θ (in the tube's (e1, z) plane at ψ, body frame) of N at (t, ψ), and its
    /// derivative in ψ.
    fn theta_n(&self, j: &Jet, sp: &SheetPt, psi: f64) -> (f64, f64) {
        let nb = j.r[0].tr_mul(&sp.n);
        let nbp = j.r[0].tr_mul(&sp.npsi);
        let (e1, e1d) = (self.e1(psi), self.e1d(psi));
        let (a, b) = (nb.dot(&self.z), nb.dot(&e1));
        let (ap, bp) = (nbp.dot(&self.z), nbp.dot(&e1) + nb.dot(&e1d));
        (a.atan2(b), (b * ap - a * bp) / (a * a + b * b))
    }

    /// The tube's point (ψ, θ) placed by `j`, with its derivatives in ψ and θ.
    fn torus_pt(&self, j: &Jet, psi: f64, th: f64) -> (P3, V3, V3) {
        let (e1, e1d) = (self.e1(psi), self.e1d(psi));
        let q = self.c.coords + e1 * (self.big + self.r * th.cos()) + self.z * (self.r * th.sin());
        let qp = e1d * (self.big + self.r * th.cos());
        let qt = (-e1 * th.sin() + self.z * th.cos()) * self.r;
        (P3::from(j.r[0] * q + j.d[0]), j.r[0] * qp, j.r[0] * qt)
    }

    /// The point of K at `pos`.
    fn eval(&self, pos: &Pos) -> KPt {
        let psi = pos.psi;
        match pos.chart {
            LEAD | TRAIL => {
                let (t, s) = if pos.chart == LEAD {
                    (pos.a, 1.0)
                } else {
                    (1.0 - pos.a, -1.0)
                };
                let j = self.motion.jet(t);
                let sp = self.sheet_pt(&j, psi);
                KPt {
                    p: P3::from(sp.s[0] + sp.n * (s * self.r)),
                    own: t,
                }
            }
            c if self.pipe(c).is_some() => {
                // (A line's pipe: the tube's section there at the instant a, half round it
                // from the one sheet's join to the other's, or on round from that.)
                let (k, half) = self.pipe(c).unwrap();
                let line = self.lines[k];
                let j = self.motion.jet(pos.a);
                let sp = self.sheet_pt(&j, line);
                let th = self.theta_n(&j, &sp, line).0 + PI * (half as f64 + psi);
                KPt {
                    p: self.torus_pt(&j, line, th).0,
                    own: pos.a,
                }
            }
            c if self.band(c).is_some() => {
                let (j, sheet) = self.band(c).unwrap();
                let w = self.band_at(j, pos.a, psi);
                self.eval(&Pos {
                    chart: sheet,
                    a: if sheet == LEAD { w.x } else { 1.0 - w.x },
                    psi: w.y,
                })
            }
            c if c >= PINCH => {
                let tp = self.pinch_at(c - PINCH, pos.a, psi);
                let j = self.motion.jet(tp.x);
                let (p, _, _) = self.torus_pt(&j, tp.y, psi);
                KPt { p, own: tp.x }
            }
            _ => {
                // The leading half at the end runs from θ(N) down by π; the trailing half at the
                // start from θ(−N) down by π.
                let t = if pos.chart == END { 1.0 } else { 0.0 };
                let j = self.motion.jet(t);
                let sp = self.sheet_pt(&j, psi);
                let (th0, _) = self.theta_n(&j, &sp, psi);
                let th0 = if pos.chart == END { th0 } else { th0 + PI };
                let th = th0 - PI * pos.a;
                let (p, _, _) = self.torus_pt(&j, psi, th);
                KPt { p, own: t }
            }
        }
    }

    /// The distance of `p` from the core circle placed by `pl`, less r.
    fn f_at(&self, pl: &(M3, V3), p: &P3) -> f64 {
        let q = pl.0.tr_mul(&(p.coords - pl.1)) - self.c.coords;
        let h = q.dot(&self.z);
        let rho = (q - self.z * h).norm();
        (rho - self.big).hypot(h) - self.r
    }

    fn f(&self, t: f64, p: &P3) -> f64 {
        self.f_at(&self.motion.place(t), p)
    }

    /// The least of f over [a, b] by golden-section search (an end where f only rises from it).
    fn golden(&self, p: &P3, mut a: f64, mut b: f64) -> Least {
        let g = 0.5 * (5f64.sqrt() - 1.0);
        let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
        let (mut fc, mut fd) = (self.f(c, p), self.f(d, p));
        for _ in 0..64 {
            // (To a hundred-millionth of the motion: the least itself is then good to
            // rounding, being flat there; an instant wanted exactly is made so by
            // [`Self::foot`].)
            if b - a <= 1e-8 {
                break;
            }
            if fc < fd {
                b = d;
                d = c;
                fd = fc;
                c = b - g * (b - a);
                fc = self.f(c, p);
            } else {
                a = c;
                c = d;
                fc = fd;
                d = a + g * (b - a);
                fd = self.f(d, p);
            }
        }
        let mut best = if fc < fd { (c, fc) } else { (d, fd) };
        for e in [a, b] {
            let fe = self.f(e, p);
            if fe < best.1 {
                best = (e, fe);
            }
        }
        best
    }

    /// How far inside the placed torus a point must lie at some instant to count as covered: a
    /// point of K lies on it at its own instant, to rounding.
    fn delta(&self) -> f64 {
        1e-13 * self.size
    }

    /// The least of f over the whole motion for `p` (how deep inside the swept solid it lies,
    /// negative inside), with its instant: the samples' local leasts near the tube refined.
    fn least(&self, p: &P3) -> Least {
        let vals = self.samples(p);
        let n = vals.len();
        let bound = 0.5 * self.r;
        let mut best: Least = (0.0, f64::INFINITY);
        for i in 0..n {
            let left = i == 0 || vals[i] <= vals[i - 1];
            let right = i + 1 == n || vals[i] <= vals[i + 1];
            if !(left && right) || vals[i] > bound {
                continue;
            }
            let m = self.golden(p, self.ts[i.saturating_sub(1)], self.ts[(i + 1).min(n - 1)]);
            if m.1 < best.1 {
                best = m;
            }
        }
        best
    }

    /// f for `p` at each sampled instant that may be a least of it near the tube (within half
    /// its radius), and infinity at the rest: read at every STRIDE-th first, then between two
    /// of those wherever the circle's travel between them ([`Self::spans`]) could bring it
    /// that near.
    fn samples(&self, p: &P3) -> Vec<f64> {
        let n = self.places.len();
        let bound = 0.5 * self.r;
        let mut vals = vec![f64::INFINITY; n];
        for i in (0..n).step_by(STRIDE) {
            vals[i] = self.f_at(&self.places[i], p);
        }
        for (k, span) in self.spans.iter().enumerate() {
            let (a, b) = (k * STRIDE, (k + 1) * STRIDE);
            if 0.5 * (vals[a] + vals[b] - span) > bound {
                continue;
            }
            for i in a + 1..b {
                vals[i] = self.f_at(&self.places[i], p);
            }
        }
        vals
    }

    /// Every local least of f over the motion for `p` near the tube, refined, the least first.
    fn leasts(&self, p: &P3) -> Vec<Least> {
        let vals = self.samples(p);
        let n = vals.len();
        let bound = 0.5 * self.r;
        let mut out: Vec<Least> = (0..n)
            .filter(|&i| {
                let left = i == 0 || vals[i] <= vals[i - 1];
                let right = i + 1 == n || vals[i] <= vals[i + 1];
                left && right && vals[i] <= bound
            })
            .map(|i| self.golden(p, self.ts[i.saturating_sub(1)], self.ts[(i + 1).min(n - 1)]))
            .collect();
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    /// The step between the instants [`Self::least`] samples.
    fn step(&self) -> f64 {
        self.ts[1] - self.ts[0]
    }

    /// The least of f for `p` over the instants within two samples of `tc`.
    fn least_near(&self, p: &P3, tc: f64) -> Least {
        let w = 2.0 * self.step();
        let (a, b) = ((tc - w).max(0.0), (tc + w).min(1.0));
        const N: usize = 8;
        let at = |i: usize| a + (b - a) * i as f64 / N as f64;
        let vals: [f64; N + 1] = std::array::from_fn(|i| self.f(at(i), p));
        // (Least at an edge of the window that is no end of the motion: the least is beyond
        // it, and what is read here is no instant the torus touches the point at.)
        let low = (0..=N)
            .min_by(|&i, &j| vals[i].total_cmp(&vals[j]))
            .unwrap();
        if (low == 0 && a > 0.0) || (low == N && b < 1.0) {
            return self.least(p);
        }
        let mut best: Least = (a, f64::INFINITY);
        for i in 0..=N {
            let left = i == 0 || vals[i] <= vals[i - 1];
            let right = i == N || vals[i] <= vals[i + 1];
            if !(left && right) {
                continue;
            }
            let m = self.golden(p, at(i.saturating_sub(1)), at((i + 1).min(N)));
            if m.1 < best.1 {
                best = m;
            }
        }
        best
    }

    /// The least of f for a sheet's point `p` over the instants beside its own, `own`, where
    /// the torus touches it (f nothing there, and least or greatest): an instant a few samples
    /// off that holds it, which the samples can't tell from its own (beside a fold the two
    /// come together). With the touch divided out, f / (t − own)², what is left has one well
    /// there, below nothing just where another instant holds the point; that instant's least
    /// is then beyond the well's bottom. Nothing, at `own`, where none holds it.
    fn near_own(&self, p: &P3, own: f64) -> Least {
        let w = 4.0 * self.step();
        let (a, b) = ((own - w).max(0.0), (own + w).min(1.0));
        // (No nearer its own instant than this: the quotient is lost to rounding there.)
        let hole = 1e-3 * w;
        let q = |t: f64| {
            let mut u = t - own;
            if u.abs() < hole {
                u = if (u < 0.0 || own + hole > 1.0) && own - hole >= 0.0 {
                    -hole
                } else {
                    hole
                };
            }
            self.f(own + u, p) / (u * u)
        };
        const N: usize = 16;
        let at = |i: usize| a + (b - a) * i as f64 / N as f64;
        let (mut bi, mut bq) = (0, f64::INFINITY);
        for i in 0..=N {
            let v = q(at(i));
            if v < bq {
                (bi, bq) = (i, v);
            }
        }
        // (The well read off the parabola through its lowest sample and the two beside it:
        // clear of nothing, no instant here holds the point, and the search is spared.)
        if bi > 0 && bi < N {
            let (q0, q1, q2) = (q(at(bi - 1)), bq, q(at(bi + 1)));
            let bend = q0 - 2.0 * q1 + q2;
            if bend > 0.0 {
                let floor = q1 - (q2 - q0).powi(2) / (8.0 * bend);
                if floor > 0.25 * q1 && q1 > 0.0 {
                    return (own, 0.0);
                }
            }
        }
        let (mut lo, mut hi) = (at(bi.saturating_sub(1)), at((bi + 1).min(N)));
        let g = 0.5 * (5f64.sqrt() - 1.0);
        let (mut c, mut d) = (hi - g * (hi - lo), lo + g * (hi - lo));
        let (mut qc, mut qd) = (q(c), q(d));
        for _ in 0..40 {
            if qc < qd {
                hi = d;
                d = c;
                qd = qc;
                c = hi - g * (hi - lo);
                qc = q(c);
            } else {
                lo = c;
                c = d;
                qc = qd;
                d = lo + g * (hi - lo);
                qd = q(d);
            }
        }
        let (tq, vq) = if qc < qd { (c, qc) } else { (d, qd) };
        let (tq, vq) = if bq < vq { (at(bi), bq) } else { (tq, vq) };
        if vq >= 0.0 {
            return (own, 0.0);
        }
        if tq < own {
            self.golden(p, a, tq)
        } else {
            self.golden(p, tq, b)
        }
    }

    /// Whether a sheet's point at a is beside a join (within [`Self::NEAR`] of the sheet's
    /// start or end) and the instant `tau` between it and that end of the motion: what
    /// [`Self::end_cover`] alone reads (the torus there holds the point or doesn't; an instant
    /// between found to hold it by a hair is the same touch, less well read).
    fn beside_join(&self, own: f64, tau: f64) -> bool {
        (own < Self::NEAR && tau < 4.0 * Self::NEAR)
            || (own > 1.0 - Self::NEAR && tau > 1.0 - 4.0 * Self::NEAR)
    }

    /// How near a join (in the measure of the motion) a sheet's cover by the torus there is
    /// read off the line through two values further in.
    const NEAR: f64 = 3e-5;

    /// Whether the sheet's point at (a, ψ) is covered (inside the swept solid: the torus holds
    /// it at some instant), with the instant that covers it deepest.
    fn sheet_cover(&self, chart: usize, w: P2) -> (bool, f64) {
        let kp = self.eval(&Pos {
            chart,
            a: w.x,
            psi: w.y,
        });
        if let Some(mark) = self.seam(chart, w, kp.own) {
            return (true, mark);
        }
        let p = kp.p;
        let mut l = self.least(&p);
        if !self.beside_join(kp.own, l.0) {
            let n = self.near_own(&p, kp.own);
            if n.1 < l.1 {
                l = n;
            }
            if l.1 < -self.delta() {
                return (true, l.0);
            }
        }
        self.end_cover(chart, w).map_or((false, l.0), |e| (true, e))
    }

    /// By how much a chart's place clears what covers it about the instant `tau` (as a
    /// boundary's point records it: an instant near which the torus holds the far side, or a
    /// seam's mark), nothing just on that boundary; and the instant itself, as it is here. Two
    /// such boundaries cross where both are nothing: a corner of the place's curve. (A cover's
    /// margin is in the measure of [`Self::delta`].)
    fn margin(&self, chart: usize, w: P2, tau: f64) -> (f64, f64) {
        let kp = self.eval(&Pos {
            chart,
            a: w.x,
            psi: w.y,
        });
        if tau < -0.5 {
            // (The seam's mark says whose it is: a pinch's, or past those a band's.)
            let mark = (-1.0 - tau).round() as usize;
            let v = if let Some(band) = mark
                .checked_sub(self.pinches.len())
                .and_then(|j| self.bands.get(j).map(|b| (j, b)))
            {
                let (j, b) = band;
                if self.band(chart).is_some() {
                    1.0 - b.out(w)
                } else {
                    b.out(self.band_uv(j, kp.own, w.y)) - 1.0
                }
            } else if chart >= PINCH {
                1.0 - w.x.abs()
            } else {
                let across = self.across(kp.own, w.y);
                self.pinches
                    .get(mark)
                    .map_or(f64::NAN, |pn| (pn.shape * across).norm() - 1.0)
            };
            return (v, tau);
        }
        let l = if (tau - kp.own).abs() < 4.0 * self.step() {
            self.near_own(&kp.p, kp.own)
        } else {
            self.least_from(&kp.p, tau)
        };
        // (In the measure of the least hold: one at the place's own instant, which holds it
        // by nothing, clears by one.)
        ((l.1 + self.delta()) / self.delta(), l.0)
    }

    /// The least of f for `p` that the instant `tau` runs down to: the same least followed as
    /// the point moves, however far its instant has moved.
    fn least_from(&self, p: &P3, tau: f64) -> Least {
        let h = self.step();
        let tau = tau.clamp(0.0, 1.0);
        let (mut a, mut m, mut b) = ((tau - h).max(0.0), tau, (tau + h).min(1.0));
        let (mut fa, mut fm, mut fb) = (self.f(a, p), self.f(m, p), self.f(b, p));
        // (Down the slope, a sample at a time, till it turns up or the motion ends.)
        for _ in 0..64 {
            if fa < fm && a > 0.0 {
                (b, fb) = (m, fm);
                (m, fm) = (a, fa);
                a = (a - h).max(0.0);
                fa = self.f(a, p);
            } else if fb < fm && b < 1.0 {
                (a, fa) = (m, fm);
                (m, fm) = (b, fb);
                b = (b + h).min(1.0);
                fb = self.f(b, p);
            } else {
                break;
            }
        }
        let _ = (fa, fb);
        self.golden(p, a, b)
    }

    /// The end of the motion whose torus holds the sheet's point at (a, ψ), if either does.
    /// (Beside a join the sheet touches that torus, and the two part only as the cube of the
    /// distance from where their crossing runs into the join: no instant covers a point there
    /// by as much as [`Self::delta`]. The torus's own distance tells its sides apart: it is of
    /// the second order in the distance from the join, so with that divided out it changes
    /// sign across the crossing right up to the join; the last few hundred-thousandths of the
    /// motion, where the distance itself is lost to rounding, are read off the line through
    /// two values further in.)
    fn end_cover(&self, chart: usize, w: P2) -> Option<f64> {
        const NEAR: f64 = Tube::NEAR;
        // (A pinch's chart is nowhere near a join: the ends hold its points as any instant
        // does.)
        if chart >= PINCH && self.pipe(chart).is_none() {
            return None;
        }
        let n = self.places.len() - 1;
        let f = |end: f64, a: f64| {
            let p = self.eval(&Pos { chart, a, psi: w.y }).p;
            self.f_at(&self.places[if end > 0.5 { n } else { 0 }], &p)
        };
        // The join this point is nearer (a sheet's a runs from its own first join), how far
        // from it, and the end of the motion whose torus that is.
        let low = w.x < 0.5;
        let x = if low { w.x } else { 1.0 - w.x };
        let at = |x: f64| if low { x } else { 1.0 - x };
        // (A pipe's a runs with the motion, as the leading sheet's does.)
        let near = if (chart != TRAIL) == low { 0.0 } else { 1.0 };
        // (Held by as much as any instant must hold a point; within a hundredth of the motion
        // of the join, by that less in the measure of the square of the distance from it.)
        const WIDE: f64 = 1e-2;
        let delta = self.delta();
        let touching = if x >= WIDE {
            f(near, w.x) < -delta
        } else {
            let g = |x: f64| f(near, at(x)) / (x * x);
            let g = if x >= NEAR {
                g(x)
            } else {
                let (g1, g2) = (g(NEAR), g(2.0 * NEAR));
                g1 + (x - NEAR) * (g2 - g1) / NEAR
            };
            g < -delta / (WIDE * WIDE)
        };
        if touching {
            Some(near)
        } else if f(1.0 - near, w.x) < -delta {
            Some(1.0 - near)
        } else {
            None
        }
    }

    /// [`Self::sheet_cover`] read only about the instant `tc`.
    fn sheet_cover_near(&self, chart: usize, w: P2, tc: f64) -> (bool, f64) {
        if tc < 0.0 {
            // (About a seam: no instant to read about.)
            return self.sheet_cover(chart, w);
        }
        let kp = self.eval(&Pos {
            chart,
            a: w.x,
            psi: w.y,
        });
        if let Some(mark) = self.seam(chart, w, kp.own) {
            return (true, mark);
        }
        let p = kp.p;
        let mut l = self.least_near(&p, tc);
        if !self.beside_join(kp.own, l.0) {
            let n = self.near_own(&p, kp.own);
            if n.1 < l.1 {
                l = n;
            }
            if l.1 < -self.delta() {
                return (true, l.0);
            }
        }
        self.end_cover(chart, w).map_or((false, l.0), |e| (true, e))
    }

    /// Where on the sheets a point `p` lies at the instant `t` inside the motion (a local least
    /// of its distance, at r): the chart and coordinates.
    fn sheet_pos(&self, t: f64, p: &P3) -> Pos {
        let j = self.motion.jet(t);
        let q = j.r[0].tr_mul(&(p.coords - j.d[0])) - self.c.coords;
        let h = q.dot(&self.z);
        let w = q - self.z * h;
        let psi = w.dot(&self.y).atan2(w.dot(&self.x));
        let sp = self.sheet_pt(&j, psi);
        if (p.coords - sp.s[0]).dot(&sp.n) >= 0.0 {
            Pos {
                chart: LEAD,
                a: t,
                psi,
            }
        } else {
            Pos {
                chart: TRAIL,
                a: 1.0 - t,
                psi,
            }
        }
    }

    /// The velocity of the core circle's point at ψ at the instant `t`, across the circle: its
    /// parts along the circle's radius there and along its axis (in the body's frame). The
    /// torus touches what it sweeps where its normal is square to this; where it is nothing
    /// (the point moving along the circle), all round the tube there.
    fn across(&self, t: f64, psi: f64) -> V2 {
        let j = self.motion.jet(t);
        let v = j.r[0].tr_mul(&(j.r[1] * self.k(psi) + j.d[1]));
        V2::new(v.dot(&self.e1(psi)), v.dot(&self.z))
    }

    /// The derivative of [`Self::across`] in (t, ψ).
    fn across_d(&self, t: f64, psi: f64) -> nalgebra::Matrix2<f64> {
        let h = 1e-6;
        let dt = (self.across((t + h).min(1.0), psi) - self.across((t - h).max(0.0), psi))
            / ((t + h).min(1.0) - (t - h).max(0.0));
        let dp = (self.across(t, psi + h) - self.across(t, psi - h)) / (2.0 * h);
        nalgebra::Matrix2::from_columns(&[dt, dp])
    }

    /// The pinches of the core circle's sweep: the (t, ψ) inside the motion where
    /// [`Self::across`] is nothing, found from its least sizes over a grid.
    fn find_pinches(&self) -> Vec<P2> {
        let (nt, np) = (400, 256);
        let size = |i: usize, k: usize| {
            self.across(i as f64 / nt as f64, TAU * (k % np) as f64 / np as f64)
                .norm()
        };
        // (The grid's rows side by side: a hundred thousand readings, each a jet of the motion.)
        let grid: Vec<f64> = {
            use rayon::prelude::*;
            (0..(nt + 1) * np)
                .into_par_iter()
                .map(|n| size(n / np, n % np))
                .collect()
        };
        let at = |i: usize, k: usize| grid[i * np + k % np];
        let mut out: Vec<P2> = Vec::new();
        for i in 0..=nt {
            for k in 0..np {
                let v = at(i, k);
                let least = (i.saturating_sub(1)..=(i + 1).min(nt))
                    .all(|a| [k + np - 1, k, k + 1].iter().all(|&b| at(a, b) >= v));
                if !least {
                    continue;
                }
                let mut x = P2::new(i as f64 / nt as f64, TAU * k as f64 / np as f64);
                let mut found = false;
                for _ in 0..30 {
                    let w = self.across(x.x, x.y);
                    if w.norm() < 1e-13 * (1.0 + v) {
                        found = true;
                        break;
                    }
                    let Some(inv) = self.across_d(x.x, x.y).try_inverse() else {
                        break;
                    };
                    let step = inv * w;
                    if step.norm() > 0.05 {
                        break;
                    }
                    x -= step;
                    if !(0.0..=1.0).contains(&x.x) {
                        break;
                    }
                }
                let x = P2::new(x.x, wrap(x.y, 0.0, TAU));
                if found
                    && !out
                        .iter()
                        .any(|o| (o.x - x.x).abs() < 1e-6 && wrap(o.y - x.y, -PI, TAU).abs() < 1e-6)
                {
                    out.push(x);
                }
            }
        }
        out
    }

    /// The near pinches of the core circle's sweep: the (t, ψ) inside the motion where
    /// [`Self::across`] is least without being nothing (there its derivative is singular: the
    /// velocity across the circle folds over, a line of it passing by nothing), with its size
    /// there. Found from its least sizes over a grid, each refined on finer ones.
    fn find_near_pinches(&self, pinches: &[P2]) -> Vec<(P2, f64)> {
        let (nt, np) = (400, 256);
        let grid: Vec<f64> = {
            use rayon::prelude::*;
            (0..(nt + 1) * np)
                .into_par_iter()
                .map(|n| {
                    self.across(
                        (n / np) as f64 / nt as f64,
                        TAU * (n % np) as f64 / np as f64,
                    )
                    .norm()
                })
                .collect()
        };
        let at = |i: usize, k: usize| grid[i * np + k % np];
        let mut out: Vec<(P2, f64)> = Vec::new();
        for i in 1..nt {
            for k in 0..np {
                let v = at(i, k);
                let least =
                    (i - 1..=i + 1).all(|a| [k + np - 1, k, k + 1].iter().all(|&b| at(a, b) >= v));
                if !least {
                    continue;
                }
                let mut c = P2::new(i as f64 / nt as f64, TAU * k as f64 / np as f64);
                let mut r = V2::new(1.0 / nt as f64, TAU / np as f64);
                let mut best = v;
                for _ in 0..24 {
                    for a in -4i32..=4 {
                        for b in -4i32..=4 {
                            let w = P2::new(
                                (c.x + r.x * a as f64 / 4.0).clamp(0.0, 1.0),
                                c.y + r.y * b as f64 / 4.0,
                            );
                            let s = self.across(w.x, w.y).norm();
                            if s < best {
                                (best, c) = (s, w);
                            }
                        }
                    }
                    r *= 0.5;
                }
                let c = P2::new(c.x, wrap(c.y, 0.0, TAU));
                let near =
                    |o: &P2| (o.x - c.x).abs() < 1e-3 && wrap(o.y - c.y, -PI, TAU).abs() < 1e-3;
                if !pinches.iter().any(near) && !out.iter().any(|o| near(&o.0)) {
                    out.push((c, best));
                }
            }
        }
        out
    }

    /// The (t, ψ) of pinch `k`'s chart at (u, θ): where [`Self::across`] is square to the
    /// tube's normal at θ, u of the way to the seam.
    fn pinch_at(&self, k: usize, u: f64, th: f64) -> P2 {
        let pn = &self.pinches[k];
        let want = V2::new(-th.sin(), th.cos()) * (u * pn.rho(th));
        let mut x = pn.c + pn.inv * want;
        for _ in 0..8 {
            let miss = self.across(x.x.clamp(0.0, 1.0), x.y) - want;
            if miss.norm() <= 1e-14 * (1.0 + want.norm()) {
                break;
            }
            let Some(inv) = self.across_d(x.x.clamp(0.0, 1.0), x.y).try_inverse() else {
                break;
            };
            x -= inv * miss;
        }
        P2::new(x.x.clamp(0.0, 1.0), x.y)
    }

    /// The pinch whose chart covers the sheets' (t, ψ): where they are left off.
    fn in_pinch(&self, t: f64, psi: f64) -> Option<usize> {
        if self.pinches.is_empty() {
            return None;
        }
        let w = self.across(t, psi);
        self.pinches.iter().position(|pn| {
            let d = V2::new(t - pn.c.x, wrap(psi - pn.c.y, -PI, TAU));
            (pn.shape * w).norm() < 1.0
                && d.x.hypot(d.y * pn.reach.x / pn.reach.y) < 2.0 * pn.reach.x
        })
    }

    /// The place on pinch `k`'s chart of `x`, touched by the torus at the instant `t`.
    fn pinch_uv(&self, k: usize, t: f64, x: &P3) -> P2 {
        let j = self.motion.jet(t);
        let q = j.r[0].tr_mul(&(x.coords - j.d[0])) - self.c.coords;
        let h = q.dot(&self.z);
        let flat = q - self.z * h;
        let psi = flat.dot(&self.y).atan2(flat.dot(&self.x));
        let th = h.atan2(flat.dot(&self.e1(psi)) - self.big);
        let w = self.across(t, psi);
        P2::new(
            w.dot(&V2::new(-th.sin(), th.cos())) / self.pinches[k].rho(th),
            th,
        )
    }

    /// The ψ of the core circle's point under a sheet's or a pinch chart's place.
    fn psi_of(&self, chart: usize, w: P2) -> f64 {
        if let Some((k, _)) = self.pipe(chart) {
            self.lines[k]
        } else if let Some((j, _)) = self.band(chart) {
            self.band_at(j, w.x, w.y).y
        } else if chart >= PINCH {
            self.pinch_at(chart - PINCH, w.x, w.y).y
        } else {
            w.y
        }
    }

    /// The seam a chart's place is beyond, if it is: a sheet's inside a pinch (left to the
    /// pinch's chart), a pinch chart's past its `rho` (left to the sheets). As an instant no
    /// motion has, −1 less the pinch's index: what covers the place is the other chart.
    fn seam(&self, chart: usize, w: P2, own: f64) -> Option<f64> {
        let band_mark = |j: usize| -1.0 - (self.pinches.len() + j) as f64;
        if let Some((j, _)) = self.band(chart) {
            (self.bands[j].out(w) > 1.0).then_some(band_mark(j))
        } else if chart >= PINCH {
            (w.x.abs() > 1.0).then_some(-1.0 - (chart - PINCH) as f64)
        } else {
            self.in_pinch(own, w.y)
                .map(|k| -1.0 - k as f64)
                .or_else(|| self.in_band(own, w.y).map(band_mark))
        }
    }

    /// The line and the half of its pipe a chart is of, if it is a pipe's (the charts after
    /// the bands': two to a line).
    fn pipe(&self, chart: usize) -> Option<(usize, usize)> {
        let i = chart.checked_sub(PINCH + self.pinches.len() + 2 * self.bands.len())?;
        (i / 2 < self.lines.len()).then_some((i / 2, i % 2))
    }

    /// The chart of line `k`'s pipe's half.
    fn pipe_chart(&self, k: usize, half: usize) -> usize {
        PINCH + self.pinches.len() + 2 * self.bands.len() + 2 * k + half
    }

    /// How far round line `k`'s pipe a point the torus touches at `t` is, from the first
    /// sheet's join there: in half turns, from nothing to two.
    fn pipe_turn(&self, k: usize, t: f64, p: &P3) -> f64 {
        let line = self.lines[k];
        let j = self.motion.jet(t);
        let q = j.r[0].tr_mul(&(p.coords - j.d[0])) - self.c.coords;
        let h = q.dot(&self.z);
        let th = h.atan2((q - self.z * h).dot(&self.e1(line)) - self.big);
        let sp = self.sheet_pt(&j, line);
        wrap(th - self.theta_n(&j, &sp, line).0, 0.0, TAU) / PI
    }

    /// The band and the sheet a chart is of, if it is a band's.
    fn band(&self, chart: usize) -> Option<(usize, usize)> {
        let i = chart.checked_sub(PINCH + self.pinches.len())?;
        (i / 2 < self.bands.len()).then_some((i / 2, if i % 2 == 0 { LEAD } else { TRAIL }))
    }

    /// The chart of band `j` for a sheet.
    fn band_chart(&self, j: usize, sheet: usize) -> usize {
        PINCH + self.pinches.len() + 2 * j + usize::from(sheet == TRAIL)
    }

    /// The (t, ψ) of band `j`'s charts at (τ, η).
    fn band_at(&self, j: usize, tau: f64, eta: f64) -> P2 {
        let b = &self.bands[j];
        let x = b.c + b.ex * (b.mid(eta) + b.w * tau.sinh()) + b.ey * eta;
        P2::new(x.x.clamp(0.0, 1.0), x.y)
    }

    /// The place on band `j`'s charts of a (t, ψ).
    fn band_uv(&self, j: usize, t: f64, psi: f64) -> P2 {
        let b = &self.bands[j];
        let d = V2::new(t - b.c.x, wrap(psi - b.c.y, -PI, TAU));
        let eta = d.dot(&b.ey);
        P2::new(((d.dot(&b.ex) - b.mid(eta)) / b.w).asinh(), eta)
    }

    /// The band whose charts cover the sheets' (t, ψ): where they are left off.
    fn in_band(&self, t: f64, psi: f64) -> Option<usize> {
        (0..self.bands.len()).find(|&j| self.bands[j].out(self.band_uv(j, t, psi)) < 1.0)
    }

    /// The rate at which the distance of `p` from the core circle changes at the instant `t`.
    fn rate(&self, t: f64, p: &P3) -> f64 {
        let j = self.motion.jet(t);
        let w = p.coords - j.d[0];
        let q = j.r[0].tr_mul(&w) - self.c.coords;
        let h = q.dot(&self.z);
        let flat = q - self.z * h;
        let Some(e) = flat.try_normalize(1e-300) else {
            return 0.0;
        };
        let Some(u) = (q - e * self.big).try_normalize(1e-300) else {
            return 0.0;
        };
        u.dot(&(j.r[1].tr_mul(&w) - j.r[0].tr_mul(&j.d[1])))
    }

    /// The instant near `t` at which the distance of `p` from the core circle is stationary: `t`
    /// (a least found by search, good to the root of rounding) made exact.
    fn foot(&self, t: f64, p: &P3) -> f64 {
        let g = |t: f64| self.rate(t, p);
        let t = t.clamp(0.0, 1.0);
        let (mut t0, mut t1) = (t, if t + 1e-7 <= 1.0 { t + 1e-7 } else { t - 1e-7 });
        let (mut g0, mut g1) = (g(t0), g(t1));
        for _ in 0..8 {
            if g1 == g0 {
                break;
            }
            let t2 = t1 - g1 * (t1 - t0) / (g1 - g0);
            // (No further than a sample of the motion: beyond is another least.)
            if !(t2 - t).abs().le(&self.step()) {
                return t;
            }
            let t2 = t2.clamp(0.0, 1.0);
            (t0, g0) = (t1, g1);
            t1 = t2;
            g1 = g(t1);
            if (t1 - t0).abs() < 1e-15 {
                break;
            }
        }
        t1
    }

    /// Whether `x`, on the torus where the motion starts (`chart` START) or stops (END), is on
    /// the half of it the motion leaves bare: the side of its joins the torus moves away from
    /// as it sets out, or toward as it arrives.
    fn bare(&self, chart: usize, x: &P3) -> bool {
        // (On the join itself, to rounding, it is: the bare half's edge.)
        let slack = 1e-10 * self.size;
        if chart == START {
            self.rate(0.0, x) >= -slack
        } else {
            self.rate(1.0, x) <= slack
        }
    }

    /// The end of the motion the instant `tau` (a least of the distance of `x`, found by
    /// search) is, if it is one: within a millionth of it, with the distance still falling
    /// toward it (a least inside the motion, however near its end, is a sheet's).
    fn at_end(&self, tau: f64, x: &P3) -> Option<usize> {
        if tau <= 1e-6 && self.bare(START, x) {
            Some(START)
        } else if tau >= 1.0 - 1e-6 && self.bare(END, x) {
            Some(END)
        } else {
            None
        }
    }
}

/// [`crate::Kernel::sweep_solid`] of a whole torus by the rigid motion `steps`: None when `b`
/// isn't one; an error when its boundary isn't made out exactly.
/// A torus's sweep made ready to trace: its tube (with its pinches' and near pinches' charts),
/// its sheets' patches read by membership, and the tori where the motion starts and stops cut
/// along the joins.
struct Prep<'a> {
    tube: Tube<'a>,
    size: f64,
    /// How near its sheets the fitted surfaces keep.
    fit_tol: f64,
    /// Each face's patch: the sheets' first, then the tori's pieces.
    patches: Vec<Patch>,
    sheets: Vec<Sheet>,
    /// The torus where the motion starts and where it stops.
    tori: [super::surface::Surface; 2],
    /// The tori's pieces, each a body of one face.
    pieces: Vec<Body>,
    /// Whether a torus's piece holds a (ψ, θ), loosely.
    holds: Vec<Box<dyn Fn(P2) -> bool + Send + Sync>>,
    /// Whether a torus's piece is on the half of it the motion leaves bare.
    bare: Vec<bool>,
}

impl Prep<'_> {
    fn faces(&self) -> Faces<'_> {
        Faces {
            tube: &self.tube,
            patches: &self.patches,
            sheets: self.sheets.len(),
            holds: &self.holds,
            bare: &self.bare,
            tori: &self.tori,
        }
    }
}

/// [`torus`]'s first part: None when `b` isn't a torus; an error when its sweep isn't one the
/// charts cover.
fn prepare<'a>(b: &Body, motion: &'a Motion<'a>) -> Option<Result<Prep<'a>>> {
    use rayon::prelude::*;
    let [f] = b.faces.as_slice() else {
        return None;
    };
    let super::surface::Surface::Torus { f: fr, rmaj, rmin } = &f.surf else {
        return None;
    };
    if !(rmin < rmaj) || !f.sense || b.solids.len() != 1 {
        return None;
    }
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let t_start = web_time::Instant::now();
    let refused = |why: String| -> Option<Result<Prep<'a>>> {
        if debug {
            eprintln!("tube sweep: refused: {why}");
        }
        Some(Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!("the swept torus: {why}"),
        }))
    };
    let size = b.bbox().diagonal().max(super::tol::POINT);
    let mut tube = Tube::new(motion, fr, *rmaj, *rmin, size);
    // Σ regular (its tangents apart everywhere): its normal, and so the sheets, defined.
    let least = (0..=200usize)
        .into_par_iter()
        .map(|i| {
            let j = motion.jet(i as f64 / 200.0);
            (0..128)
                .map(|k| tube.sheet_pt(&j, TAU * k as f64 / 128.0).sin)
                .fold(f64::INFINITY, f64::min)
        })
        .reduce(|| f64::INFINITY, f64::min);
    tube.lines = tube.find_lines();
    if debug && !tube.lines.is_empty() {
        eprintln!("tube sweep: lines of pinches at ψ {:?}", tube.lines);
    }
    // (Each a side of the sheets' quarters, and an even number: the normal taken alternate
    // ways between them.)
    if tube.lines.len() % 2 == 1
        || tube.lines.iter().any(|l| {
            let q = l / (0.5 * PI);
            (q - q.round()).abs() > 1e-9
        })
    {
        return refused(format!(
            "its core circle's sweep pinches along lines its patches don't end at ({:?})",
            tube.lines
        ));
    }
    let on_line = |c: &P2| tube.line_by(c.y, 1e-3).is_some();
    let mut pinches = tube.find_pinches();
    pinches.retain(|c| !on_line(c));
    let mut near_pinches = tube.find_near_pinches(&pinches);
    near_pinches.retain(|c| !on_line(&c.0));
    if std::env::var_os("CADRS_TUBE_NEAR").is_some() {
        for &(c, least) in &near_pinches {
            let d = tube.across_d(c.x, c.y).svd(true, true);
            eprintln!(
                "  near pinch at ({:.6}, {:.6}): across {least:.4e}; its derivative's singular values {:.4e}, {:.4e}; across it {:?}, along it {:?}",
                c.x,
                c.y,
                d.singular_values[0],
                d.singular_values[1],
                d.v_t.as_ref().map(|v| v.row(0).transpose()),
                d.v_t.as_ref().map(|v| v.row(1).transpose())
            );
        }
    }
    if debug {
        eprintln!(
            "tube sweep: the core circle's sweep least regular {least:.5}; pinches {pinches:?}"
        );
    }
    if least < 1e-3 && pinches.is_empty() && tube.lines.is_empty() {
        return refused(format!("its core circle's sweep folds ({least:.4})"));
    }
    if let Some(c) = pinches.iter().find(|c| !(0.02..=0.98).contains(&c.x)) {
        return refused(format!(
            "its core circle's sweep pinches at the motion's end ({:.4})",
            c.x
        ));
    }
    // The sheets' patches: chunks of at most a quarter radian of turn by quarters of ψ, each
    // read by membership on a coarse grid, fitted where any of it is kept, and cut in four
    // where the fit misses.
    // (The turn along the motion, summed: a whole turn and back comes to nothing net.)
    let mut turn = 0.0;
    let mut prev = motion.place(0.0).0;
    for i in 1..=512 {
        let ri = motion.place(i as f64 / 512.0).0;
        turn += nalgebra::Rotation3::from_matrix_unchecked(prev.transpose() * ri).angle();
        prev = ri;
    }
    let sheet_chunks = ((turn / 0.25).ceil() as usize).max(2);
    let npsi = 4;
    // Each pinch's chart, out to a tenth of a patch from it each way.
    for c in pinches {
        let Some(inv) = tube.across_d(c.x, c.y).try_inverse() else {
            return refused("its core circle's sweep pinches along a curve".into());
        };
        let wide = std::env::var("CADRS_TUBE_REACH")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.1);
        let reach = V2::new(wide / sheet_chunks as f64, wide * TAU / npsi as f64);
        let shape = nalgebra::Matrix2::new(1.0 / reach.x, 0.0, 0.0, 1.0 / reach.y) * inv;
        tube.pinches.push(Pinch {
            c,
            inv,
            reach,
            shape,
        });
    }
    // Each near pinch's bands, where the sheets turn within less of (t, ψ) than their own
    // patches follow (a tenth of a patch along the motion, as a pinch's seam stands off): out
    // across the line to where the turn is that wide, and along it to where it is at its
    // sharpest.
    let follows = 0.1 / sheet_chunks as f64;
    for (c, least) in near_pinches {
        let svd = tube.across_d(c.x, c.y).svd(false, true);
        let (Some(vt), sigma) = (svd.v_t, svd.singular_values[0]) else {
            continue;
        };
        let w = least / sigma;
        if w >= follows {
            continue;
        }
        let (ex, ey) = (
            V2::new(vt[(0, 0)], vt[(0, 1)]),
            V2::new(vt[(1, 0)], vt[(1, 1)]),
        );
        // (The line itself, η along it: where across it the velocity across the circle is
        // least, followed out from the near pinch each way, with how wide the turn is there.)
        let least_at = |eta: f64, about: f64| -> (f64, f64) {
            let size = |xi: f64| {
                let x = c + ex * xi + ey * eta;
                tube.across(x.x.clamp(0.0, 1.0), x.y).norm()
            };
            let (mut at, mut h) = (about, follows);
            let mut best = size(at);
            for _ in 0..30 {
                for xi in [at - h, at + h] {
                    let v = size(xi);
                    if v < best {
                        (best, at) = (v, xi);
                    }
                }
                h *= 0.6;
            }
            (at, best / sigma)
        };
        let step = 0.01;
        let mut line: Vec<(f64, f64)> = vec![(0.0, 0.0)];
        let mut far: f64 = 0.0;
        for side in [1.0, -1.0] {
            let (mut eta, mut at) = (0.0, 0.0);
            // (Out to where the turn is wide enough, and a quarter as far again for the chart.)
            let mut end = f64::INFINITY;
            while eta < end {
                eta += step;
                let (xi, wide) = least_at(side * eta, at);
                at = xi;
                line.push((side * eta, xi));
                if wide >= follows && end.is_infinite() {
                    far = far.max(eta);
                    end = 1.3 * eta + step;
                }
                if eta > 1.5 {
                    return refused(format!(
                        "its core circle's sweep all but pinches along a curve (from {:.4}, {:.4})",
                        c.x, c.y
                    ));
                }
            }
        }
        // (A polynomial through the line's points, by least squares.)
        let bend = {
            let mut a = nalgebra::SMatrix::<f64, 5, 5>::zeros();
            let mut b = nalgebra::SVector::<f64, 5>::zeros();
            for &(eta, xi) in &line {
                let row = nalgebra::SVector::<f64, 5>::from_fn(|i, _| eta.powi(i as i32));
                a += row * row.transpose();
                b += row * xi;
            }
            let Some(x) = a.lu().solve(&b) else {
                return refused("a near pinch's line isn't made out".into());
            };
            [x[0], x[1], x[2], x[3], x[4]]
        };
        let band = Band {
            c,
            ex,
            ey,
            w,
            reach: V2::new((follows / w).asinh(), 1.2 * far),
            bend,
        };
        // (The chart's whole box inside the motion.)
        let inside = [-1.0, 1.0].iter().all(|&a| {
            [-1.0, 0.0, 1.0].iter().all(|&b| {
                let x = c
                    + ex * (band.mid(PINCH_REACH * band.reach.y * b)
                        + w * (PINCH_REACH * band.reach.x * a).sinh())
                    + ey * (PINCH_REACH * band.reach.y * b);
                (0.02..=0.98).contains(&x.x)
            })
        });
        if !inside {
            return refused(format!(
                "its core circle's sweep all but pinches at the motion's end ({:.4})",
                c.x
            ));
        }
        if debug {
            eprintln!(
                "tube sweep: a near pinch at ({:.6}, {:.6}), the turn {w:.3e} wide there: its bands reach {:.3} in τ, {:.3} along (the line's bend {:?})",
                c.x, c.y, band.reach.x, band.reach.y, band.bend
            );
        }
        tube.bands.push(band);
    }
    let tube = tube;
    if let Ok(v) = std::env::var("CADRS_TUBE_ACROSS") {
        // t, ψ, r: the least size of the velocity across the circle within r of there (r of ψ
        // in units of a turn), on a grid and then refined on finer ones.
        let v: Vec<f64> = v.split(',').filter_map(|w| w.trim().parse().ok()).collect();
        if let [t, psi, r] = v[..] {
            let (mut c, mut r) = (P2::new(t, psi), r);
            for _ in 0..8 {
                let mut best = (f64::INFINITY, c);
                for i in 0..=200 {
                    for k in 0..=200 {
                        let w = P2::new(
                            (c.x + r * (i as f64 / 100.0 - 1.0)).clamp(0.0, 1.0),
                            c.y + TAU * r * (k as f64 / 100.0 - 1.0),
                        );
                        let a = tube.across(w.x, w.y).norm();
                        if a < best.0 {
                            best = (a, w);
                        }
                    }
                }
                let d = tube.across_d(best.1.x, best.1.y);
                eprintln!(
                    "  across least {:.6e} at ({:.9}, {:.9}) within {r:.2e}: there {:?}, its derivative's determinant {:.4e} (columns {:?}, {:?})",
                    best.0,
                    best.1.x,
                    best.1.y,
                    tube.across(best.1.x, best.1.y),
                    d.determinant(),
                    d.column(0).transpose(),
                    d.column(1).transpose()
                );
                c = best.1;
                r *= 0.1;
            }
        }
    }
    if std::env::var_os("CADRS_TUBE_SEAM").is_some() {
        // (Each pinch's seam in (t, ψ), and how far the chart's place read back is off.)
        for (k, pn) in tube.pinches.iter().enumerate() {
            eprintln!(
                "  pinch {k} at ({:.6}, {:.6}), reach ({:.4}, {:.4}), {sheet_chunks} chunks",
                pn.c.x, pn.c.y, pn.reach.x, pn.reach.y
            );
            for i in 0..64 {
                let th = TAU * i as f64 / 64.0;
                let w = tube.pinch_at(k, 1.0, th);
                let x = tube
                    .eval(&Pos {
                        chart: PINCH + k,
                        a: 1.0,
                        psi: th,
                    })
                    .p;
                let back = tube.pinch_uv(k, w.x, &x);
                eprintln!(
                    "    θ {th:.4}: rho {:.4e}, (t, ψ) ({:.6}, {:.6}), least regular there {:.3e}, read back ({:.6}, {:.6})",
                    pn.rho(th),
                    w.x,
                    w.y,
                    tube.across(w.x, w.y).norm(),
                    back.x,
                    back.y
                );
            }
        }
    }
    let mut patches: Vec<Patch> = Vec::new();
    for k in 0..tube.pinches.len() {
        for q in 0..4 {
            patches.push((
                PINCH + k,
                -PINCH_REACH,
                PINCH_REACH,
                TAU * q as f64 / 4.0,
                TAU * (q + 1) as f64 / 4.0,
            ));
        }
    }
    for (j, b) in tube.bands.iter().enumerate() {
        for sheet in [LEAD, TRAIL] {
            patches.push((
                tube.band_chart(j, sheet),
                -PINCH_REACH * b.reach.x,
                PINCH_REACH * b.reach.x,
                -PINCH_REACH * b.reach.y,
                PINCH_REACH * b.reach.y,
            ));
        }
    }
    for k in 0..tube.lines.len() {
        for half in 0..2 {
            patches.push((tube.pipe_chart(k, half), 0.0, 1.0, 0.0, 1.0));
        }
    }
    for chart in [LEAD, TRAIL] {
        for q in 0..npsi {
            patches.push((
                chart,
                0.0,
                1.0,
                TAU * q as f64 / npsi as f64,
                TAU * (q + 1) as f64 / npsi as f64,
            ));
        }
    }
    // (How finely each patch is read by membership: a sheet's in sixteenths of a quarter
    // radian of turn by thirty-seconds of a quarter of ψ, a pinch's chart 32 by 32.)
    let grid_of = |p: Patch| -> (usize, usize) {
        if p.0 >= PINCH && tube.pipe(p.0).is_none() {
            (32, 32)
        } else {
            (16 * sheet_chunks, 32)
        }
    };
    if let Ok(probe) = std::env::var("CADRS_TUBE_PROBE") {
        // chart, a0, ψ0, a1, ψ1, n: membership along the segment.
        let v: Vec<f64> = probe
            .split(',')
            .filter_map(|w| w.trim().parse().ok())
            .collect();
        if let [chart, a0, p0, a1, p1, n] = v[..] {
            for i in 0..=n as usize {
                let s = i as f64 / n;
                let w = P2::new(a0 + (a1 - a0) * s, p0 + (p1 - p0) * s);
                let x = tube
                    .eval(&Pos {
                        chart: chart as usize,
                        a: w.x,
                        psi: w.y,
                    })
                    .p;
                let l = tube.least(&x);
                let c = tube.sheet_cover(chart as usize, w);
                let m = tube.places.len() - 1;
                {
                    let t = tube.foot(c.1.clamp(0.0, 1.0), &x);
                    let pos = tube.sheet_pos(t, &x);
                    let back = tube.eval(&pos).p;
                    eprintln!(
                        "  probe: the instant made exact {t:.12} (rate there {:.3e}, distance less r {:.3e}); on the sheets at chart {} ({:.9}, {:.9}), {:.3e} from it; across there {:?}, in pinch {:?}",
                        tube.rate(t, &x),
                        tube.f(t, &x),
                        pos.chart,
                        pos.a,
                        pos.psi,
                        (back - x).norm(),
                        tube.across(t, pos.psi),
                        tube.in_pinch(t, pos.psi)
                    );
                }
                eprintln!(
                    "  probe ({:.9}, {:.9}) [{:.7}, {:.7}, {:.7}]: covered {} at {:.9}; least {:.3e} at {:.9}; ends {:.3e}, {:.3e}; end cover {:?}; rates {:.3e}, {:.3e}; near own {:?}; at end {:?}",
                    w.x,
                    w.y,
                    x.x,
                    x.y,
                    x.z,
                    c.0,
                    c.1,
                    l.1,
                    l.0,
                    tube.f_at(&tube.places[0], &x),
                    tube.f_at(&tube.places[m], &x),
                    tube.end_cover(chart as usize, w),
                    tube.rate(0.0, &x),
                    tube.rate(1.0, &x),
                    tube.near_own(
                        &x,
                        tube.eval(&Pos {
                            chart: chart as usize,
                            a: w.x,
                            psi: w.y
                        })
                        .own
                    ),
                    tube.at_end(c.1, &x)
                );
            }
        }
    }
    // (How near its sheets the fitted surfaces keep: the curves between them, and what of each
    // is kept, are read off the sheets themselves, exactly.)
    let fit_tol = std::env::var("CADRS_TUBE_FIT")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(SHEET_FIT)
        * size;
    super::par::start();
    let mut faces: Vec<Body> = Vec::with_capacity(48);
    // Which side of its torus's joins each of the tori's pieces is: the half the motion leaves
    // bare, or the half it passes over.
    let mut torus_bare: Vec<bool> = Vec::new();
    let mut torus_patches: Vec<Patch> = Vec::new();
    // The torus where the motion starts (START) and stops (END), exactly, in quarters round and
    // halves up the tube (u = ψ round the axis, v = θ round the tube), each cut along the joins
    // (the circles k ± rN there, where the sheets meet it, tangent), so that the sheets' edges
    // are its pieces' edges; the pieces the motion covers are judged so.
    let torus_at = |t: f64| -> super::surface::Surface {
        let (rm, dm) = motion.place(t);
        super::surface::Surface::Torus {
            f: Frame {
                o: P3::from(rm * tube.c.coords + dm),
                x: rm * tube.x,
                y: rm * tube.y,
                z: rm * tube.z,
            },
            rmaj: tube.big,
            rmin: tube.r,
        }
    };
    let tori = [torus_at(0.0), torus_at(1.0)];
    // θ of the lead join (N's angle in the tube's section) at instant t and ψ; the trail join
    // is half a turn on.
    let theta_join = |t: f64, psi: f64| -> f64 {
        let j = motion.jet(t);
        let sp = tube.sheet_pt(&j, psi);
        tube.theta_n(&j, &sp, psi).0
    };
    // (The halves round the tube begun where the joins keep clearest of the quarters' corners:
    // a join through a corner is a cut of no length there.)
    let v_off = (0..64)
        .map(|j| PI * j as f64 / 64.0)
        .max_by(|a, b| {
            let clear = |c: f64| {
                [0.0, 1.0]
                    .iter()
                    .flat_map(|&t| (0..4).map(move |q| (t, TAU * q as f64 / 4.0)))
                    .map(|(t, psi)| {
                        let d = wrap(theta_join(t, psi) - c, 0.0, PI);
                        d.min(PI - d)
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            clear(*a).total_cmp(&clear(*b))
        })
        .unwrap_or(0.0);
    // (Each quarter's cuts on its own: they share nothing.)
    let quarters: Vec<(usize, f64, usize, usize)> = [(START, 0.0), (END, 1.0)]
        .into_iter()
        .flat_map(|(chart, t_end)| {
            (0..4).flat_map(move |q| (0..2).map(move |h| (chart, t_end, q, h)))
        })
        .collect();
    let cut: Vec<Result<Vec<(Body, Patch, bool)>>> = quarters
        .par_iter()
        .map(|&(chart, t_end, q, h)| {
            let surf = &tori[usize::from(t_end > 0.5)];
            let mut out: Vec<(Body, Patch, bool)> = Vec::new();
            {
                let (u0, u1) = (TAU * q as f64 / 4.0, TAU * (q + 1) as f64 / 4.0);
                let (v0, v1) = (v_off + PI * h as f64, v_off + PI * (h + 1) as f64);
                let quarter =
                    match super::face_build::face_from_surface(surf, Some(((u0, u1), (v0, v1)))) {
                        Ok(b) => b,
                        Err(e) => return Err(e),
                    };
                // The joins' curves across this quarter: θ_J(ψ) (continuous in ψ) brought into
                // [v0, v1] by whole turns, each stretch inside a cut from side to side.
                let mut cuts = Vec::new();
                let mut pos: HashMap<usize, P3> = HashMap::new();
                let nb = quarter.verts.len();
                let mut next_id = nb;
                for half in [0.0, PI] {
                    let n = 128;
                    let mut th: Vec<f64> = Vec::with_capacity(n + 1);
                    for i in 0..=n {
                        let psi = u0 + (u1 - u0) * i as f64 / n as f64;
                        let mut v = theta_join(t_end, psi) + half;
                        if let Some(&last) = th.last() {
                            // (Continuous: the turn nearest the last.)
                            v += ((last - v) / TAU).round() * TAU;
                        }
                        th.push(v);
                    }
                    let theta_at = |psi: f64, k: f64| -> f64 {
                        // (θ_J near the sampled values: the sample's turn.)
                        let i = (((psi - u0) / (u1 - u0)) * n as f64)
                            .round()
                            .clamp(0.0, n as f64) as usize;
                        let v = theta_join(t_end, psi) + half;
                        v + ((th[i] - v) / TAU).round() * TAU + k * TAU
                    };
                    for k in [-2.0, -1.0, 0.0, 1.0, 2.0] {
                        // Where θ_J + k·2π runs within [v0, v1]: stretches between crossings
                        // of the sides, found by bisection on it.
                        let inside = |v: f64| (v0..=v1).contains(&v);
                        let mut run: Vec<(P3, P2)> = Vec::new();
                        let mut flush =
                            |run: &mut Vec<(P3, P2)>, cuts: &mut Vec<super::sweep_moving::FaceCut>| {
                                if run.len() >= 2 {
                                    // The curve through the points, with points added midway
                                    // (in ψ) wherever it strays from the join there: the
                                    // join's angle swings fast where the core's sweep nearly
                                    // folds.
                                    let fit = |run: &[(P3, P2)]| {
                                        let mut params = vec![0.0];
                                        for w in run.windows(2) {
                                            params.push(
                                                params.last().unwrap() + (w[1].0 - w[0].0).norm(),
                                            );
                                        }
                                        let p3: Vec<P3> = run.iter().map(|p| p.0).collect();
                                        // (Quintic: within a ten-billionth of the size
                                        // from far fewer points than a cubic takes.)
                                        let deg = match p3.len() {
                                            6.. => 5,
                                            4.. => 3,
                                            _ => 1,
                                        };
                                        (
                                            super::nurbs::NurbsCurve::interpolate(
                                                &p3, &params, deg, None, None,
                                            ),
                                            params,
                                        )
                                    };
                                    let mut c = fit(run).0;
                                    for _ in 0..14 {
                                        let (curve, params) = fit(run);
                                        let mut add: Vec<(usize, (P3, P2))> = Vec::new();
                                        for i in 0..run.len() - 1 {
                                            let psi = 0.5 * (run[i].1.x + run[i + 1].1.x);
                                            if (run[i + 1].1.x - run[i].1.x).abs() < 1e-9 {
                                                continue;
                                            }
                                            let v = theta_at(psi, k).clamp(v0, v1);
                                            let x = surf.eval(psi, v);
                                            // (How far the curve passes from the join's point there: by its
                                                    // nearest point, not one at a like parameter, which is off
                                                    // along the curve.)
                                                    let off = {
                                                let d = |t: f64| (curve.point(t) - x).norm();
                                                let g = 0.5 * (5f64.sqrt() - 1.0);
                                                let (mut lo, mut hi) = (params[i], params[i + 1]);
                                                for _ in 0..40 {
                                                    let (c, e) =
                                                        (hi - g * (hi - lo), lo + g * (hi - lo));
                                                    if d(c) < d(e) {
                                                        hi = e;
                                                    } else {
                                                        lo = c;
                                                    }
                                                }
                                                d(0.5 * (lo + hi))
                                            };
                                            if off > 1e-10 * size {
                                                add.push((i + 1, (x, P2::new(psi, v))));
                                            }
                                        }
                                        c = curve;
                                        if add.is_empty() {
                                            break;
                                        }
                                        for (n, (i, p)) in add.into_iter().enumerate() {
                                            run.insert(i + n, p);
                                        }
                                    }
                                    let c = Curve::Nurbs(std::sync::Arc::new(c));
                                    let v = [next_id, next_id + 1];
                                    next_id += 2;
                                    pos.insert(v[0], run[0].0);
                                    pos.insert(v[1], run[run.len() - 1].0);
                                    cuts.push(super::sweep_moving::FaceCut {
                                        c,
                                        pts: std::mem::take(run),
                                        v,
                                        params: None,
                                    });
                                }
                                run.clear();
                            };
                        let point = |psi: f64, v: f64| (surf.eval(psi, v), P2::new(psi, v));
                        for i in 0..=n {
                            let psi = u0 + (u1 - u0) * i as f64 / n as f64;
                            let v = th[i] + k * TAU;
                            let was_in = i > 0 && inside(th[i - 1] + k * TAU);
                            if inside(v) {
                                if !was_in && i > 0 {
                                    // In from a side: the crossing between the samples.
                                    let side = if v > th[i - 1] + k * TAU { v0 } else { v1 };
                                    let (mut lo, mut hi) =
                                        (u0 + (u1 - u0) * (i - 1) as f64 / n as f64, psi);
                                    for _ in 0..60 {
                                        let m = 0.5 * (lo + hi);
                                        if inside(theta_at(m, k)) {
                                            hi = m;
                                        } else {
                                            lo = m;
                                        }
                                    }
                                    run.push(point(hi, side));
                                }
                                run.push(point(psi, v));
                            } else if was_in {
                                // Out through a side.
                                let side = if v > v1 { v1 } else { v0 };
                                let (mut lo, mut hi) =
                                    (u0 + (u1 - u0) * (i - 1) as f64 / n as f64, psi);
                                for _ in 0..60 {
                                    let m = 0.5 * (lo + hi);
                                    if inside(theta_at(m, k)) {
                                        lo = m;
                                    } else {
                                        hi = m;
                                    }
                                }
                                run.push(point(lo, side));
                                flush(&mut run, &mut cuts);
                            }
                        }
                        flush(&mut run, &mut cuts);
                    }
                }
                let pieces = if cuts.is_empty() {
                    vec![quarter]
                } else {
                    match super::sweep_moving::cut_face(
                        &quarter,
                        0,
                        cuts,
                        &pos,
                        size,
                        1e-7 * size,
                        None,
                    ) {
                        Ok(ps) => ps,
                        Err(why) => {
                            return Err(KernelError::InvalidResult {
                                op: "solid sweep",
                                why: format!("the swept torus: its torus along the joins: {why}"),
                            });
                        }
                    }
                };
                if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                    let js: Vec<String> = [u0, 0.5 * (u0 + u1), u1]
                        .iter()
                        .map(|&psi| {
                            let th = theta_join(t_end, psi);
                            // (v · n on the torus there, which the join should make zero, and
                            // a little either side of it.)
                            let j = motion.jet(t_end);
                            let vn = |th: f64| {
                                let (_, _, tt) = tube.torus_pt(&j, psi, th);
                                let (p, _, _) = tube.torus_pt(&j, psi, th);
                                let sp = tube.sheet_pt(&j, psi);
                                let n = (p.coords - sp.s[0]).normalize();
                                let vel = sp.s[1] + j.r[1] * (j.r[0].tr_mul(&(p.coords - sp.s[0])));
                                let _ = tt;
                                vel.dot(&n)
                            };
                            format!(
                                "ψ {psi:.3}: θ_J {:.4} (v·n {:.2e}; at θ_J±0.1: {:.2e}, {:.2e})",
                                wrap(th, 0.0, TAU),
                                vn(th),
                                vn(th - 0.1),
                                vn(th + 0.1)
                            )
                        })
                        .collect();
                    eprintln!(
                        "  torus quarter {chart} u {u0:.4}..{u1:.4} v {v0:.4}..{v1:.4}: {} pieces; joins {js:?}",
                        pieces.len()
                    );
                }
                let core = |uv: P2| -> P3 {
                    let super::surface::Surface::Torus { f, rmaj, .. } = surf else {
                        unreachable!()
                    };
                    f.o + (f.x * uv.x.cos() + f.y * uv.x.sin()) * *rmaj
                };
                for mut piece in pieces {
                    let polys = super::classify::face_polygons(&piece, &piece.faces[0]);
                    let Some(&uv) = super::boolean::interior_points_of_polys(&polys).first() else {
                        continue;
                    };
                    let f = &piece.faces[0];
                    let (p, su, sv) = f.surf.d1(uv.x, uv.y);
                    let natural = su.cross(&sv) * if f.sense { 1.0 } else { -1.0 };
                    if natural.dot(&(p - core(uv))) < 0.0 {
                        Body::reverse_face(&mut piece.faces[0]);
                    }
                    let bare = tube.bare(chart, &p);
                    out.push((piece, (chart, u0, u1, v0, v1), bare));
                }
            }
            Ok(out)
        })
        .collect();
    for c in cut {
        match c {
            Ok(pieces) => {
                for (piece, patch, bare) in pieces {
                    faces.push(piece);
                    torus_patches.push(patch);
                    torus_bare.push(bare);
                }
            }
            Err(e) => return Some(Err(e)),
        }
    }
    // Where each face is, for the curves' other sides.
    // (Loosely: a join between two pieces is told exactly, by the side of it a point is on, and
    // pieces on one side of the joins are well apart.)
    // (The cuts are polygons here, off the joins by their chords' sag.)
    let hold_tol = 1e-4 * size;
    let holds: Vec<Box<dyn Fn(P2) -> bool + Send + Sync>> = faces
        .iter()
        .map(|b| {
            let polys = super::classify::face_polygons(b, &b.faces[0]);
            let surf = b.faces[0].surf.clone();
            Box::new(move |uv: P2| {
                super::classify::in_face(&polys, &surf, uv, hold_tol)
                    != super::classify::InFace::Outside
            }) as Box<dyn Fn(P2) -> bool + Send + Sync>
        })
        .collect();
    let sheets: Vec<Sheet> = patches
        .iter()
        .map(|&p| Sheet::read(&tube, p, grid_of(p)))
        .collect();
    let kept_patches: Vec<Patch> = sheets
        .iter()
        .map(|sh| sh.patch)
        .chain(torus_patches.iter().copied())
        .collect();
    if debug {
        eprintln!(
            "tube sweep: the tori's {} pieces cut along the joins and {} sheet patches read by {:.0} ms",
            faces.len(),
            sheets.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    Some(Ok(Prep {
        tube,
        size,
        fit_tol,
        patches: kept_patches,
        sheets,
        tori,
        pieces: faces,
        holds,
        bare: torus_bare,
    }))
}

/// [`crate::Kernel::sweep_solid`] of a whole torus by a rigid motion: None when `b` isn't one;
/// an error when its boundary isn't made out exactly.
pub(crate) fn torus(k: &mut NativeKernel, b: &Body, motion: &Motion) -> Option<Result<OpResult>> {
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let t_start = web_time::Instant::now();
    let refused = |why: String| -> Option<Result<OpResult>> {
        if debug {
            eprintln!("tube sweep: refused: {why}");
        }
        Some(Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!("the swept torus: {why}"),
        }))
    };
    let prep = match prepare(b, motion)? {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    let (tube, size, fit_tol) = (&prep.tube, prep.size, prep.fit_tol);
    let place_r = |t: f64| {
        let (m, d) = motion.place(t);
        (nalgebra::Rotation3::from_matrix_unchecked(m), d)
    };
    let (c, z, big, r) = (tube.c, tube.z, tube.big, tube.r);
    let oracle = super::sweep_moving::Oracle::with_measure(
        Box::new(move |q: &V3| {
            let w = q - c.coords;
            let h = w.dot(&z);
            ((w - z * h).norm() - big).hypot(h) - r
        }),
        &place_r,
        2048,
    );
    let (sheets, kept_patches, tori) = (&prep.sheets, &prep.patches, &prep.tori);
    let n_sheets = sheets.len();
    let delta = tube.delta();
    let (fits, given, on_sheet) = {
        let t_fitted = web_time::Instant::now();
        let mut given = trace(&prep, 0, None, &[]);
        drop_twins(&mut given, &|i| i < n_sheets, size, fit_tol);
        let n_sheet_curves = given.len();
        tori_cross((&prep, 0, START), (&prep, 0, END), &mut given);
        let t_traced = web_time::Instant::now();
        if debug {
            let ms = |a: web_time::Instant, b: web_time::Instant| (b - a).as_secs_f64() * 1e3;
            eprintln!(
                "tube sweep: {} sheet patches of {} tried ({:.0} ms), {} faces with the tori's; {} curves on the sheets and {} between the tori ({:.0} ms)",
                n_sheets,
                n_sheets,
                ms(t_start, t_fitted),
                kept_patches.len(),
                n_sheet_curves,
                given.len() - n_sheet_curves,
                ms(t_fitted, t_traced),
            );
            // Curves' ends near one another but not together.
            let ends = std::env::var_os("CADRS_TUBE_ENDS").is_some();
            for (i, g) in given.iter().enumerate().filter(|_| ends) {
                for (j, h) in given.iter().enumerate().skip(i + 1) {
                    for p in [g.pts[0], g.pts[g.pts.len() - 1]] {
                        for q in [h.pts[0], h.pts[h.pts.len() - 1]] {
                            let d = (p.0 - q.0).norm();
                            if d > 1e-9 * size && d < 2e-6 * size {
                                eprintln!(
                                    "  ends {d:.2e} apart: curve {i} of faces ({}, {}) at {:?} | {:?}, curve {j} of faces ({}, {}) at {:?} | {:?}; {:?}",
                                    g.a, g.b, p.1, p.2, h.a, h.b, q.1, q.2, p.0
                                );
                                for x in [p.0, q.0] {
                                    let l = tube.least(&x);
                                    eprintln!(
                                        "    there: ends {:.3e}, {:.3e}; rates {:.3e}, {:.3e}; least {:.3e} at {:.9} (exact {:.12})",
                                        tube.f(0.0, &x),
                                        tube.f(1.0, &x),
                                        tube.rate(0.0, &x),
                                        tube.rate(1.0, &x),
                                        l.1,
                                        l.0,
                                        tube.foot(l.0, &x)
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if std::env::var_os("CADRS_TUBE_CURVES").is_some() {
                for g in &given {
                    let (p, q) = (g.pts[0], g.pts[g.pts.len() - 1]);
                    eprintln!(
                        "  curve of faces ({}, {}): {} points, {:?} -> {:?}; on the first ({:.5}, {:.5}) -> ({:.5}, {:.5}), on the second ({:.5}, {:.5}) -> ({:.5}, {:.5})",
                        g.a,
                        g.b,
                        g.pts.len(),
                        p.0,
                        q.0,
                        p.1.x,
                        p.1.y,
                        q.1.x,
                        q.1.y,
                        p.2.x,
                        p.2.y,
                        q.2.x,
                        q.2.y
                    );
                    // (A step much longer than its neighbours: a stretch the tracing left
                    // unfilled.)
                    let steps: Vec<f64> =
                        g.pts.windows(2).map(|w| (w[1].0 - w[0].0).norm()).collect();
                    for i in 0..steps.len() {
                        let about = steps[i.saturating_sub(3)..(i + 4).min(steps.len())]
                            .iter()
                            .filter(|&&s| s < steps[i])
                            .fold(0.0, |a: f64, &s| a.max(s));
                        if about > 0.0 && steps[i] > 8.0 * about {
                            let (p, q) = (g.pts[i], g.pts[i + 1]);
                            eprintln!(
                                "    a gap of {:.2e} (steps about it {:.2e}) after point {i}: on the first ({:.6}, {:.6}) -> ({:.6}, {:.6}), on the second ({:.6}, {:.6}) -> ({:.6}, {:.6})",
                                steps[i],
                                about,
                                p.1.x,
                                p.1.y,
                                q.1.x,
                                q.1.y,
                                p.2.x,
                                p.2.y,
                                q.2.x,
                                q.2.y
                            );
                        }
                    }
                    if std::env::var("CADRS_TUBE_CURVES")
                        .is_ok_and(|v| v == format!("{},{}", g.a, g.b))
                    {
                        for p in &g.pts {
                            eprintln!(
                                "    ({:.9}, {:.9}) | ({:.9}, {:.9}) at {:?}",
                                p.1.x, p.1.y, p.2.x, p.2.y, p.0
                            );
                        }
                    }
                }
            }
        }
        let (fits, on_sheet) = match fit_sheets(&prep, 0, &given) {
            Ok(f) => f,
            Err(why) => return refused(why),
        };
        if debug {
            eprintln!(
                "tube sweep: {} sheets fitted by {:.0} ms, {} poles in all: {:?}",
                fits.len(),
                t_start.elapsed().as_secs_f64() * 1e3,
                fits.iter()
                    .map(|f| f.0.cps.len() * f.0.cps[0].len())
                    .sum::<usize>(),
                fits.iter()
                    .map(|f| (f.0.cps.len(), f.0.cps[0].len()))
                    .collect::<Vec<_>>()
            );
        }
        (fits, given, on_sheet)
    };
    let kept_fits = matches!(motion, Motion::Steps(_)).then(|| fits.clone());
    let faces = match sheet_faces(&prep, fits, &on_sheet) {
        Ok(f) => f,
        Err(e) => return Some(Err(e)),
    };
    // Whether a face's place is inside the swept solid: a sheet's by its exact point there (not
    // the fitted one, a fit's miss off it), a torus's by its point, which no other instant may
    // hold.
    let member = |i: usize, uv: P2| -> bool {
        let p = kept_patches[i];
        if i < n_sheets {
            tube.cover_in(&p, uv).0
        } else {
            let x = tori[usize::from(p.0 == END)].eval(uv.x, uv.y);
            tube.least(&x).1 < -delta
        }
    };
    let least = 2.0 * PI * PI * big * r * r;
    // (The tori's pieces, cut in quarters to be trimmed, made whole again in the joining.)
    let on_torus = |s: &super::surface::Surface| matches!(s, super::surface::Surface::Torus { .. });
    let solid = match super::sweep_moving::boundary_of_some(
        faces,
        &oracle,
        size,
        least,
        f64::INFINITY,
        given,
        Some(&member),
        fit_tol,
        true,
        Some(&on_torus),
    ) {
        Ok(s) => s,
        Err(e) => {
            if debug {
                eprintln!("tube sweep: refused: {e}");
            }
            return Some(Err(e));
        }
    };
    if let Some(path) = std::env::var_os("CADRS_TUBE_OUT") {
        let _ = std::fs::write(path, super::bodyio::write(&solid));
    }
    let id = k.insert(solid);
    if let (Motion::Steps(steps), Some(fits)) = (motion, kept_fits) {
        k.swept_tori.insert(
            id.0,
            std::sync::Arc::new(SweptTorus {
                torus: b.clone(),
                steps: steps.to_vec(),
                fits,
            }),
        );
    }
    Some(Ok(OpResult {
        bodies: vec![id],
        history: History::default(),
    }))
}

/// A swept torus as it was asked for: the torus and its motion.
pub(crate) struct SweptTorus {
    torus: Body,
    steps: Vec<RigidStep>,
    /// Its sheets' fits, in their patches' order: a boolean keeps parts of the same sheets.
    fits: Vec<(super::nurbs::NurbsSurface, Dom)>,
}

/// A boolean of two swept tori, by what each motion holds: every face of either is traced in
/// its own parameters where the other's motion comes to hold its points (as it is where its
/// own does), and its pieces kept by the operation: outside the other for a union, inside it
/// for an intersection, the first's outside and the second's inside (turned about) for a
/// difference. None or an error when it isn't made out so (the caller's boolean of the two
/// bodies then stands).
pub(crate) fn boolean(
    k: &mut NativeKernel,
    op: crate::BoolOp,
    a: &SweptTorus,
    b: &SweptTorus,
) -> Option<Result<OpResult>> {
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let t_start = web_time::Instant::now();
    let refused = |why: String| -> Option<Result<OpResult>> {
        if debug {
            eprintln!("tube boolean: refused: {why}");
        }
        Some(Err(KernelError::InvalidResult {
            op: "boolean",
            why: format!("the swept tori: {why}"),
        }))
    };
    let (ma, mb) = (Motion::Steps(&a.steps), Motion::Steps(&b.steps));
    let pa = match prepare(&a.torus, &ma)? {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    let pb = match prepare(&b.torus, &mb)? {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    let (na, nsa, nsb) = (pa.patches.len(), pa.sheets.len(), pb.sheets.len());
    // (Whether each solid's faces are kept inside the other.)
    let (in_a, in_b) = match op {
        crate::BoolOp::Union => (false, false),
        crate::BoolOp::Intersect => (true, true),
        crate::BoolOp::Subtract => (false, true),
    };
    let beside_a = Other {
        prep: &pb,
        base: na,
        inside: in_a,
    };
    let beside_b = Other {
        prep: &pa,
        base: 0,
        inside: in_b,
    };
    let size = pa.size.max(pb.size);
    let fit_tol = pa.fit_tol.max(pb.fit_tol);
    // The nodes of the two boundaries' crossing: where the motions place the torus alike, the
    // points of it neither moves across itself. Both solids' sheets touch it there, and each
    // other.
    let poses = shared_poses(&pa.tube, &pb.tube);
    if poses.len() > 4 {
        return refused("the two motions run together".into());
    }
    let mut nodes_a: Vec<(P3, f64, f64)> = Vec::new();
    for &(ta, tb) in &poses {
        for chart in [LEAD, TRAIL] {
            let at = |psi: f64| {
                pa.tube
                    .eval(&Pos {
                        chart,
                        a: if chart == LEAD { ta } else { 1.0 - ta },
                        psi,
                    })
                    .p
            };
            let rate = |psi: f64| pb.tube.rate(tb, &at(psi));
            let n = 720;
            for k in 0..n {
                let (mut lo, mut hi) = (TAU * k as f64 / n as f64, TAU * (k + 1) as f64 / n as f64);
                let (rl, rh) = (rate(lo), rate(hi));
                if rl == 0.0 || rl * rh < 0.0 {
                    for _ in 0..60 {
                        let m = 0.5 * (lo + hi);
                        if rate(m) * rl > 0.0 {
                            lo = m;
                        } else {
                            hi = m;
                        }
                    }
                    nodes_a.push((at(0.5 * (lo + hi)), ta, tb));
                }
            }
        }
    }
    let nodes_b: Vec<(P3, f64, f64)> = nodes_a.iter().map(|&(x, ta, tb)| (x, tb, ta)).collect();
    if debug {
        eprintln!(
            "tube boolean: the motions place the torus alike at {poses:?}; nodes {:?}",
            nodes_a.iter().map(|n| n.0).collect::<Vec<_>>()
        );
    }
    let mut given = trace(&pa, 0, Some(&beside_a), &nodes_a);
    given.extend(trace(&pb, na, Some(&beside_b), &nodes_b));
    let is_sheet = |i: usize| i < nsa || (na..na + nsb).contains(&i);
    drop_twins(&mut given, &is_sheet, size, fit_tol);
    let n_sheet_curves = given.len();
    // The end tori's crossings: each solid's own two, and each of the one's with each of the
    // other's.
    tori_cross((&pa, 0, START), (&pa, 0, END), &mut given);
    tori_cross((&pb, na, START), (&pb, na, END), &mut given);
    for ca in [START, END] {
        for cb in [START, END] {
            tori_cross((&pa, 0, ca), (&pb, na, cb), &mut given);
        }
    }
    if debug {
        eprintln!(
            "tube boolean: {} curves on the sheets and {} between the tori by {:.0} ms",
            n_sheet_curves,
            given.len() - n_sheet_curves,
            t_start.elapsed().as_secs_f64() * 1e3
        );
        if std::env::var_os("CADRS_TUBE_CURVES").is_some() {
            for g in &given {
                let (p, q) = (g.pts[0], g.pts[g.pts.len() - 1]);
                eprintln!(
                    "  curve of faces ({}, {}): {} points, {:?} -> {:?}; on the first ({:.5}, {:.5}) -> ({:.5}, {:.5}), on the second ({:.5}, {:.5}) -> ({:.5}, {:.5})",
                    g.a,
                    g.b,
                    g.pts.len(),
                    p.0,
                    q.0,
                    p.1.x,
                    p.1.y,
                    q.1.x,
                    q.1.y,
                    p.2.x,
                    p.2.y,
                    q.2.x,
                    q.2.y
                );
                if std::env::var("CADRS_TUBE_CURVES").is_ok_and(|v| v == format!("{},{}", g.a, g.b))
                {
                    for p in &g.pts {
                        eprintln!(
                            "    ({:.9}, {:.9}) | ({:.9}, {:.9}) at {:?}",
                            p.1.x, p.1.y, p.2.x, p.2.y, p.0
                        );
                    }
                }
            }
        }
    }
    let mut faces = Vec::with_capacity(na + pb.patches.len());
    for (prep, base, swept) in [(&pa, 0, a), (&pb, na, b)] {
        // (Each sheet's fit as its own sweep made it: what a boolean keeps of a sheet is
        // part of what that kept, where the fit holds.)
        let (fits, on_sheet) = if swept.fits.len() == prep.sheets.len() {
            (swept.fits.clone(), vec![Vec::new(); prep.sheets.len()])
        } else {
            match fit_sheets(prep, base, &given) {
                Ok(f) => f,
                Err(why) => return refused(why),
            }
        };
        let mut fs = match sheet_faces(prep, fits, &on_sheet) {
            Ok(f) => f,
            Err(e) => return Some(Err(e)),
        };
        // (A difference's second solid bounds it from within: its faces turned about.)
        if base > 0 && op == crate::BoolOp::Subtract {
            for f in &mut fs {
                Body::reverse_face(&mut f.faces[0]);
            }
        }
        faces.extend(fs);
    }
    if debug {
        eprintln!(
            "tube boolean: {} faces fitted by {:.0} ms",
            faces.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    // Whether a face's place is dropped: inside its own solid (as a sweep's is), or on the
    // side of the other the operation doesn't keep.
    let member = |i: usize, uv: P2| -> bool {
        let (own, o, i) = if i < na {
            (&pa, &beside_a, i)
        } else {
            (&pb, &beside_b, i - na)
        };
        let p = own.patches[i];
        let x = if i < own.sheets.len() {
            if own.tube.cover_in(&p, uv).0 {
                return true;
            }
            own.tube
                .eval(&Pos {
                    chart: p.0,
                    a: uv.x,
                    psi: uv.y,
                })
                .p
        } else {
            let x = own.tori[usize::from(p.0 == END)].eval(uv.x, uv.y);
            if own.tube.least(&x).1 < -own.tube.delta() {
                return true;
            }
            x
        };
        (o.prep.tube.least(&x).1 < -o.prep.tube.delta()) != o.inside
    };
    let place_r = |t: f64| {
        let (m, d) = ma.place(t);
        (nalgebra::Rotation3::from_matrix_unchecked(m), d)
    };
    let (c, z, big, r) = (pa.tube.c, pa.tube.z, pa.tube.big, pa.tube.r);
    let oracle = super::sweep_moving::Oracle::with_measure(
        Box::new(move |q: &V3| {
            let w = q - c.coords;
            let h = w.dot(&z);
            ((w - z * h).norm() - big).hypot(h) - r
        }),
        &place_r,
        64,
    );
    let on_torus = |s: &super::surface::Surface| matches!(s, super::surface::Surface::Torus { .. });
    let solid = match super::sweep_moving::boundary_of_some(
        faces,
        &oracle,
        size,
        0.0,
        f64::INFINITY,
        given,
        Some(&member),
        fit_tol,
        false,
        Some(&on_torus),
    ) {
        Ok(s) => s,
        Err(e) => {
            if debug {
                eprintln!("tube boolean: refused: {e}");
            }
            return Some(Err(e));
        }
    };
    if debug {
        eprintln!(
            "tube boolean: {} faces, {} solids by {:.0} ms",
            solid.faces.len(),
            solid.solids.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    let id = k.insert(solid);
    Some(Ok(OpResult {
        bodies: vec![id],
        history: History::default(),
    }))
}

/// Where one end torus crosses another (a motion that comes back near where it began, or two
/// swept tori's ends): the zero set of the one's distance over the other's (ψ, θ), kept where
/// both are bare there and no instant of either motion covers it. Each is given by its solid,
/// its faces' first index and its chart (START or END).
fn tori_cross(
    (pa, base_a, chart_a): (&Prep, usize, usize),
    (pb, base_b, chart_b): (&Prep, usize, usize),
    given: &mut Vec<super::sweep_moving::Given>,
) {
    let (ta, tb) = (
        f64::from(u8::from(chart_a == END)),
        f64::from(u8::from(chart_b == END)),
    );
    let first = &pa.tori[usize::from(chart_a == END)];
    let (fa, fb) = (pa.faces(), pb.faces());
    let (da, db) = (pa.tube.delta(), pb.tube.delta());
    let jump = 4.0 * pa.tube.step();
    let same = std::ptr::eq(pa, pb);
    let far = |w: P2| {
        let x = first.eval(w.x, w.y);
        (pb.tube.f(tb, &x) < -db, tb)
    };
    let near = |w: P2, _: f64| far(w);
    let fd = Field {
        cover: &far,
        near: &near,
        margin: None,
    };
    for q in 0..4 {
        for h in 0..2 {
            let bx = (
                (TAU * q as f64 / 4.0, TAU * (q + 1) as f64 / 4.0),
                (PI * h as f64, PI * (h + 1) as f64),
            );
            let chains = cover_curves(
                &fd,
                bx,
                (24, 24),
                None,
                1e-6,
                jump,
                &|w: P2| first.eval(w.x, w.y),
                pa.fit_tol,
                &[],
            )
            .unwrap_or_default();
            let pair = |w: P2, _: f64, prefer: Option<(usize, usize)>| {
                let x = first.eval(w.x, w.y);
                // (On the half of each torus the motion leaves bare: past its join, the
                // sheet from there is the boundary; and under no instant between.)
                if !pa.tube.bare(chart_a, &x)
                    || !pb.tube.bare(chart_b, &x)
                    || pa.tube.least(&x).1 < -100.0 * da
                    || (!same && pb.tube.least(&x).1 < -100.0 * db)
                {
                    return None;
                }
                let (f0, uva) = fa.at(&x, ta, prefer.and_then(|p| p.0.checked_sub(base_a)))?;
                let (f1, uvb) = fb.at(&x, tb, prefer.and_then(|p| p.1.checked_sub(base_b)))?;
                Some(((base_a + f0, base_b + f1), (x, uva, uvb)))
            };
            let place = |w: P2, _: f64, (f0, f1): (usize, usize)| {
                let x = first.eval(w.x, w.y);
                (
                    x,
                    fa.clamped(f0 - base_a, &x, ta),
                    fb.clamped(f1 - base_b, &x, tb),
                )
            };
            let onto = |w: P2, d: V2, tau: f64, _: bool| onto_boundary(&near, w, d, tau, bx);
            // (Which side of each torus's join a place is: the half the motion leaves bare.)
            let bare = |w: P2| {
                let x = first.eval(w.x, w.y);
                [pa.tube.bare(chart_a, &x), pb.tube.bare(chart_b, &x)]
            };
            for mut chain in chains {
                // Where the curve crosses both joins between two of its points (tori nearly
                // one on the other, their joins a hair apart), a point between the
                // crossings: the stretch there is on both bare halves, and would be missed.
                let mut k = 0;
                while k + 1 < chain.pts.len() {
                    let ((p, tp), (q, _)) = (chain.pts[k], chain.pts[k + 1]);
                    let d = q - p;
                    let on = |s: f64| {
                        let m = P2::from(p.coords + d * s);
                        onto_boundary(&near, m, d, tp, bx).map_or(m, |r| r.0)
                    };
                    let (bp, bq) = (bare(p), bare(q));
                    let flips: Vec<f64> = (0..2)
                        .filter(|&e| bp[e] != bq[e])
                        .map(|e| {
                            let (mut lo, mut hi) = (0.0, 1.0);
                            for _ in 0..40 {
                                let m = 0.5 * (lo + hi);
                                if bare(on(m))[e] == bp[e] {
                                    lo = m;
                                } else {
                                    hi = m;
                                }
                            }
                            0.5 * (lo + hi)
                        })
                        .collect();
                    if let [s0, s1] = flips[..] {
                        chain.pts.insert(k + 1, (on(0.5 * (s0 + s1)), tp));
                        k += 1;
                    }
                    k += 1;
                }
                chain_runs(
                    &chain,
                    &pair,
                    &place,
                    &onto,
                    &|_, t| (f64::NAN, t),
                    jump,
                    &[],
                    given,
                );
            }
        }
    }
}

/// Each sheet's patch fitted over the part of it kept, held to its curves' points too (the
/// solid's faces numbered from `base`): the fits, and each sheet's curves' points.
#[allow(clippy::type_complexity)]
fn fit_sheets(
    prep: &Prep,
    base: usize,
    given: &[super::sweep_moving::Given],
) -> std::result::Result<(Vec<(super::nurbs::NurbsSurface, Dom)>, Vec<Vec<(P3, P2)>>), String> {
    use rayon::prelude::*;
    let ns = prep.sheets.len();
    let sheet = |f: usize| f.checked_sub(base).filter(|&i| i < ns);
    let mut on_sheet: Vec<Vec<(P3, P2)>> = vec![Vec::new(); ns];
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    for g in given {
        for (k, p) in g.pts.iter().enumerate() {
            for (i, w) in [(sheet(g.a), p.1), (sheet(g.b), p.2)] {
                let Some(i) = i else {
                    continue;
                };
                on_sheet[i].push((p.0, w));
                if debug {
                    // (A curve's point not where its place on the sheet is.)
                    let x = prep
                        .tube
                        .eval(&Pos {
                            chart: prep.sheets[i].patch.0,
                            a: w.x,
                            psi: w.y,
                        })
                        .p;
                    if (x - p.0).norm() > 1e-6 * prep.size {
                        eprintln!(
                            "  curve of faces ({}, {}): point {k} of {} at {:?} is {:.2e} from its place ({:.6}, {:.6}) on sheet {}",
                            g.a,
                            g.b,
                            g.pts.len(),
                            p.0,
                            (x - p.0).norm(),
                            w.x,
                            w.y,
                            base + i
                        );
                    }
                }
            }
        }
    }
    let fitted: Vec<std::result::Result<(super::nurbs::NurbsSurface, Dom), String>> = prep
        .sheets
        .par_iter()
        .zip(&on_sheet)
        .map(|(sh, pts)| fit_tensor(&prep.tube, sh, pts, prep.fit_tol))
        .collect();
    let mut fits = Vec::with_capacity(ns);
    for f in fitted {
        fits.push(f?);
    }
    if std::env::var_os("CADRS_TUBE_FITS").is_some() {
        // (How far each fitted sheet is off its chart midway between its curves' points.)
        for g in given {
            for (f, second) in [(sheet(g.a), false), (sheet(g.b), true)] {
                let Some(f) = f else {
                    continue;
                };
                let chart = prep.sheets[f].patch.0;
                let worst = g
                    .pts
                    .windows(2)
                    .map(|w| {
                        let (a, b) = if second {
                            (w[0].2, w[1].2)
                        } else {
                            (w[0].1, w[1].1)
                        };
                        let m = P2::from(0.5 * (a.coords + b.coords));
                        let x = prep
                            .tube
                            .eval(&Pos {
                                chart,
                                a: m.x,
                                psi: m.y,
                            })
                            .p;
                        ((fits[f].0.point(m.x, m.y) - x).norm(), m, (b - a).norm())
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                if let Some(w) = worst
                    && w.0 > prep.fit_tol
                {
                    eprintln!(
                        "  curve of faces ({}, {}): sheet {f}'s fit off its chart by {:.2e} at ({:.6}, {:.6}), midway of a step of {:.2e}",
                        g.a, g.b, w.0, w.1.x, w.1.y, w.2
                    );
                }
            }
        }
    }
    Ok((fits, on_sheet))
}

/// A swept torus's candidate faces: the sheets' (their fits), each turned to face away from
/// the core circle placed at its points' own instant, then the tori's pieces. A sheet is read
/// at a kept point (past a fold it faces the other way, and lies inside): the kept node with
/// the most kept round it, or, a patch covered at every node, a place on a curve of it, which
/// the part of it kept runs up to.
fn sheet_faces(
    prep: &Prep,
    fits: Vec<(super::nurbs::NurbsSurface, Dom)>,
    on_sheet: &[Vec<(P3, P2)>],
) -> Result<Vec<Body>> {
    let tube = &prep.tube;
    let mut faces: Vec<Body> = Vec::with_capacity(prep.patches.len());
    for ((sh, pts), (fit, dom)) in prep.sheets.iter().zip(on_sheet).zip(fits) {
        let (chart, a0, a1, p0, p1) = sh.patch;
        let surf = super::surface::Surface::Nurbs(std::sync::Arc::new(fit));
        let mut sheet = super::face_build::face_from_surface(&surf, Some(dom))?;
        let (mu, mv) = (sh.grid.0 as isize, sh.grid.1 as isize);
        let kept = |i: isize, j: isize| {
            (0..=mu).contains(&i)
                && (0..=mv).contains(&j)
                && !sh.nodes[(i * (mv + 1) + j) as usize].0
        };
        let w = (0..=mu)
            .flat_map(|i| (0..=mv).map(move |j| (i, j)))
            .filter(|&(i, j)| kept(i, j))
            .max_by_key(|&(i, j)| {
                (-1..=1)
                    .flat_map(|di| (-1..=1).map(move |dj| (di, dj)))
                    .filter(|&(di, dj)| kept(i + di, j + dj))
                    .count()
            })
            .map(|(i, j)| {
                P2::new(
                    a0 + (a1 - a0) * i as f64 / mu as f64,
                    p0 + (p1 - p0) * j as f64 / mv as f64,
                )
            })
            .or_else(|| pts.get(pts.len() / 2).map(|p| p.1))
            .unwrap_or(P2::new(0.5 * (a0 + a1), 0.5 * (p0 + p1)));
        let kp = tube.eval(&Pos {
            chart,
            a: w.x,
            psi: w.y,
        });
        let (rm, dm) = tube.motion.place(kp.own);
        let core = rm * tube.k(tube.psi_of(chart, w)) + dm;
        let face = &mut sheet.faces[0];
        let (_, su, sv) = face.surf.d1(w.x, w.y);
        let natural = su.cross(&sv) * if face.sense { 1.0 } else { -1.0 };
        if natural.dot(&(kp.p.coords - core)) < 0.0 {
            Body::reverse_face(face);
        }
        faces.push(sheet);
    }
    faces.extend(prep.pieces.iter().cloned());
    Ok(faces)
}

/// The instants (one of each motion) at which two tubes' tori lie one on the other: their
/// core circles the same circle.
fn shared_poses(a: &Tube, b: &Tube) -> Vec<(f64, f64)> {
    if (a.big - b.big).abs() > 1e-12 * a.size || (a.r - b.r).abs() > 1e-12 * a.size {
        return Vec::new();
    }
    let circle = |t: &Tube, at: f64| {
        let (rm, dm) = t.motion.place(at);
        (rm * t.c.coords + dm, rm * t.z)
    };
    let apart = |ta: f64, tb: f64| {
        let ((ca, za), (cb, zb)) = (circle(a, ta), circle(b, tb));
        (ca - cb).norm() + a.big * za.cross(&zb).norm()
    };
    let n = 64;
    let grid: Vec<f64> = (0..=n)
        .flat_map(|i| (0..=n).map(move |j| (i, j)))
        .map(|(i, j)| apart(i as f64 / n as f64, j as f64 / n as f64))
        .collect();
    let at = |i: usize, j: usize| grid[i * (n + 1) + j];
    let mut out: Vec<(f64, f64)> = Vec::new();
    for i in 0..=n {
        for j in 0..=n {
            let v = at(i, j);
            let least = (i.saturating_sub(1)..=(i + 1).min(n))
                .all(|x| (j.saturating_sub(1)..=(j + 1).min(n)).all(|y| at(x, y) >= v));
            if !least || v > 0.2 * a.size {
                continue;
            }
            let (mut c, mut h, mut best) = (
                (i as f64 / n as f64, j as f64 / n as f64),
                1.0 / n as f64,
                v,
            );
            for _ in 0..60 {
                for x in -2i32..=2 {
                    for y in -2i32..=2 {
                        let w = (
                            (c.0 + h * x as f64 / 2.0).clamp(0.0, 1.0),
                            (c.1 + h * y as f64 / 2.0).clamp(0.0, 1.0),
                        );
                        let d = apart(w.0, w.1);
                        if d < best {
                            (best, c) = (d, w);
                        }
                    }
                }
                h *= 0.6;
            }
            if best <= 1e-10 * a.size
                && !out
                    .iter()
                    .any(|o| (o.0 - c.0).abs() < 1e-6 && (o.1 - c.1).abs() < 1e-6)
            {
                out.push(c);
            }
        }
    }
    out
}

/// Another swept torus beside the one being traced, for a boolean of the two.
struct Other<'a, 'b> {
    prep: &'a Prep<'b>,
    /// Its faces' first index among both solids'.
    base: usize,
    /// Whether the traced solid's faces are kept where this one holds them (an intersection's,
    /// and the cutter's in a difference) or where it doesn't.
    inside: bool,
}

/// Another swept torus's instants are told from a tube's own by this much added.
const OTHER: f64 = 10.0;

/// The curves the boundary crosses each sheet's patch along, read in the patch's own (a, ψ)
/// by membership: where its points pass from kept to covered. Each point of one is touched
/// by the torus at the instant that covers the far side, on the face the torus's surface
/// passes through it on then (another sheet's patch, this one's elsewhere, or the torus
/// where the motion starts or stops): the curve is those two faces' meeting, known whole
/// with its place on each, with no search for it in space (where the faces graze, at the
/// joins and beside a fold, such a search wanders). With another swept torus beside this
/// one, where that one holds a point (or doesn't) as well, at an instant of its own. The
/// faces are numbered from `base`.
fn trace(
    this: &Prep,
    base: usize,
    other: Option<&Other>,
    nodes: &[(P3, f64, f64)],
) -> Vec<super::sweep_moving::Given> {
    use rayon::prelude::*;
    let tube = &this.tube;
    let fc = this.faces();
    let jump = 4.0 * tube.step();
    let fit_tol = this.fit_tol;
    let traced: Vec<Vec<super::sweep_moving::Given>> = this
        .sheets
        .par_iter()
        .enumerate()
        .map(|(i, sh)| {
            let (chart, a0, a1, p0, p1) = sh.patch;
            let bx = ((a0, a1), (p0, p1));
            let point = |w: P2| {
                tube.eval(&Pos {
                    chart,
                    a: w.x,
                    psi: w.y,
                })
                .p
            };
            // (Whether the other solid's hold of a point drops it, with the instant it holds
            // it deepest, or comes nearest.)
            let dropped = |l: Least, o: &Other| {
                ((l.1 < -o.prep.tube.delta()) != o.inside, OTHER + l.0)
            };
            let beside = |c: (bool, f64), w: P2| match other {
                Some(o) if !c.0 => {
                    let d = dropped(o.prep.tube.least(&point(w)), o);
                    if d.0 { d } else { c }
                }
                _ => c,
            };
            let cover = |w: P2| {
                beside(tube.cover_in(&sh.patch, w), w)
            };
            let near = |w: P2, tc: f64| match other {
                Some(o) if tc > OTHER - 0.5 => {
                    dropped(o.prep.tube.least_near(&point(w), tc - OTHER), o)
                }
                _ => tube.cover_near_in(&sh.patch, w, tc),
            };
            // (No margin past the patch's own sides: its chart runs on there, beyond the
            // motion's ends or into the next patch, and a corner sought there is none of
            // this patch's.)
            let within = |w: P2| {
                (a0 - 1e-9..=a1 + 1e-9).contains(&w.x) && (p0 - 1e-9..=p1 + 1e-9).contains(&w.y)
            };
            let margin = |w: P2, tau: f64| match other {
                _ if !within(w) => (f64::NAN, tau),
                Some(o) if tau > OTHER - 0.5 => {
                    let l = o.prep.tube.least_from(&point(w), tau - OTHER);
                    let d = o.prep.tube.delta();
                    ((l.1 + d) / d, OTHER + l.0)
                }
                _ => tube.margin_in(&sh.patch, w, tau),
            };
            let fd = Field {
                cover: &cover,
                near: &near,
                margin: Some(&margin),
            };
            // (The curves' nodes on this patch: each with its place here, and the other
            // solid's instant there.)
            let through: Vec<(P2, f64)> = nodes
                .iter()
                .filter_map(|&(x, own, others)| {
                    fc.at(&x, own, Some(i))
                        .filter(|h| h.0 == i)
                        .map(|h| (h.1, OTHER + others))
                })
                .collect();
            let places: Vec<P2> = through.iter().map(|t| t.0).collect();
            // (The patch's nodes as its own solid read them, and the other's hold of those it
            // keeps.)
            let (mu, mv) = sh.grid;
            let t_nodes = web_time::Instant::now();
            let nodes: Option<Vec<(bool, f64)>> = other.map(|_| {
                (0..(mu + 1) * (mv + 1))
                    .into_par_iter()
                    .map(|n| beside(sh.nodes[n], sh.at(n / (mv + 1), n % (mv + 1))))
                    .collect()
            });
            let t_curves = web_time::Instant::now();
            let chains = cover_curves(
                &fd,
                bx,
                sh.grid,
                Some(nodes.as_deref().unwrap_or(&sh.nodes)),
                1e-6,
                jump,
                &point,
                fit_tol,
                &through,
            )
            .unwrap_or_default();
            if std::env::var_os("CADRS_TUBE_TIMES").is_some() {
                eprintln!(
                    "  trace: sheet {i} grid {mu}x{mv}: nodes {:.0} ms, curves {:.0} ms ({} chains, {} points)",
                    (t_curves - t_nodes).as_secs_f64() * 1e3,
                    t_curves.elapsed().as_secs_f64() * 1e3,
                    chains.len(),
                    chains.iter().map(|c| c.pts.len()).sum::<usize>()
                );
            }
            let t_runs = web_time::Instant::now();
            // A curve that runs into a fold where the fold meets an end of the motion ends at
            // that corner: the sheet, the fold's pipe and the end's torus all touch there, a
            // millionth of a millionth apart for a good way round, and what the curve does in
            // the last of it can't be read; it is the end torus's from the fold's zone in.
            let mut chains = chains;
            if (chart == LEAD || chart == TRAIL) && !tube.lines.is_empty() {
                const CORNER: f64 = 5e-3;
                for chain in &mut chains {
                    if chain.closed {
                        continue;
                    }
                    for last in [false, true] {
                        let n = chain.pts.len();
                        let w = chain.pts[if last { n - 1 } else { 0 }].0;
                        let Some(side) = [p0, p1]
                            .into_iter()
                            .find(|&s| (w.y - s).abs() < 1e-6 && tube.line_by(s, 1e-9).is_some())
                        else {
                            continue;
                        };
                        let end = if w.x < CORNER {
                            0.0
                        } else if w.x > 1.0 - CORNER {
                            1.0
                        } else {
                            continue;
                        };
                        let near = |p: &(P2, f64)| {
                            (p.0.x - end).abs() < CORNER && (p.0.y - side).abs() < LINE_BY
                        };
                        let own = if chart == LEAD { end } else { 1.0 - end };
                        if last {
                            while chain.pts.len() > 1 && near(&chain.pts[chain.pts.len() - 1]) {
                                chain.pts.pop();
                            }
                            chain.pts.push((P2::new(end, side), own));
                        } else {
                            // (Its corner last: the face of the end's torus that the curve
                            // has run along is the one it ends on, of those that meet there.)
                            let k = chain.pts.iter().take_while(|p| near(p)).count();
                            chain.pts.drain(..k.min(chain.pts.len() - 1));
                            chain.pts.reverse();
                            chain.pts.push((P2::new(end, side), own));
                            break;
                        }
                    }
                }
            }
            // (A place covered across a seam, by the other chart there: found by the place's
            // own instant.)
            let pair = |w: P2, tau: f64, prefer: Option<(usize, usize)>| {
                let kp = tube.eval(&Pos {
                    chart,
                    a: w.x,
                    psi: w.y,
                });
                let x = kp.p;
                // (The face found must hold the point where it is said to: an instant that
                // covers the far side without touching here (two boundaries a hair apart, by
                // a corner) finds a place the point isn't at.)
                let held = |f: &Faces, face: usize, uv: P2| {
                    (f.point(face, uv) - x).norm() <= 1e-6 * this.size
                };
                let (fb, uvb) = match other {
                    Some(o) if tau > OTHER - 0.5 => {
                        let prefer = prefer.and_then(|p| p.1.checked_sub(o.base));
                        let of = o.prep.faces();
                        let (f, uv) = of.at(&x, tau - OTHER, prefer)?;
                        if !held(&of, f, uv) {
                            return None;
                        }
                        (o.base + f, uv)
                    }
                    _ => {
                        let prefer = prefer.and_then(|p| p.1.checked_sub(base));
                        if std::env::var("CADRS_TUBE_PAIR").is_ok_and(|v| v == i.to_string()) {
                            eprintln!(
                                "  pair on {i} at {w:?} tau {tau:.12}: at end {:?}, rates {:.3e} {:.3e}, exact {:.12} -> {:?} (prefer {prefer:?})",
                                tube.at_end(tau, &x),
                                tube.rate(0.0, &x),
                                tube.rate(1.0, &x),
                                tube.foot(tau.clamp(0.0, 1.0), &x),
                                fc.at(&x, tau, prefer)
                            );
                        }
                        let (f, uv) = if tau < -0.5 {
                            fc.across_seam(chart, &x, kp.own, tau, prefer)?
                        } else {
                            fc.at(&x, tau, prefer)?
                        };
                        if !held(&fc, f, uv) {
                            return None;
                        }
                        (base + f, uv)
                    }
                };
                Some(((base + i, fb), (x, w, uvb)))
            };
            let place = |w: P2, tau: f64, (_, fb): (usize, usize)| {
                let kp = tube.eval(&Pos {
                    chart,
                    a: w.x,
                    psi: w.y,
                });
                // (By the face asked for, whichever solid's the instant: where the two kinds
                // of stretch meet, a point of the one is asked with the other's instant.)
                // (The instant a solid's face holds the point at, where the one given is the
                // other solid's: the least of its own, nearest touching, that puts the point
                // on that face.)
                let touching = |t: &Tube, f: &Faces, face: usize| -> f64 {
                    let ls = t.leasts(&kp.p);
                    ls.iter()
                        .filter(|l| f.at(&kp.p, l.0, Some(face)).is_some_and(|h| h.0 == face))
                        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                        .or(ls.first())
                        .map_or(0.0, |l| l.0)
                };
                let uvb = match other {
                    Some(o) if (o.base..o.base + o.prep.patches.len()).contains(&fb) => {
                        let (f, face) = (o.prep.faces(), fb - o.base);
                        let tau = if tau > OTHER - 0.5 {
                            tau - OTHER
                        } else {
                            touching(&o.prep.tube, &f, face)
                        };
                        f.clamped(face, &kp.p, tau)
                    }
                    _ => {
                        let tau = if tau < -0.5 {
                            kp.own
                        } else if tau > OTHER - 0.5 {
                            touching(tube, &fc, fb - base)
                        } else {
                            tau
                        };
                        fc.clamped(fb - base, &kp.p, tau)
                    }
                };
                (kp.p, w, uvb)
            };
            let onto = |w: P2, d: V2, tau: f64, full: bool| {
                if full {
                    onto_boundary(&|w, _| cover(w), w, d, tau, bx)
                } else {
                    onto_by_margin(&margin, &near, w, d, tau, bx)
                        .or_else(|| onto_boundary(&near, w, d, tau, bx))
                }
            };
            // (Each chain's runs on its own, and put together in the chains' order.)
            let run_of = |chain: &Chain| {
                let mut out = Vec::new();
                if let Ok(want) = std::env::var("CADRS_TUBE_CROSS") {
                    let v: Vec<f64> = want.split(',').filter_map(|w| w.parse().ok()).collect();
                    if v.len() == 2
                        && chain
                            .pts
                            .iter()
                            .any(|p| (p.0 - P2::new(v[0], v[1])).norm() < 1e-6)
                    {
                        eprintln!(
                            "  chain on face {i} (chart {chart}, box {bx:?}), closed {}: {:?}",
                            chain.closed, chain.pts
                        );
                    }
                }
                chain_runs(
                    chain, &pair, &place, &onto, &margin, jump, &places, &mut out,
                );
                out
            };
            let out: Vec<super::sweep_moving::Given> =
                chains.par_iter().map(run_of).flatten_iter().collect();
            if std::env::var_os("CADRS_TUBE_TIMES").is_some() {
                eprintln!(
                    "  trace: sheet {i}: runs {:.0} ms ({} curves)",
                    t_runs.elapsed().as_secs_f64() * 1e3,
                    out.len()
                );
            }
            out
        })
        .collect();
    if std::env::var_os("CADRS_TUBE_TIMES").is_some() {
        let n: Vec<usize> = BY_MARGIN
            .iter()
            .map(|c| c.swap(0, std::sync::atomic::Ordering::Relaxed))
            .collect();
        eprintln!(
            "  trace: by the margin, {} of {} crossings and {} of {} places onto the curve",
            n[1], n[0], n[3], n[2]
        );
    }
    traced.into_iter().flatten().collect()
}

/// Two patches' crossing is read from each, and a patch's crossing of itself from each of its
/// two places: one reading of the two kept. Where one is a part of the other (a face's own
/// reading of a curve gives out where its parameters are stretched thin, beside a pinch's
/// seam, and its neighbour's doesn't), the whole one; of two alike, the lower patch's or the
/// first. A reading without its twin stays. `is_sheet`: whether a face is a sheet's patch
/// (the tori's pieces read nothing).
pub(super) fn drop_twins(
    given: &mut Vec<super::sweep_moving::Given>,
    is_sheet: &dyn Fn(usize) -> bool,
    size: f64,
    fit_tol: f64,
) {
    let near = 1e-5 * size;
    let length = |g: &super::sweep_moving::Given| -> f64 {
        g.pts.windows(2).map(|w| (w[1].0 - w[0].0).norm()).sum()
    };
    // (Whether every point of g lies on h's polyline.)
    let along = |g: &super::sweep_moving::Given, h: &super::sweep_moving::Given| -> bool {
        g.pts.iter().all(|p| {
            h.pts.windows(2).any(|w| {
                let (a, b) = (w[0].0, w[1].0);
                let d = b - a;
                let t = if d.norm_squared() > 0.0 {
                    ((p.0 - a).dot(&d) / d.norm_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                // (A chord lies off its curve by its sag, which the tracing holds
                // to this.)
                let sag = (0.5 * fit_tol * d.norm_squared()).cbrt();
                (a + d * t - p.0).norm() <= near + 2.0 * sag
            })
        })
    };
    if std::env::var_os("CADRS_TUBE_TWINS").is_some() {
        // (How far each reading's points are off its twins' polylines, at worst.)
        for (i, g) in given.iter().enumerate() {
            for (j, h) in given.iter().enumerate() {
                if i == j || (h.a, h.b) != (g.b, g.a) || !is_sheet(g.b) {
                    continue;
                }
                let off = |p: &P3| {
                    h.pts
                        .windows(2)
                        .map(|w| {
                            let (a, b) = (w[0].0, w[1].0);
                            let d = b - a;
                            let t =
                                ((p - a).dot(&d) / d.norm_squared().max(1e-300)).clamp(0.0, 1.0);
                            (a + d * t - p).norm()
                        })
                        .fold(f64::INFINITY, f64::min)
                };
                let worst = g
                    .pts
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (off(&p.0), k))
                    .max_by(|x, y| x.0.total_cmp(&y.0))
                    .unwrap();
                eprintln!(
                    "  reading {i} ({}, {}) of {} points, {:.6} long, against {j} of {} points, {:.6} long: furthest off {:.3e} at point {}; along {}",
                    g.a,
                    g.b,
                    g.pts.len(),
                    length(g),
                    h.pts.len(),
                    length(h),
                    worst.0,
                    worst.1,
                    along(g, h)
                );
            }
        }
    }
    let mut keep = vec![true; given.len()];
    for i in 0..given.len() {
        let g = &given[i];
        let lg = length(g);
        let twin = (0..given.len()).any(|j| {
            let h = &given[j];
            // (Its twin, read from the other face; or the same curve read again from this
            // one: an island the grid made of a spike's tip, which the stretch round the
            // spike reaches too once its corner is found.)
            let (twin, again) = ((h.a, h.b) == (g.b, g.a), (h.a, h.b) == (g.a, g.b));
            if j == i || !keep[j] || !(twin || again) {
                return false;
            }
            if !along(g, h) {
                return false;
            }
            // (A part of the other goes. Each along the other, they are one curve read
            // twice: the one that winds back on itself, longer for it, goes; of two
            // alike, the higher patch's or the later.)
            let lh = length(h);
            !along(h, g)
                || lg > lh + near
                || ((lg - lh).abs() <= near && if again { j < i } else { g.a > g.b })
        });
        if twin {
            keep[i] = false;
        }
    }
    let mut k = 0;
    given.retain(|_| {
        k += 1;
        keep[k - 1]
    });
}

/// A patch of a chart: its chart, a from `.1` to `.2`, ψ from `.3` to `.4` (a torus's piece:
/// its quarter's ψ and θ).
type Patch = (usize, f64, f64, f64, f64);

/// A sheet's patch (or a pinch's chart's) read by membership over a grid of `grid.0` by
/// `grid.1` cells: at each node, covered or kept, with the instant that covers it.
struct Sheet {
    patch: Patch,
    grid: (usize, usize),
    nodes: Vec<(bool, f64)>,
}

/// A box of a chart: its a from `.0.0` to `.0.1`, its ψ from `.1.0` to `.1.1`.
type Dom = ((f64, f64), (f64, f64));

impl Sheet {
    fn read(tube: &Tube, patch: Patch, grid: (usize, usize)) -> Sheet {
        use rayon::prelude::*;
        let (_, a0, a1, p0, p1) = patch;
        let (mu, mv) = grid;
        let nodes = (0..(mu + 1) * (mv + 1))
            .into_par_iter()
            .map(|k| {
                let (i, j) = (k / (mv + 1), k % (mv + 1));
                tube.cover_in(
                    &patch,
                    P2::new(
                        a0 + (a1 - a0) * i as f64 / mu as f64,
                        p0 + (p1 - p0) * j as f64 / mv as f64,
                    ),
                )
            })
            .collect();
        Sheet { patch, grid, nodes }
    }

    /// The node (i, j)'s place.
    fn at(&self, i: usize, j: usize) -> P2 {
        let (_, a0, a1, p0, p1) = self.patch;
        P2::new(
            a0 + (a1 - a0) * i as f64 / self.grid.0 as f64,
            p0 + (p1 - p0) * j as f64 / self.grid.1 as f64,
        )
    }

    /// Whether the sheet is kept at (u, v), where its fit must hold: by its cell's corners
    /// where they agree, read exactly where they don't.
    fn live(&self, tube: &Tube, u: f64, v: f64) -> bool {
        let (_, a0, a1, p0, p1) = self.patch;
        let (mu, mv) = self.grid;
        let i = ((((u - a0) / (a1 - a0)) * mu as f64) as usize).min(mu - 1);
        let j = ((((v - p0) / (p1 - p0)) * mv as f64) as usize).min(mv - 1);
        let covered = [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)]
            .iter()
            .filter(|&&(i, j)| self.nodes[i * (mv + 1) + j].0)
            .count();
        match covered {
            4 => false,
            0 => true,
            _ => !tube.cover_in(&self.patch, P2::new(u, v)).0,
        }
    }
}

/// How near a sheet its fitted surface keeps, in the measure of the body's size.
const SHEET_FIT: f64 = 1e-7;

/// The degree of the B-splines fitted through a sheet's points.
const PATCH_DEGREE: usize = 5;

/// The B-spline (of `PATCH_DEGREE`) through a chart's points at the sites `us` by `vs`.
pub(super) fn tensor(
    at: &(dyn Fn(f64, f64) -> P3 + Sync),
    us: &[f64],
    vs: &[f64],
) -> super::nurbs::NurbsSurface {
    use super::nurbs::{H, NurbsCurve, NurbsSurface, hom};
    use rayon::prelude::*;
    // Along u at each v, then the poles' columns along v (each row and each column on its
    // own: a sheet of many sites is most of a sweep's fitting).
    let rows: Vec<NurbsCurve> = vs
        .par_iter()
        .map(|&v| {
            let pts: Vec<P3> = us.iter().map(|&u| at(u, v)).collect();
            NurbsCurve::interpolate(&pts, us, PATCH_DEGREE, None, None)
        })
        .collect();
    let nu = rows[0].cps.len();
    let cols: Vec<NurbsCurve> = (0..nu)
        .into_par_iter()
        .map(|i| {
            let col: Vec<P3> = rows
                .iter()
                .map(|c| super::nurbs::dehom(&c.cps[i]))
                .collect();
            NurbsCurve::interpolate(&col, vs, PATCH_DEGREE, None, None)
        })
        .collect();
    let kv = cols.last().map(|c| c.knots.clone()).unwrap_or_default();
    let cps: Vec<Vec<H>> = cols
        .iter()
        .map(|c| {
            c.cps
                .iter()
                .map(|h| hom(&super::nurbs::dehom(h), 1.0))
                .collect()
        })
        .collect();
    NurbsSurface {
        du: rows[0].degree,
        dv: PATCH_DEGREE.min(vs.len() - 1),
        ku: rows[0].knots.clone(),
        kv,
        cps,
    }
}

/// A sheet's patch fitted within `tol` over the box of what is kept of it (its kept nodes, a
/// cell wider, and its curves' points `pts`, which it is held to as well): one B-spline through
/// the chart's points at sites halved wherever it misses the chart midway, along a or along ψ
/// as the miss is, and only where the sheet is kept: what is covered bounds nothing, and past
/// a fold bends too sharply to fit. With nothing kept, any surface over the whole patch.
fn fit_tensor(
    tube: &Tube,
    sh: &Sheet,
    pts: &[(P3, P2)],
    tol: f64,
) -> std::result::Result<(super::nurbs::NurbsSurface, Dom), String> {
    use rayon::prelude::*;
    let (chart, a0, a1, p0, p1) = sh.patch;
    let (mu, mv) = sh.grid;
    let (mut lo, mut hi) = (
        P2::new(f64::INFINITY, f64::INFINITY),
        P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    let mut take = |w: P2| {
        lo = P2::new(lo.x.min(w.x), lo.y.min(w.y));
        hi = P2::new(hi.x.max(w.x), hi.y.max(w.y));
    };
    for i in 0..=mu {
        for j in 0..=mv {
            if !sh.nodes[i * (mv + 1) + j].0 {
                take(sh.at(i.saturating_sub(1), j.saturating_sub(1)));
                take(sh.at((i + 1).min(mu), (j + 1).min(mv)));
            }
        }
    }
    for (_, w) in pts {
        take(*w);
    }
    let nothing = lo.x > hi.x;
    if nothing {
        (lo, hi) = (P2::new(a0, p0), P2::new(a1, p1));
    }
    // (A box of some width each way.)
    let (ca, cp) = ((a1 - a0) / mu as f64, (p1 - p0) / mv as f64);
    if hi.x - lo.x < ca {
        (lo.x, hi.x) = ((lo.x - ca).max(a0), (hi.x + ca).min(a1));
    }
    if hi.y - lo.y < cp {
        (lo.y, hi.y) = ((lo.y - cp).max(p0), (hi.y + cp).min(p1));
    }
    let dom: Dom = ((lo.x, hi.x), (lo.y, hi.y));
    let at = |u: f64, v: f64| {
        tube.eval(&Pos {
            chart,
            a: u,
            psi: v,
        })
        .p
    };
    let sites = |lo: f64, hi: f64| -> Vec<f64> {
        (0..=8).map(|i| lo + (hi - lo) * i as f64 / 8.0).collect()
    };
    let (mut us, mut vs) = (sites(lo.x, hi.x), sites(lo.y, hi.y));
    for _ in 0..16 {
        let surf = tensor(&at, &us, &vs);
        if nothing {
            return Ok((surf, dom));
        }
        let bad =
            |u: f64, v: f64| (surf.point(u, v) - at(u, v)).norm() > tol && sh.live(tube, u, v);
        let (nu, nv) = (us.len() - 1, vs.len() - 1);
        let mid = |x: &[f64], i: usize| 0.5 * (x[i] + x[i + 1]);
        // (Along a site's line the surface is the curve through that line's points: a miss
        // midway along it is that direction's.)
        let mut split_u: Vec<bool> = (0..nu)
            .into_par_iter()
            .map(|i| vs.iter().any(|&v| bad(mid(&us, i), v)))
            .collect();
        let mut split_v: Vec<bool> = (0..nv)
            .into_par_iter()
            .map(|j| us.iter().any(|&u| bad(u, mid(&vs, j))))
            .collect();
        // (The cells' centres where neither way is split yet: read together, then taken in
        // order, a centre whose row or column an earlier one has split left be as before.)
        let centres: Vec<(usize, usize)> = (0..nu)
            .flat_map(|i| (0..nv).map(move |j| (i, j)))
            .filter(|&(i, j)| !(split_u[i] || split_v[j]))
            .collect();
        let off: Vec<bool> = centres
            .par_iter()
            .map(|&(i, j)| bad(mid(&us, i), mid(&vs, j)))
            .collect();
        for (&(i, j), off) in centres.iter().zip(off) {
            if off && !(split_u[i] || split_v[j]) {
                split_u[i] = true;
                split_v[j] = true;
            }
        }
        // (At its curves' places, to the chart there: a curve's point may stand a hair off
        // its place, where the place is ill found (faces touching), which its edge's
        // tolerance takes up.)
        for (_, w) in pts {
            if (surf.point(w.x, w.y) - at(w.x, w.y)).norm() > tol {
                let i = us.partition_point(|&u| u <= w.x).clamp(1, nu) - 1;
                let j = vs.partition_point(|&v| v <= w.y).clamp(1, nv) - 1;
                if !(split_u[i] || split_v[j]) {
                    split_u[i] = true;
                    split_v[j] = true;
                }
            }
        }
        if !split_u.iter().chain(&split_v).any(|s| *s) {
            return Ok((surf, dom));
        }
        if us.len() > 600 || vs.len() > 300 {
            break;
        }
        let halve = |x: &[f64], split: &[bool]| -> Vec<f64> {
            let mut out = Vec::with_capacity(2 * x.len());
            for i in 0..x.len() - 1 {
                out.push(x[i]);
                if split[i] {
                    out.push(0.5 * (x[i] + x[i + 1]));
                }
            }
            out.push(x[x.len() - 1]);
            out
        };
        us = halve(&us, &split_u);
        vs = halve(&vs, &split_v);
    }
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        let surf = tensor(&at, &us, &vs);
        let worst = pts
            .iter()
            .map(|(x, w)| {
                (
                    (surf.point(w.x, w.y) - x).norm(),
                    *w,
                    (at(w.x, w.y) - x).norm(),
                )
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        eprintln!(
            "  fit of chart {chart} gave up at {} by {} sites (least spacing {:.2e}, {:.2e}): the curve point furthest off {worst:?} (its miss, its place, the sheet's own miss there)",
            us.len(),
            vs.len(),
            us.windows(2)
                .map(|w| w[1] - w[0])
                .fold(f64::INFINITY, f64::min),
            vs.windows(2)
                .map(|w| w[1] - w[0])
                .fold(f64::INFINITY, f64::min)
        );
    }
    Err(format!(
        "a patch of its boundary ({chart}, a {:.6}..{:.6}, ψ {:.6}..{:.6}) bends too sharply to fit",
        lo.x, hi.x, lo.y, hi.y
    ))
}

/// Where the faces are: which holds a point the torus touches at an instant.
struct Faces<'a> {
    tube: &'a Tube<'a>,
    /// Each face's patch: the sheets' first, then the tori's pieces (each its quarter's box).
    patches: &'a [Patch],
    /// How many of the faces are sheets' patches.
    sheets: usize,
    /// Whether a torus's piece (by its index less `sheets`) holds a (ψ, θ), loosely.
    holds: &'a [Box<dyn Fn(P2) -> bool + Send + Sync>],
    /// Whether a torus's piece is on the half of it the motion leaves bare.
    bare: &'a [bool],
    /// The torus where the motion starts and where it stops.
    tori: &'a [super::surface::Surface; 2],
}

impl Faces<'_> {
    /// The (a, ψ) on sheet patch `i` of the sheets' place (a, ψ), when it holds it.
    fn on_sheet(&self, i: usize, a: f64, psi: f64) -> Option<P2> {
        let p = self.patches[i];
        let v = wrap(psi, p.3 - 1e-9, TAU);
        ((p.1 - 1e-9..=p.2 + 1e-9).contains(&a) && v <= p.4 + 1e-9)
            .then(|| P2::new(a.clamp(p.1, p.2), v.clamp(p.3, p.4)))
    }

    /// The (ψ, θ) of `x` on torus piece `i`, within its box by whole turns.
    fn torus_uv(&self, i: usize, x: &P3) -> P2 {
        let p = self.patches[i];
        let (u, v) = self.tori[usize::from(p.0 == END)].project(x, None);
        P2::new(
            wrap(u, 0.5 * (p.1 + p.2) - PI, TAU),
            wrap(v, 0.5 * (p.3 + p.4) - PI, TAU),
        )
    }

    /// The (ψ, θ) of `x` on torus piece `i`, when it holds it.
    fn on_torus(&self, i: usize, x: &P3) -> Option<P2> {
        let p = self.patches[i];
        let uv = self.torus_uv(i, x);
        if uv.x < p.1 - 1e-9 || uv.x > p.2 + 1e-9 || uv.y < p.3 - 1e-9 || uv.y > p.4 + 1e-9 {
            return None;
        }
        let uv = P2::new(uv.x.clamp(p.1, p.2), uv.y.clamp(p.3, p.4));
        (self.bare[i - self.sheets] == self.tube.bare(p.0, x) && (self.holds[i - self.sheets])(uv))
            .then_some(uv)
    }

    /// The face the torus's surface passes through `x` on at the instant `tau` (the torus
    /// itself at the motion's ends, a sheet's patch between), with `x`'s place on it: `prefer`
    /// where several hold it (on an edge they share).
    fn at(&self, x: &P3, tau: f64, prefer: Option<usize>) -> Option<(usize, P2)> {
        let mut first = None;
        if let Some(chart) = self.tube.at_end(tau, x) {
            for i in self.sheets..self.patches.len() {
                if self.patches[i].0 != chart {
                    continue;
                }
                if let Some(uv) = self.on_torus(i, x) {
                    if prefer == Some(i) {
                        return Some((i, uv));
                    }
                    first.get_or_insert((i, uv));
                }
            }
            if first.is_none() && std::env::var_os("CADRS_TUBE_AT").is_some() {
                let rows: Vec<String> = (self.sheets..self.patches.len())
                    .filter(|&i| self.patches[i].0 == chart)
                    .map(|i| {
                        let p = self.patches[i];
                        let uv = self.torus_uv(i, x);
                        let boxed = uv.x >= p.1 - 1e-9
                            && uv.x <= p.2 + 1e-9
                            && uv.y >= p.3 - 1e-9
                            && uv.y <= p.4 + 1e-9;
                        format!(
                            "{i}: box {} bare {} (the point {}) holds {}",
                            boxed,
                            self.bare[i - self.sheets],
                            self.tube.bare(p.0, x),
                            (self.holds[i - self.sheets])(uv)
                        )
                    })
                    .filter(|r| r.contains("box true"))
                    .collect();
                eprintln!("  no piece of torus {chart} holds {x:?} (instant {tau}): {rows:?}");
            }
        } else {
            let t = self.tube.foot(tau, x);
            let mut pos = self.tube.sheet_pos(t, x);
            // (On a line of pinches' own section of the tube: on its pipe.)
            if let Some(k) = (0..self.tube.lines.len())
                .find(|&k| wrap(pos.psi - self.tube.lines[k], -PI, TAU).abs() <= 1e-6)
            {
                let turn = self.tube.pipe_turn(k, t, x);
                let half = usize::from(turn >= 1.0);
                pos = Pos {
                    chart: self.tube.pipe_chart(k, half),
                    a: t,
                    psi: turn - half as f64,
                };
            } else if let Some(k) = self.tube.in_pinch(t, pos.psi) {
                let uv = self.tube.pinch_uv(k, t, x);
                pos = Pos {
                    chart: PINCH + k,
                    a: uv.x,
                    psi: uv.y,
                };
            } else if let Some(j) = self.tube.in_band(t, pos.psi) {
                let uv = self.tube.band_uv(j, t, pos.psi);
                pos = Pos {
                    chart: self.tube.band_chart(j, pos.chart),
                    a: uv.x,
                    psi: uv.y,
                };
            }
            first = self.sheet_at(&pos, prefer);
        }
        first
    }

    /// The sheet's patch (or pinch chart's) holding a place of its chart.
    fn sheet_at(&self, pos: &Pos, prefer: Option<usize>) -> Option<(usize, P2)> {
        let mut first = None;
        for i in 0..self.sheets {
            if self.patches[i].0 != pos.chart {
                continue;
            }
            if let Some(uv) = self.on_sheet(i, pos.a, pos.psi) {
                if prefer == Some(i) {
                    return Some((i, uv));
                }
                first.get_or_insert((i, uv));
            }
        }
        first
    }

    /// The face across a seam (marked `mark`) from a place of `chart` at `x` (its own instant
    /// `own`): the pinch's chart's or the band's from a sheet, the sheet's from either.
    fn across_seam(
        &self,
        chart: usize,
        x: &P3,
        own: f64,
        mark: f64,
        prefer: Option<usize>,
    ) -> Option<(usize, P2)> {
        let sheet = self.tube.sheet_pos(own, x);
        let mark = (-1.0 - mark).round() as usize;
        let pos = if chart >= PINCH {
            sheet
        } else if let Some(j) = mark.checked_sub(self.tube.pinches.len()) {
            let uv = self.tube.band_uv(j, own, sheet.psi);
            Pos {
                chart: self.tube.band_chart(j, sheet.chart),
                a: uv.x,
                psi: uv.y,
            }
        } else {
            let uv = self.tube.pinch_uv(mark, own, x);
            Pos {
                chart: PINCH + mark,
                a: uv.x,
                psi: uv.y,
            }
        };
        self.sheet_at(&pos, prefer)
    }

    /// The point of face `i` at a place of it, exactly (a sheet's by its chart, not its fit).
    fn point(&self, i: usize, uv: P2) -> P3 {
        let p = self.patches[i];
        if i < self.sheets {
            self.tube
                .eval(&Pos {
                    chart: p.0,
                    a: uv.x,
                    psi: uv.y,
                })
                .p
        } else {
            self.tori[usize::from(p.0 == END)].eval(uv.x, uv.y)
        }
    }

    /// The place of `x` (touched at the instant `tau`) on face `i`, brought into its box: a
    /// point found on its boundary.
    fn clamped(&self, i: usize, x: &P3, tau: f64) -> P2 {
        let p = self.patches[i];
        let uv = if let Some((k, half)) = self.tube.pipe(p.0).filter(|_| i < self.sheets) {
            let t = self.tube.foot(tau, x);
            // (The turn nearest this half's own stretch of it.)
            let turn = self.tube.pipe_turn(k, t, x) - half as f64;
            P2::new(t, wrap(turn, -0.5, 2.0))
        } else if let Some((j, _)) = self.tube.band(p.0).filter(|_| i < self.sheets) {
            let t = self.tube.foot(tau, x);
            self.tube.band_uv(j, t, self.tube.sheet_pos(t, x).psi)
        } else if i < self.sheets && p.0 >= PINCH {
            let uv = self.tube.pinch_uv(p.0 - PINCH, self.tube.foot(tau, x), x);
            P2::new(uv.x, wrap(uv.y, 0.5 * (p.3 + p.4) - PI, TAU))
        } else if i < self.sheets {
            let pos = self.tube.sheet_pos(self.tube.foot(tau, x), x);
            P2::new(pos.a, wrap(pos.psi, 0.5 * (p.3 + p.4) - PI, TAU))
        } else {
            self.torus_uv(i, x)
        };
        // (A hair inside a side is on it: a patch holds places that much past its sides, so
        // the place where a curve passes to the next is found that much late.)
        let snap = |v: f64, lo: f64, hi: f64| {
            if (v - lo).abs() <= 4e-9 {
                lo
            } else if (v - hi).abs() <= 4e-9 {
                hi
            } else {
                v.clamp(lo, hi)
            }
        };
        P2::new(snap(uv.x, p.1, p.2), snap(uv.y, p.3, p.4))
    }
}

/// Membership over a face's own parameters: whether a place is covered (inside the swept
/// solid) with the instant that covers it, read in full (`cover`) or only about an instant
/// (`near`: cheaper, and enough beside a place already read).
pub(super) struct Field<'a> {
    pub(super) cover: &'a (dyn Fn(P2) -> (bool, f64) + Sync),
    pub(super) near: &'a (dyn Fn(P2, f64) -> (bool, f64) + Sync),
    /// How far a place is from changing membership about an instant, signed (and that instant
    /// as it moves), where the caller has it: nothing where membership changes, of one sign
    /// either side, smooth between. With it a change is closed in on by its slope, not halved.
    pub(super) margin: Option<&'a (dyn Fn(P2, f64) -> (f64, f64) + Sync)>,
}

/// A curve between kept and covered: its points in the face's parameters, each with the
/// instant that covers the far side there; a closed one repeats its first point.
pub(super) struct Chain {
    pub(super) pts: Vec<(P2, f64)>,
    pub(super) closed: bool,
}

/// Where membership changes between a kept place and a covered one (covered at `tau`), along
/// the segment between them: by bisection, read in full at first and then about the covering
/// instant as it moves, and the kept side checked in full at the end (another instant may
/// cover it sooner).
fn crossing(fd: &Field, kept: P2, cov: P2, mut tau: f64) -> (P2, f64) {
    const ROUNDS: usize = 6;
    let at = |s: f64| P2::from(kept.coords + (cov - kept) * s);
    let (mut lo, mut hi) = (0.0, 1.0);
    for round in 0..ROUNDS {
        // (The last round in full throughout: whatever it finds stands.)
        let last = round + 1 == ROUNDS;
        let mut halvings = 0..48;
        // By the margin, once the first reads in full have closed in on the instant: false
        // position between the two sides (Illinois), then the place it comes to checked a hair
        // either side by the membership itself. Anything amiss, and the halving goes on as
        // it would have.
        if let (Some(margin), false) = (fd.margin, last) {
            BY_MARGIN[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            for it in halvings.by_ref().take(6) {
                let _ = it;
                let mid = 0.5 * (lo + hi);
                let (c, t) = (fd.cover)(at(mid));
                if c {
                    hi = mid;
                    tau = t;
                } else {
                    lo = mid;
                }
            }
            let (mut a, mut b) = (lo, hi);
            let (mut fa, mut fb) = (margin(at(a), tau).0, margin(at(b), tau).0);
            let mut found = None;
            if fa.is_finite() && fb.is_finite() && fa * fb < 0.0 {
                // (Which end the last step kept: kept twice, it counts half.)
                let mut kept_end = 0;
                for _ in 0..40 {
                    if b - a < 1e-13 {
                        found = Some(0.5 * (a + b));
                        break;
                    }
                    let x = (a * fb - b * fa) / (fb - fa);
                    let x = if x > a && x < b { x } else { 0.5 * (a + b) };
                    let fx = margin(at(x), tau).0;
                    if !fx.is_finite() {
                        break;
                    }
                    if fx == 0.0 {
                        found = Some(x);
                        break;
                    }
                    if fx * fa > 0.0 {
                        (a, fa) = (x, fx);
                        if kept_end == 2 {
                            fb *= 0.5;
                        }
                        kept_end = 2;
                    } else {
                        (b, fb) = (x, fx);
                        if kept_end == 1 {
                            fa *= 0.5;
                        }
                        kept_end = 1;
                    }
                }
            }
            if let Some(x) = found {
                let e = 2e-13;
                let (lo2, hi2) = ((x - e).max(lo), (x + e).min(hi));
                let (c_lo, _) = (fd.near)(at(lo2), tau);
                let (c_hi, t_hi) = (fd.near)(at(hi2), tau);
                if !c_lo && c_hi {
                    (lo, hi, tau) = (lo2, hi2, t_hi);
                    halvings = 0..0;
                    BY_MARGIN[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        for it in halvings {
            let mid = 0.5 * (lo + hi);
            let (c, t) = if it < 6 || last {
                (fd.cover)(at(mid))
            } else {
                (fd.near)(at(mid), tau)
            };
            if c {
                hi = mid;
                tau = t;
            } else {
                lo = mid;
            }
        }
        // (A little back on the kept side, clear of the change just found, which the two
        // reads place a rounding apart.)
        let back = lo * (1.0 - 1e-6) - 1e-9;
        if last || back <= 0.0 {
            break;
        }
        let (c, t) = (fd.cover)(at(back));
        if !c {
            break;
        }
        hi = back;
        lo = 0.0;
        tau = t;
    }
    (at(0.5 * (lo + hi)), tau)
}

/// How often a change of membership was closed in on by its margin, and how often that stood
/// (`CADRS_TUBE_TIMES` prints them).
static BY_MARGIN: [std::sync::atomic::AtomicUsize; 4] = [
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
    std::sync::atomic::AtomicUsize::new(0),
];

/// [`onto_boundary`] by the margin: the secant of it along the normal from `w`, and the place
/// it comes to checked a hair either side by `test` (membership changes between them). None
/// where the margin isn't to be had, the secant doesn't settle within the reach, or the check
/// fails: the caller then searches as it would have.
pub(super) fn onto_by_margin(
    margin: &(dyn Fn(P2, f64) -> (f64, f64) + Sync),
    test: &(dyn Fn(P2, f64) -> (bool, f64) + Sync),
    w: P2,
    d: V2,
    tau: f64,
    ((u0, u1), (v0, v1)): ((f64, f64), (f64, f64)),
) -> Option<(P2, f64)> {
    use std::sync::atomic::Ordering::Relaxed;
    BY_MARGIN[2].fetch_add(1, Relaxed);
    let n = V2::new(-d.y, d.x).try_normalize(1e-300)?;
    let at = |s: f64| P2::new((w.x + n.x * s).clamp(u0, u1), (w.y + n.y * s).clamp(v0, v1));
    let (c0, t0) = test(w, tau);
    let tc = if c0 { t0 } else { tau };
    let reach = d.norm().max(0.05);
    let f = |s: f64| margin(at(s), tc).0;
    let (mut a, mut fa) = (0.0, f(0.0));
    if !fa.is_finite() {
        return None;
    }
    let mut b = (1e-3 * d.norm()).clamp(1e-9, reach);
    let mut fb = f(b);
    let mut settled = fa == 0.0;
    for _ in 0..16 {
        if settled {
            break;
        }
        if !fb.is_finite() || fb == fa {
            return None;
        }
        let x = b - fb * (b - a) / (fb - fa);
        if !x.is_finite() || x.abs() > reach {
            return None;
        }
        (a, fa) = (b, fb);
        b = x;
        fb = f(b);
        settled = (b - a).abs() < 1e-13 || fb == 0.0;
    }
    if !settled {
        return None;
    }
    let s = if fa == 0.0 { a } else { b };
    let e = 4e-13;
    let (c_lo, t_lo) = test(at(s - e), tc);
    let (c_hi, t_hi) = test(at(s + e), tc);
    if c_lo == c_hi {
        return None;
    }
    BY_MARGIN[3].fetch_add(1, Relaxed);
    Some((at(s), if c_lo { t_lo } else { t_hi }))
}

/// The place where membership changes nearest `w` along the normal to the direction `d` (in
/// the face's parameters, within the box `bx`), by `test` about the instant `tau`: found by
/// doubling steps either way and bisected; None when there is none within a twentieth of a
/// unit, or the length of `d` if that is more.
pub(super) fn onto_boundary(
    test: &(dyn Fn(P2, f64) -> (bool, f64) + Sync),
    w: P2,
    d: V2,
    tau: f64,
    ((u0, u1), (v0, v1)): ((f64, f64), (f64, f64)),
) -> Option<(P2, f64)> {
    let n = V2::new(-d.y, d.x).try_normalize(1e-300)?;
    let at = |s: f64| P2::new((w.x + n.x * s).clamp(u0, u1), (w.y + n.y * s).clamp(v0, v1));
    let (c0, t0) = test(w, tau);
    let mut tc = if c0 { t0 } else { tau };
    // (As far off as the chord is long: a chord across a corner of the curve stands that far
    // from it.)
    let reach = d.norm().max(0.05);
    let mut h: f64 = 1e-9;
    let mut found = None;
    loop {
        for s in [h.min(reach), -h.min(reach)] {
            let (c, t) = test(at(s), tc);
            if c != c0 {
                found = Some((s, t, c));
                break;
            }
        }
        if found.is_some() || h >= reach {
            break;
        }
        h *= 2.0;
    }
    let (s1, t1, c1) = found?;
    if c1 {
        tc = t1;
    }
    let (mut lo, mut hi) = (0.0, s1);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        let (c, t) = test(at(mid), tc);
        if c {
            tc = t;
        }
        if c == c0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((at(0.5 * (lo + hi)), tc))
}

/// The curves between kept and covered over the box `bx` of a face's parameters, by marching
/// squares over a `mu × mv` grid (`nodes`: membership at its nodes, read here when not given)
/// with each crossing found by [`crossing`], each chain then made dense enough that a chord
/// between neighbours strays at most `tol` from the curve, and a cubic through their points in
/// space (`point`) at most about `fit`. Open chains run from a side of the
/// box to a side; closed ones repeat their first point. None when the march fails (a cell with
/// three crossings). `jump`: how far apart two neighbours' covering instants may be and the
/// curve between them still be read about them (further, another instant takes over between
/// them, and it is read in full).
#[allow(clippy::too_many_arguments)]
pub(super) fn cover_curves(
    fd: &Field,
    bx: ((f64, f64), (f64, f64)),
    (mu, mv): (usize, usize),
    nodes: Option<&[(bool, f64)]>,
    tol: f64,
    jump: f64,
    point: &(dyn Fn(P2) -> P3 + Sync),
    fit: f64,
    through: &[(P2, f64)],
) -> Option<Vec<Chain>> {
    let ((u0, u1), (v0, v1)) = bx;
    let at = |i: usize, j: usize| {
        P2::new(
            u0 + (u1 - u0) * i as f64 / mu as f64,
            v0 + (v1 - v0) * j as f64 / mv as f64,
        )
    };
    let read: Vec<(bool, f64)>;
    let nodes = match nodes {
        Some(n) => n,
        None => {
            read = (0..=mu)
                .flat_map(|i| (0..=mv).map(move |j| (i, j)))
                .map(|(i, j)| (fd.cover)(at(i, j)))
                .collect();
            &read
        }
    };
    let node = |i: usize, j: usize| nodes[i * (mv + 1) + j];
    if nodes.iter().all(|n| n.0) || nodes.iter().all(|n| !n.0) {
        return Some(Vec::new());
    }
    // The crossing on each grid edge whose ends differ: keyed by (i, j, along u?).
    type Key = (usize, usize, bool);
    // (Each grid edge membership changes along: the changes found side by side, none of
    // them the others' business.)
    let mut edges: Vec<(Key, P2, P2, f64)> = Vec::new();
    for i in 0..=mu {
        for j in 0..=mv {
            let a = node(i, j);
            for (ni, nj, along) in [(i + 1, j, true), (i, j + 1, false)] {
                if ni > mu || nj > mv {
                    continue;
                }
                let b = node(ni, nj);
                if a.0 == b.0 {
                    continue;
                }
                edges.push(if a.0 {
                    ((i, j, along), at(ni, nj), at(i, j), a.1)
                } else {
                    ((i, j, along), at(i, j), at(ni, nj), b.1)
                });
            }
        }
    }
    let cross: HashMap<Key, (P2, f64)> = {
        use rayon::prelude::*;
        edges
            .par_iter()
            .map(|&(key, kept, cov, tau)| (key, crossing(fd, kept, cov, tau)))
            .collect()
    };
    if let Ok(want) = std::env::var("CADRS_TUBE_CROSS") {
        let v: Vec<f64> = want.split(',').filter_map(|w| w.parse().ok()).collect();
        for (&(i, j, along), c) in &cross {
            if v.len() == 2 && (c.0 - P2::new(v[0], v[1])).norm() < 1e-4 {
                let (ni, nj) = if along { (i + 1, j) } else { (i, j + 1) };
                eprintln!(
                    "  crossing {c:?} between nodes {:?} ({:?}) and {:?} ({:?}) of box {bx:?}, grid {mu} x {mv}",
                    at(i, j),
                    node(i, j),
                    at(ni, nj),
                    node(ni, nj)
                );
            }
        }
    }
    // Each cell's crossings paired (a saddle's by membership at its centre), as links.
    let mut links: HashMap<Key, Vec<Key>> = HashMap::new();
    for i in 0..mu {
        for j in 0..mv {
            let sides: [Key; 4] = [
                (i, j, false),
                (i + 1, j, false),
                (i, j, true),
                (i, j + 1, true),
            ];
            let hits: Vec<Key> = sides
                .into_iter()
                .filter(|k| cross.contains_key(k))
                .collect();
            let pairs: Vec<[Key; 2]> = match hits.len() {
                2 => vec![[hits[0], hits[1]]],
                4 => {
                    let (p, q) = (at(i, j), at(i + 1, j + 1));
                    let centre = (fd.cover)(P2::new(0.5 * (p.x + q.x), 0.5 * (p.y + q.y))).0;
                    // Corner (i, j) and the centre alike: its two sides' crossings apart.
                    if centre == node(i, j).0 {
                        vec![[sides[0], sides[3]], [sides[2], sides[1]]]
                    } else {
                        vec![[sides[0], sides[2]], [sides[1], sides[3]]]
                    }
                }
                0 => Vec::new(),
                _ => return None,
            };
            for [p, q] in pairs {
                links.entry(p).or_default().push(q);
                links.entry(q).or_default().push(p);
            }
        }
    }
    let on_side = |k: &Key| -> bool {
        let &(i, j, along) = k;
        if along {
            j == 0 || j == mv
        } else {
            i == 0 || i == mu
        }
    };
    let mut used: std::collections::HashSet<Key> = Default::default();
    let mut chains: Vec<Chain> = Vec::new();
    let mut keys: Vec<Key> = cross.keys().copied().collect();
    keys.sort();
    // Open chains from the sides first, then closed ones from whatever is left.
    for pass in 0..2 {
        for &s0 in &keys {
            if used.contains(&s0) || (pass == 0) != on_side(&s0) {
                continue;
            }
            let mut pts = vec![cross[&s0]];
            used.insert(s0);
            let mut prev = s0;
            let mut cur = *links.get(&s0)?.first()?;
            let mut closed = false;
            loop {
                pts.push(cross[&cur]);
                if cur == s0 {
                    closed = true;
                    break;
                }
                used.insert(cur);
                if on_side(&cur) {
                    break;
                }
                let next = links.get(&cur)?.iter().copied().find(|k| *k != prev)?;
                prev = cur;
                cur = next;
                if pts.len() > 4 * (mu + 1) * (mv + 1) {
                    return None;
                }
            }
            chains.push(Chain { pts, closed });
        }
    }
    // A node of the curves (`through`: its place, and the instant that covers there: two
    // curves crossing where the faces touch) is a point of each of the chains' passes by it,
    // which the grid, coarser than the passes are apart there, has cut across. Along a pass
    // the covering instant runs through the node's: the node goes between the two points of
    // it either side of that, on up to two passes within a few cells of it.
    for &(wn, tn) in through {
        let off = |p: P2| {
            ((p.x - wn.x) * mu as f64 / (u1 - u0)).hypot((p.y - wn.y) * mv as f64 / (v1 - v0))
        };
        let mut passes: Vec<(f64, usize, usize)> = Vec::new();
        for (c, chain) in chains.iter().enumerate() {
            for k in 0..chain.pts.len() - 1 {
                let ((p, tp), (q, tq)) = (chain.pts[k], chain.pts[k + 1]);
                // (Covered by the node's kind of instant either side, the one before it and
                // the other after.)
                if (tp - tn).abs() < 2.0 && (tq - tn).abs() < 2.0 && (tp - tn) * (tq - tn) <= 0.0 {
                    passes.push((off(p).min(off(q)), c, k));
                }
            }
        }
        passes.retain(|p| p.0 < 6.0);
        passes.sort_by(|a, b| a.0.total_cmp(&b.0));
        passes.truncate(2);
        // (The later of two on one chain first: the other's place in it stands.)
        passes.sort_by_key(|p| std::cmp::Reverse((p.1, p.2)));
        for (_, c, k) in passes {
            if std::env::var_os("CADRS_TUBE_NODES").is_some() {
                let pts = &chains[c].pts;
                eprintln!(
                    "  node {wn:?} ({tn:.9}) into chain {c} of {} points after {k}: {:?}",
                    pts.len(),
                    &pts[k.saturating_sub(2)..(k + 4).min(pts.len())]
                );
            }
            chains[c].pts.insert(k + 1, (wn, tn));
        }
    }
    // Each chain made dense: a point put on the curve between two where the chord strays
    // (each pass, between the points the last one put in: the rest are settled).
    use rayon::prelude::*;
    let full = |w: P2, _: f64| (fd.cover)(w);
    for chain in &mut chains {
        let mut fresh = vec![true; chain.pts.len()];
        for _ in 0..12 {
            let pts = &chain.pts;
            let add: Vec<(usize, (P2, f64))> = (0..pts.len() - 1)
                .into_par_iter()
                .filter(|&i| fresh[i] || fresh[i + 1])
                .filter_map(|i| {
                    let ((p, tp), (q, tq)) = (pts[i], pts[i + 1]);
                    let d = q - p;
                    if d.norm() < 4.0 * tol {
                        return None;
                    }
                    let mid = P2::new(0.5 * (p.x + q.x), 0.5 * (p.y + q.y));
                    let (r, t) = if (tp - tq).abs() > jump {
                        onto_boundary(&full, mid, d, tp, bx)
                    } else {
                        let tm = 0.5 * (tp + tq);
                        fd.margin
                            .and_then(|m| onto_by_margin(m, fd.near, mid, d, tm, bx))
                            .or_else(|| onto_boundary(fd.near, mid, d, tm, bx))
                    }?;
                    if (r - mid).norm() >= d.norm() {
                        return None;
                    }
                    // (By a node two curves run close, the covering instant before the node's
                    // along the one and after it along the other: a point found between two
                    // of the one is of the one.)
                    let strayed = through.iter().any(|&(wn, tn)| {
                        let cells = |w: P2| {
                            ((w.x - wn.x) * mu as f64 / (u1 - u0))
                                .hypot((w.y - wn.y) * mv as f64 / (v1 - v0))
                        };
                        let (sp, sq, sr) = (tp - tn, tq - tn, t - tn);
                        sp.abs() < 2.0
                            && sq.abs() < 2.0
                            && cells(p).min(cells(q)) < 8.0
                            && sp * sq >= 0.0
                            && sr * (sp + sq) < 0.0
                    });
                    if strayed {
                        return None;
                    }
                    // (In space too: a curve straight in the face's parameters may wind round
                    // a thin tube. A cubic through points a chord h apart, the curve s off the
                    // chord midway, misses it by about s³ / h².)
                    let (xp, xq) = (point(p), point(q));
                    let h = (xq - xp).norm();
                    let sag = (point(r) - P3::from(0.5 * (xp.coords + xq.coords))).norm();
                    ((r - mid).norm() > tol || sag.powi(3) > 0.5 * fit * h * h)
                        .then_some((i + 1, (r, t)))
                })
                .collect();
            if add.is_empty() {
                break;
            }
            fresh.iter_mut().for_each(|f| *f = false);
            for (k, (i, r)) in add.into_iter().enumerate() {
                chain.pts.insert(i + k, r);
                fresh.insert(i + k, true);
            }
        }
        if std::env::var_os("CADRS_TUBE_NODES").is_some() {
            for &(wn, _) in through {
                for (k, _) in chain.pts.iter().enumerate().filter(|(_, p)| p.0 == wn) {
                    eprintln!(
                        "  node {wn:?} in its chain made dense, at {k} of {}: {:?}",
                        chain.pts.len(),
                        &chain.pts[k.saturating_sub(5)..(k + 6).min(chain.pts.len())]
                    );
                }
            }
        }
    }
    Some(chains)
}

/// A curve's point: where it is, and its place on each of its two faces.
pub(super) type CurvePt = (P3, P2, P2);

/// What a chain's point is between: its place, its covering instant and the faces preferred
/// (those of the stretch so far), to the two faces and the point on them.
pub(super) type PairAt<'a> =
    dyn Fn(P2, f64, Option<(usize, usize)>) -> Option<((usize, usize), CurvePt)> + 'a;

/// A chain's stretches, each between two faces: `pair` reads a chain point (its place and
/// covering instant; the faces of the stretch so far preferred where it lies on an edge) as
/// the two faces and the point on them, None where the curve bounds nothing (buried, or a
/// face not found); where the faces change between two points, the place it changes at is
/// found by bisection along the curve (`onto` brings a place of the chord back onto it, read in
/// full where asked), and ends one stretch and starts the next (`place`: a point given its
/// faces, on their boundary). A stretch of one face with itself (a sheet crossing itself
/// beside a fold) holds the curve twice, out to the fold's tip and back: its first half is
/// kept, the other being its far side.
#[allow(clippy::too_many_arguments)]
pub(super) fn chain_runs(
    chain: &Chain,
    pair: &PairAt,
    place: &dyn Fn(P2, f64, (usize, usize)) -> CurvePt,
    onto: &dyn Fn(P2, V2, f64, bool) -> Option<(P2, f64)>,
    margin: &dyn Fn(P2, f64) -> (f64, f64),
    jump: f64,
    through: &[P2],
    out: &mut Vec<super::sweep_moving::Given>,
) {
    let first = out.len();
    let mut pts: Vec<(P2, f64)> = chain.pts.clone();
    let node = |w: &P2| through.iter().any(|n| n == w);
    if chain.closed && pts.len() > 2 {
        // Started where the faces change, if anywhere (the first point repeated at the end).
        pts.pop();
        let n = pts.len();
        let faces: Vec<Option<(usize, usize)>> = pts
            .iter()
            .map(|&(w, t)| pair(w, t, None).map(|h| h.0))
            .collect();
        if let Some(k) = (0..n).find(|&i| faces[i] != faces[(i + 1) % n]) {
            pts.rotate_left((k + 1) % n);
        }
        let first = pts[0];
        pts.push(first);
    }
    if let Ok(v) = std::env::var("CADRS_TUBE_CHAIN") {
        // u, v, radius: the chain's points (place, covering instant, faces) within that of there.
        let v: Vec<f64> = v.split(',').filter_map(|w| w.trim().parse().ok()).collect();
        if let [u, w, r] = v[..] {
            for (i, (p, t)) in pts.iter().enumerate() {
                if (p - P2::new(u, w)).norm() < r {
                    eprintln!(
                        "  chain point {i}: ({:.9}, {:.9}) covered at {t:.9}: {:?}",
                        p.x,
                        p.y,
                        pair(*p, *t, None).map(|h| (h.0, h.1.2))
                    );
                }
            }
        }
    }
    // (A run's points with their places on the chain and covering instants.)
    type RunPt = (CurvePt, P2, f64);
    // (Whether a place found from between two of a chain's lies between them along it.)
    let between = |m: P2, w0: P2, w1: P2| -> bool {
        let d = w1 - w0;
        let s = (m - w0).dot(&d);
        s > 0.0 && s < d.norm_squared()
    };
    let mut flush = |run: &mut Vec<RunPt>, faces: Option<(usize, usize)>| {
        run.dedup_by(|q, p| (q.0.0 - p.0.0).norm() <= 1e-12 * (1.0 + p.0.0.coords.norm()));
        if let Some((a, b)) = faces
            && run.len() >= 2
        {
            // (Too few points for a cubic through them: more put between.)
            for _ in 0..2 {
                if run.len() >= 5 {
                    break;
                }
                let mut k = 0;
                while k + 1 < run.len() {
                    let ((_, w0, t0), (_, w1, t1)) = (run[k], run[k + 1]);
                    let d = w1 - w0;
                    let full = (t1 - t0).abs() > jump;
                    if d.norm() > 1e-7
                        && let Some((m, t)) = onto(
                            P2::from(0.5 * (w0.coords + w1.coords)),
                            d,
                            if full { t0 } else { 0.5 * (t0 + t1) },
                            full,
                        )
                        && between(m, w0, w1)
                        // (Of this stretch: beside a corner the nearest boundary may be the
                        // next stretch's.)
                        && let Some((f, pt)) = pair(m, t, Some((a, b)))
                        && f == (a, b)
                    {
                        run.insert(k + 1, (pt, m, t));
                        k += 1;
                    }
                    k += 1;
                }
            }
            if a == b {
                // (Where its two places change sides the curve has passed its tip: found
                // between the two points either side, until they are as good as one.)
                let apart = |p: &CurvePt| p.1 - p.2;
                if let Some(k) =
                    (0..run.len() - 1).find(|&k| apart(&run[k].0).dot(&apart(&run[k + 1].0)) < 0.0)
                {
                    let most = run.iter().map(|r| apart(&r.0).norm()).fold(0.0, f64::max);
                    let (mut lo, mut hi) = (run[k], run[k + 1]);
                    for _ in 0..48 {
                        if apart(&lo.0).norm().max(apart(&hi.0).norm()) <= 1e-6 * most {
                            break;
                        }
                        let d = hi.1 - lo.1;
                        let Some((m, t)) = onto(
                            P2::from(0.5 * (lo.1.coords + hi.1.coords)),
                            d,
                            0.5 * (lo.2 + hi.2),
                            false,
                        )
                        .filter(|(m, _)| between(*m, lo.1, hi.1)) else {
                            break;
                        };
                        let c = (place(m, t, (a, b)), m, t);
                        if apart(&c.0).dot(&apart(&lo.0)) > 0.0 {
                            lo = c;
                        } else {
                            hi = c;
                        }
                    }
                    // (The curve out to its tip, its two places one there; the points found
                    // on the way left out, crowded as they are against it.)
                    run.truncate(k + 1);
                    lo.0.2 = lo.0.1;
                    run.push(lo);
                }
            }
            let mut pts: Vec<CurvePt> = run.drain(..).map(|r| r.0).collect();
            if a == b {
                // Out to where its two places come together, and no further.
                let apart = |p: &CurvePt| (p.1 - p.2).norm();
                let (tip, least) = pts
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (i, apart(p)))
                    .min_by(|x, y| x.1.total_cmp(&y.1))
                    .unwrap();
                let most = pts.iter().map(apart).fold(0.0, f64::max);
                if least < 0.05 * most && tip > 0 && tip + 1 < pts.len() {
                    pts.truncate(tip + 1);
                    let t = &mut pts[tip];
                    t.2 = t.1;
                }
            }
            if pts.len() >= 2 {
                out.push(super::sweep_moving::Given { a, b, pts });
            }
        }
        run.clear();
    };
    // (The few points of a stretch next a corner at `wc`, before it and after, put in their
    // order from it.)
    // (As many as are out of their order: a point put between the two ends of a cut across
    // a thin spike may lie many points back along either side of it.)
    let before = |run: &mut Vec<RunPt>, wc: P2| {
        let far = |r: &RunPt| (r.1 - wc).norm();
        let n = run.len();
        let mut k = n.saturating_sub(4);
        let mut most = run[k..].iter().map(far).fold(0.0, f64::max);
        while k > 0 && n - k < 48 && far(&run[k - 1]) < most {
            k -= 1;
            most = most.max(far(&run[k]));
        }
        run[k..].sort_by(|x, y| far(y).total_cmp(&far(x)));
    };
    let after = |pts: &mut [(P2, f64)], i: usize, wc: P2, faces: Option<(usize, usize)>| {
        let far = |p: &(P2, f64)| (p.0 - wc).norm();
        let mut j = i;
        let mut most = far(&pts[i]);
        while j + 1 < pts.len()
            && j < i + 48
            && (j < i + 3 || far(&pts[j + 1]) < most)
            && (pts[j + 1].1 - pts[j].1).abs() <= jump
            && pair(pts[j + 1].0, pts[j + 1].1, faces).map(|h| h.0) == faces
        {
            j += 1;
            most = most.max(far(&pts[j]));
        }
        let was: Vec<P2> = pts[i..=j].iter().map(|p| p.0).collect();
        pts[i..=j].sort_by(|x, y| (x.0 - wc).norm().total_cmp(&(y.0 - wc).norm()));
        if std::env::var_os("CADRS_TUBE_NODES").is_some()
            && was != pts[i..=j].iter().map(|p| p.0).collect::<Vec<_>>()
        {
            eprintln!(
                "  points {i}..={j} put in their order from {wc:?} (faces {faces:?}): they were {was:?}"
            );
        }
    };
    let mut run: Vec<RunPt> = Vec::new();
    let mut cur: Option<(usize, usize)> = None;
    for i in 0..pts.len() {
        let (w, tau) = pts[i];
        let here = pair(w, tau, cur);
        let faces = here.map(|h| h.0);
        let mut last = (w, tau, here);
        // (A turn from what an instant covers onto a seam, or off one, is a corner too, though
        // the face across it be the same: the stretches either side are two curves. So may a
        // leap of the covering instant be, the face across being the same face at another
        // place of it (where this one cuts through that one's crossing of itself): a corner
        // if one is found there.)
        let marked = i > 0 && faces.is_some() && (tau < -0.5) != (pts[i - 1].1 < -0.5);
        let leapt = i > 0
            && faces.is_some()
            && faces == cur
            && tau > -0.5
            && pts[i - 1].1 > -0.5
            && (tau - pts[i - 1].1).abs() > jump;
        let turned = marked || leapt;
        if i == 0 {
            cur = faces;
        } else if faces != cur || turned {
            // Where the faces change between the last point and this: once, or through others
            // between (a corner of a third face the curve cuts across).
            let (p0, t0) = pts[i - 1];
            let d = w - p0;
            let full = (tau - t0).abs() > jump;
            let on = |s: f64| -> Option<(P2, f64)> {
                let m = P2::from(p0.coords + d * s);
                let t = if s < 0.5 { t0 } else { tau };
                onto(m, d, if full { t } else { t0 + (tau - t0) * s }, full)
            };
            // A corner of the curve (it turns from the boundary of what one instant covers to
            // that of another's, or a seam's: where three faces meet): where both boundaries'
            // margins are nothing, found from between the two points, so that every face's
            // reading of the corner is the one point.
            let corner = (|| {
                let (c, f) = (cur?, faces?);
                if !full {
                    return None;
                }
                // (From the last point on the one boundary, the first on the other, or between:
                // a boundary's margin is read only where what covers beyond it still has a
                // least of its own, which it may not have far off the boundary.)
                let ends = (p0, w);
                // (Where the two meet in a thin spike the grid cut the tip off, the corner is
                // as many times as far off as the cut is wide as the spike is sharp: down to a
                // degree of angle here.)
                let reach = 60.0 * d.norm() + 1e-9;
                let tries = [
                    ends.0,
                    ends.1,
                    P2::from(0.5 * (ends.0.coords + ends.1.coords)),
                ];
                tries.into_iter().find_map(|start| {
                    let mut w = start;
                    let (mut ta, mut tb) = (t0, tau);
                    let trace = std::env::var_os("CADRS_TUBE_CORNER").is_some();
                    if trace {
                        eprintln!(
                            "  corner of {c:?} and {f:?} from {start:?} (instants {ta:.9}, {tb:.9}; reach {reach:.2e})"
                        );
                    }
                    for _ in 0..64 {
                        let (ma, na) = margin(w, ta);
                        let (mb, nb) = margin(w, tb);
                        if trace {
                            eprintln!("    at {w:?}: margins {ma:.3e} ({na:.9}), {mb:.3e} ({nb:.9})");
                        }
                        if !(ma.is_finite() && mb.is_finite()) {
                            return None;
                        }
                        (ta, tb) = (na, nb);
                        let h = 1e-7;
                        let col = |dw: V2| {
                            let (pa, pb) = (margin(w + dw, ta).0, margin(w + dw, tb).0);
                            let (qa, qb) = (margin(w - dw, ta).0, margin(w - dw, tb).0);
                            V2::new(pa - qa, pb - qb) / (2.0 * h)
                        };
                        let jac = nalgebra::Matrix2::from_columns(&[
                            col(V2::new(h, 0.0)),
                            col(V2::new(0.0, h)),
                        ]);
                        let mut step = jac.try_inverse()? * V2::new(ma, mb);
                        if !step.norm().is_finite() {
                            return None;
                        }
                        // (No further at a time than a few times the cut across: the margins
                        // are no straight lines, and a long step lands by another corner of
                        // the same two boundaries.)
                        let most = 4.0 * d.norm() + 1e-9;
                        if step.norm() > most {
                            step *= most / step.norm();
                        }
                        w -= step;
                        if (w - start).norm() > reach {
                            return None;
                        }
                        if step.norm() < 1e-13 {
                            // (A corner indeed: both margins nothing there, and the faces across
                            // them the two stretches'.)
                            let (ma, mb) = (margin(w, ta).0, margin(w, tb).0);
                            let (pa, pb) = (pair(w, ta, cur), pair(w, tb, faces));
                            let ok = ma.abs() < 0.1
                                && mb.abs() < 0.1
                                && pa.map(|h| h.0) == cur
                                && pb.map(|h| h.0) == faces
                                // (Where only the instant leapt: two places of the face
                                // across indeed, at two instants.)
                                && (!leapt
                                    || ((ta - tb).abs() > 0.25 * jump
                                        && pa.zip(pb).is_some_and(|(x, y)| {
                                            (x.1.2 - y.1.2).norm() > 1e-6
                                        })));
                            return ok.then_some((w, ta, tb, c, f));
                        }
                    }
                    None
                })
            })();
            if let Some((wc, ta, tb, c, f)) = corner {
                // (Where the two stretches meet in a spike too thin for the grid they were
                // read on, the chain cuts its tip off, and a point put between the two ends
                // of the cut may lie past its neighbour from the corner: the few points next
                // the corner, either side, put in their order from it.)
                before(&mut run, wc);
                after(&mut pts, i, wc, faces);
                last = (pts[i].0, pts[i].1, pair(pts[i].0, pts[i].1, faces));
                run.push((place(wc, ta, c), wc, ta));
                flush(&mut run, cur);
                run.push((place(wc, tb, f), wc, tb));
                cur = faces;
            }
            let mut from = 0.0;
            for _ in 0..8 {
                if cur == faces {
                    break;
                }
                let (mut lo, mut hi) = (from, 1.0);
                for _ in 0..40 {
                    let s = 0.5 * (lo + hi);
                    if on(s).is_some_and(|(m, t)| pair(m, t, cur).map(|h| h.0) == cur) {
                        lo = s;
                    } else {
                        hi = s;
                    }
                }
                // (The stretch's last point, if it is one of it: where the change is at the
                // very point before, what is read there may be the next stretch's.)
                if let Some(c) = cur
                    && let Some((m, t)) = on(lo)
                    && pair(m, t, cur).is_some_and(|h| h.0 == c)
                {
                    before(&mut run, m);
                    run.push((place(m, t, c), m, t));
                }
                flush(&mut run, cur);
                // (What the curve is between just past there: this point's faces, or others.)
                let past = on(hi);
                let next = match past {
                    Some((m, t)) if hi < 1.0 - 1e-9 => pair(m, t, faces).map(|h| h.0),
                    _ => faces,
                };
                if let Some(c) = next
                    && let Some((m, t)) = past
                    && pair(m, t, next).is_some_and(|h| h.0 == c)
                {
                    run.push((place(m, t, c), m, t));
                }
                cur = next;
                from = hi;
            }
            if cur != faces {
                flush(&mut run, cur);
                cur = faces;
            } else if let Some(&(_, wc, _)) = run.last()
                && corner.is_none()
                && !leapt
            {
                after(&mut pts, i, wc, faces);
                last = (pts[i].0, pts[i].1, pair(pts[i].0, pts[i].1, faces));
            }
        }
        if let (w, tau, Some((_, pt))) = last {
            // (A node ends one stretch and begins the next: the faces touch there, and two
            // curves cross.)
            if node(&w) && !run.is_empty() {
                if std::env::var_os("CADRS_TUBE_NODES").is_some() {
                    eprintln!(
                        "  node at {w:?} (point {i} of {}): the stretch before it ends {:?}; after it {:?}",
                        pts.len(),
                        run.iter().rev().take(5).map(|r| r.1).collect::<Vec<_>>(),
                        pts.iter()
                            .skip(i + 1)
                            .take(4)
                            .map(|p| p.0)
                            .collect::<Vec<_>>()
                    );
                }
                before(&mut run, w);
                run.push((pt, w, tau));
                flush(&mut run, cur);
                if i + 1 < pts.len() {
                    after(&mut pts, i + 1, w, cur);
                }
            }
            run.push((pt, w, tau));
        }
    }
    flush(&mut run, cur);
    // Two stretches end to end between the same faces, the curve running straight on from one
    // into the other (a stretch between them too short to keep, or of faces not found), are one.
    let mut k = first + 1;
    while k < out.len() {
        let (p, q) = (&out[k - 1], &out[k]);
        let (n, m) = (p.pts.len(), q.pts.len());
        let on = (p.a, p.b) == (q.a, q.b)
            && !node(&q.pts[0].1)
            && n >= 2
            && m >= 2
            && (p.pts[n - 1].0 - q.pts[0].0).norm() <= 1e-12 * (1.0 + q.pts[0].0.coords.norm())
            && (p.pts[n - 1].1 - q.pts[0].1).norm() <= 1e-12
            && (p.pts[n - 1].2 - q.pts[0].2).norm() <= 1e-9
            && {
                let (d0, d1) = (p.pts[n - 1].1 - p.pts[n - 2].1, q.pts[1].1 - q.pts[0].1);
                d0.dot(&d1) > 0.9 * d0.norm() * d1.norm()
            };
        if on {
            let tail = out.remove(k);
            out[k - 1].pts.extend(tail.pts.into_iter().skip(1));
        } else {
            k += 1;
        }
    }
    // (A closed chain's last stretch and its first likewise, round where it was begun.)
    if chain.closed && out.len() >= first + 2 {
        let (p, q) = (&out[out.len() - 1], &out[first]);
        let (n, m) = (p.pts.len(), q.pts.len());
        let on = (p.a, p.b) == (q.a, q.b)
            && !node(&q.pts[0].1)
            && n >= 2
            && m >= 2
            && (p.pts[n - 1].0 - q.pts[0].0).norm() <= 1e-12 * (1.0 + q.pts[0].0.coords.norm())
            && (p.pts[n - 1].1 - q.pts[0].1).norm() <= 1e-12
            && (p.pts[n - 1].2 - q.pts[0].2).norm() <= 1e-9
            && {
                let (d0, d1) = (p.pts[n - 1].1 - p.pts[n - 2].1, q.pts[1].1 - q.pts[0].1);
                d0.dot(&d1) > 0.9 * d0.norm() * d1.norm()
            };
        if on {
            let mut head = out.pop().unwrap().pts;
            head.extend(std::mem::take(&mut out[first].pts).into_iter().skip(1));
            out[first].pts = head;
        }
    }
}
