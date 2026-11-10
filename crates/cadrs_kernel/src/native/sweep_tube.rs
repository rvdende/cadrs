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
struct Jet {
    r: [M3; 3],
    d: [V3; 3],
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

/// A torus and its motion, with the samples of the motion its inside test uses.
pub(crate) struct Tube<'a> {
    steps: &'a [RigidStep],
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
    pinches: Vec<Pinch>,
}

/// A local least of the distance over the motion, less r: (instant, value).
type Least = (f64, f64);

impl<'a> Tube<'a> {
    fn new(steps: &'a [RigidStep], fr: &Frame, big: f64, r: f64, size: f64) -> Tube<'a> {
        // Samples a few thousandths of a turn apart.
        let (r0, _) = place(steps, 0.0);
        let mut turn = 0.0;
        let mut prev = r0;
        for i in 1..=256 {
            let (ri, _) = place(steps, i as f64 / 256.0);
            turn += nalgebra::Rotation3::from_matrix_unchecked(prev.transpose() * ri).angle();
            prev = ri;
        }
        let n = ((turn / 0.003).ceil() as usize).clamp(1024, 8192);
        let ts: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
        let places = ts.iter().map(|&t| place(steps, t)).collect();
        Tube {
            steps,
            c: fr.o,
            x: fr.x,
            y: fr.y,
            z: fr.z,
            big,
            r,
            size,
            ts,
            places,
            pinches: Vec::new(),
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
        let len = m.norm();
        let n = m / len;
        let mp = s[4].cross(&s[2]) + s[1].cross(&s[5]);
        let npsi = (mp - n * n.dot(&mp)) / len;
        SheetPt {
            s,
            n,
            npsi,
            sin: len / (s[1].norm() * s[2].norm()),
        }
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
                let j = jet(self.steps, t);
                let sp = self.sheet_pt(&j, psi);
                KPt {
                    p: P3::from(sp.s[0] + sp.n * (s * self.r)),
                    own: t,
                }
            }
            c if c >= PINCH => {
                let tp = self.pinch_at(c - PINCH, pos.a, psi);
                let j = jet(self.steps, tp.x);
                let (p, _, _) = self.torus_pt(&j, tp.y, psi);
                KPt { p, own: tp.x }
            }
            _ => {
                // The leading half at the end runs from θ(N) down by π; the trailing half at the
                // start from θ(−N) down by π.
                let t = if pos.chart == END { 1.0 } else { 0.0 };
                let j = jet(self.steps, t);
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
        self.f_at(&place(self.steps, t), p)
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
        let vals: Vec<f64> = self.places.iter().map(|pl| self.f_at(pl, p)).collect();
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
            let v = if chart >= PINCH {
                1.0 - w.x.abs()
            } else {
                let psi = w.y;
                let across = self.across(kp.own, psi);
                self.pinches
                    .iter()
                    .min_by(|a, b| {
                        let d = |pn: &Pinch| (kp.own - pn.c.x).hypot(wrap(psi - pn.c.y, -PI, TAU));
                        d(a).total_cmp(&d(b))
                    })
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
        if chart >= PINCH {
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
        let near = if (chart == LEAD) == low { 0.0 } else { 1.0 };
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
        let j = jet(self.steps, t);
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
        let j = jet(self.steps, t);
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
        let grid: Vec<f64> = (0..=nt)
            .flat_map(|i| (0..np).map(move |k| (i, k)))
            .map(|(i, k)| size(i, k))
            .collect();
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
        let j = jet(self.steps, t);
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
        if chart >= PINCH {
            self.pinch_at(chart - PINCH, w.x, w.y).y
        } else {
            w.y
        }
    }

    /// The seam a chart's place is beyond, if it is: a sheet's inside a pinch (left to the
    /// pinch's chart), a pinch chart's past its `rho` (left to the sheets). As an instant no
    /// motion has, −1 less the pinch's index: what covers the place is the other chart.
    fn seam(&self, chart: usize, w: P2, own: f64) -> Option<f64> {
        if chart >= PINCH {
            (w.x.abs() > 1.0).then_some(-1.0 - (chart - PINCH) as f64)
        } else {
            self.in_pinch(own, w.y).map(|k| -1.0 - k as f64)
        }
    }

    /// The rate at which the distance of `p` from the core circle changes at the instant `t`.
    fn rate(&self, t: f64, p: &P3) -> f64 {
        let j = jet(self.steps, t);
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
pub fn torus(k: &mut NativeKernel, b: &Body, steps: &[RigidStep]) -> Option<Result<OpResult>> {
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
    let refused = |why: String| -> Option<Result<OpResult>> {
        if debug {
            eprintln!("tube sweep: refused: {why}");
        }
        Some(Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!("the swept torus: {why}"),
        }))
    };
    let size = b.bbox().diagonal().max(super::tol::POINT);
    let mut tube = Tube::new(steps, fr, *rmaj, *rmin, size);
    // Σ regular (its tangents apart everywhere): its normal, and so the sheets, defined.
    let mut least = f64::INFINITY;
    for i in 0..=200 {
        let j = jet(steps, i as f64 / 200.0);
        for k in 0..128 {
            least = least.min(tube.sheet_pt(&j, TAU * k as f64 / 128.0).sin);
        }
    }
    let pinches = tube.find_pinches();
    if debug {
        eprintln!(
            "tube sweep: the core circle's sweep least regular {least:.5}; pinches {pinches:?}"
        );
    }
    if least < 1e-3 && pinches.is_empty() {
        return refused(format!("its core circle's sweep folds ({least:.4})"));
    }
    // (A pinch's chart joins up but for slivers beside its seam: until those are right, a
    // pinched sweep is refused, unless asked for.)
    if !pinches.is_empty() && std::env::var_os("CADRS_TUBE_PINCH").is_none() {
        return refused(format!(
            "its core circle's sweep pinches (at {:.4}, {:.4})",
            pinches[0].x, pinches[0].y
        ));
    }
    if let Some(c) = pinches.iter().find(|c| !(0.02..=0.98).contains(&c.x)) {
        return refused(format!(
            "its core circle's sweep pinches at the motion's end ({:.4})",
            c.x
        ));
    }
    let place_r = |t: f64| {
        let (m, d) = place(steps, t);
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
    // The sheets' patches: chunks of at most a quarter radian of turn by quarters of ψ, each
    // read by membership on a coarse grid, fitted where any of it is kept, and cut in four
    // where the fit misses.
    // (The turn along the motion, summed: a whole turn and back comes to nothing net.)
    let mut turn = 0.0;
    let mut prev = place(steps, 0.0).0;
    for i in 1..=512 {
        let ri = place(steps, i as f64 / 512.0).0;
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
        let reach = V2::new(0.1 / sheet_chunks as f64, 0.1 * TAU / npsi as f64);
        let shape = nalgebra::Matrix2::new(1.0 / reach.x, 0.0, 0.0, 1.0 / reach.y) * inv;
        tube.pinches.push(Pinch {
            c,
            inv,
            reach,
            shape,
        });
    }
    let tube = tube;
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
        if p.0 >= PINCH {
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
        let (rm, dm) = place(steps, t);
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
        let j = jet(steps, t);
        let sp = tube.sheet_pt(&j, psi);
        tube.theta_n(&j, &sp, psi).0
    };
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
                let (v0, v1) = (PI * h as f64, PI * (h + 1) as f64);
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
                    for k in [-1.0, 0.0, 1.0] {
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
                            let j = jet(steps, t_end);
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
    let delta = tube.delta();
    let jump = 4.0 * tube.step();
    // The sheets read by membership, their curves traced, and each then fitted over the part of
    // it kept (which only the curves and the reading tell).
    if debug {
        eprintln!(
            "tube sweep: the tori's {} pieces cut along the joins by {:.0} ms",
            faces.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    let (sheets, fits, kept_patches, given, on_sheet) = {
        let sheets: Vec<Sheet> = patches
            .iter()
            .map(|&p| Sheet::read(&tube, p, grid_of(p)))
            .collect();
        let t_fitted = web_time::Instant::now();
        let kept_patches: Vec<Patch> = sheets
            .iter()
            .map(|sh| sh.patch)
            .chain(torus_patches.iter().copied())
            .collect();
        let n_sheets = sheets.len();
        let fc = Faces {
            tube: &tube,
            patches: &kept_patches,
            sheets: n_sheets,
            holds: &holds,
            bare: &torus_bare,
            tori: &tori,
        };
        // The curves the boundary crosses each sheet's patch along, read in the patch's own (a, ψ)
        // by membership: where its points pass from kept to covered. Each point of one is touched
        // by the torus at the instant that covers the far side, on the face the torus's surface
        // passes through it on then (another sheet's patch, this one's elsewhere, or the torus
        // where the motion starts or stops): the curve is those two faces' meeting, known whole
        // with its place on each, with no search for it in space (where the faces graze, at the
        // joins and beside a fold, such a search wanders).
        let traced: Vec<Vec<super::sweep_moving::Given>> = sheets
            .par_iter()
            .enumerate()
            .map(|(i, sh)| {
                let (chart, a0, a1, p0, p1) = sh.patch;
                let cover = |w: P2| tube.sheet_cover(chart, w);
                let near = |w: P2, tc: f64| tube.sheet_cover_near(chart, w, tc);
                let fd = Field {
                    cover: &cover,
                    near: &near,
                };
                let bx = ((a0, a1), (p0, p1));
                let point = |w: P2| {
                    tube.eval(&Pos {
                        chart,
                        a: w.x,
                        psi: w.y,
                    })
                    .p
                };
                let chains = cover_curves(
                    &fd,
                    bx,
                    sh.grid,
                    Some(&sh.nodes),
                    1e-6,
                    jump,
                    &point,
                    fit_tol,
                )
                .unwrap_or_default();
                // (A place covered across a seam, by the other chart there: found by the place's
                // own instant.)
                let pair = |w: P2, tau: f64, prefer: Option<(usize, usize)>| {
                    let kp = tube.eval(&Pos {
                        chart,
                        a: w.x,
                        psi: w.y,
                    });
                    let x = kp.p;
                    if std::env::var("CADRS_TUBE_PAIR").is_ok_and(|v| v == i.to_string()) {
                        eprintln!(
                            "  pair on {i} at {w:?} tau {tau:.12}: at end {:?}, rates {:.3e} {:.3e}, exact {:.12} -> {:?} (prefer {prefer:?})",
                            tube.at_end(tau, &x),
                            tube.rate(0.0, &x),
                            tube.rate(1.0, &x),
                            tube.foot(tau.clamp(0.0, 1.0), &x),
                            fc.at(&x, tau, prefer.map(|p| p.1))
                        );
                    }
                    let (fb, uvb) = if tau < -0.5 {
                        fc.across_seam(chart, &x, kp.own, prefer.map(|p| p.1))?
                    } else {
                        fc.at(&x, tau, prefer.map(|p| p.1))?
                    };
                    Some(((i, fb), (x, w, uvb)))
                };
                let place = |w: P2, tau: f64, (_, fb): (usize, usize)| {
                    let kp = tube.eval(&Pos {
                        chart,
                        a: w.x,
                        psi: w.y,
                    });
                    let tau = if tau < -0.5 { kp.own } else { tau };
                    (kp.p, w, fc.clamped(fb, &kp.p, tau))
                };
                let margin = |w: P2, tau: f64| tube.margin(chart, w, tau);
                let onto = |w: P2, d: V2, tau: f64, full: bool| {
                    if full {
                        onto_boundary(&|w, _| cover(w), w, d, tau, bx)
                    } else {
                        onto_boundary(&near, w, d, tau, bx)
                    }
                };
                let mut out = Vec::new();
                for chain in chains {
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
                    chain_runs(&chain, &pair, &place, &onto, &margin, jump, &mut out);
                }
                out
            })
            .collect();
        let mut given: Vec<super::sweep_moving::Given> = traced.into_iter().flatten().collect();
        // (Two patches' crossing is read from each, and a patch's crossing of itself from each of
        // its two places: one reading of the two kept, the lower patch's or the first. A reading
        // without its twin stays: a stretch too short for the other's grid to have found.)
        {
            let same = 1e-6 * size;
            let ends = |g: &super::sweep_moving::Given| (g.pts[0].0, g.pts[g.pts.len() - 1].0);
            let mut keep = vec![true; given.len()];
            for i in 0..given.len() {
                let g = &given[i];
                if g.b >= n_sheets {
                    continue;
                }
                let (g0, g1) = ends(g);
                let twin = (0..given.len()).any(|j| {
                    let h = &given[j];
                    let (h0, h1) = ends(h);
                    j != i
                        && keep[j]
                        && (h.a, h.b) == (g.b, g.a)
                        && (g.a > g.b || j < i)
                        && (((g0 - h0).norm() <= same && (g1 - h1).norm() <= same)
                            || ((g0 - h1).norm() <= same && (g1 - h0).norm() <= same))
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
        let n_sheet_curves = given.len();
        // Where the torus at the start crosses the torus at the end (a motion that comes back near
        // where it began): the zero set of the one's distance over the other's (ψ, θ), kept where
        // no other instant covers it.
        {
            let start = &tori[0];
            let far = |w: P2| {
                let x = start.eval(w.x, w.y);
                (tube.f(1.0, &x) < -delta, 1.0)
            };
            let near = |w: P2, _: f64| far(w);
            let fd = Field {
                cover: &far,
                near: &near,
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
                        &|w: P2| start.eval(w.x, w.y),
                        fit_tol,
                    )
                    .unwrap_or_default();
                    let pair = |w: P2, _: f64, prefer: Option<(usize, usize)>| {
                        let x = start.eval(w.x, w.y);
                        // (On the half of each torus the motion leaves bare: past its join, the
                        // sheet from there is the boundary; and under no instant between.)
                        if !tube.bare(START, &x)
                            || !tube.bare(END, &x)
                            || tube.least(&x).1 < -100.0 * delta
                        {
                            return None;
                        }
                        let (fa, uva) = fc.at(&x, 0.0, prefer.map(|p| p.0))?;
                        let (fb, uvb) = fc.at(&x, 1.0, prefer.map(|p| p.1))?;
                        Some(((fa, fb), (x, uva, uvb)))
                    };
                    let place = |w: P2, _: f64, (fa, fb): (usize, usize)| {
                        let x = start.eval(w.x, w.y);
                        (x, fc.clamped(fa, &x, 0.0), fc.clamped(fb, &x, 1.0))
                    };
                    let onto =
                        |w: P2, d: V2, tau: f64, _: bool| onto_boundary(&near, w, d, tau, bx);
                    // (Which side of each torus's join a place is: the half the motion leaves bare.)
                    let bare = |w: P2| {
                        let x = start.eval(w.x, w.y);
                        [tube.bare(START, &x), tube.bare(END, &x)]
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
                            &mut given,
                        );
                    }
                }
            }
        }
        let t_traced = web_time::Instant::now();
        if debug {
            let ms = |a: web_time::Instant, b: web_time::Instant| (b - a).as_secs_f64() * 1e3;
            eprintln!(
                "tube sweep: {} sheet patches of {} tried ({:.0} ms), {} faces with the tori's; {} curves on the sheets and {} between the tori ({:.0} ms)",
                n_sheets,
                patches.len(),
                ms(t_start, t_fitted),
                faces.len(),
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
        // Each sheet's patch fitted over the part of it kept, held to its curves' points too.
        let mut on_sheet: Vec<Vec<(P3, P2)>> = vec![Vec::new(); n_sheets];
        for g in &given {
            for p in &g.pts {
                if g.a < n_sheets {
                    on_sheet[g.a].push((p.0, p.1));
                }
                if g.b < n_sheets {
                    on_sheet[g.b].push((p.0, p.2));
                }
            }
        }
        let fitted: Vec<std::result::Result<(super::nurbs::NurbsSurface, Dom), String>> = sheets
            .par_iter()
            .zip(&on_sheet)
            .map(|(sh, pts)| fit_tensor(&tube, sh, pts, fit_tol))
            .collect();
        let mut fits = Vec::with_capacity(n_sheets);
        for f in fitted {
            match f {
                Ok(f) => fits.push(f),
                Err(why) => return refused(why),
            }
        }
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
        (sheets, fits, kept_patches, given, on_sheet)
    };
    let n_sheets = sheets.len();
    // The sheets' faces, each turned to face away from the core circle placed at its points'
    // own instant: read at a kept point (past a fold the sheet faces the other way, and lies
    // inside), the kept node with the most kept round it, or, a patch covered at every node, a
    // place on a curve of it, which the part of it kept runs up to.
    let mut sheet_faces: Vec<Body> = Vec::with_capacity(n_sheets + faces.len());
    for ((sh, pts), (fit, dom)) in sheets.iter().zip(&on_sheet).zip(fits) {
        let (chart, a0, a1, p0, p1) = sh.patch;
        let surf = super::surface::Surface::Nurbs(std::sync::Arc::new(fit));
        let mut sheet = match super::face_build::face_from_surface(&surf, Some(dom)) {
            Ok(b) => b,
            Err(e) => return Some(Err(e)),
        };
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
        let (rm, dm) = place(steps, kp.own);
        let core = rm * tube.k(tube.psi_of(chart, w)) + dm;
        let face = &mut sheet.faces[0];
        let (_, su, sv) = face.surf.d1(w.x, w.y);
        let natural = su.cross(&sv) * if face.sense { 1.0 } else { -1.0 };
        if natural.dot(&(kp.p.coords - core)) < 0.0 {
            Body::reverse_face(face);
        }
        sheet_faces.push(sheet);
    }
    sheet_faces.append(&mut faces);
    let faces = sheet_faces;
    // Whether a face's place is inside the swept solid: a sheet's by its exact point there (not
    // the fitted one, a fit's miss off it), a torus's by its point, which no other instant may
    // hold.
    let member = |i: usize, uv: P2| -> bool {
        let p = kept_patches[i];
        if i < n_sheets {
            tube.sheet_cover(p.0, uv).0
        } else {
            let x = tori[usize::from(p.0 == END)].eval(uv.x, uv.y);
            tube.least(&x).1 < -delta
        }
    };
    let least = 2.0 * PI * PI * big * r * r;
    let solid = match super::sweep_moving::boundary_of(
        faces,
        &oracle,
        size,
        least,
        f64::INFINITY,
        given,
        Some(&member),
        fit_tol,
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
    Some(Ok(OpResult {
        bodies: vec![id],
        history: History::default(),
    }))
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
        let (chart, a0, a1, p0, p1) = patch;
        let (mu, mv) = grid;
        let nodes = (0..(mu + 1) * (mv + 1))
            .into_par_iter()
            .map(|k| {
                let (i, j) = (k / (mv + 1), k % (mv + 1));
                tube.sheet_cover(
                    chart,
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
        let (chart, a0, a1, p0, p1) = self.patch;
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
            _ => !tube.sheet_cover(chart, P2::new(u, v)).0,
        }
    }
}

/// How near a sheet its fitted surface keeps, in the measure of the body's size.
const SHEET_FIT: f64 = 1e-7;

/// The degree of the B-splines fitted through a sheet's points.
const PATCH_DEGREE: usize = 5;

/// The B-spline (of `PATCH_DEGREE`) through a chart's points at the sites `us` by `vs`.
fn tensor(at: &dyn Fn(f64, f64) -> P3, us: &[f64], vs: &[f64]) -> super::nurbs::NurbsSurface {
    use super::nurbs::{H, NurbsCurve, NurbsSurface, hom};
    // Along u at each v, then the poles' columns along v.
    let rows: Vec<NurbsCurve> = vs
        .iter()
        .map(|&v| {
            let pts: Vec<P3> = us.iter().map(|&u| at(u, v)).collect();
            NurbsCurve::interpolate(&pts, us, PATCH_DEGREE, None, None)
        })
        .collect();
    let nu = rows[0].cps.len();
    let mut cps: Vec<Vec<H>> = Vec::with_capacity(nu);
    let mut kv = Vec::new();
    for i in 0..nu {
        let col: Vec<P3> = rows
            .iter()
            .map(|c| super::nurbs::dehom(&c.cps[i]))
            .collect();
        let c = NurbsCurve::interpolate(&col, vs, PATCH_DEGREE, None, None);
        kv = c.knots.clone();
        cps.push(
            c.cps
                .iter()
                .map(|h| hom(&super::nurbs::dehom(h), 1.0))
                .collect(),
        );
    }
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
            .map(|i| vs.iter().any(|&v| bad(mid(&us, i), v)))
            .collect();
        let mut split_v: Vec<bool> = (0..nv)
            .map(|j| us.iter().any(|&u| bad(u, mid(&vs, j))))
            .collect();
        let centres: Vec<(usize, usize)> =
            (0..nu).flat_map(|i| (0..nv).map(move |j| (i, j))).collect();
        for (i, j) in centres {
            if !(split_u[i] || split_v[j]) && bad(mid(&us, i), mid(&vs, j)) {
                split_u[i] = true;
                split_v[j] = true;
            }
        }
        for (x, w) in pts {
            if (surf.point(w.x, w.y) - x).norm() > tol {
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
        } else {
            let t = self.tube.foot(tau, x);
            let mut pos = self.tube.sheet_pos(t, x);
            // (Inside a pinch, on its chart: the sheets are left off there.)
            if let Some(k) = self.tube.in_pinch(t, pos.psi) {
                let uv = self.tube.pinch_uv(k, t, x);
                pos = Pos {
                    chart: PINCH + k,
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

    /// The face across a seam from a place of `chart` at `x` (its own instant `own`): the
    /// pinch's chart's from a sheet, the sheet's from a pinch's chart.
    fn across_seam(
        &self,
        chart: usize,
        x: &P3,
        own: f64,
        prefer: Option<usize>,
    ) -> Option<(usize, P2)> {
        let pos = if chart >= PINCH {
            self.tube.sheet_pos(own, x)
        } else {
            let k = self
                .tube
                .in_pinch(own, self.tube.sheet_pos(own, x).psi)
                .or_else(|| {
                    // (On the seam itself, to rounding: the nearest pinch.)
                    let psi = self.tube.sheet_pos(own, x).psi;
                    (0..self.tube.pinches.len()).min_by(|&a, &b| {
                        let d = |k: usize| {
                            let c = self.tube.pinches[k].c;
                            (own - c.x).hypot(wrap(psi - c.y, -PI, TAU))
                        };
                        d(a).total_cmp(&d(b))
                    })
                })?;
            let uv = self.tube.pinch_uv(k, own, x);
            Pos {
                chart: PINCH + k,
                a: uv.x,
                psi: uv.y,
            }
        };
        self.sheet_at(&pos, prefer)
    }

    /// The place of `x` (touched at the instant `tau`) on face `i`, brought into its box: a
    /// point found on its boundary.
    fn clamped(&self, i: usize, x: &P3, tau: f64) -> P2 {
        let p = self.patches[i];
        let uv = if i < self.sheets && p.0 >= PINCH {
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
struct Field<'a> {
    cover: &'a (dyn Fn(P2) -> (bool, f64) + Sync),
    near: &'a (dyn Fn(P2, f64) -> (bool, f64) + Sync),
}

/// A curve between kept and covered: its points in the face's parameters, each with the
/// instant that covers the far side there; a closed one repeats its first point.
struct Chain {
    pts: Vec<(P2, f64)>,
    closed: bool,
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
        for it in 0..48 {
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

/// The place where membership changes nearest `w` along the normal to the direction `d` (in
/// the face's parameters, within the box `bx`), by `test` about the instant `tau`: found by
/// doubling steps either way and bisected; None when there is none within a twentieth of a
/// unit, or the length of `d` if that is more.
fn onto_boundary(
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
fn cover_curves(
    fd: &Field,
    bx: ((f64, f64), (f64, f64)),
    (mu, mv): (usize, usize),
    nodes: Option<&[(bool, f64)]>,
    tol: f64,
    jump: f64,
    point: &(dyn Fn(P2) -> P3 + Sync),
    fit: f64,
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
    let mut cross: HashMap<Key, (P2, f64)> = HashMap::new();
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
                let c = if a.0 {
                    crossing(fd, at(ni, nj), at(i, j), a.1)
                } else {
                    crossing(fd, at(i, j), at(ni, nj), b.1)
                };
                if let Ok(want) = std::env::var("CADRS_TUBE_CROSS") {
                    let v: Vec<f64> = want.split(',').filter_map(|w| w.parse().ok()).collect();
                    if v.len() == 2 && (c.0 - P2::new(v[0], v[1])).norm() < 1e-4 {
                        eprintln!(
                            "  crossing {:?} between nodes {:?} ({:?}) and {:?} ({:?}) of box {bx:?}, grid {mu} x {mv}; read there now {:?} and {:?}",
                            c,
                            at(i, j),
                            a,
                            at(ni, nj),
                            b,
                            (fd.cover)(at(i, j)),
                            (fd.cover)(at(ni, nj))
                        );
                    }
                }
                cross.insert((i, j, along), c);
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
                        onto_boundary(fd.near, mid, d, 0.5 * (tp + tq), bx)
                    }?;
                    if (r - mid).norm() >= d.norm() {
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
    }
    Some(chains)
}

/// A curve's point: where it is, and its place on each of its two faces.
type CurvePt = (P3, P2, P2);

/// What a chain's point is between: its place, its covering instant and the faces preferred
/// (those of the stretch so far), to the two faces and the point on them.
type PairAt<'a> = dyn Fn(P2, f64, Option<(usize, usize)>) -> Option<((usize, usize), CurvePt)> + 'a;

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
fn chain_runs(
    chain: &Chain,
    pair: &PairAt,
    place: &dyn Fn(P2, f64, (usize, usize)) -> CurvePt,
    onto: &dyn Fn(P2, V2, f64, bool) -> Option<(P2, f64)>,
    margin: &dyn Fn(P2, f64) -> (f64, f64),
    jump: f64,
    out: &mut Vec<super::sweep_moving::Given>,
) {
    let first = out.len();
    let mut pts: Vec<(P2, f64)> = chain.pts.clone();
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
    // (A run's points with their places on the chain and covering instants.)
    type RunPt = (CurvePt, P2, f64);
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
                        && (m - w0).norm() < d.norm()
                        && (m - w1).norm() < d.norm()
                    {
                        run.insert(k + 1, (place(m, t, (a, b)), m, t));
                        k += 1;
                    }
                    k += 1;
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
    let mut run: Vec<RunPt> = Vec::new();
    let mut cur: Option<(usize, usize)> = None;
    for i in 0..pts.len() {
        let (w, tau) = pts[i];
        let here = pair(w, tau, cur);
        let faces = here.map(|h| h.0);
        if i == 0 {
            cur = faces;
        } else if faces != cur {
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
                let reach = 3.0 * d.norm() + 1e-9;
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
                    for _ in 0..24 {
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
                        let step = jac.try_inverse()? * V2::new(ma, mb);
                        if !step.norm().is_finite() || step.norm() > reach {
                            return None;
                        }
                        w -= step;
                        if (w - start).norm() > reach {
                            return None;
                        }
                        if step.norm() < 1e-13 {
                            // (A corner indeed: both margins nothing there, and the faces across
                            // them the two stretches'.)
                            let (ma, mb) = (margin(w, ta).0, margin(w, tb).0);
                            let ok = ma.abs() < 0.1
                                && mb.abs() < 0.1
                                && pair(w, ta, cur).map(|h| h.0) == cur
                                && pair(w, tb, faces).map(|h| h.0) == faces;
                            return ok.then_some((w, ta, tb, c, f));
                        }
                    }
                    None
                })
            })();
            if let Some((wc, ta, tb, c, f)) = corner {
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
                if let Some(c) = cur
                    && let Some((m, t)) = on(lo)
                {
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
                {
                    run.push((place(m, t, c), m, t));
                }
                cur = next;
                from = hi;
            }
            if cur != faces {
                flush(&mut run, cur);
                cur = faces;
            }
        }
        if let Some((_, pt)) = here {
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
