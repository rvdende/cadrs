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
        let mp = s[4].cross(&s[2]) + s[1].cross(&s[5]);
        let npsi = (mp - n * n.dot(&mp)) / len;
        SheetPt {
            s,
            n,
            npsi,
            sin: len / (s[1].norm() * s[2].norm()),
        }
    }

    /// How far the sheet (LEAD: Σ + rN, TRAIL: Σ − rN) is from folding at (a, ψ): the ratio of
    /// its area element to Σ's, (1 ∓ rκ₁)(1 ∓ rκ₂) for Σ's principal curvatures κ (toward N:
    /// the offset along N stretches where Σ bends away from N, κ < 0); negative where it has
    /// folded over (past a radius of curvature of Σ).
    fn fold(&self, chart: usize, a: f64, psi: f64) -> f64 {
        let (t, s) = if chart == LEAD {
            (a, -1.0)
        } else {
            (1.0 - a, 1.0)
        };
        let j = jet(self.steps, t);
        let sp = self.sheet_pt(&j, psi);
        let (e, f, g) = (
            sp.s[1].dot(&sp.s[1]),
            sp.s[1].dot(&sp.s[2]),
            sp.s[2].dot(&sp.s[2]),
        );
        let (l, m, n) = (sp.s[3].dot(&sp.n), sp.s[4].dot(&sp.n), sp.s[5].dot(&sp.n));
        let det = e * g - f * f;
        let kg = (l * n - m * m) / det;
        let h = (e * n - 2.0 * f * m + g * l) / (2.0 * det);
        let r = s * self.r;
        (1.0 + r * h).powi(2) - r * r * (h * h - kg)
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
            // (A hair past either join, round the leading half, is the join.)
            let snap = |a: f64| -> f64 {
                if a > 1.0 && a < 1.0 + 1e-6 {
                    1.0
                } else if a > 2.0 - 1e-6 {
                    0.0
                } else {
                    a
                }
            };
            if t >= 1.0 {
                Pos {
                    chart: END,
                    a: snap(wrap(th0 - th, 0.0, TAU) / PI),
                    psi,
                }
            } else {
                Pos {
                    chart: START,
                    a: snap(wrap(th0 + PI - th, 0.0, TAU) / PI),
                    psi,
                }
            }
        }
    }
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
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        // Where the sheets fold over: 1 ± r κ for Σ's principal curvatures κ (toward N).
        let (mut lead_least, mut trail_least) = (f64::INFINITY, f64::INFINITY);
        let (mut at_l, mut at_t) = ((0.0, 0.0), (0.0, 0.0));
        for i in 0..=400 {
            let t = i as f64 / 400.0;
            let j = jet(steps, t);
            for k in 0..256 {
                let psi = TAU * k as f64 / 256.0;
                let sp = tube.sheet_pt(&j, psi);
                let (e, f, g) = (
                    sp.s[1].dot(&sp.s[1]),
                    sp.s[1].dot(&sp.s[2]),
                    sp.s[2].dot(&sp.s[2]),
                );
                let (l, m, n) = (sp.s[3].dot(&sp.n), sp.s[4].dot(&sp.n), sp.s[5].dot(&sp.n));
                let det = e * g - f * f;
                let kg = (l * n - m * m) / det;
                let h = (e * n - 2.0 * f * m + g * l) / (2.0 * det);
                let d = (h * h - kg).max(0.0).sqrt();
                for kappa in [h - d, h + d] {
                    if 1.0 - tube.r * kappa < lead_least {
                        lead_least = 1.0 - tube.r * kappa;
                        at_l = (t, psi);
                    }
                    if 1.0 + tube.r * kappa < trail_least {
                        trail_least = 1.0 + tube.r * kappa;
                        at_t = (t, psi);
                    }
                }
            }
        }
        eprintln!(
            "tube sweep: fold {least:.4}; least 1 − rκ on the lead sheet {lead_least:.4} at {at_l:?}, least 1 + rκ on the trail sheet {trail_least:.4} at {at_t:?}"
        );
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
    // The candidates: each chart cut in patches (the sheets in chunks of at most a quarter
    // radian of turn, the tori's halves in two; each a quarter of ψ), each a cubic B-spline
    // through K's exact points on one grid (so neighbours share their edges), turned to face
    // away from the core circle placed at its points' own instant. A patch the swept solid
    // covers wholly (every sample of it inside) is left out, and one that won't fit is cut in
    // four and each quarter tried (where the core's sweep nearly folds, the sheets bend sharply,
    // and past a radius of curvature they turn back inside the solid).
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
    let mut patches: Vec<Patch> = Vec::new();
    for chart in [LEAD, TRAIL] {
        for i in 0..sheet_chunks {
            for q in 0..npsi {
                patches.push((
                    chart,
                    i as f64 / sheet_chunks as f64,
                    (i + 1) as f64 / sheet_chunks as f64,
                    TAU * q as f64 / npsi as f64,
                    TAU * (q + 1) as f64 / npsi as f64,
                ));
            }
        }
    }
    let fit_tol = 1e-9 * size;
    let fitted: Vec<std::result::Result<Vec<Fitted>, String>> = {
        use rayon::prelude::*;
        super::par::start();
        patches
            .par_iter()
            .map(|&p| fit_patch(&tube, &oracle, p, fit_tol, 0))
            .collect()
    };
    let mut fitted_patches: Vec<Fitted> = Vec::new();
    for r in fitted {
        match r {
            Ok(v) => fitted_patches.extend(v),
            Err(why) => {
                if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                    eprintln!("tube sweep: refused: {why}");
                }
                return Some(Err(KernelError::InvalidResult {
                    op: "solid sweep",
                    why: format!("the swept torus: {why}"),
                }));
            }
        }
    }
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        let mut by_chart = [0usize; 4];
        for (p, ..) in &fitted_patches {
            by_chart[p.0] += 1;
        }
        eprintln!(
            "tube sweep: {} patches tried, {} kept (lead, end, trail, start: {by_chart:?}), {} folded",
            patches.len(),
            fitted_patches.len(),
            fitted_patches.iter().filter(|f| !f.2.is_empty()).count()
        );
    }
    // The faces, and the patch each is of (a folded patch gives several).
    let mut faces: Vec<Body> = Vec::with_capacity(fitted_patches.len());
    let mut kept_patches: Vec<Patch> = Vec::new();
    for fitted in fitted_patches {
        let patch = fitted.0;
        match patch_faces(&tube, steps, fitted, size) {
            Ok(fs) => {
                for f in fs {
                    faces.push(f);
                    kept_patches.push(patch);
                }
            }
            Err(e) => return Some(Err(e)),
        }
    }
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
    for (chart, t_end) in [(START, 0.0), (END, 1.0)] {
        let surf = &tori[usize::from(t_end > 0.5)];
        for q in 0..4 {
            for h in 0..2 {
                let (u0, u1) = (TAU * q as f64 / 4.0, TAU * (q + 1) as f64 / 4.0);
                let (v0, v1) = (PI * h as f64, PI * (h + 1) as f64);
                let quarter =
                    match super::face_build::face_from_surface(surf, Some(((u0, u1), (v0, v1)))) {
                        Ok(b) => b,
                        Err(e) => return Some(Err(e)),
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
                                        let deg = if p3.len() >= 4 { 3 } else { 1 };
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
                                            let tm = 0.5 * (params[i] + params[i + 1]);
                                            let off = (0..=4)
                                                .map(|j| {
                                                    let t = params[i]
                                                        + (params[i + 1] - params[i])
                                                            * (0.3 + 0.1 * j as f64);
                                                    (curve.point(t) - x).norm()
                                                })
                                                .fold(f64::INFINITY, f64::min);
                                            let _ = tm;
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
                    match super::sweep_moving::cut_face(&quarter, 0, cuts, &pos, size) {
                        Ok(ps) => ps,
                        Err(why) => {
                            return Some(Err(KernelError::InvalidResult {
                                op: "solid sweep",
                                why: format!("the swept torus: its torus along the joins: {why}"),
                            }));
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
                        "  torus quarter {chart} u {u0:.4}..{u1:.4} v {v0:.4}..{v1:.4}: {} pieces (faces from {}); joins {js:?}",
                        pieces.len(),
                        faces.len()
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
                    faces.push(piece);
                    kept_patches.push((chart, u0, u1, v0, v1));
                }
            }
        }
    }
    // Which face holds the place on K a point is at an instant that touches it.
    let located = kept_patches.clone();
    let hold_tol = 1e-9 * size;
    let holds: std::sync::Arc<Vec<Box<dyn Fn(P2) -> bool + Send + Sync>>> = faces
        .iter()
        .map(|b| {
            let polys = super::classify::face_polygons(b, &b.faces[0]);
            let surf = b.faces[0].surf.clone();
            Box::new(move |uv: P2| {
                super::classify::in_face(&polys, &surf, uv, hold_tol)
                    != super::classify::InFace::Outside
            }) as Box<dyn Fn(P2) -> bool + Send + Sync>
        })
        .collect::<Vec<_>>()
        .into();
    // The torus's pieces holding a point on it at an end (several on a join cut), each with
    // the point's (u, v).
    let torus_faces = {
        let holds = holds.clone();
        let located = located.clone();
        let tori = tori.clone();
        std::sync::Arc::new(move |t_end: f64, x: &P3| -> Vec<(usize, P2)> {
            let chart = if t_end > 0.5 { END } else { START };
            let surf = &tori[usize::from(t_end > 0.5)];
            let (u, v) = surf.project(x, None);
            let (u, v) = (wrap(u, 0.0, TAU), wrap(v, 0.0, TAU));
            located
                .iter()
                .enumerate()
                .filter(|(_, p)| p.0 == chart)
                .filter_map(|(i, p)| {
                    let uu = wrap(u, p.1, TAU);
                    let vv = wrap(v, p.3, TAU);
                    let on_u = (p.1 - 1e-9..=p.2 + 1e-9).contains(&uu);
                    let on_v = (p.3 - 1e-9..=p.4 + 1e-9).contains(&vv);
                    let uv = P2::new(uu.clamp(p.1, p.2), vv.clamp(p.3, p.4));
                    (on_u && on_v && holds[i](uv)).then_some((i, uv))
                })
                .collect()
        })
    };
    let holds_located = holds.clone();
    let torus_faces_located = torus_faces.clone();
    let tube_ref = &tube;
    let oracle = oracle.with_locate(Box::new(move |t: f64, x: &P3| {
        // (An instant found a hair inside the motion's end is the end: the torus there.)
        let t = if t < 1e-9 {
            0.0
        } else if t > 1.0 - 1e-9 {
            1.0
        } else {
            t
        };
        let mut out: Vec<(usize, P2)> = Vec::new();
        let sheet_at = |chart: usize, a: f64, psi: f64, out: &mut Vec<(usize, P2)>| {
            let near = |p: &Patch| {
                let psi = wrap(psi, p.3, TAU);
                p.0 == chart
                    && (p.1 - 1e-9..=p.2 + 1e-9).contains(&a)
                    && (p.3 - 1e-9..=p.4 + 1e-9).contains(&psi)
            };
            out.extend(
                located
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| near(p))
                    .map(|(i, p)| {
                        (
                            i,
                            P2::new(a.clamp(p.1, p.2), wrap(psi, p.3, TAU).clamp(p.3, p.4)),
                        )
                    })
                    .filter(|(i, uv)| holds_located[*i](*uv)),
            );
        };
        if t <= 0.0 || t >= 1.0 {
            out.extend(torus_faces_located(t, x));
            // On a join: the sheet there too.
            let pos = tube_ref.pos_at(if t <= 0.0 { 1e-12 } else { 1.0 - 1e-12 }, x);
            for (chart, a) in [(LEAD, 0.0), (LEAD, 1.0), (TRAIL, 0.0), (TRAIL, 1.0)] {
                let own = if chart == LEAD { a } else { 1.0 - a };
                if (own - t).abs() > 1e-9 {
                    continue;
                }
                let j = tube_ref.eval(&Pos {
                    chart,
                    a,
                    psi: pos.psi,
                });
                if (j.p - x).norm() <= 1e-9 * tube_ref.size {
                    sheet_at(chart, a, pos.psi, &mut out);
                }
            }
        } else {
            let pos = tube_ref.pos_at(t, x);
            sheet_at(pos.chart, pos.a, pos.psi, &mut out);
        }
        out
    }));
    // Where each sheet crosses the torus at the motion's ends, traced in the sheet's own
    // (a, ψ) as the zero set of the torus's distance there with the touch along the join
    // divided out (the sheet leaves the join tangent to the torus, and dips inside it where
    // the circle slides along itself; the two graze, and a crossing traced in space wanders).
    let given = {
        let sheet_chunks = kept_patches
            .iter()
            .filter(|p| p.0 == LEAD)
            .map(|p| ((p.2 - p.1).recip()).round() as usize)
            .max()
            .unwrap_or(2);
        let ma = (4 * sheet_chunks).max(64);
        let mut given = Vec::new();
        for (chart, t_end) in [(LEAD, 0.0), (LEAD, 1.0), (TRAIL, 0.0), (TRAIL, 1.0)] {
            let a_join = if (chart == LEAD) == (t_end == 0.0) {
                0.0
            } else {
                1.0
            };
            // (Read no nearer the join than 1e-5: there the distance is of second order in
            // the distance from the join, and at the join itself only round-off.)
            let field = |a: f64, psi: f64| -> f64 {
                let d = (a - a_join).abs().max(1e-5);
                let a = if a_join == 0.0 { d } else { 1.0 - d };
                let kp = tube.eval(&Pos { chart, a, psi });
                tube.f(t_end, &kp.p) / (d * d)
            };
            let Some(chains) = zero_curves(&field, (0.0, 1.0), (0.0, TAU), (ma, 256), true, 1e-4)
            else {
                continue;
            };
            let sheet_face = |a: f64, psi: f64| -> Option<(usize, P2)> {
                kept_patches.iter().enumerate().find_map(|(i, p)| {
                    let uv = P2::new(a.clamp(p.1, p.2), wrap(psi, p.3, TAU).clamp(p.3, p.4));
                    (p.0 == chart
                        && (p.1 - 1e-9..=p.2 + 1e-9).contains(&a)
                        && (p.3 - 1e-9..=p.4 + 1e-9).contains(&wrap(psi, p.3, TAU))
                        && holds[i](uv))
                    .then_some((i, uv))
                })
            };
            let torus_face =
                |x: &P3| -> Option<(usize, P2)> { torus_faces(t_end, x).into_iter().next() };
            // A chain point: where it is, and the faces it is on (the sheet's, the torus's).
            let at = |w: P2| -> (P3, Option<((usize, P2), (usize, P2))>) {
                let x = tube
                    .eval(&Pos {
                        chart,
                        a: w.x,
                        psi: w.y,
                    })
                    .p;
                let faces = sheet_face(w.x, w.y).zip(torus_face(&x));
                (x, faces)
            };
            let pair = |w: P2| at(w).1.map(|(s, t)| (s.0, t.0));
            for chain in chains {
                if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                    let mut runs: Vec<(Option<(usize, usize)>, usize)> = Vec::new();
                    for w in &chain.pts {
                        let pr = pair(*w);
                        match runs.last_mut() {
                            Some((q, n)) if *q == pr => *n += 1,
                            _ => runs.push((pr, 1)),
                        }
                    }
                    let (w0, w1) = (chain.pts[0], chain.pts[chain.pts.len() - 1]);
                    let wm = chain.pts[chain.pts.len() / 2];
                    let xm = tube
                        .eval(&Pos {
                            chart,
                            a: wm.x,
                            psi: wm.y,
                        })
                        .p;
                    let posm = tori[usize::from(t_end > 0.5)].project(&xm, None);
                    eprintln!(
                        "  chain on chart {chart} at t {t_end}: {} points from ({:.4}, {:.4}) to ({:.4}, {:.4}), closed {:?}, runs {runs:?}; midway ({:.4}, {:.4}): sheet face {:?}, torus face {:?} (torus place {posm:?}, depth {:.2e})",
                        chain.pts.len(),
                        w0.x,
                        w0.y,
                        w1.x,
                        w1.y,
                        chain.closed,
                        wm.x,
                        wm.y,
                        sheet_face(wm.x, wm.y).map(|f| f.0),
                        torus_face(&xm).map(|f| f.0),
                        oracle.depth(&xm).0
                    );
                }
                if std::env::var_os("CADRS_TUBE_CHAIN").is_some() {
                    let n = chain.pts.len();
                    for (i, w) in chain.pts.iter().enumerate() {
                        if i >= 3 && i + 3 < n {
                            continue;
                        }
                        let x = tube
                            .eval(&Pos {
                                chart,
                                a: w.x,
                                psi: w.y,
                            })
                            .p;
                        let j = tube
                            .eval(&Pos {
                                chart,
                                a: a_join,
                                psi: w.y,
                            })
                            .p;
                        eprintln!(
                            "    point {i}: ({:.9}, {:.9}) F {:.3e} f {:.3e} off the join by {:.3e}, sheet {:?}, torus {:?}",
                            w.x,
                            w.y,
                            field(w.x, w.y),
                            tube.f(t_end, &x),
                            (x - j).norm(),
                            sheet_face(w.x, w.y).map(|f| f.0),
                            torus_face(&x).map(|f| f.0)
                        );
                    }
                }
                let mut pts = chain.pts;
                // (ψ continuous along the chain: the march gives it within one turn, jumping
                // where the chain crosses the seam.)
                for i in 1..pts.len() {
                    let d = pts[i].y - pts[i - 1].y;
                    if d.abs() > PI {
                        pts[i].y -= (d / TAU).round() * TAU;
                    }
                }
                if let Some(_) = chain.closed {
                    // Started where the faces change, if anywhere, the first point repeated.
                    pts.pop();
                    let n = pts.len();
                    if let Some(k) = (0..n).find(|&i| pair(pts[i]) != pair(pts[(i + 1) % n])) {
                        pts.rotate_left((k + 1) % n);
                    }
                    let first = pts[0];
                    pts.push(first);
                }
                // Runs of one pair of faces; where the pair changes between two points, the
                // point between them where it does (by bisection on the chord) ends one run
                // and starts the next.
                let mut run: Vec<(P3, P2, P2)> = Vec::new();
                let mut run_pair: Option<(usize, usize)> = None;
                // A point on the join (a side of the domain) taken for the faces of the run
                // it ends or starts, its (u, v) on each clamped into them (the lookup there is
                // ambiguous: the fold's end, where the curve meets the join, is on both pieces).
                let on_side = |w: P2| w.x <= 0.0 || w.x >= 1.0;
                let clamped = |w: P2, x: &P3, (sf, tf): (usize, usize)| -> (P3, P2, P2) {
                    let ps = kept_patches[sf];
                    let pt = kept_patches[tf];
                    let surf = &tori[usize::from(t_end > 0.5)];
                    // (On the join, within a hundred-thousandth of the size of a vertex of the sheet's
                    // face there (a fold's end: the curve meets the join where the fold does,
                    // and the two are found apart by the tolerance each is found to): that
                    // vertex.)
                    let (w, x) = if on_side(w) {
                        let mut best: Option<(f64, P3, P2)> = None;
                        let face = &faces[sf];
                        for v in &face.verts {
                            let d = (v.p - x).norm();
                            if d <= 1e-5 * size && best.is_none_or(|(bd, ..)| d < bd) {
                                let (u, vv) = face.faces[0].surf.project(&v.p, None);
                                let a_v = if (u - w.x).abs() < 0.5 { u } else { w.x };
                                if (a_v - w.x).abs() < 1e-6 {
                                    best = Some((d, v.p, P2::new(w.x, vv)));
                                }
                            }
                        }
                        match best {
                            Some((_, p, uv)) => (uv, p),
                            None => (w, *x),
                        }
                    } else {
                        (w, *x)
                    };
                    let (u, v) = surf.project(&x, None);
                    (
                        x,
                        P2::new(
                            w.x.clamp(ps.1, ps.2),
                            wrap(w.y, ps.3, TAU).clamp(ps.3, ps.4),
                        ),
                        P2::new(
                            wrap(u, pt.1, TAU).clamp(pt.1, pt.2),
                            wrap(v, pt.3, TAU).clamp(pt.3, pt.4),
                        ),
                    )
                };
                let join_pt = |psi: f64| {
                    tube.eval(&Pos {
                        chart,
                        a: a_join,
                        psi,
                    })
                    .p
                };
                let mut flush = |run: &mut Vec<(P3, P2, P2)>, pr: Option<(usize, usize)>| {
                    // (Points that repeat dropped; a run of one place is nothing.)
                    run.dedup_by(|q, p| (q.0 - p.0).norm() <= 1e-9 * size);
                    // A run from or to the join leaves it tangent to it, within the fit of
                    // either for a stretch: that stretch is taken as the join itself, the run
                    // starting on the join at its first point clearly off it.
                    if let Some(pair) = pr
                        && run.len() >= 2
                    {
                        let delta = 1e-5 * size;
                        let near_join = |p: &(P3, P2, P2)| (p.0 - join_pt(p.1.y)).norm() <= delta;
                        for end in 0..2 {
                            if end == 1 {
                                run.reverse();
                            }
                            if near_join(&run[0]) {
                                match run.iter().position(|p| !near_join(p)) {
                                    Some(i) if i > 0 => {
                                        // (From the join where the run is last within a
                                        // hundredth of that of it: a shallow start, not a
                                        // step square to the join.)
                                        let off = |p: &(P3, P2, P2)| (p.0 - join_pt(p.1.y)).norm();
                                        let i0 = (0..i)
                                            .rev()
                                            .find(|&j| off(&run[j]) <= 0.01 * delta)
                                            .unwrap_or(0);
                                        let psi = run[i0].1.y;
                                        let x = join_pt(psi);
                                        let start = clamped(P2::new(a_join, psi), &x, pair);
                                        run.drain(0..i);
                                        run.insert(0, start);
                                    }
                                    Some(_) => {}
                                    None => run.clear(),
                                }
                            }
                            if end == 1 {
                                run.reverse();
                            }
                            if run.is_empty() {
                                break;
                            }
                        }
                    }
                    if let Some((a, b)) = pr
                        && run.len() >= 2
                    {
                        given.push(super::sweep_moving::Given {
                            a,
                            b,
                            pts: std::mem::take(run),
                        });
                    }
                    run.clear();
                };
                for i in 0..pts.len() {
                    let w = pts[i];
                    let (x, faces) = at(w);
                    let pr = faces.map(|(s, t)| (s.0, t.0));
                    if pr != run_pair {
                        if pr.is_none() && on_side(w) && !run.is_empty() {
                            // The run ends on the join.
                            if let Some(rp) = run_pair {
                                run.push(clamped(w, &x, rp));
                            }
                            flush(&mut run, run_pair);
                            run_pair = None;
                            continue;
                        }
                        if run_pair.is_none() && i > 0 && on_side(pts[i - 1]) && pr.is_some() {
                            // The run starts on the join.
                            let w0 = pts[i - 1];
                            let x0 = tube
                                .eval(&Pos {
                                    chart,
                                    a: w0.x,
                                    psi: w0.y,
                                })
                                .p;
                            run.push(clamped(w0, &x0, pr.unwrap()));
                            run_pair = pr;
                            if let Some((s, t)) = faces {
                                run.push((x, s.1, t.1));
                            }
                            continue;
                        }
                        // The change between the last point and this: found on the chord.
                        if i > 0 {
                            // (Along the curve: each point of the chord between the two put
                            // back onto the zero set.)
                            let (p, q) = (pts[i - 1], w);
                            let on_curve = |s: f64| -> P2 {
                                let m = P2::new(p.x + (q.x - p.x) * s, p.y + (q.y - p.y) * s);
                                onto_zero(&field, m, q - p).unwrap_or(m)
                            };
                            let (mut slo, mut shi) = (0.0, 1.0);
                            for _ in 0..50 {
                                let sm = 0.5 * (slo + shi);
                                if pair(on_curve(sm)) == run_pair {
                                    slo = sm;
                                } else {
                                    shi = sm;
                                }
                            }
                            let (lo, hi) = (on_curve(slo), on_curve(shi));
                            if std::env::var_os("CADRS_TUBE_CHAIN").is_some() {
                                eprintln!(
                                    "    run change between ({:.5}, {:.5}) {:?} and ({:.5}, {:.5}) {:?}: at ({:.6}, {:.6}) {:?} | ({:.6}, {:.6}) {:?}",
                                    p.x,
                                    p.y,
                                    run_pair,
                                    q.x,
                                    q.y,
                                    pr,
                                    lo.x,
                                    lo.y,
                                    pair(lo),
                                    hi.x,
                                    hi.y,
                                    pair(hi)
                                );
                            }
                            // (The point between, placed on each run's faces by their ranges:
                            // a lookup there, on their boundary, is ambiguous.)
                            let xl = tube
                                .eval(&Pos {
                                    chart,
                                    a: lo.x,
                                    psi: lo.y,
                                })
                                .p;
                            if let Some(rp) = run_pair {
                                run.push(clamped(lo, &xl, rp));
                            }
                            flush(&mut run, run_pair);
                            if let Some(np) = pr {
                                let xh = tube
                                    .eval(&Pos {
                                        chart,
                                        a: hi.x,
                                        psi: hi.y,
                                    })
                                    .p;
                                run.push(clamped(hi, &xh, np));
                            }
                        }
                        run_pair = pr;
                    }
                    if let Some((s, t)) = faces {
                        if on_side(w) {
                            // (On the join: snapped to a vertex of the face there.)
                            run.push(clamped(w, &x, (s.0, t.0)));
                        } else {
                            run.push((x, s.1, t.1));
                        }
                    }
                }
                flush(&mut run, run_pair);
            }
        }
        // (Every pair of a sheet's face and the torus's at either end is given: the curves
        // above are the whole of their crossings, so none is traced from a seed.)
        for (i, p) in kept_patches.iter().enumerate() {
            if p.0 != LEAD && p.0 != TRAIL {
                continue;
            }
            for (j, q) in kept_patches.iter().enumerate() {
                if q.0 == START || q.0 == END {
                    given.push(super::sweep_moving::Given {
                        a: i,
                        b: j,
                        pts: Vec::new(),
                    });
                }
            }
        }
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "tube sweep: {} curves given where the sheets cross the tori at the ends ({:?})",
                given.iter().filter(|g| g.pts.len() >= 2).count(),
                given
                    .iter()
                    .filter(|g| g.pts.len() >= 2)
                    .map(|g| (g.a, g.b, g.pts.len()))
                    .collect::<Vec<_>>()
            );
        }
        given
    };
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        // The sheets' edges along the joins against the joins' exact points, and the tori's
        // join points against them.
        let mut worst_fit = 0.0f64;
        let mut worst_torus = 0.0f64;
        for (i, p) in kept_patches.iter().enumerate() {
            if p.0 != LEAD && p.0 != TRAIL {
                continue;
            }
            let t_of = |a: f64| if p.0 == LEAD { a } else { 1.0 - a };
            for a in [0.0, 1.0] {
                if (p.1 - a).abs() > 1e-9 && (p.2 - a).abs() > 1e-9 {
                    continue;
                }
                let t_end = t_of(a);
                let surf = &faces[i].faces[0].surf;
                let torus = &tori[usize::from(t_end > 0.5)];
                for k in 0..=1000 {
                    let psi = p.3 + (p.4 - p.3) * k as f64 / 1000.0;
                    let exact = tube.eval(&Pos { chart: p.0, a, psi }).p;
                    worst_fit = worst_fit.max((surf.eval(a, psi) - exact).norm());
                    let th = theta_join(t_end, psi) + if p.0 == LEAD { 0.0 } else { PI };
                    worst_torus = worst_torus.max((torus.eval(psi, th) - exact).norm());
                }
            }
        }
        let mut worst_cut = 0.0f64;
        for (i, p) in kept_patches.iter().enumerate() {
            if p.0 != START && p.0 != END {
                continue;
            }
            let t_end = if p.0 == END { 1.0 } else { 0.0 };
            let body = &faces[i];
            for e in &body.edges {
                if e.degenerate || !matches!(e.curve, Curve::Nurbs(_)) {
                    continue;
                }
                // (A join cut: its points are on a join; measured against the nearer join.)
                for k in 0..=100 {
                    let t = e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 100.0;
                    let x = e.curve.point(t);
                    let (psi, _) = tori[usize::from(t_end > 0.5)].project(&x, None);
                    let th = theta_join(t_end, psi);
                    let d = [0.0, PI]
                        .iter()
                        .map(|h| (tori[usize::from(t_end > 0.5)].eval(psi, th + h) - x).norm())
                        .fold(f64::INFINITY, f64::min);
                    if d < 1e-3 {
                        worst_cut = worst_cut.max(d);
                    }
                }
            }
        }
        eprintln!(
            "tube sweep: along the joins, the sheets' fitted edges stray {worst_fit:.2e} from the exact joins, the tori's join points {worst_torus:.2e}, the tori's join cuts {worst_cut:.2e}"
        );
    }
    let least = 2.0 * PI * PI * big * r * r;
    let solid =
        match super::sweep_moving::boundary_of(faces, &oracle, size, least, f64::INFINITY, given) {
            Ok(s) => s,
            Err(e) => {
                if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                    eprintln!("tube sweep: refused: {e}");
                    // Each point named: its instants (own and partners) and where on K each puts
                    // it, and whether a kept patch holds that place.
                    let msg = e.to_string();
                    for part in msg.split('[').skip(1).take(12) {
                        let Some(end) = part.find(']') else { continue };
                        let nums: Vec<f64> = part[..end]
                            .split(',')
                            .filter_map(|v| v.trim().parse().ok())
                            .collect();
                        if nums.len() != 3 {
                            continue;
                        }
                        let x = P3::new(nums[0], nums[1], nums[2]);
                        let mut leasts = tube.others(&x, -1.0, 1.0);
                        leasts.sort_by(|a, b| a.1.total_cmp(&b.1));
                        eprint!("  point {x:?}:");
                        for (t, v) in leasts.iter().take(3) {
                            let pos = tube.pos_at(*t, &x);
                            let held = kept_patches.iter().any(|&(c, a0, a1, p0, p1)| {
                                c == pos.chart
                                    && (a0..=a1).contains(&pos.a)
                                    && (p0..=p1).contains(&wrap(pos.psi, p0, TAU))
                            });
                            eprint!(" t {t:.6} f {v:.3e} at {pos:?} held {held};");
                        }
                        eprintln!();
                    }
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

/// A patch of K: its chart, a from `.1` to `.2`, ψ from `.3` to `.4`.
type Patch = (usize, f64, f64, f64, f64);

/// Whether the swept solid covers the patch deeply: every point of a grid over it (its sides
/// included) is inside at some other instant by a thousandth of the size at least (no part of
/// the boundary can run between such points).
fn covered(tube: &Tube, oracle: &super::sweep_moving::Oracle, p: Patch, m: usize) -> bool {
    let (chart, a0, a1, p0, p1) = p;
    let eps = 1e-7 * tube.size;
    let deep = 1e-3 * tube.size;
    for i in 0..=m {
        for j in 0..=m {
            let pos = Pos {
                chart,
                a: a0 + (a1 - a0) * i as f64 / m as f64,
                psi: p0 + (p1 - p0) * j as f64 / m as f64,
            };
            let kp = tube.eval(&pos);
            let (rm, dm) = place(tube.steps, kp.own);
            let core = rm * tube.k(pos.psi) + dm;
            let Some(out) = (kp.p.coords - core).try_normalize(1e-300) else {
                return false;
            };
            if oracle.depth(&(kp.p + out * eps)).0 > -deep {
                return false;
            }
        }
    }
    true
}

/// How many times a patch that won't fit is cut in four.
const PATCH_SPLITS: u32 = 6;

/// A patch fitted: its surface, and the curves in (a, ψ) along which its sheet folds (each from
/// one side of the patch to another; empty where it doesn't fold).
type Fitted = (Patch, super::nurbs::NurbsSurface, Vec<Vec<P2>>);

/// The curves in the patch along which its sheet folds (where [`Tube::fold`] is zero), each a
/// chain of points from one side of the patch to another; None when a fold curve closes inside
/// the patch (when the patch is to be cut smaller). The chains are empty when the sheet is
/// clear of folding throughout.
fn fold_curves(tube: &Tube, p: Patch, m: usize) -> Option<Vec<Vec<P2>>> {
    let (chart, a0, a1, p0, p1) = p;
    if chart == END || chart == START {
        return Some(Vec::new());
    }
    let span = ((a1 - a0).powi(2) + (p1 - p0).powi(2)).sqrt();
    let chains = zero_curves(
        &|a, psi| tube.fold(chart, a, psi),
        (a0, a1),
        (p0, p1),
        (m, m),
        false,
        1e-3 * span / m as f64,
    )?;
    chains
        .iter()
        .all(|c| c.closed.is_none())
        .then(|| chains.into_iter().map(|c| c.pts).collect())
}

/// A curve where a function of (a, ψ) is zero: its points, and, closed, the index of the
/// repeated point (its last, the first again).
struct ZeroCurve {
    pts: Vec<P2>,
    closed: Option<usize>,
}

/// The curves along which `f` is zero over the box `(a0, a1) × (p0, p1)`, by marching squares
/// over an `ma × mp` grid (ψ periodic over its range when `periodic`: no chain ends on a ψ
/// side) with the crossings refined by bisection, each chain's points then made dense enough
/// that a chord between neighbours strays at most `tol` from the curve (in (a, ψ)). Open
/// chains run from a side to a side; closed ones repeat their first point. None when the march
/// fails (a cell with three crossings).
fn zero_curves(
    f: &dyn Fn(f64, f64) -> f64,
    (a0, a1): (f64, f64),
    (p0, p1): (f64, f64),
    (ma, mp): (usize, usize),
    periodic: bool,
    tol: f64,
) -> Option<Vec<ZeroCurve>> {
    let at = |i: usize, j: usize| {
        (
            a0 + (a1 - a0) * i as f64 / ma as f64,
            p0 + (p1 - p0) * j as f64 / mp as f64,
        )
    };
    // Grid values; a periodic ψ's last row is its first.
    let val = |phi: &Vec<f64>, i: usize, j: usize| -> f64 {
        let j = if periodic { j % mp } else { j };
        phi[i * (mp + 1) + j]
    };
    let mut phi = vec![0.0; (ma + 1) * (mp + 1)];
    for i in 0..=ma {
        for j in 0..=mp {
            let (a, psi) = at(i, j);
            phi[i * (mp + 1) + j] = f(a, psi);
        }
    }
    let least = phi.iter().copied().fold(f64::INFINITY, f64::min);
    let most = phi.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if least > 0.0 || most < 0.0 {
        return Some(Vec::new());
    }
    // The crossing on each grid edge with a sign change: keyed by (i, j, along a?), j taken
    // modulo mp when periodic.
    let key = |i: usize, j: usize, along_a: bool| -> (usize, usize, bool) {
        (i, if periodic { j % mp } else { j }, along_a)
    };
    let mut cross: HashMap<(usize, usize, bool), P2> = HashMap::new();
    let refine = |(x0, y0): (f64, f64), (x1, y1): (f64, f64), f0: f64| -> P2 {
        let (mut lo, mut hi) = (0.0, 1.0);
        let mut flo = f0;
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            let (a, psi) = (x0 + (x1 - x0) * mid, y0 + (y1 - y0) * mid);
            let fm = f(a, psi);
            if (fm < 0.0) == (flo < 0.0) {
                lo = mid;
                flo = fm;
            } else {
                hi = mid;
            }
        }
        let mid = 0.5 * (lo + hi);
        P2::new(x0 + (x1 - x0) * mid, y0 + (y1 - y0) * mid)
    };
    let jmax = if periodic { mp - 1 } else { mp };
    for i in 0..=ma {
        for j in 0..=jmax {
            let f0 = val(&phi, i, j);
            if i < ma {
                let f1 = val(&phi, i + 1, j);
                if (f0 < 0.0) != (f1 < 0.0) {
                    cross.insert(key(i, j, true), refine(at(i, j), at(i + 1, j), f0));
                }
            }
            if j < mp {
                let f1 = val(&phi, i, j + 1);
                if (f0 < 0.0) != (f1 < 0.0) {
                    cross.insert(key(i, j, false), refine(at(i, j), at(i, j + 1), f0));
                }
            }
        }
    }
    // Each cell's crossings paired (a saddle's by the sign at its centre), as links between
    // crossings.
    let mut links: HashMap<(usize, usize, bool), Vec<(usize, usize, bool)>> = HashMap::new();
    for i in 0..ma {
        for j in 0..mp {
            let sides = [
                key(i, j, false),
                key(i + 1, j, false),
                key(i, j, true),
                key(i, j + 1, true),
            ];
            let hits: Vec<(usize, usize, bool)> = sides
                .into_iter()
                .filter(|k| cross.contains_key(k))
                .collect();
            let pairs: Vec<[(usize, usize, bool); 2]> = match hits.len() {
                2 => vec![[hits[0], hits[1]]],
                4 => {
                    let (a, psi) = at(i, j);
                    let (a2, psi2) = at(i + 1, j + 1);
                    let centre = f(0.5 * (a + a2), 0.5 * (psi + psi2));
                    let corner = val(&phi, i, j);
                    // Corner (i, j) and the centre alike: its two sides' crossings apart.
                    if (centre < 0.0) == (corner < 0.0) {
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
    // A crossing on a side of the box (where open chains end).
    let on_side = |k: &(usize, usize, bool)| -> bool {
        let &(i, j, along_a) = k;
        if along_a {
            !periodic && (j == 0 || j == mp)
        } else {
            i == 0 || i == ma
        }
    };
    let mut used: std::collections::HashSet<(usize, usize, bool)> = Default::default();
    let mut chains: Vec<ZeroCurve> = Vec::new();
    let mut keys: Vec<(usize, usize, bool)> = cross.keys().copied().collect();
    keys.sort();
    // Open chains from the sides first, then closed ones from whatever is left.
    for pass in 0..2 {
        for &s0 in &keys {
            if used.contains(&s0) || (pass == 0) != on_side(&s0) {
                continue;
            }
            let mut chain = vec![cross[&s0]];
            used.insert(s0);
            let mut prev = s0;
            let mut cur = *links.get(&s0)?.first()?;
            let mut closed = None;
            loop {
                chain.push(cross[&cur]);
                if cur == s0 {
                    closed = Some(chain.len() - 1);
                    break;
                }
                used.insert(cur);
                if on_side(&cur) {
                    break;
                }
                let next = links.get(&cur)?.iter().copied().find(|k| *k != prev)?;
                prev = cur;
                cur = next;
                if chain.len() > 4 * (ma + 1) * (mp + 1) {
                    return None;
                }
            }
            chains.push(ZeroCurve { pts: chain, closed });
        }
    }
    // Each chain made dense enough that a chord between neighbours stays within `tol` of the
    // curve: a point put on it between two where the chord strays.
    for chain in &mut chains {
        for _ in 0..10 {
            let mut add: Vec<(usize, P2)> = Vec::new();
            for i in 0..chain.pts.len() - 1 {
                let (p, q) = (chain.pts[i], chain.pts[i + 1]);
                let mid = P2::new(0.5 * (p.x + q.x), 0.5 * (p.y + q.y));
                let Some(r) = onto_zero(f, mid, q - p) else {
                    continue;
                };
                if (r - mid).norm() > tol {
                    add.push((i + 1, r));
                }
            }
            if add.is_empty() {
                break;
            }
            for (k, (i, r)) in add.into_iter().enumerate() {
                chain.pts.insert(i + k, r);
            }
            if let Some(c) = &mut chain.closed {
                *c = chain.pts.len() - 1;
            }
        }
    }
    Some(chains)
}

/// The point where `f` is zero near `w`, along the normal to the direction `d` in (a, ψ): the
/// nearest change of sign either way, found by doubling steps out from `w` and bisected (the
/// secant method wanders where f is down to round-off); None when there is none within a
/// tenth of a unit.
fn onto_zero(f: &dyn Fn(f64, f64) -> f64, w: P2, d: V2) -> Option<P2> {
    let n = V2::new(-d.y, d.x).try_normalize(1e-300)?;
    let g = |s: f64| f(w.x + n.x * s, w.y + n.y * s);
    let g0 = g(0.0);
    if g0 == 0.0 {
        return Some(w);
    }
    let mut h = 1e-9;
    let mut bracket: Option<(f64, f64)> = None;
    while h < 0.1 {
        for s in [h, -h] {
            if (g(s) < 0.0) != (g0 < 0.0) {
                bracket = Some((0.0, s));
                break;
            }
        }
        if bracket.is_some() {
            break;
        }
        h *= 2.0;
    }
    let (mut lo, mut hi) = bracket?;
    let mut glo = g0;
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if mid == lo || mid == hi {
            break;
        }
        let gm = g(mid);
        if (gm < 0.0) == (glo < 0.0) {
            lo = mid;
            glo = gm;
        } else {
            hi = mid;
        }
    }
    let s = 0.5 * (lo + hi);
    Some(P2::new(w.x + n.x * s, w.y + n.y * s))
}

/// The patch fitted within `tol` (on a grid doubled from 8 to 64), or cut in four and each
/// quarter fitted; a patch the solid covers deeply is left out (none). An error names a patch
/// the solid doesn't cover that won't fit however small.
fn fit_patch(
    tube: &Tube,
    oracle: &super::sweep_moving::Oracle,
    p: Patch,
    tol: f64,
    depth: u32,
) -> std::result::Result<Vec<Fitted>, String> {
    let (chart, a0, a1, p0, p1) = p;
    if covered(tube, oracle, p, 8) {
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "  patch {chart} a {a0:.4}..{a1:.4} ψ {p0:.4}..{p1:.4} left out: deeply covered"
            );
        }
        return Ok(Vec::new());
    }
    let folds = fold_curves(tube, p, 32);
    let mut n = 8;
    loop {
        let (surf, worst) = patch_fit(tube, chart, a0, a1, p0, p1, n);
        if worst <= tol
            && let Some(folds) = &folds
        {
            return Ok(vec![(p, surf, folds.clone())]);
        }
        if n >= 64 {
            if std::env::var_os("CADRS_TUBE_PATCHES").is_some() {
                let mut sin = f64::INFINITY;
                for i in 0..=8 {
                    let j = jet(
                        tube.steps,
                        match chart {
                            LEAD => a0 + (a1 - a0) * i as f64 / 8.0,
                            TRAIL => 1.0 - a0 - (a1 - a0) * i as f64 / 8.0,
                            END => 1.0,
                            _ => 0.0,
                        },
                    );
                    for k in 0..=8 {
                        sin = sin.min(tube.sheet_pt(&j, p0 + (p1 - p0) * k as f64 / 8.0).sin);
                    }
                }
                eprintln!(
                    "  patch {chart} a {a0:.4}..{a1:.4} psi {p0:.4}..{p1:.4} depth {depth}: worst {worst:.3e}, min sin {sin:.4}"
                );
            }
            break;
        }
        n *= 2;
    }
    if depth >= PATCH_SPLITS {
        return Err(format!(
            "a patch of its boundary ({chart}, a {a0:.6}..{a1:.6}, ψ {p0:.6}..{p1:.6}) bends too \
             sharply to fit"
        ));
    }
    use rayon::prelude::*;
    let (am, pm) = (0.5 * (a0 + a1), 0.5 * (p0 + p1));
    let quarters = [
        (chart, a0, am, p0, pm),
        (chart, am, a1, p0, pm),
        (chart, a0, am, pm, p1),
        (chart, am, a1, pm, p1),
    ];
    let parts: Vec<_> = quarters
        .par_iter()
        .map(|&q| fit_patch(tube, oracle, q, tol, depth + 1))
        .collect();
    let mut out = Vec::new();
    for part in parts {
        out.extend(part?);
    }
    Ok(out)
}

/// The faces of a fitted patch: one, or where its sheet folds, its pieces cut along the fold
/// curves.
fn patch_faces(
    tube: &Tube,
    steps: &[RigidStep],
    (patch, surf, folds): Fitted,
    size: f64,
) -> Result<Vec<Body>> {
    let (chart, a0, a1, p0, p1) = patch;
    let surf = super::surface::Surface::Nurbs(std::sync::Arc::new(surf));
    let sheet = super::face_build::face_from_surface(&surf, Some(((a0, a1), (p0, p1))))?;
    let pieces = if folds.is_empty() {
        vec![sheet]
    } else {
        let nb = sheet.verts.len();
        let mut pos: HashMap<usize, P3> = HashMap::new();
        let mut cuts = Vec::new();
        for (k, chain) in folds.iter().enumerate() {
            let pts: Vec<(P3, P2)> = chain.iter().map(|w| (surf.eval(w.x, w.y), *w)).collect();
            let mut params = vec![0.0];
            for w in pts.windows(2) {
                params.push(params.last().unwrap() + (w[1].0 - w[0].0).norm());
            }
            let p3: Vec<P3> = pts.iter().map(|p| p.0).collect();
            let deg = if p3.len() >= 4 { 3 } else { 1 };
            let c = Curve::Nurbs(std::sync::Arc::new(super::nurbs::NurbsCurve::interpolate(
                &p3, &params, deg, None, None,
            )));
            let v = [nb + 2 * k, nb + 2 * k + 1];
            pos.insert(v[0], pts[0].0);
            pos.insert(v[1], pts[pts.len() - 1].0);
            cuts.push(super::sweep_moving::FaceCut { c, pts, v });
        }
        super::sweep_moving::cut_face(&sheet, 0, cuts, &pos, size).map_err(|why| {
            KernelError::InvalidResult {
                op: "solid sweep",
                why: format!("the swept torus: a sheet's fold: {why}"),
            }
        })?
    };
    // The pieces turned to face away from the core circle placed at their points' own instant,
    // judged on a piece not folded over.
    let inner_of = |piece: &Body| -> Option<P2> {
        let polys = super::classify::face_polygons(piece, &piece.faces[0]);
        super::boolean::interior_points_of_polys(&polys)
            .first()
            .copied()
    };
    let mut reverse = None;
    for piece in &pieces {
        let Some(uv) = inner_of(piece) else { continue };
        if !folds.is_empty() && tube.fold(chart, uv.x, uv.y) < 0.0 {
            continue;
        }
        let kp = tube.eval(&Pos {
            chart,
            a: uv.x,
            psi: uv.y,
        });
        let (rm, dm) = place(steps, kp.own);
        let core = rm * tube.k(uv.y) + dm;
        let outward = kp.p.coords - core;
        let f = &piece.faces[0];
        let (_, su, sv) = f.surf.d1(uv.x, uv.y);
        let natural = su.cross(&sv) * if f.sense { 1.0 } else { -1.0 };
        reverse = Some(natural.dot(&outward) < 0.0);
        break;
    }
    let Some(reverse) = reverse else {
        return Ok(Vec::new());
    };
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some() && !folds.is_empty();
    if debug {
        eprintln!(
            "  patch {chart} a {a0:.4}..{a1:.4} ψ {p0:.4}..{p1:.4}: {} fold curves of {:?} points ({:?}), {} pieces",
            folds.len(),
            folds.iter().map(|c| c.len()).collect::<Vec<_>>(),
            folds
                .iter()
                .map(|c| (c[0], c[c.len() - 1]))
                .collect::<Vec<_>>(),
            pieces.len()
        );
        for a in [a0, a1] {
            let row: Vec<f64> = (0..=8)
                .map(|j| tube.fold(chart, a, p0 + (p1 - p0) * j as f64 / 8.0))
                .collect();
            eprintln!("    fold along a = {a:.4}: {row:?}");
        }
        for psi in [p0, p1] {
            let row: Vec<f64> = (0..=8)
                .map(|i| tube.fold(chart, a0 + (a1 - a0) * i as f64 / 8.0, psi))
                .collect();
            eprintln!("    fold along ψ = {psi:.4}: {row:?}");
        }
    }
    let mut out = Vec::with_capacity(pieces.len());
    for mut piece in pieces {
        let inner = inner_of(&piece);
        // (A piece folded over lies inside the solid, under the sheet beyond the fold, and is
        // judged so; it stays a candidate, since other faces' curves cross it.)
        if debug {
            let polys = super::classify::face_polygons(&piece, &piece.faces[0]);
            let n: usize = polys.iter().map(|p| p.len()).sum();
            let (mut lo, mut hi) = (
                P2::new(f64::INFINITY, f64::INFINITY),
                P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
            );
            for q in polys.iter().flatten() {
                lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
                hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
            }
            eprintln!(
                "    piece: {} loops, {n} polygon points, uv {lo:?}..{hi:?}, interior {inner:?}, fold there {:?}",
                piece.faces[0].loops.len(),
                inner.map(|uv| tube.fold(chart, uv.x, uv.y))
            );
        }
        if inner.is_none() {
            continue;
        }
        if reverse {
            Body::reverse_face(&mut piece.faces[0]);
        }
        out.push(piece);
    }
    Ok(out)
}

/// The degree of the B-splines fitted through K's points (quintic: its error falls as the sixth
/// power of the grid's spacing, so a coarse grid fits to a billionth of the size).
const PATCH_DEGREE: usize = 5;

/// The B-spline (of `PATCH_DEGREE`) through K's points on an n × n grid over the patch (chart, a
/// from `a0` to `a1`, ψ from `p0` to `p1`), and how far it strays from K midway between them.
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
            NurbsCurve::interpolate(&pts, &us, PATCH_DEGREE, None, None)
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
        let c = NurbsCurve::interpolate(&col, &vs, PATCH_DEGREE, None, None);
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
        dv: PATCH_DEGREE.min(vs.len() - 1),
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
