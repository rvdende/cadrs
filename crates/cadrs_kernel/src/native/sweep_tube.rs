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
//! circle placed then at most r; along K that distance is r at the point's own instant, and the
//! point is on the boundary unless another instant (another local least of the distance over the
//! motion) brings the circle nearer. So the kept part of K is where g, the least of the other
//! local leasts less r, is at least zero, and its edges are the curves g = 0, each followed over
//! K from a point found between a kept and a covered sample by a predictor-corrector walk on the
//! exact geometry (the partner instant followed with it); where a third instant starts covering
//! a curve, three curves meet at a point found exactly and the walk turns the corner.
//!
//! K's kept part, cut along those curves, is split into faces: the sheets on B-spline surfaces
//! interpolating them in (t, ψ) to a millionth of a millionth of the size where kept (refined
//! where they stray), the tori exactly; each curve's p-curves are the (t, ψ) or (ψ, θ) of its
//! exact points on the face it bounds. The faces are joined edge to edge.

use super::kernel::NativeKernel;
use super::math::*;
use super::topo::Body;
use crate::*;
use nalgebra::Matrix3;
use std::f64::consts::FRAC_PI_2;

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
    nt: V3,
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

/// A place on K: its chart, the coordinate a ∈ [0, 1] across it (σ less the chart), and ψ.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pos {
    chart: usize,
    a: f64,
    psi: f64,
}

impl Pos {
    fn sigma(&self) -> f64 {
        self.chart as f64 + self.a
    }
}

/// A point of K: where it is, its derivatives in a and ψ, its own instant.
struct KPt {
    p: P3,
    pa: V3,
    pp: V3,
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
        let mt = s[3].cross(&s[2]) + s[1].cross(&s[4]);
        let mp = s[4].cross(&s[2]) + s[1].cross(&s[5]);
        let nt = (mt - n * n.dot(&mt)) / len;
        let npsi = (mp - n * n.dot(&mp)) / len;
        SheetPt {
            s,
            n,
            nt,
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
                let (t, s, dt) = if pos.chart == LEAD {
                    (pos.a, 1.0, 1.0)
                } else {
                    (1.0 - pos.a, -1.0, -1.0)
                };
                let j = jet(self.steps, t);
                let sp = self.sheet_pt(&j, psi);
                KPt {
                    p: P3::from(sp.s[0] + sp.n * (s * self.r)),
                    pa: (sp.s[1] + sp.nt * (s * self.r)) * dt,
                    pp: sp.s[2] + sp.npsi * (s * self.r),
                    own: t,
                }
            }
            _ => {
                // The leading half at the end runs from θ(N) down by π; the trailing half at the
                // start from θ(−N) down by π.
                let t = if pos.chart == END { 1.0 } else { 0.0 };
                let j = jet(self.steps, t);
                let sp = self.sheet_pt(&j, psi);
                let (th0, dth) = self.theta_n(&j, &sp, psi);
                let th0 = if pos.chart == END { th0 } else { th0 + PI };
                let th = th0 - PI * pos.a;
                let (p, tp, tt) = self.torus_pt(&j, psi, th);
                KPt {
                    p,
                    pa: -tt * PI,
                    pp: tp + tt * dth,
                    own: t,
                }
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
            if b - a <= 1e-15 {
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

    /// The local leasts of f for `p` other than at its own instant `own`: the samples' local
    /// leasts refined, and any a hair either side of `own` (where a fold brings one near it).
    /// Only those whose sample is below `upto` are refined.
    fn others(&self, p: &P3, own: f64, upto: f64) -> Vec<Least> {
        let vals: Vec<f64> = self.places.iter().map(|pl| self.f_at(pl, p)).collect();
        let n = vals.len();
        let h = self.ts[1] - self.ts[0];
        let mut out: Vec<Least> = Vec::new();
        let push = |m: Least, out: &mut Vec<Least>| {
            if (m.0 - own).abs() < 1e-7 {
                return;
            }
            if !out.iter().any(|o| (o.0 - m.0).abs() < 1e-7) {
                out.push(m);
            }
        };
        for i in 0..n {
            let left = i == 0 || vals[i] <= vals[i - 1];
            let right = i + 1 == n || vals[i] <= vals[i + 1];
            if !(left && right) || vals[i] > upto {
                continue;
            }
            let (a, b) = (self.ts[i.saturating_sub(1)], self.ts[(i + 1).min(n - 1)]);
            // (Own's bracket: searched either side of it below.)
            if own > a - 1e-12 && own < b + 1e-12 && vals[i] > -1e-3 * self.r {
                continue;
            }
            let m = self.golden(p, a, b);
            push(m, &mut out);
        }
        // A hair either side of its own instant, out to two samples: an interior least only.
        let tiny = 1e-6;
        for (a, b) in [(own - 2.0 * h, own - tiny), (own + tiny, own + 2.0 * h)] {
            let (a, b) = (a.max(0.0), b.min(1.0));
            if b - a < tiny {
                continue;
            }
            let m = self.golden(p, a, b);
            let at_end = (m.0 - a).abs() < 1e-9 || (m.0 - b).abs() < 1e-9;
            let at_motion_end = m.0 <= 0.0 || m.0 >= 1.0;
            if (!at_end || (at_motion_end && (m.0 - own).abs() > 2.0 * tiny)) && m.1 <= upto {
                push(m, &mut out);
            }
        }
        out
    }

    /// g at `x` (a point of K whose own instant is `own`): the least other local least, or
    /// infinity when none is near zero.
    fn g(&self, x: &P3, own: f64) -> Option<Least> {
        self.others(x, own, 0.05 * self.r.max(1e-3 * self.size))
            .into_iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The partner least near instant `t` refined (within `w` either way, clamped to the
    /// motion).
    fn near(&self, p: &P3, t: f64, w: f64) -> Least {
        self.golden(p, (t - w).max(0.0), (t + w).min(1.0))
    }

    /// The unit vector from the core circle at instant `t` to `p` (in the world): the gradient
    /// of the distance there.
    fn away(&self, t: f64, p: &P3) -> V3 {
        let (r, d) = place(self.steps, t);
        let q = r.tr_mul(&(p.coords - d)) - self.c.coords;
        let h = q.dot(&self.z);
        let w = q - self.z * h;
        let near = if w.norm() > 0.0 {
            w.normalize() * self.big
        } else {
            self.x * self.big
        };
        (r * (q - near)).normalize()
    }

    /// Where on K a point `p` lies at instant `t` (a local least of its distance, at r): the
    /// chart and coordinates.
    fn pos_at(&self, t: f64, p: &P3) -> Pos {
        let j = jet(self.steps, t);
        let q = j.r[0].tr_mul(&(p.coords - j.d[0])) - self.c.coords;
        let h = q.dot(&self.z);
        let w = q - self.z * h;
        let psi = w.dot(&self.y).atan2(w.dot(&self.x));
        if t > 0.0 && t < 1.0 {
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
        } else {
            let sp = self.sheet_pt(&j, psi);
            let (th0, _) = self.theta_n(&j, &sp, psi);
            let th = h.atan2(w.dot(&self.e1(psi)) - self.big);
            if t >= 1.0 {
                Pos {
                    chart: END,
                    a: wrap(th0 - th, 0.0, TAU) / PI,
                    psi,
                }
            } else {
                Pos {
                    chart: START,
                    a: wrap(th0 + PI - th, 0.0, TAU) / PI,
                    psi,
                }
            }
        }
    }
}

/// A point of a traced curve: where on K, the point, the partner's instant, and whether a
/// corner (where the partner changes) is there.
#[derive(Clone, Copy, Debug)]
struct TPt {
    pos: Pos,
    p: P3,
    partner: f64,
    corner: bool,
}

/// How a walk along a curve ended.
#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    Closed,
    /// Where the partner instant merges with the point's own (the curve's crease dies out).
    Cusp,
    Failed,
}

/// A curve traced on K: its points, and how it ended (a closed one repeats its first point).
struct Contour {
    pts: Vec<TPt>,
    end: End,
}

/// Walks along g = 0 on K.
struct Walker<'t, 'a> {
    tube: &'t Tube<'a>,
}

/// h at a point of K: the partner's distance less r, the partner's instant, h's gradient in
/// (a, ψ), the point, and whether the partner is a least inside the window it was looked for in
/// (not stopped at a side of it short of the motion's ends).
struct Hv {
    v: f64,
    t: f64,
    g: V2,
    kp: KPt,
    free: bool,
}

/// The same place on K in the chart across the join it lies on (a = 0 or 1).
fn across(pos: Pos) -> Pos {
    if pos.a >= 1.0 {
        Pos {
            chart: (pos.chart + 1) % 4,
            a: 0.0,
            psi: pos.psi,
        }
    } else {
        Pos {
            chart: (pos.chart + 3) % 4,
            a: 1.0,
            psi: pos.psi,
        }
    }
}

/// How near a partner's instant may come to the point's own before the curve is taken to be
/// dying out (and its end found exactly).
const GAP_END: f64 = 2e-4;

impl Walker<'_, '_> {
    fn size(&self) -> f64 {
        self.tube.size
    }

    /// The window a partner is looked for in, about `t2`: two samples either way, never half
    /// way to the point's own instant.
    fn window(&self, t2: f64, own: f64) -> (f64, f64) {
        let w = ((self.tube.ts[1] - self.tube.ts[0]) * 2.0).min(0.5 * (t2 - own).abs());
        ((t2 - w).max(0.0), (t2 + w).min(1.0))
    }

    fn h(&self, pos: &Pos, win: (f64, f64)) -> Hv {
        let kp = self.tube.eval(pos);
        let (t, v) = self.tube.golden(&kp.p, win.0, win.1);
        let span = (win.1 - win.0).max(1e-300);
        // (A least at a side, to within the search's round-off: f there is flat to a few
        // parts in 1e16, so the search wanders a hair.)
        let free =
            (win.0 <= 0.0 || t - win.0 > 1e-6 * span) && (win.1 >= 1.0 || win.1 - t > 1e-6 * span);
        let e = self.tube.away(t, &kp.p);
        Hv {
            v,
            t,
            g: V2::new(kp.pa.dot(&e), kp.pp.dot(&e)),
            kp,
            free,
        }
    }

    /// `pos` moved onto h = 0 (the partner in `win`) along h's gradient, or along a only / ψ
    /// only (`fix`: 1 holds ψ, 2 holds a).
    fn correct(&self, mut pos: Pos, win: (f64, f64), fix: u8) -> Option<(Pos, Hv)> {
        let tol = 1e-13 * self.size();
        for _ in 0..30 {
            let hv = self.h(&pos, win);
            if hv.v.abs() <= tol {
                return Some((pos, hv));
            }
            let g = hv.g;
            let step = match fix {
                1 => V2::new(hv.v / g.x, 0.0),
                2 => V2::new(0.0, hv.v / g.y),
                _ => g * (hv.v / g.norm_squared()),
            };
            if !step.x.is_finite() || !step.y.is_finite() {
                return None;
            }
            pos.a -= step.x;
            pos.psi -= step.y;
            if !(-0.25..=1.25).contains(&pos.a) {
                return None;
            }
        }
        None
    }

    /// The point near `pos` where the partners in windows `wb` and `wc` both bring the circle
    /// to r (a corner).
    fn corner(&self, mut pos: Pos, wb: (f64, f64), wc: (f64, f64)) -> Option<(Pos, Hv, Hv)> {
        let tol = 1e-13 * self.size();
        for _ in 0..40 {
            let hb = self.h(&pos, wb);
            let hc = self.h(&pos, wc);
            if hb.v.abs().max(hc.v.abs()) <= tol {
                return Some((pos, hb, hc));
            }
            let (da, dp) = solve2(hb.g.x, hb.g.y, hc.g.x, hc.g.y, hb.v, hc.v)?;
            pos.a -= da;
            pos.psi -= dp;
            if !(-0.25..=1.25).contains(&pos.a) {
                return None;
            }
        }
        None
    }

    /// The unit tangent of h = 0 at `kp` (gradient `g`) in (a, ψ), scaled to unit speed in
    /// space, and its direction in space.
    fn tangent(&self, kp: &KPt, g: V2) -> Option<(V2, V3)> {
        let tau = V2::new(-g.y, g.x);
        let v = kp.pa * tau.x + kp.pp * tau.y;
        let s = v.norm();
        (s > 1e-300).then(|| (tau / s, v / s))
    }

    /// The second and third derivatives of f at instant `t` for `p` (where f and its first
    /// derivative are zero), by differences `d` apart; one-sided at the motion's ends, where
    /// the third isn't given.
    fn bends(&self, p: &P3, t: f64, d: f64) -> (f64, f64) {
        let f = |x: f64| self.tube.f(x, p);
        if t >= 1.0 || t <= 0.0 {
            let s = if t >= 1.0 { -1.0 } else { 1.0 };
            // Richardson's: the second derivative from 2 f(t ± d) / d², at d and at 2d.
            let g1 = 2.0 * f(t + s * d) / (d * d);
            let g2 = 2.0 * f(t + 2.0 * s * d) / (4.0 * d * d);
            return (2.0 * g1 - g2, 0.0);
        }
        let (fp, fm, fp2, fm2, f0) = (f(t + d), f(t - d), f(t + 2.0 * d), f(t - 2.0 * d), f(t));
        (
            (fp + fm - 2.0 * f0) / (d * d),
            (fp2 - 2.0 * fp + 2.0 * fm - fm2) / (2.0 * d * d * d),
        )
    }

    /// Where a curve dying out near `pos` (its partner near its own instant) ends exactly: on a
    /// join, where the torus at the end and the sheet stop touching to second order along it
    /// (the second derivative of f at the motion's end zero); inside a sheet, where the second
    /// and third derivatives at the point's own instant are both zero (the swallowtail of the
    /// sheet's fold).
    fn cusp(&self, pos: Pos, t2: f64) -> Option<(Pos, KPt)> {
        let tube = self.tube;
        let kp = tube.eval(&pos);
        let at_end = |t: f64| t <= 0.0 || t >= 1.0;
        let d = 1e-3;
        if at_end(kp.own) || at_end(t2) {
            // On the join of the sheet the curve is on or nearing, at the end it nears.
            let end = if at_end(kp.own) { kp.own } else { t2 };
            let lead = match pos.chart {
                LEAD => true,
                TRAIL => false,
                END => pos.a < 0.5,
                _ => pos.a >= 0.5,
            };
            let (chart, a) = match (lead, end >= 1.0) {
                (true, true) => (LEAD, 1.0),
                (true, false) => (LEAD, 0.0),
                (false, true) => (TRAIL, 0.0),
                (false, false) => (TRAIL, 1.0),
            };
            let at = |psi: f64| -> f64 {
                let q = tube.eval(&Pos { chart, a, psi });
                self.bends(&q.p, end, d).0
            };
            let (mut x0, mut x1) = (pos.psi, pos.psi + 1e-4);
            let (mut f0, mut f1) = (at(x0), at(x1));
            for _ in 0..60 {
                if f1 == f0 {
                    break;
                }
                let x2 = x1 - f1 * (x1 - x0) / (f1 - f0);
                (x0, f0) = (x1, f1);
                x1 = x2;
                f1 = at(x1);
                if (x1 - x0).abs() < 1e-14 {
                    break;
                }
            }
            if (x1 - pos.psi).abs() > 0.1 || !x1.is_finite() {
                return None;
            }
            let q = Pos { chart, a, psi: x1 };
            return Some((q, tube.eval(&q)));
        }
        // Inside a sheet: Newton's on the two derivatives at the own instant, from half way
        // to the partner's place on the same sheet.
        let bend = |q: &Pos| -> (f64, f64) {
            let k = tube.eval(q);
            self.bends(&k.p, k.own, d)
        };
        let mut q = pos;
        let ta = if pos.chart == LEAD { t2 } else { 1.0 - t2 };
        q.a = 0.5 * (q.a + ta);
        for _ in 0..40 {
            let (b2, b3) = bend(&q);
            let e = 1e-7;
            let (b2a, b3a) = bend(&Pos { a: q.a + e, ..q });
            let (b2p, b3p) = bend(&Pos {
                psi: q.psi + e,
                ..q
            });
            let (da, dp) = solve2(
                (b2a - b2) / e,
                (b2p - b2) / e,
                (b3a - b3) / e,
                (b3p - b3) / e,
                b2,
                b3,
            )?;
            q.a -= da;
            q.psi -= dp;
            if da.abs() + dp.abs() < 1e-13 {
                break;
            }
            if !(0.0..=1.0).contains(&q.a) {
                return None;
            }
        }
        let k = tube.eval(&q);
        ((k.p - kp.p).norm() < 0.05 * self.size()).then_some((q, k))
    }

    /// Follows g = 0 from `start` (partner near `t2`) in direction `dir` (in space) until it
    /// closes, dies out or fails; the corners where a third instant takes over are turned.
    fn walk(&self, start: Pos, t2: f64, dir: V3) -> Contour {
        let size = self.size();
        let tube = self.tube;
        let mut pos = start;
        let k0 = tube.eval(&pos);
        let h0 = self.h(&pos, self.window(t2, k0.own));
        let mut t2 = h0.t;
        let p0 = h0.kp.p;
        let mut pts = vec![TPt {
            pos,
            p: p0,
            partner: t2,
            corner: false,
        }];
        let failed = |pts: Vec<TPt>| Contour {
            pts,
            end: End::Failed,
        };
        let hmax = 4e-3 * size;
        let hmin = 1e-10 * size;
        let mut h = 0.25 * hmax;
        let mut prev = dir;
        let mut length = 0.0;
        let upto = 0.05 * tube.r.max(1e-3 * size);
        for _ in 0..200_000 {
            let own = tube.eval(&pos).own;
            let win = self.window(t2, own);
            let hv = self.h(&pos, win);
            let gap = (t2 - own).abs();
            if gap < GAP_END {
                return match self.cusp(pos, t2) {
                    Some((q, kq)) => {
                        pts.push(TPt {
                            pos: q,
                            p: kq.p,
                            partner: kq.own,
                            corner: false,
                        });
                        Contour {
                            pts,
                            end: End::Cusp,
                        }
                    }
                    None => failed(pts),
                };
            }
            let Some((tau, vel)) = self.tangent(&hv.kp, hv.g) else {
                return failed(pts);
            };
            let sgn = if vel.dot(&prev) >= 0.0 { 1.0 } else { -1.0 };
            let (tau, vel) = (tau * sgn, vel * sgn);
            // On a join, heading over it: into the next chart.
            if (pos.a >= 1.0 && tau.x > 0.0) || (pos.a <= 0.0 && tau.x < 0.0) {
                pos = across(pos);
                if let Some(l) = pts.last_mut() {
                    l.pos = pos;
                }
                prev = vel;
                continue;
            }
            // The predictor, cut short to land on a join or a quarter line it would cross.
            let mut step = tau * h;
            let mut fix = 0u8;
            let na = pos.a + step.x;
            if !(0.0..=1.0).contains(&na) {
                let to = if na > 1.0 { 1.0 } else { 0.0 };
                step *= (to - pos.a) / step.x;
                fix = 2;
            }
            if fix == 0 && step.y != 0.0 {
                let k = pos.psi / FRAC_PI_2;
                let line = if step.y > 0.0 {
                    ((k + 1e-9).floor() + 1.0) * FRAC_PI_2
                } else {
                    ((k - 1e-9).ceil() - 1.0) * FRAC_PI_2
                };
                if (pos.psi + step.y - line) * step.y.signum() > 0.0 {
                    step *= (line - pos.psi) / step.y;
                    fix = 1;
                }
            }
            let mut np = Pos {
                chart: pos.chart,
                a: pos.a + step.x,
                psi: pos.psi + step.y,
            };
            if fix == 2 {
                np.a = np.a.round();
            }
            if fix == 1 {
                np.psi = (np.psi / FRAC_PI_2).round() * FRAC_PI_2;
            }
            let mut why = "no correction";
            let next = self.correct(np, win, fix).and_then(|(q, hq)| {
                let d = (hq.kp.p - hv.kp.p).norm();
                let (_, vq) = self.tangent(&hq.kp, hq.g)?;
                let ok_len = d < 1.5 * h && (fix != 0 || d > 0.5 * h);
                let ok_turn = vq.dot(&vel) > 0.98;
                let ngap = (hq.t - hq.kp.own).abs();
                let ok_gap = ngap > 0.6 * gap || ngap < GAP_END;
                why = if !ok_len {
                    "length"
                } else if !ok_turn {
                    "turn"
                } else if !hq.free {
                    "partner at its window's side"
                } else {
                    "gap"
                };
                (ok_len && ok_turn && hq.free && ok_gap).then_some((q, hq))
            });
            let Some((q, hq)) = next else {
                h *= 0.5;
                if h < hmin {
                    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                        eprintln!(
                            "  walk stuck at {pos:?} (own {own:.9}, partner {t2:.9}, |grad| {:.3e}): {why}",
                            hv.g.norm()
                        );
                    }
                    return failed(pts);
                }
                continue;
            };
            let kq = &hq.kp;
            let nt2 = hq.t;
            // A third instant covering it: the corner between.
            let others = tube.others(&kq.p, kq.own, upto);
            let third = others
                .iter()
                .filter(|m| (m.0 - nt2).abs() > 1e-6)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .copied();
            if let Some(tc) = third
                && tc.1 < -1e-12 * size
            {
                let mid = Pos {
                    chart: pos.chart,
                    a: 0.5 * (pos.a + q.a),
                    psi: 0.5 * (pos.psi + q.psi),
                };
                if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                    eprintln!(
                        "  corner at {q:?}: own {:.9} partner {:.9} (window {win:?}, free {}, h {:e}) third {:?} others {:?}",
                        kq.own, nt2, hq.free, hq.v, tc, others
                    );
                }
                let wc = self.window(tc.0, kq.own);
                let Some((cp, hb, hc)) = self.corner(mid, win, wc) else {
                    return failed(pts);
                };
                pts.push(TPt {
                    pos: cp,
                    p: hc.kp.p,
                    partner: hc.t,
                    corner: true,
                });
                // On along the new partner's curve, the way that leaves the old one's cover.
                let Some((tauc, velc)) = self.tangent(&hc.kp, hc.g) else {
                    return failed(pts);
                };
                prev = if tauc.dot(&hb.g) >= 0.0 { velc } else { -velc };
                length += (hc.kp.p - hv.kp.p).norm();
                pos = cp;
                t2 = hc.t;
                h = (h * 0.5).max(1e-6 * size);
                continue;
            }
            length += (kq.p - hv.kp.p).norm();
            pts.push(TPt {
                pos: q,
                p: kq.p,
                partner: nt2,
                corner: false,
            });
            pos = q;
            t2 = nt2;
            prev = vel;
            if length > 4.0 * hmax && (kq.p - p0).norm() < 0.75 * h {
                let first = pts[0];
                pts.push(first);
                return Contour {
                    pts,
                    end: End::Closed,
                };
            }
            h = (h * 1.5).min(hmax);
        }
        failed(pts)
    }
}

/// Whether K at `pos` is kept (no other instant brings the circle within r), with g.
fn kept_at(tube: &Tube, pos: &Pos) -> (bool, Option<Least>) {
    let kp = tube.eval(pos);
    let g = tube.g(&kp.p, kp.own);
    (g.is_none_or(|m| m.1 > 0.0), g)
}

/// The curves g = 0 on K: from points between a kept and a covered sample of a grid over each
/// chart, each followed both ways.
fn network(tube: &Tube, na: usize, npsi: usize) -> Vec<Contour> {
    use rayon::prelude::*;
    let size = tube.size;
    let grid: Vec<Pos> = (0..4)
        .flat_map(|c| {
            (0..=na).flat_map(move |i| {
                (0..npsi).map(move |j| Pos {
                    chart: c,
                    a: i as f64 / na as f64,
                    psi: TAU * j as f64 / npsi as f64,
                })
            })
        })
        .collect();
    let class: Vec<(bool, Option<Least>)> = grid.par_iter().map(|p| kept_at(tube, p)).collect();
    let idx = |c: usize, i: usize, j: usize| (c * (na + 1) + i) * npsi + j % npsi;
    let walker = Walker { tube };
    let mut out: Vec<Contour> = Vec::new();
    let mut seeds = 0usize;
    for c in 0..4 {
        for i in 0..=na {
            for j in 0..npsi {
                for (di, dj) in [(1usize, 0usize), (0, 1)] {
                    if i + di > na {
                        continue;
                    }
                    let (k, l) = (idx(c, i, j), idx(c, i + di, j + dj));
                    if class[k].0 == class[l].0 {
                        continue;
                    }
                    // Bisected to the boundary, the covered end's partner leading.
                    let (mut pin, mut pout) = if class[k].0 {
                        (grid[l], grid[k])
                    } else {
                        (grid[k], grid[l])
                    };
                    if dj == 1 && j + 1 == npsi {
                        if pin.psi == 0.0 {
                            pin.psi = TAU;
                        } else {
                            pout.psi = TAU;
                        }
                    }
                    let mut part = if class[k].0 { class[l].1 } else { class[k].1 };
                    for _ in 0..48 {
                        let m = Pos {
                            chart: c,
                            a: 0.5 * (pin.a + pout.a),
                            psi: 0.5 * (pin.psi + pout.psi),
                        };
                        let (kept, g) = kept_at(tube, &m);
                        if kept {
                            pout = m;
                        } else {
                            pin = m;
                            part = g;
                        }
                    }
                    let Some(part) = part else { continue };
                    let own = tube.eval(&pin).own;
                    let Some((pos, hv)) = walker.correct(pin, walker.window(part.0, own), 0) else {
                        continue;
                    };
                    let (t2, kp) = (hv.t, hv.kp);
                    // (On a curve already followed.)
                    let near = out.iter().any(|ct| {
                        ct.pts.iter().any(|tp| {
                            tp.pos.chart == pos.chart
                                && (tp.p - kp.p).norm() < 2e-3 * size
                                && (tp.partner - t2).abs() < 1e-2
                        })
                    });
                    if near {
                        continue;
                    }
                    seeds += 1;
                    let Some((_, vel)) = walker.tangent(&kp, hv.g) else {
                        continue;
                    };
                    let fwd = walker.walk(pos, t2, vel);
                    let ct = if fwd.end == End::Closed {
                        fwd
                    } else {
                        let back = walker.walk(pos, t2, -vel);
                        let mut pts: Vec<TPt> = back.pts.into_iter().rev().collect();
                        pts.extend(fwd.pts.into_iter().skip(1));
                        let end = match (back.end, fwd.end) {
                            (End::Failed, _) | (_, End::Failed) => End::Failed,
                            _ => End::Cusp,
                        };
                        Contour { pts, end }
                    };
                    out.push(ct);
                }
            }
        }
    }
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        let covered = class.iter().filter(|c| !c.0).count();
        eprintln!(
            "tube sweep: {} samples, {covered} covered, {seeds} seeds, {} curves",
            class.len(),
            out.len()
        );
        for (i, ct) in out.iter().enumerate() {
            let charts: Vec<usize> = {
                let mut v: Vec<usize> = ct.pts.iter().map(|p| p.pos.chart).collect();
                v.dedup();
                v
            };
            let corners = ct.pts.iter().filter(|p| p.corner).count();
            let len: f64 = ct.pts.windows(2).map(|w| (w[1].p - w[0].p).norm()).sum();
            eprintln!(
                "  curve {i}: {:?}, {} points, length {len:.4}, charts {charts:?}, {corners} corners, from {:?} (partner {:.4}) to {:?} (partner {:.4})",
                ct.end,
                ct.pts.len(),
                ct.pts[0].pos,
                ct.pts[0].partner,
                ct.pts[ct.pts.len() - 1].pos,
                ct.pts[ct.pts.len() - 1].partner,
            );
        }
    }
    if let Some(path) = std::env::var_os("CADRS_TUBE_DUMP") {
        use std::fmt::Write as _;
        let mut txt = String::new();
        for (i, ct) in out.iter().enumerate() {
            for tp in &ct.pts {
                let kp = tube.eval(&tp.pos);
                let others = tube.others(&tp.p, kp.own, 1.0);
                let _ = writeln!(
                    txt,
                    "{i} {} {:.12} {:.12} {:.9} {:.9} {} {:?}",
                    tp.pos.chart,
                    tp.pos.a,
                    tp.pos.psi,
                    kp.own,
                    tp.partner,
                    u8::from(tp.corner),
                    others
                        .iter()
                        .map(|m| (format!("{:.6}", m.0), format!("{:.3e}", m.1)))
                        .collect::<Vec<_>>()
                );
            }
        }
        let _ = std::fs::write(path, txt);
    }
    out
}

/// [`crate::Kernel::sweep_solid`] of a whole torus by the rigid motion `steps`: None when `b`
/// isn't one, or the core circle's sweep folds (its normal is undefined somewhere).
pub fn torus(k: &mut NativeKernel, b: &Body, steps: &[RigidStep]) -> Option<Result<OpResult>> {
    let [f] = b.faces.as_slice() else {
        return None;
    };
    let super::surface::Surface::Torus { f: fr, rmaj, rmin } = &f.surf else {
        return None;
    };
    if !(rmin < rmaj) || !f.sense || b.solids.len() != 1 {
        return None;
    }
    let size = b.bbox().diagonal().max(super::tol::POINT);
    let tube = Tube::new(steps, fr, *rmaj, *rmin, size);
    // Σ regular: its tangents apart everywhere.
    let mut least = f64::INFINITY;
    for i in 0..=200 {
        let j = jet(steps, i as f64 / 200.0);
        for k in 0..128 {
            least = least.min(tube.sheet_pt(&j, TAU * k as f64 / 128.0).sin);
        }
    }
    if least < 0.02 {
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!("tube sweep: its core circle's sweep nearly folds ({least:.4})");
        }
        return Some(Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!("the swept torus: its core circle's sweep nearly folds ({least:.4})"),
        }));
    }
    // The candidates: each chart cut in patches (the sheets in chunks of at most a quarter
    // radian of turn, the tori's halves in two; each a quarter of ψ), each a cubic B-spline
    // through K's exact points on one grid (so neighbours share their edges), turned to face
    // away from the core circle placed at its points' own instant.
    // (The turn along the motion, summed: a whole turn and back comes to nothing net.)
    let mut turn = 0.0;
    let mut prev = place(steps, 0.0).0;
    for i in 1..=512 {
        let ri = place(steps, i as f64 / 512.0).0;
        turn += nalgebra::Rotation3::from_matrix_unchecked(prev.transpose() * ri).angle();
        prev = ri;
    }
    let sheet_chunks = ((turn / 0.25).ceil() as usize).max(2);
    let (npsi, ntorus) = (4, 2);
    let mut patches: Vec<(usize, f64, f64, f64, f64)> = Vec::new();
    for chart in [LEAD, END, TRAIL, START] {
        let m = if chart == LEAD || chart == TRAIL {
            sheet_chunks
        } else {
            ntorus
        };
        for i in 0..m {
            for q in 0..npsi {
                patches.push((
                    chart,
                    i as f64 / m as f64,
                    (i + 1) as f64 / m as f64,
                    TAU * q as f64 / npsi as f64,
                    TAU * (q + 1) as f64 / npsi as f64,
                ));
            }
        }
    }
    // Each fitted on a grid doubled until it strays at most a billionth of the size (its
    // neighbours' shared edges then agree within twice that).
    let fit_tol = 1e-9 * size;
    let surfaces: Vec<(super::nurbs::NurbsSurface, f64)> = {
        use rayon::prelude::*;
        super::par::start();
        patches
            .par_iter()
            .map(|&(chart, a0, a1, p0, p1)| {
                let mut n = 8;
                loop {
                    let f = patch_fit(&tube, chart, a0, a1, p0, p1, n);
                    if f.1 <= fit_tol || n >= 64 {
                        return f;
                    }
                    n *= 2;
                }
            })
            .collect()
    };
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        let mut worst = [0.0f64; 4];
        for (pt, f) in patches.iter().zip(&surfaces) {
            worst[pt.0] = worst[pt.0].max(f.1);
        }
        let bad: Vec<_> = patches
            .iter()
            .zip(&surfaces)
            .filter(|(_, f)| f.1 > fit_tol)
            .map(|(p, f)| {
                (
                    p.0,
                    (p.1 * 1e3).round() / 1e3,
                    (p.3 * 1e3).round() / 1e3,
                    f.1,
                )
            })
            .take(6)
            .collect();
        eprintln!(
            "tube sweep: {} patches, worst fit by chart (lead, end, trail, start) {worst:?}; {bad:?}",
            patches.len()
        );
    }
    let mut faces: Vec<Body> = Vec::with_capacity(patches.len());
    for (&(chart, a0, a1, p0, p1), (surf, _)) in patches.iter().zip(surfaces) {
        let surf = super::surface::Surface::Nurbs(std::sync::Arc::new(surf));
        let mut sheet =
            match super::face_build::face_from_surface(&surf, Some(((a0, a1), (p0, p1)))) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
        let (am, pm) = (0.5 * (a0 + a1), 0.5 * (p0 + p1));
        let kp = tube.eval(&Pos {
            chart,
            a: am,
            psi: pm,
        });
        let (rm, dm) = place(steps, kp.own);
        let core = rm * tube.k(pm) + dm;
        let outward = kp.p.coords - core;
        let face = &mut sheet.faces[0];
        let (_, su, sv) = face.surf.d1(am, pm);
        let natural = su.cross(&sv) * if face.sense { 1.0 } else { -1.0 };
        if natural.dot(&outward) < 0.0 {
            Body::reverse_face(face);
        }
        faces.push(sheet);
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
    let least = 2.0 * PI * PI * big * r * r;
    let solid = match super::sweep_moving::boundary_of(faces, &oracle, size, least, f64::INFINITY) {
        Ok(s) => s,
        Err(e) => {
            if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                eprintln!("tube sweep: refused: {e}");
            }
            return Some(Err(e));
        }
    };
    let id = k.insert(solid);
    Some(Ok(OpResult {
        bodies: vec![id],
        history: History::default(),
    }))
}

/// The cubic B-spline through K's points on an n × n grid over the patch (chart, a from `a0`
/// to `a1`, ψ from `p0` to `p1`), and how far it strays from K midway between them.
fn patch_fit(
    tube: &Tube,
    chart: usize,
    a0: f64,
    a1: f64,
    p0: f64,
    p1: f64,
    n: usize,
) -> (super::nurbs::NurbsSurface, f64) {
    use super::nurbs::{H, NurbsCurve, NurbsSurface, hom};
    let us: Vec<f64> = (0..=n)
        .map(|i| a0 + (a1 - a0) * i as f64 / n as f64)
        .collect();
    let vs: Vec<f64> = (0..=n)
        .map(|j| p0 + (p1 - p0) * j as f64 / n as f64)
        .collect();
    let at = |a: f64, psi: f64| tube.eval(&Pos { chart, a, psi }).p;
    // Along u at each v, then the poles' columns along v.
    let rows: Vec<NurbsCurve> = vs
        .iter()
        .map(|&v| {
            let pts: Vec<P3> = us.iter().map(|&u| at(u, v)).collect();
            NurbsCurve::interpolate(&pts, &us, 3, None, None)
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
        let c = NurbsCurve::interpolate(&col, &vs, 3, None, None);
        kv = c.knots.clone();
        cps.push(
            c.cps
                .iter()
                .map(|h| hom(&super::nurbs::dehom(h), 1.0))
                .collect(),
        );
    }
    let surf = NurbsSurface {
        du: rows[0].degree,
        dv: 3.min(vs.len() - 1),
        ku: rows[0].knots.clone(),
        kv,
        cps,
    };
    let mut worst: f64 = 0.0;
    for w in us.windows(2) {
        for x in vs.windows(2) {
            let (u, v) = (0.5 * (w[0] + w[1]), 0.5 * (x[0] + x[1]));
            worst = worst.max((surf.point(u, v) - at(u, v)).norm());
        }
    }
    (surf, worst)
}
