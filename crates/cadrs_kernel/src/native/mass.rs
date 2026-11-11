//! Exact mass properties by the divergence theorem, integrated over each face's trimmed (u, v)
//! domain with Green's theorem: ∫∫_D h du dv = ∮_∂D H dv, H(u, v) = ∫_{u₀}^{u} h(s, v) ds,
//! where the boundary runs along the exact p-curves. Both integrals are composite
//! Gauss–Legendre, with rules matched to the integrands where they are polynomials
//! (research/algorithms/mass_tessellation_hlr.md, cards 1 and 3):
//! - along a cylinder's, cone's or extrusion's rulings and over a plane the integrands are
//!   polynomials of degree 4 at most (2 or 3 points);
//! - round an axis they are trigonometric polynomials of degree 4 at most, which 8 points on
//!   pieces of a quarter turn integrate to rounding;
//! - on a polynomial B-spline surface of degree p the volume's integrand is a polynomial of degree
//!   3p − 2 in u on each knot span, exact with ⌈(3p − 1)/2⌉ points ([`body_volume`]); the area's
//!   |N| is only smooth, so the full properties keep 16 points a span (⌈5p/2⌉, the inertia's,
//!   from degree 7).
//!
//! So analytic faces come out to rounding error.

use rayon::prelude::*;
use std::sync::OnceLock;

use super::curve::Curve;
use super::math::*;
use super::nurbs::NurbsSurface;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Face, co_range};

/// Number of integrated quantities (see `integrand`).
pub const NQ: usize = 14;

/// The most pieces of at most π/8 a p-curve is cut into on a periodic face (256 turns): more
/// is no geometry, only a parameter range a file got wrong, which must not loop for ever.
const MAX_PIECES: f64 = 4096.0;

/// The pieces (radians) an angle round an axis is integrated in, with [`ANGLE_POINTS`] points
/// each: the integrands are trigonometric polynomials of degree 4 at most in it (degree 6 with
/// the other direction's), which that rule integrates to about 1e-17 of their coefficients.
const ANGLE_STEP: f64 = PI / 4.0;
const ANGLE_POINTS: usize = 8;

pub type Q = [f64; NQ];

/// Number of quantities integrated here: [`Q`]'s, then the flux moments [`shifted`] needs to
/// move them to another origin (`∫N`, `∫x·nx, ∫y·ny, ∫z·nz`, `∫xy·nx, ∫yz·ny, ∫zx·nz`,
/// `∫y·nx, ∫z·ny, ∫x·nz`).
const NE: usize = NQ + 12;

/// [`Q`] with the moments of [`NE`].
type QE = [f64; NE];

/// The integrand at (u, v) relative to `shift`: area terms (scaled by |N|) and volume terms
/// (by the vector N = Su × Sv; the divergence theorem turns them into volume integrals). With
/// `ALL` false only the volume's (`out[4]`): this and the integrators below take `ALL` so
/// [`body_volume`] runs only the one quantity, on the fewer points its integrand needs.
#[inline]
fn integrand<const ALL: bool>(s: &Surface, u: f64, v: f64, shift: &V3, out: &mut QE, w: f64) {
    let (p, su, sv) = s.d1(u, v);
    accumulate::<ALL>(&p, &su, &sv, shift, out, w);
}

/// [`integrand`] from the point and first derivatives.
#[inline]
fn accumulate<const ALL: bool>(p: &P3, su: &V3, sv: &V3, shift: &V3, out: &mut QE, w: f64) {
    let n = su.cross(sv);
    let (x, y, z) = (p.x - shift.x, p.y - shift.y, p.z - shift.z);
    let (nx, ny, nz) = (n.x * w, n.y * w, n.z * w);
    if !ALL {
        out[4] += (x * nx + y * ny + z * nz) / 3.0;
        return;
    }
    let a = n.norm() * w;
    out[0] += a;
    out[1] += x * a;
    out[2] += y * a;
    out[3] += z * a;
    out[4] += (x * nx + y * ny + z * nz) / 3.0;
    out[5] += x * x * 0.5 * nx;
    out[6] += y * y * 0.5 * ny;
    out[7] += z * z * 0.5 * nz;
    out[8] += x * x * x / 3.0 * nx;
    out[9] += y * y * y / 3.0 * ny;
    out[10] += z * z * z / 3.0 * nz;
    out[11] += x * x * y * 0.5 * nx;
    out[12] += y * y * z * 0.5 * ny;
    out[13] += z * z * x * 0.5 * nz;
    out[14] += nx;
    out[15] += ny;
    out[16] += nz;
    out[17] += x * nx;
    out[18] += y * ny;
    out[19] += z * nz;
    out[20] += x * y * nx;
    out[21] += y * z * ny;
    out[22] += z * x * nz;
    out[23] += y * nx;
    out[24] += z * ny;
    out[25] += x * nz;
}

/// Face integrals `q` (with [`NE`]'s moments) about `from` moved to `to`: x − d for x, d = to −
/// from, expanded in the moments.
fn shifted(q: &QE, from: &V3, to: &V3) -> Q {
    let d = to - from;
    let (dx, dy, dz) = (d.x, d.y, d.z);
    let mut r = [0.0; NQ];
    r[0] = q[0];
    r[1] = q[1] - dx * q[0];
    r[2] = q[2] - dy * q[0];
    r[3] = q[3] - dz * q[0];
    r[4] = q[4] - (dx * q[14] + dy * q[15] + dz * q[16]) / 3.0;
    r[5] = q[5] - dx * q[17] + dx * dx * 0.5 * q[14];
    r[6] = q[6] - dy * q[18] + dy * dy * 0.5 * q[15];
    r[7] = q[7] - dz * q[19] + dz * dz * 0.5 * q[16];
    r[8] = q[8] - 2.0 * dx * q[5] + dx * dx * q[17] - dx * dx * dx / 3.0 * q[14];
    r[9] = q[9] - 2.0 * dy * q[6] + dy * dy * q[18] - dy * dy * dy / 3.0 * q[15];
    r[10] = q[10] - 2.0 * dz * q[7] + dz * dz * q[19] - dz * dz * dz / 3.0 * q[16];
    r[11] = q[11] - dy * q[5] - dx * q[20] + dx * dy * q[17] + dx * dx * 0.5 * q[23]
        - dx * dx * dy * 0.5 * q[14];
    r[12] = q[12] - dz * q[6] - dy * q[21] + dy * dz * q[18] + dy * dy * 0.5 * q[24]
        - dy * dy * dz * 0.5 * q[15];
    r[13] = q[13] - dx * q[7] - dz * q[22] + dz * dx * q[19] + dz * dz * 0.5 * q[25]
        - dz * dz * dx * 0.5 * q[16];
    r
}

/// The most points a rule here has.
const MAX_GAUSS: usize = 24;

/// The Gauss–Legendre rule with `n` points on [−1, 1] (1 to [`MAX_GAUSS`]): math's tables for 4,
/// 8 and 16, the others from the Legendre polynomials' roots (Newton's method, to rounding).
fn gauss(n: usize) -> Gauss {
    static RULES: OnceLock<Vec<(Vec<f64>, Vec<f64>)>> = OnceLock::new();
    match n {
        4 => {
            return Gauss {
                x: GAUSS4.x,
                w: GAUSS4.w,
            };
        }
        8 => {
            return Gauss {
                x: GAUSS8.x,
                w: GAUSS8.w,
            };
        }
        16 => {
            return Gauss {
                x: GAUSS16.x,
                w: GAUSS16.w,
            };
        }
        _ => {}
    }
    let (x, w) =
        &RULES.get_or_init(|| (0..=MAX_GAUSS).map(legendre_rule).collect())[n.clamp(1, MAX_GAUSS)];
    Gauss { x, w }
}

/// The nodes and weights of the n-point Gauss–Legendre rule, ascending.
fn legendre_rule(n: usize) -> (Vec<f64>, Vec<f64>) {
    let (mut x, mut w) = (vec![0.0; n], vec![0.0; n]);
    // P_n(z) and P_n'(z) by the three-term recurrence.
    let legendre = |z: f64| {
        let (mut p0, mut p1) = (1.0, z);
        for k in 2..=n {
            let k = k as f64;
            (p0, p1) = (p1, ((2.0 * k - 1.0) * z * p1 - (k - 1.0) * p0) / k);
        }
        (p1, n as f64 * (z * p1 - p0) / (z * z - 1.0))
    };
    for i in 0..n.div_ceil(2) {
        let mut z = (PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        for _ in 0..100 {
            let (p, dp) = legendre(z);
            let dz = p / dp;
            z -= dz;
            if dz.abs() <= 1e-16 {
                break;
            }
        }
        let (_, dp) = legendre(z);
        x[i] = -z;
        x[n - 1 - i] = z;
        w[i] = 2.0 / ((1.0 - z * z) * dp * dp);
        w[n - 1 - i] = w[i];
    }
    (x, w)
}

/// Four-point Gauss–Legendre on [−1, 1].
const GAUSS4: Gauss = Gauss {
    x: &[
        -0.861_136_311_594_052_6,
        -0.339_981_043_584_856_3,
        0.339_981_043_584_856_3,
        0.861_136_311_594_052_6,
    ],
    w: &[
        0.347_854_845_137_453_9,
        0.652_145_154_862_546_1,
        0.652_145_154_862_546_1,
        0.347_854_845_137_453_9,
    ],
};

/// Whether a B-spline surface is polynomial: its weights all equal (files often flag polynomial
/// patches rational).
fn polynomial(ns: &NurbsSurface) -> bool {
    let w0 = ns.cps[0][0].w;
    ns.cps
        .iter()
        .flatten()
        .all(|c| (c.w - w0).abs() <= 1e-14 * w0.abs())
}

/// How a face's inner integral along u is cut and sampled (once per face).
struct Inner {
    /// The pieces: a B-spline surface's knot spans in u (its interior knots), each cut into
    /// `cut`; else pieces of at most `step` (one piece when infinite).
    knots: Vec<f64>,
    cut: usize,
    step: f64,
    /// Points per piece.
    n: usize,
    /// A B-spline surface's distinct knots in u, its ends too (for [`Inner::table`]).
    breaks: Vec<f64>,
}

impl Inner {
    fn of<const ALL: bool>(s: &Surface) -> Self {
        match s {
            Surface::Nurbs(ns) => {
                let (bu, _) = ns.breaks();
                let knots = bu[1..bu.len() - 1].to_vec();
                if !ALL && polynomial(ns) {
                    // The volume's integrand: a polynomial of degree 3p − 2 a span.
                    let n = (3 * ns.du).saturating_sub(1).div_ceil(2);
                    return Self {
                        knots,
                        cut: 1,
                        step: f64::INFINITY,
                        n: n.clamp(1, MAX_GAUSS),
                        breaks: bu,
                    };
                }
                // The area's |N| is only smooth (a polynomial's square root, near zero by a
                // collapsed edge): 16 points a span, and one or two long spans cut in four (the GPU
                // kernel's `inner_job` does the same); more on a polynomial surface of degree 7 or
                // more, whose inertia integrands need ⌈5p/2⌉. (8 points a bicubic span, exact for
                // the rest, leave the area 1e-9 to 2e-4 off on SMLib's models: card 2's distortion
                // check, |N| varying 4 times over a span's points, misses those spans.)
                let n = if polynomial(ns) {
                    (5 * ns.du).div_ceil(2).max(16)
                } else {
                    16
                };
                Self {
                    knots,
                    cut: 4,
                    step: f64::INFINITY,
                    n: n.min(MAX_GAUSS),
                    breaks: bu,
                }
            }
            // Linear in u and v with a constant normal: polynomials of degree 3 at most.
            Surface::Plane { .. } => Self {
                knots: Vec::new(),
                cut: 1,
                step: f64::INFINITY,
                n: if ALL { 2 } else { 1 },
                breaks: Vec::new(),
            },
            // Round the axis (spheres, tori, surfaces of revolution).
            _ => Self {
                knots: Vec::new(),
                cut: 1,
                step: ANGLE_STEP,
                n: ANGLE_POINTS,
                breaks: Vec::new(),
            },
        }
    }

    /// The basis of B-spline surface `ns` at this plan's points on each whole span of u that
    /// `jobs` (`[u0, u, v, w]`) reach: most of an inner integral's points on a surface of many
    /// spans stand on those, at the same places whatever the job. None where the jobs are too
    /// few or reach too few spans to earn it.
    fn table(&self, ns: &NurbsSurface, jobs: &[[f64; 4]]) -> Option<super::nurbs::SpanRule> {
        if jobs.len() < 4 {
            return None;
        }
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for j in jobs {
            lo = lo.min(j[0]).min(j[1]);
            hi = hi.max(j[0]).max(j[1]);
        }
        let i0 = self.breaks.partition_point(|b| *b < lo);
        let i1 = self.breaks.partition_point(|b| *b <= hi);
        // (Two spans or more: one or two are cut further, at places of the job's own.)
        if i1 < i0 + 3 {
            return None;
        }
        ns.span_rule(&self.breaks[i0..i1], gauss(self.n).x)
    }

    /// The pieces between `a` and `b` (either way round), into `out`.
    fn pieces(&self, a: f64, b: f64, out: &mut Vec<f64>) {
        out.clear();
        out.push(a);
        if self.step.is_finite() {
            let n = ((b - a).abs() / self.step).ceil().max(1.0) as usize;
            for i in 1..n {
                out.push(a + (b - a) * i as f64 / n as f64);
            }
            out.push(b);
            return;
        }
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let i0 = self.knots.partition_point(|k| *k <= lo + 1e-14);
        let i1 = self.knots.partition_point(|k| *k < hi - 1e-14);
        let inside = &self.knots[i0..i1.max(i0)];
        // One or two spans of a rational surface: cut each (|N| varies within a long span).
        let cut = if inside.len() <= 1 { self.cut } else { 1 };
        let mut prev = a;
        let mut push = |x: f64, out: &mut Vec<f64>| {
            for i in 1..=cut {
                out.push(prev + (x - prev) * i as f64 / cut as f64);
            }
            prev = x;
        };
        if a < b {
            for &k in inside {
                push(k, out);
            }
        } else {
            for &k in inside.iter().rev() {
                push(k, out);
            }
        }
        push(b, out);
    }
}

/// H(u, v) for all quantities, weighted by `w` (the outer quadrature weight × v'(t)).
#[allow(clippy::too_many_arguments)]
fn inner<const ALL: bool>(
    s: &Surface,
    plan: &Inner,
    u0: f64,
    u: f64,
    v: f64,
    shift: &V3,
    w: f64,
    acc: &mut QE,
    scratch: &mut Vec<f64>,
    table: Option<&super::nurbs::SpanRule>,
) {
    if (u - u0).abs() < tol::TINY {
        return;
    }
    if matches!(
        s,
        Surface::Sphere { .. } | Surface::Torus { .. } | Surface::Revolution { .. }
    ) {
        turned_inner::<ALL>(s, u0, u, v, shift, w, acc);
        return;
    }
    plan.pieces(u0, u, scratch);
    let g = gauss(plan.n);
    // A B-spline surface along u at this v: its control net contracted along v once, then a
    // curve's evaluation per point.
    let iso = if let Surface::Nurbs(ns) = s {
        Some(ns.iso_v(v, u0, u))
    } else {
        None
    };
    let eval = |t: f64| match &iso {
        Some(iso) => iso.d1(t),
        None => s.d1(t, v),
    };
    for k in 0..scratch.len() - 1 {
        let (lo, hi) = (scratch[k], scratch[k + 1]);
        let (m, r) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
        // (A whole span the table holds: its points' bases from there, the same to the bit.)
        let held = table.and_then(|t| Some((t, t.span(lo, hi)?)));
        for (i, (x, gw)) in g.x.iter().zip(g.w).enumerate() {
            let (p, su, sv) = held
                .zip(iso.as_ref())
                .and_then(|((t, span), iso)| iso.d1_on(t, span, i))
                .unwrap_or_else(|| eval(m + r * x));
            accumulate::<ALL>(&p, &su, &sv, shift, acc, w * gw * r);
        }
    }
}

/// [`inner`] on a surface turned about an axis, u its turn (a sphere, a torus, a revolved
/// meridian): at fixed v the point and the normal are each of degree 1 in (cos u, sin u) and |N|
/// is constant, so every quantity's integrand is a trigonometric polynomial in u of degree 4 at
/// most (the inertia's products of four; the volume's 2). Its coefficients come exactly from 11
/// samples round the turn (7 for the volume), and the integral over [u0, u] in closed form: 11
/// evaluations where the Gauss rule took 8 or 16 on each eighth of a turn.
fn turned_inner<const ALL: bool>(
    s: &Surface,
    u0: f64,
    u: f64,
    v: f64,
    shift: &V3,
    w: f64,
    acc: &mut QE,
) {
    // The meridian at v in the frame's own coordinates, and its derivative along v: the surface
    // there is it turned by u about the frame's z.
    let (f, l, dl) = match s {
        Surface::Sphere { f, r } => {
            let (sv, cv) = v.sin_cos();
            (
                f,
                V3::new(r * cv, 0.0, r * sv),
                V3::new(-r * sv, 0.0, r * cv),
            )
        }
        Surface::Torus { f, rmaj, rmin } => {
            let (sv, cv) = v.sin_cos();
            (
                f,
                V3::new(rmaj + rmin * cv, 0.0, rmin * sv),
                V3::new(-rmin * sv, 0.0, rmin * cv),
            )
        }
        Surface::Revolution { f, m } => {
            let (p, d1, _) = m.d2(v);
            (
                f,
                f.local(&p),
                V3::new(d1.dot(&f.x), d1.dot(&f.y), d1.dot(&f.z)),
            )
        }
        _ => unreachable!("a surface turned about an axis"),
    };
    let table = if ALL { &*TURNS_11 } else { &*TURNS_7 };
    let m = table.len();
    let deg = (m - 1) / 2;
    // ∫ cos l t and ∫ sin l t over [u0, u], the differences as products (short spans keep
    // their digits).
    let (mid, half) = (0.5 * (u + u0), 0.5 * (u - u0));
    let mut ic = [0.0; 6];
    let mut is = [0.0; 6];
    ic[0] = u - u0;
    for l in 1..=deg {
        let lf = l as f64;
        let k = 2.0 * (lf * half).sin() / lf;
        let (sm, cm) = (lf * mid).sin_cos();
        ic[l] = cm * k;
        is[l] = sm * k;
    }
    for row in table {
        let mut wj = ic[0];
        for l in 1..=deg {
            let (cl, sl) = row[l];
            wj += 2.0 * (cl * ic[l] + sl * is[l]);
        }
        let (c, sn) = row[1];
        let rot = |q: &V3| V3::new(q.x * c - q.y * sn, q.x * sn + q.y * c, q.z);
        let drot = |q: &V3| V3::new(-q.x * sn - q.y * c, q.x * c - q.y * sn, 0.0);
        let p = f.world_point(&rot(&l));
        let (su, sv) = (f.world_vec(&drot(&l)), f.world_vec(&rot(&dl)));
        accumulate::<ALL>(&p, &su, &sv, shift, acc, w * wj / m as f64);
    }
}

/// The sample turns round the circle for [`turned_inner`], `m` of them: each one's cosine and
/// sine of l times it, l from 0 to (m − 1) / 2.
fn turns(m: usize) -> Vec<[(f64, f64); 6]> {
    (0..m)
        .map(|j| {
            let t = TAU * j as f64 / m as f64;
            std::array::from_fn(|l| {
                let (s, c) = (l as f64 * t).sin_cos();
                (c, s)
            })
        })
        .collect()
}

static TURNS_11: std::sync::LazyLock<Vec<[(f64, f64); 6]>> = std::sync::LazyLock::new(|| turns(11));
static TURNS_7: std::sync::LazyLock<Vec<[(f64, f64); 6]>> = std::sync::LazyLock::new(|| turns(7));

/// Surfaces whose integrand is a low-degree polynomial along v (ruled along v): their inner
/// integral runs along v, exactly with a few points.
fn ruled_in_v(s: &Surface) -> bool {
    matches!(
        s,
        Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Extrusion { .. }
            | Surface::Plane { .. }
    )
}

/// G(u, v) = ∫_{v0}^{v} h(u, s) ds for all quantities, weighted by `w`. Along the rulings x is
/// linear in v and N constant (a cylinder's, an extrusion's) or linear (a cone's): the inertia
/// integrands are cubics or quartics, the volume's linear or quadratic (exact with 2 or 3, 1 or 2
/// points). A cone's |N| has a kink at the apex: the pieces either side of it apart.
fn inner_v<const ALL: bool>(
    s: &Surface,
    u: f64,
    v0: f64,
    v: f64,
    shift: &V3,
    w: f64,
    acc: &mut QE,
) {
    if (v - v0).abs() < tol::TINY {
        return;
    }
    let (n, apex) = match s {
        Surface::Cone { r, a, .. } => (
            if ALL { 3 } else { 2 },
            (a.sin().abs() > tol::ANGULAR_FINE).then(|| -r / a.sin()),
        ),
        _ => (if ALL { 2 } else { 1 }, None),
    };
    let g = gauss(n);
    let mut run = |a: f64, b: f64| {
        let (m, r) = ((a + b) * 0.5, (b - a) * 0.5);
        for (x, gw) in g.x.iter().zip(g.w) {
            integrand::<ALL>(s, u, m + r * x, shift, acc, w * gw * r);
        }
    };
    match apex.filter(|va| (va - v0) * (va - v) < 0.0) {
        Some(va) => {
            run(v0, va);
            run(va, v);
        }
        None => run(v0, v),
    }
}

/// The outer rule along a p-curve: how finely its pieces are cut further (`extra` pieces per
/// piece over a turn of `step`, none when infinite) and the points per piece (None: by the
/// number of pieces, for p-curves the integrand is only smooth along).
struct Outer {
    step: f64,
    n: Option<usize>,
}

/// [`Outer`] for p-curve `pc` on surface `s`, the inner integral along u (`along_v` false) or v.
fn outer_rule<const ALL: bool>(s: &Surface, pc: &Curve, along_v: bool) -> Outer {
    let old = || {
        let periodic =
            s.u_periodic() || (!along_v && (s.v_periodic() || matches!(s, Surface::Sphere { .. })));
        Outer {
            step: if periodic { PI / 8.0 } else { f64::INFINITY },
            n: None,
        }
    };
    let poly_pc = |c: &Curve| match c {
        Curve::Nurbs(n) if !n.is_rational() => Some(n.degree),
        _ => None,
    };
    match (s, pc) {
        // Round a cylinder's or cone's axis along a line in (u, v): a trigonometric polynomial
        // times a polynomial of low degree in the line's parameter.
        (Surface::Cylinder { .. } | Surface::Cone { .. }, Curve::Line { .. }) if along_v => Outer {
            step: ANGLE_STEP,
            n: Some(ANGLE_POINTS),
        },
        // A plane: H is a polynomial of degree 4 (the volume's 2) in (u, v), so along a p-curve of
        // degree r the integrand is one of degree 5r − 1 (3r − 1); round a circle, trigonometric.
        (Surface::Plane { .. }, Curve::Line { .. }) => Outer {
            step: f64::INFINITY,
            n: Some(if ALL { 3 } else { 2 }),
        },
        (Surface::Plane { .. }, Curve::Circle { .. } | Curve::Ellipse { .. }) => Outer {
            step: f64::INFINITY,
            n: Some(ANGLE_POINTS),
        },
        (Surface::Plane { .. }, _) if poly_pc(pc).is_some() => {
            let r = poly_pc(pc).unwrap_or(3);
            Outer {
                step: f64::INFINITY,
                n: Some(
                    if ALL {
                        (5 * r).div_ceil(2)
                    } else {
                        (3 * r).div_ceil(2)
                    }
                    .min(16),
                ),
            }
        }
        // Along a sphere's or torus's meridian (u fixed): a trigonometric polynomial in v.
        (Surface::Sphere { .. } | Surface::Torus { .. }, Curve::Line { d, .. })
            if !along_v && d.x == 0.0 =>
        {
            Outer {
                step: ANGLE_STEP,
                n: Some(ANGLE_POINTS),
            }
        }
        // A line across a polynomial B-spline surface's knot cells: the volume's H has degree
        // (3p − 1, 3q − 2) there.
        (Surface::Nurbs(ns), Curve::Line { .. }) if !ALL && polynomial(ns) => Outer {
            step: f64::INFINITY,
            n: Some(
                (3 * ns.du + 3 * ns.dv)
                    .saturating_sub(2)
                    .div_ceil(2)
                    .clamp(1, 16),
            ),
        },
        _ => old(),
    }
}

/// `ts` with each piece of p-curve `pc` cut into pieces sweeping at most `step` in u (and in v
/// with `v_too`), by the sweep between its ends (a long p-curve's many short pieces stay whole).
fn cut_pieces(pc: &Curve, ts: Vec<f64>, step: f64, v_too: bool) -> Vec<f64> {
    if !step.is_finite() {
        return ts;
    }
    let mut r = Vec::with_capacity(ts.len());
    r.push(ts[0]);
    let mut pa = pc.point(ts[0]);
    for w in ts.windows(2) {
        let pb = pc.point(w[1]);
        let sweep = (pb.x - pa.x)
            .abs()
            .max(if v_too { (pb.y - pa.y).abs() } else { 0.0 });
        let extra = (sweep / step).ceil().clamp(1.0, MAX_PIECES) as usize;
        for i in 1..extra {
            r.push(w[0] + (w[1] - w[0]) * i as f64 / extra as f64);
        }
        r.push(w[1]);
        pa = pb;
    }
    r
}

/// `ts` with each piece cut into `n` equal pieces.
fn cut_each(ts: Vec<f64>, n: usize) -> Vec<f64> {
    let mut r = Vec::with_capacity((ts.len() - 1) * n + 1);
    r.push(ts[0]);
    for w in ts.windows(2) {
        for i in 1..=n {
            r.push(w[0] + (w[1] - w[0]) * i as f64 / n as f64);
        }
    }
    r
}

/// The points per piece along a p-curve the integrand is only smooth along: 16, or fewer on one
/// of hundreds of short pieces (an intersection curve's, a fitted one's: four points a piece are
/// plenty; for the volume alone, which booleans and imports weigh solids by, from 64 pieces).
fn smooth_rule<const ALL: bool>(pc: &Curve, pieces: usize) -> usize {
    let fine = matches!(pc, Curve::Nurbs(_)) && pieces > 64;
    if fine && (pieces > 512 || !ALL) {
        4
    } else if fine {
        8
    } else {
        16
    }
}

/// Face integrals with the inner integral along v: ∫∫ h = −∮ G du.
fn face_integrals_v<const ALL: bool>(body: &Body, f: &Face, shift: &V3) -> QE {
    let mut acc = [0.0; NE];
    let mut v_ref = f64::INFINITY;
    for l in &f.loops {
        for c in l {
            let e = &body.edges[c.edge];
            v_ref = v_ref.min(c.pc.point(e.t[0]).y).min(c.pc.point(e.t[1]).y);
        }
    }
    if !v_ref.is_finite() {
        return acc;
    }
    for l in &f.loops {
        for c in l {
            let e = &body.edges[c.edge];
            let (t0, t1) = co_range(e, c.fwd);
            let (lo, hi) = (t0.min(t1), t0.max(t1));
            let mut ts = c.pc.pieces(lo, hi);
            let rule = outer_rule::<ALL>(&f.surf, &c.pc, true);
            // Periodic: pieces of at most a quarter or an eighth turn; a general extrusion's curve
            // pieces in four (|N| varies along them).
            if rule.step.is_finite() {
                ts = cut_pieces(&c.pc, ts, rule.step, false);
            } else if matches!(f.surf, Surface::Extrusion { .. })
                && !matches!(c.pc, Curve::Nurbs(_))
            {
                ts = cut_each(ts, 4);
            }
            // And where the p-curve crosses the surface's knots (the integrand is only piecewise
            // polynomial there: a straight p-curve across a loft's many spans), or an extruded
            // B-spline's (along u: |N| has a kink at each).
            match &f.surf {
                Surface::Nurbs(ns) => ts = split_at_knots(&c.pc, &ts, &ns.ku, &ns.kv),
                Surface::Extrusion { c: ec, .. } => {
                    ts = split_at_knots(&c.pc, &ts, &curve_knots(ec), NO_KNOTS)
                }
                _ => {}
            }
            if t0 > t1 {
                ts.reverse();
            }
            let g = gauss(
                rule.n
                    .unwrap_or_else(|| smooth_rule::<ALL>(&c.pc, ts.len())),
            );
            for w in ts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (m, r) = ((a + b) * 0.5, (b - a) * 0.5);
                for (x, gw) in g.x.iter().zip(g.w) {
                    let t = m + r * x;
                    let (uv, duv, _) = c.pc.d2(t);
                    if duv.x == 0.0 {
                        continue;
                    }
                    inner_v::<ALL>(&f.surf, uv.x, v_ref, uv.y, shift, -gw * r * duv.x, &mut acc);
                }
            }
        }
    }
    if !f.sense {
        for q in acc.iter_mut().take(4) {
            *q = -*q;
        }
    }
    acc
}

/// The uv rectangle of a face bounded by one loop of iso-lines (an untrimmed patch: lofts,
/// sweeps), with the loop's orientation (+1 counter-clockwise in uv).
fn iso_rectangle(body: &Body, f: &Face) -> Option<([f64; 4], f64)> {
    let [l] = f.loops.as_slice() else { return None };
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    let mut twice = 0.0;
    for c in l {
        let e = &body.edges[c.edge];
        let (t0, t1) = co_range(e, c.fwd);
        let (a, b) = (c.pc.point(t0), c.pc.point(t1));
        match &c.pc {
            Curve::Line { d, .. } if d.x == 0.0 || d.y == 0.0 => {}
            Curve::Nurbs(_) => {
                // A fitted iso-line.
                let tol = tol::REL_FINE * (1.0 + a.x.abs().max(a.y.abs()));
                let (same_u, same_v) = ((a.x - b.x).abs() < tol, (a.y - b.y).abs() < tol);
                if same_u == same_v {
                    return None;
                }
                for k in 1..8 {
                    let q = c.pc.point(t0 + (t1 - t0) * k as f64 / 8.0);
                    if (same_u && (q.x - a.x).abs() > tol) || (same_v && (q.y - a.y).abs() > tol) {
                        return None;
                    }
                }
            }
            _ => return None,
        }
        for p in [a, b] {
            u0 = u0.min(p.x);
            u1 = u1.max(p.x);
            v0 = v0.min(p.y);
            v1 = v1.max(p.y);
        }
        twice += a.x * b.y - b.x * a.y;
    }
    let area = (u1 - u0) * (v1 - v0);
    if !(area > 0.0) || ((twice.abs() * 0.5) - area).abs() > tol::REL * area {
        return None;
    }
    Some(([u0, u1, v0, v1], twice.signum()))
}

/// The face integrals over an iso-rectangle of a B-spline surface: a tensor Gauss rule on each
/// knot span (the integrand is polynomial along polynomial directions, smooth along rational
/// ones).
fn rect_integrals<const ALL: bool>(s: &Surface, ns: &NurbsSurface, r: [f64; 4], shift: &V3) -> QE {
    let mut acc = [0.0; NE];
    let (_, bv) = ns.breaks();
    let mut vs = vec![r[2]];
    vs.extend(
        bv.into_iter()
            .filter(|k| *k > r[2] + 1e-14 && *k < r[3] - 1e-14),
    );
    vs.push(r[3]);
    // Along v a polynomial surface's volume integrand has degree 3q − 2 a span (exact with
    // ⌈(3q − 1)/2⌉ points). The area's |N| is only smooth: 16 points a span (one or two spans cut
    // in four; across many short spans 8), as for a rational surface's.
    let g = if !ALL && polynomial(ns) {
        gauss(
            (3 * ns.dv)
                .saturating_sub(1)
                .div_ceil(2)
                .clamp(1, MAX_GAUSS),
        )
    } else {
        if vs.len() <= 3 {
            let mut out = vec![r[2]];
            for w in vs.windows(2) {
                for i in 1..=4 {
                    out.push(w[0] + (w[1] - w[0]) * i as f64 / 4.0);
                }
            }
            vs = out;
        }
        gauss(if vs.len() > 17 { 8 } else { 16 })
    };
    // Along v the points and weights; along u the inner integral (as Green's theorem's jobs, so
    // a GPU can take them).
    let mut jobs: Vec<[f64; 4]> = Vec::with_capacity((vs.len() - 1) * g.x.len());
    for wv in vs.windows(2) {
        let (mv, rv) = ((wv[0] + wv[1]) * 0.5, (wv[1] - wv[0]) * 0.5);
        for (y, hw) in g.x.iter().zip(g.w) {
            jobs.push([r[0], r[1], mv + rv * y, hw * rv]);
        }
    }
    let mut scratch = Vec::with_capacity(64);
    run_jobs::<ALL>(
        s,
        &Inner::of::<ALL>(s),
        &jobs,
        shift,
        &mut acc,
        &mut scratch,
    );
    acc
}

/// Whether a B-spline surface is a smooth fit of something else: quintic or more both ways
/// (four times differentiable across its knots) and of many spans (256). Its faces are
/// integrated strictly inside them ([`scan_integrals`]).
fn smooth_fit(ns: &NurbsSurface) -> bool {
    let spans = |k: &[f64]| k.windows(2).filter(|w| w[1] > w[0]).count();
    ns.du >= 5 && ns.dv >= 5 && spans(&ns.ku) * spans(&ns.kv) >= 256
}

/// A stretch of a face's boundary in (u, v) along which v only rises or only falls.
struct Arc<'a> {
    pc: &'a Curve,
    /// Its ends' parameters, the lower in v first, and their v.
    t: [f64; 2],
    v: [f64; 2],
    /// Whether the boundary runs up it (v rising the way the loop goes).
    up: bool,
}

impl Arc<'_> {
    /// The parameter where the arc is at height `v` (between its ends'), and the u there: by
    /// bisection helped by Newton's steps, from `from` (where the last height asked for was
    /// found, or NaN).
    fn at(&self, v: f64, from: f64) -> (f64, f64) {
        // (v rises from `lo` to `hi`, whichever way the parameter runs.)
        let (mut lo, mut hi) = (self.t[0], self.t[1]);
        let mut t = if from.is_finite() {
            from
        } else {
            lo + (hi - lo) * (v - self.v[0]) / (self.v[1] - self.v[0])
        };
        for _ in 0..80 {
            let (p, d, _) = self.pc.d2(t);
            let miss = p.y - v;
            if miss.abs() <= 1e-15 * (1.0 + v.abs()) {
                return (t, p.x);
            }
            if miss > 0.0 {
                hi = t;
            } else {
                lo = t;
            }
            let next = t - miss / d.y;
            t = if d.y != 0.0 && (next - lo) * (next - hi) < 0.0 {
                next
            } else {
                0.5 * (lo + hi)
            };
            if (hi - lo).abs() <= 1e-15 * (1.0 + lo.abs().max(hi.abs())) {
                break;
            }
        }
        (t, self.pc.point(t).x)
    }
}

/// The face integrals of a trimmed face of a smooth fit ([`smooth_fit`]) taken strictly inside
/// it: along each of a rule's lines of constant v, over the stretches of u the face holds
/// there. (Green's theorem as [`face_integrals_of`] uses it carries every inner integral from
/// one reference u to the boundary, across whatever of the surface lies between: on a fit that
/// holds only where its face is, a swept sheet's about a hole cut out of it, that was a wrong
/// area. And a line for each point of the boundary's rule is many more lines than a face of
/// hundreds of spans needs.) The lines are laid in strips of v between the boundary's corners,
/// its turning points in v and the surface's knots in v: within one the face's stretches
/// change smoothly with v, but for the square root they close or open by at a turning point,
/// which the rule there is laid out to take (v by the square of its own measure). (A fitted
/// p-curve's own knots are not strip ends: what its fit wavers by between them, a
/// hundred-millionth of such a face's area, the rule smooths over.) None where the boundary
/// isn't made out so (the caller's way then).
fn scan_integrals<const ALL: bool>(
    body: &Body,
    f: &Face,
    ns: &NurbsSurface,
    shift: &V3,
) -> Option<QE> {
    let mut arcs: Vec<Arc> = Vec::new();
    let (_, bv) = ns.breaks();
    let (v_lo, v_hi) = (bv[0], bv[bv.len() - 1]);
    let mut events: Vec<f64> = bv;
    let mut turns: Vec<f64> = Vec::new();
    for l in &f.loops {
        for c in l {
            let e = &body.edges[c.edge];
            let (t0, t1) = co_range(e, c.fwd);
            let (lo, hi) = (t0.min(t1), t0.max(t1));
            // The parameters where v turns: within each smooth piece, where its rate in v
            // changes sign between samples.
            let mut cuts = vec![lo];
            for w in c.pc.pieces(lo, hi).windows(2) {
                const SAMPLES: usize = 4;
                let rate = |t: f64| c.pc.d2(t).1.y;
                let mut prev = (w[0], rate(w[0]));
                for i in 1..=SAMPLES {
                    let t = w[0] + (w[1] - w[0]) * i as f64 / SAMPLES as f64;
                    let r = rate(t);
                    if r * prev.1 < 0.0 {
                        let (mut a, mut b) = (prev.0, t);
                        for _ in 0..60 {
                            let m = 0.5 * (a + b);
                            if rate(m) * prev.1 > 0.0 {
                                a = m;
                            } else {
                                b = m;
                            }
                        }
                        let t = 0.5 * (a + b);
                        turns.push(c.pc.point(t).y);
                        cuts.push(t);
                    }
                    if r != 0.0 {
                        prev = (t, r);
                    }
                }
            }
            cuts.push(hi);
            for w in cuts.windows(2) {
                let (va, vb) = (c.pc.point(w[0]).y, c.pc.point(w[1]).y);
                if !(va.is_finite() && vb.is_finite()) {
                    return None;
                }
                events.push(va);
                events.push(vb);
                if va == vb {
                    continue;
                }
                // (An end the boundary comes to all but level, a corner at a turning point:
                // taken as one.)
                for (t, v) in [(w[0], va), (w[1], vb)] {
                    let d = c.pc.d2(t).1;
                    if d.y.abs() <= 0.5 * d.x.abs() {
                        turns.push(v);
                    }
                }
                let rising = vb > va;
                arcs.push(Arc {
                    pc: &c.pc,
                    t: if rising { [w[0], w[1]] } else { [w[1], w[0]] },
                    v: [va.min(vb), va.max(vb)],
                    up: rising == (t1 > t0),
                });
            }
        }
    }
    if arcs.is_empty() {
        return None;
    }
    events.sort_by(f64::total_cmp);
    events.dedup();
    let (b_lo, b_hi) = arcs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), arc| {
            (a.min(arc.v[0]), z.max(arc.v[1]))
        });
    // (A boundary past the surface's own range in v, a closed surface's face over its seam:
    // not laid out here.)
    let slack = 1e-9 * (v_hi - v_lo);
    if b_lo < v_lo - slack || b_hi > v_hi + slack {
        return None;
    }
    // The strips, each with whether the boundary turns at its bottom and at its top; one it
    // turns at both ends of in two halves.
    let turn_at = |v: f64| turns.contains(&v);
    let mut strips: Vec<(f64, f64, bool, bool)> = Vec::new();
    for w in events.windows(2) {
        let (va, vb) = (w[0], w[1]);
        if va < b_lo || vb > b_hi || vb - va <= 1e-12 * (1.0 + va.abs().max(vb.abs())) {
            continue;
        }
        match (turn_at(va), turn_at(vb)) {
            (true, true) => {
                let m = 0.5 * (va + vb);
                strips.push((va, m, true, false));
                strips.push((m, vb, false, true));
            }
            (a, b) => strips.push((va, vb, a, b)),
        }
    }
    // (As over a rectangle of the surface's parameters: sixteen points a strip, eight across
    // many short ones.)
    let g = gauss(if strips.len() > 16 { 8 } else { 16 });
    let mut jobs: Vec<[f64; 4]> = Vec::new();
    // (Strips where the boundary's pieces don't quite meet, a stretch short: how much of the
    // height they leave out.)
    let mut left_out = 0.0;
    let mut from: Vec<f64> = vec![f64::NAN; arcs.len()];
    let mut crossings: Vec<(f64, bool)> = Vec::new();
    for &(va, vb, turn_a, turn_b) in &strips {
        let mid = 0.5 * (va + vb);
        let active: Vec<usize> = (0..arcs.len())
            .filter(|&i| arcs[i].v[0] < mid && arcs[i].v[1] > mid)
            .collect();
        let whole = active
            .iter()
            .all(|&i| arcs[i].v[0] <= va && arcs[i].v[1] >= vb);
        if !whole || active.len() % 2 == 1 {
            left_out += vb - va;
            continue;
        }
        for &i in &active {
            from[i] = f64::NAN;
        }
        let h = vb - va;
        for (x, gw) in g.x.iter().zip(g.w) {
            // (s from 0 to 1 up the strip; where the boundary turns at an end, v by the
            // square of how far s is from it.)
            let s = 0.5 * (1.0 + x);
            let (v, w) = if turn_a {
                (va + h * s * s, gw * h * s)
            } else if turn_b {
                (vb - h * (1.0 - s) * (1.0 - s), gw * h * (1.0 - s))
            } else {
                (va + h * s, 0.5 * gw * h)
            };
            crossings.clear();
            for &i in &active {
                let (t, u) = arcs[i].at(v, from[i]);
                from[i] = t;
                crossings.push((u, arcs[i].up));
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in crossings.chunks(2) {
                // (The face lies between a stretch of boundary going one way and one going
                // the other; its side says which way round the loops run, as Green's sum
                // would.)
                if pair[0].1 == pair[1].1 {
                    return None;
                }
                let sign = if pair[1].1 { 1.0 } else { -1.0 };
                jobs.push([pair[0].0, pair[1].0, v, sign * w]);
            }
        }
    }
    if left_out > 1e-7 * (b_hi - b_lo) {
        return None;
    }
    let mut acc = [0.0; NE];
    let mut scratch = Vec::with_capacity(64);
    let surf = &f.surf;
    run_jobs::<ALL>(
        surf,
        &Inner::of::<ALL>(surf),
        &jobs,
        shift,
        &mut acc,
        &mut scratch,
    );
    if !f.sense {
        for q in acc.iter_mut().take(4) {
            *q = -*q;
        }
    }
    Some(acc)
}

/// A B-spline face as the same face of its surface with u and v swapped (its p-curves with
/// them, and its sense, as the natural normal turns over), where the surface has four times
/// as many spans along u as along v, or more (a screw's long faces); else None.
fn transposed(f: &Face) -> Option<Face> {
    let Surface::Nurbs(ns) = &f.surf else {
        return None;
    };
    let spans = |k: &[f64]| k.windows(2).filter(|w| w[1] > w[0]).count();
    let (su, sv) = (spans(&ns.ku), spans(&ns.kv));
    if su < 8 || su < 4 * sv {
        return None;
    }
    let (nu, nv) = (ns.cps.len(), ns.cps.first()?.len());
    let swapped = super::nurbs::NurbsSurface {
        du: ns.dv,
        dv: ns.du,
        ku: ns.kv.clone(),
        kv: ns.ku.clone(),
        cps: (0..nv)
            .map(|j| (0..nu).map(|i| ns.cps[i][j]).collect())
            .collect(),
    };
    let swap = Affine {
        m: nalgebra::Matrix3::new(0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0),
        t: V3::zeros(),
    };
    Some(Face {
        surf: Surface::Nurbs(std::sync::Arc::new(swapped)),
        sense: !f.sense,
        loops: f
            .loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|c| super::topo::Coedge {
                        edge: c.edge,
                        fwd: c.fwd,
                        pc: c.pc.transformed(&swap),
                    })
                    .collect()
            })
            .collect(),
    })
}

/// The 14 face integrals of a face, relative to `shift`, oriented by its sense.
pub fn face_integrals(body: &Body, f: &Face, shift: &V3) -> Q {
    let q = face_integrals_of::<true>(body, f, shift);
    std::array::from_fn(|k| q[k])
}

/// [`face_integrals`] with [`NE`]'s moments, or with `ALL` false only the volume's (`[4]`; the
/// others zero).
pub(crate) fn face_integrals_of<const ALL: bool>(body: &Body, f: &Face, shift: &V3) -> QE {
    if ruled_in_v(&f.surf) && !matches!(f.surf, Surface::Plane { .. }) {
        return face_integrals_v::<ALL>(body, f, shift);
    }
    if let Surface::Nurbs(ns) = &f.surf
        && ns.du <= 3
        && ns.dv <= 3
        && let Some((r, orient)) = iso_rectangle(body, f)
    {
        let mut acc = rect_integrals::<ALL>(&f.surf, ns, r, shift);
        for q in acc.iter_mut() {
            *q *= orient;
        }
        if !f.sense {
            for q in acc.iter_mut().take(4) {
                *q = -*q;
            }
        }
        return acc;
    }
    // A smooth fit, which holds only where its face is: taken strictly inside the face.
    if let Surface::Nurbs(ns) = &f.surf
        && smooth_fit(ns)
        && let Some(acc) = scan_integrals::<ALL>(body, f, ns, shift)
    {
        return acc;
    }
    // A B-spline with four times as many spans along u as along v: its parameters swapped,
    // so that the inner integral (taken along u, across every span from the reference to each
    // point of the boundary) crosses the few.
    if let Some(t) = transposed(f) {
        return face_integrals_of::<ALL>(body, &t, shift);
    }
    // A surface of revolution whose face is a rectangle of its parameters (a revolved
    // section's side): its exact NURBS over the rectangle, by the tensor rule (each point along a
    // boundary otherwise turned its meridian's B-spline round for every inner point).
    if let Surface::Revolution { .. } = &f.surf
        && let Some((r, orient)) = iso_rectangle(body, f)
    {
        let ns = f.surf.to_nurbs((r[0], r[1]), (r[2], r[3]));
        let ((u0, u1), (v0, v1)) = ns.range();
        let surf = Surface::Nurbs(std::sync::Arc::new(ns.clone()));
        let mut acc = rect_integrals::<ALL>(&surf, &ns, [u0, u1, v0, v1], shift);
        for q in acc.iter_mut() {
            *q *= orient;
        }
        if !f.sense {
            for q in acc.iter_mut().take(4) {
                *q = -*q;
            }
        }
        return acc;
    }
    let mut acc = [0.0; NE];
    let mut scratch = Vec::with_capacity(64);
    // The reference u: the smallest u on the boundary keeps H small.
    let mut u_ref = f64::INFINITY;
    for l in &f.loops {
        for c in l {
            let e = &body.edges[c.edge];
            u_ref = u_ref.min(c.pc.point(e.t[0]).x).min(c.pc.point(e.t[1]).x);
        }
    }
    if !u_ref.is_finite() {
        return acc;
    }
    let plan = Inner::of::<ALL>(&f.surf);
    let v_periodic = f.surf.v_periodic() || matches!(f.surf, Surface::Sphere { .. });
    // The inner integrals gathered as jobs (shared across the cores when there are many; a
    // B-spline face's run on the GPU when it is on).
    let mut jobs: Option<Vec<[f64; 4]>> = Some(Vec::new());
    for l in &f.loops {
        for c in l {
            let e = &body.edges[c.edge];
            let (t0, t1) = co_range(e, c.fwd);
            // Pieces along the p-curve.
            let (lo, hi) = (t0.min(t1), t0.max(t1));
            let mut ts = c.pc.pieces(lo, hi);
            // Refine where u or v sweep far on periodic surfaces.
            let rule = outer_rule::<ALL>(&f.surf, &c.pc, false);
            ts = cut_pieces(&c.pc, ts, rule.step, v_periodic);
            // And where the p-curve crosses the surface's knots (the integrand is only piecewise
            // polynomial there: a straight p-curve across a loft's many spans), or a revolved
            // B-spline's (along v).
            match &f.surf {
                Surface::Nurbs(ns) => ts = split_at_knots(&c.pc, &ts, &ns.ku, &ns.kv),
                Surface::Revolution { m, .. } => {
                    ts = split_at_knots(&c.pc, &ts, NO_KNOTS, &curve_knots(m))
                }
                _ => {}
            }
            if t0 > t1 {
                ts.reverse();
            }
            let g = gauss(
                rule.n
                    .unwrap_or_else(|| smooth_rule::<ALL>(&c.pc, ts.len())),
            );
            for w in ts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (m, r) = ((a + b) * 0.5, (b - a) * 0.5);
                for (x, gw) in g.x.iter().zip(g.w) {
                    let t = m + r * x;
                    let (uv, duv, _) = c.pc.d2(t);
                    let dv = duv.y;
                    if dv == 0.0 {
                        continue;
                    }
                    if let Some(jobs) = jobs.as_mut() {
                        jobs.push([u_ref, uv.x, uv.y, gw * r * dv]);
                    } else {
                        inner::<ALL>(
                            &f.surf,
                            &plan,
                            u_ref,
                            uv.x,
                            uv.y,
                            shift,
                            gw * r * dv,
                            &mut acc,
                            &mut scratch,
                            None,
                        );
                    }
                }
            }
        }
    }
    if let Some(jobs) = jobs {
        run_jobs::<ALL>(&f.surf, &plan, &jobs, shift, &mut acc, &mut scratch);
    }
    if !f.sense {
        for q in acc.iter_mut().take(4) {
            *q = -*q;
        }
    }
    acc
}

/// No knots (a direction a surface is smooth along), for [`split_at_knots`].
const NO_KNOTS: &[f64] = &[0.0, 0.0];

/// A curve's knot vector, or [`NO_KNOTS`] for an analytic one.
fn curve_knots(c: &Curve) -> Vec<f64> {
    match c {
        Curve::Nurbs(n) => n.knots.clone(),
        _ => NO_KNOTS.to_vec(),
    }
}

/// `ts` (increasing) with the parameters where the p-curve crosses an interior knot of `ku`
/// (in u) or `kv` (in v) added (by bisection; each piece taken as monotone).
fn split_at_knots(pc: &Curve, ts: &[f64], ku: &[f64], kv: &[f64]) -> Vec<f64> {
    let inner = |k: &[f64]| -> Vec<f64> {
        let (a, b) = (k[0], k[k.len() - 1]);
        let mut out: Vec<f64> = k.iter().copied().filter(|&x| x > a && x < b).collect();
        out.dedup();
        out
    };
    let (iu, iv) = (inner(ku), inner(kv));
    let mut out = vec![ts[0]];
    for w in ts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (pa, pb) = (pc.point(a), pc.point(b));
        let mut cuts: Vec<f64> = Vec::new();
        for (knots, coord) in [(&iu, 0usize), (&iv, 1usize)] {
            let (ca, cb) = (pa[coord], pb[coord]);
            let (lo, hi) = (ca.min(cb), ca.max(cb));
            for &kn in knots
                .iter()
                .filter(|&&kn| kn > lo + tol::ROUNDOFF_FINE && kn < hi - tol::ROUNDOFF_FINE)
            {
                let (mut x0, mut x1) = (a, b);
                for _ in 0..60 {
                    let m = 0.5 * (x0 + x1);
                    if (pc.point(m)[coord] - kn) * (cb - ca) < 0.0 {
                        x0 = m;
                    } else {
                        x1 = m;
                    }
                }
                cuts.push(0.5 * (x0 + x1));
            }
        }
        cuts.sort_by(f64::total_cmp);
        for t in cuts {
            if t - out.last().unwrap() > tol::REL_FINE * (1.0 + t.abs())
                && b - t > tol::REL_FINE * (1.0 + t.abs())
            {
                out.push(t);
            }
        }
        out.push(b);
    }
    out
}

/// Jobs below this many stay on the CPU (a dispatch costs about a millisecond).
#[cfg(feature = "gpu")]
const GPU_MIN_JOBS: usize = 512;

/// Whether mass properties may use the GPU: off unless `CADRS_GPU=1` (or [`set_gpu`]). On
/// pi7zoll-gehäuse's B-spline faces its dispatches (a millisecond each, one queue for all the
/// rebuild's threads) lost to the CPU's cores: 7.7 s against 6.5 s for its Part Studio.
static GPU_ON: std::sync::LazyLock<std::sync::atomic::AtomicBool> =
    std::sync::LazyLock::new(|| {
        let on = std::env::var("CADRS_GPU").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("on"));
        std::sync::atomic::AtomicBool::new(on)
    });

/// Turns the GPU path on or off (for comparisons; off unless `CADRS_GPU=1`, and only there
/// when the kernel is built with the `gpu` feature and a GPU is there).
pub fn set_gpu(on: bool) {
    GPU_ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether mass properties may use the GPU now.
pub fn gpu_active() -> bool {
    #[cfg(feature = "gpu")]
    {
        GPU_ON.load(std::sync::atomic::Ordering::Relaxed) && cadrs_gpu::get().is_some()
    }
    #[cfg(not(feature = "gpu"))]
    {
        false
    }
}

/// The inner integrals `[u0, u, v, w]` of a face, summed into `acc`: on the GPU when the kernel
/// is built with it, one is there and there are enough jobs; else here.
fn run_jobs<const ALL: bool>(
    s: &Surface,
    plan: &Inner,
    jobs: &[[f64; 4]],
    shift: &V3,
    acc: &mut QE,
    scratch: &mut Vec<f64>,
) {
    #[cfg(feature = "gpu")]
    if jobs.len() >= GPU_MIN_JOBS
        && GPU_ON.load(std::sync::atomic::Ordering::Relaxed)
        && let (Surface::Nurbs(ns), Some(gpu)) = (s, cadrs_gpu::get())
        && ns.du <= 3
        && ns.dv <= 3
    {
        let (nu, nv) = (ns.cps.len(), ns.cps[0].len());
        let ctrl: Vec<f64> = ns
            .cps
            .iter()
            .flatten()
            .flat_map(|h| [h.x, h.y, h.z, h.w])
            .collect();
        let mut consts = vec![shift.x, shift.y, shift.z];
        consts.extend_from_slice(GAUSS16.x);
        consts.extend_from_slice(GAUSS16.w);
        let q = cadrs_gpu::QuadParams {
            nu: nu as u32,
            nv: nv as u32,
            du: ns.du as u32,
            dv: ns.dv as u32,
            nku: ns.ku.len() as u32,
            nkv: ns.kv.len() as u32,
            jobs: 0,
            _pad: 0,
        };
        for r in gpu.quad_inner(q, &ctrl, &ns.ku, &ns.kv, &consts, jobs) {
            for (a, x) in acc.iter_mut().zip(r) {
                *a += x;
            }
        }
        return;
    }
    // (Many in chunks across the cores, the chunks' sums added in order: the same sum however
    // they are shared out.)
    let table = match s {
        Surface::Nurbs(ns) => plan.table(ns, jobs),
        _ => None,
    };
    let table = table.as_ref();
    const CHUNK: usize = 32;
    if jobs.len() >= 2 * CHUNK {
        super::par::start();
        let parts: Vec<QE> = jobs
            .par_chunks(CHUNK)
            .map(|c| {
                let mut a = [0.0; NE];
                let mut sc = Vec::with_capacity(64);
                for j in c {
                    inner::<ALL>(
                        s, plan, j[0], j[1], j[2], shift, j[3], &mut a, &mut sc, table,
                    );
                }
                a
            })
            .collect();
        for p in parts {
            for (a, x) in acc.iter_mut().zip(p) {
                *a += x;
            }
        }
        return;
    }
    for j in jobs {
        inner::<ALL>(s, plan, j[0], j[1], j[2], shift, j[3], acc, scratch, table);
    }
}

/// [`body_mass`]'s volume (to rounding) for a fraction of the work: only the volume's integrand,
/// on the fewer points it needs.
/// (Booleans and offsets weigh solids by it to drop slivers and check orientation.)
pub fn body_volume(body: &Body) -> f64 {
    if !body.is_solid() {
        return 0.0;
    }
    faces_volume(body)
}

/// The volume a body's faces bound, roughly, for telling its sign (which way a closed shell
/// faces) or one shell's size from another's without integrating its surfaces: the midpoint
/// rule over a grid of each face's parameters, twice over (a coarse grid and one twice as
/// fine). None unless the two agree within a fifth, which a volume far from nothing among its
/// faces' terms does; the caller then integrates.
pub(crate) fn rough_volume(body: &Body) -> Option<f64> {
    use super::classify::{InFace, face_polygons, in_face};
    let bb = body.bbox();
    if bb.is_empty() {
        return None;
    }
    let shift = bb.center().coords;
    let one = |f: &Face| -> Option<[f64; 2]> {
        let polys = face_polygons(body, f);
        let (mut lo, mut hi) = (
            P2::new(f64::INFINITY, f64::INFINITY),
            P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for q in polys.iter().flatten() {
            lo = P2::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = P2::new(hi.x.max(q.x), hi.y.max(q.y));
        }
        if !(hi.x > lo.x && hi.y > lo.y) {
            return None;
        }
        let sign = if f.sense { 1.0 } else { -1.0 };
        let at = |n: usize| -> f64 {
            let (du, dv) = ((hi.x - lo.x) / n as f64, (hi.y - lo.y) / n as f64);
            let mut v = 0.0;
            for i in 0..n {
                for j in 0..n {
                    let uv = P2::new(lo.x + (i as f64 + 0.5) * du, lo.y + (j as f64 + 0.5) * dv);
                    if in_face(&polys, &f.surf, uv, 0.0) != InFace::Inside {
                        continue;
                    }
                    let (p, su, sv) = f.surf.d1(uv.x, uv.y);
                    v += (p.coords - shift).dot(&su.cross(&sv));
                }
            }
            sign * v * du * dv / 3.0
        };
        Some([at(12), at(24)])
    };
    let terms: Vec<Option<[f64; 2]>> = if super::par::threads_for(body.faces.len(), true) {
        body.faces.par_iter().map(one).collect()
    } else {
        body.faces.iter().map(one).collect()
    };
    let (mut coarse, mut fine) = (0.0, 0.0);
    for t in terms {
        let [c, f] = t?;
        coarse += c;
        fine += f;
    }
    (fine.is_finite() && (fine - coarse).abs() <= 0.2 * fine.abs()).then_some(fine)
}

/// A body's volume ([`body_volume`]) where only its sign or its rough size is wanted:
/// [`rough_volume`] where that settles it, else integrated.
pub(crate) fn volume_for_sign(body: &Body) -> f64 {
    // (As [`body_volume`]: nothing for a body with no solid made out yet.)
    if !body.is_solid() {
        return 0.0;
    }
    // (Only for B-splines of many spans, which the integral walks over span by span: the
    // analytic surfaces integrate along their boundaries alone, and a revolved or a small
    // B-spline face by a tensor rule, all quicker than they are sampled.)
    let long = |f: &Face| {
        let Surface::Nurbs(n) = &f.surf else {
            return false;
        };
        let spans = |k: &[f64]| k.windows(2).filter(|w| w[1] > w[0]).count();
        spans(&n.ku) * spans(&n.kv) >= 256
    };
    if !body.faces.iter().any(long) {
        return faces_volume(body);
    }
    rough_volume(body).unwrap_or_else(|| faces_volume(body))
}

/// The volume a body's faces bound, whether or not they are made up into shells and solids
/// yet (closed and facing out taken on trust).
pub(crate) fn faces_volume(body: &Body) -> f64 {
    let bb = body.bbox();
    let shift = if bb.is_empty() {
        V3::zeros()
    } else {
        bb.center().coords
    };
    faces_volume_terms(body, &shift).iter().sum()
}

/// What each face of a body adds to the volume its faces bound, about `shift` (their sum is
/// the volume, for closed faces facing out; a closed shell's own faces' sum is its own, whatever
/// the shift).
pub(crate) fn faces_volume_terms(body: &Body, shift: &V3) -> Vec<f64> {
    let faces: Vec<usize> = (0..body.faces.len()).collect();
    let one = |&i: &usize| face_integrals_of::<false>(body, &body.faces[i], shift)[4];
    let heavy = body.faces.iter().any(costly);
    if super::par::threads_for(faces.len(), heavy) {
        faces.par_iter().map(one).collect()
    } else {
        faces.iter().map(one).collect()
    }
}

/// Mass properties of a body (solids; a body without solids is a sheet: area only).
pub fn body_mass(body: &Body) -> crate::MassProperties {
    let bb = body.bbox();
    let shift = if bb.is_empty() {
        V3::zeros()
    } else {
        bb.center().coords
    };
    let faces: Vec<usize> = (0..body.faces.len()).collect();
    let one = |&i: &usize| {
        let f = &body.faces[i];
        if gpu_active() || !costly(f) {
            return face_integrals(body, f, &shift);
        }
        let (center, q) = face_integrals_about_center(body, i);
        shifted(&q, &center, &shift)
    };
    let heavy = body.faces.iter().any(costly);
    let qs: Vec<Q> = if super::par::threads_for(faces.len(), heavy) {
        faces.par_iter().map(one).collect()
    } else {
        faces.iter().map(one).collect()
    };
    let mut q = [0.0; NQ];
    for f in &qs {
        for k in 0..NQ {
            q[k] += f[k];
        }
    }
    let area = q[0];
    let (length, moment) = wire_length(body);
    let solid = body.is_solid();
    let vol = if solid { q[4] } else { 0.0 };
    let (center, inertia) = if solid && vol.abs() > tol::TINY {
        let c = V3::new(q[5], q[6], q[7]) / vol;
        let (xx, yy, zz) = (q[8], q[9], q[10]);
        let (xy, yz, zx) = (q[11], q[12], q[13]);
        // About the shift origin.
        let io = nalgebra::Matrix3::new(yy + zz, -xy, -zx, -xy, xx + zz, -yz, -zx, -yz, xx + yy);
        let ic = io - (nalgebra::Matrix3::identity() * c.norm_squared() - c * c.transpose()) * vol;
        (P3::from(c + shift), ic)
    } else if area.abs() > tol::TINY {
        (
            P3::from(V3::new(q[1], q[2], q[3]) / area + shift),
            nalgebra::Matrix3::zeros(),
        )
    } else if length > tol::TINY {
        (P3::from(moment / length), nalgebra::Matrix3::zeros())
    } else if !body.points.is_empty() {
        let sum = body
            .points
            .iter()
            .fold(V3::zeros(), |s, &v| s + body.verts[v].p.coords);
        (
            P3::from(sum / body.points.len() as f64),
            nalgebra::Matrix3::zeros(),
        )
    } else {
        (P3::from(shift), nalgebra::Matrix3::zeros())
    };
    crate::MassProperties {
        volume: vol,
        surface_area: area,
        center_of_mass: center,
        inertia,
        length,
    }
}

/// The total length of a body's wires and its first moment ∫ p ds (Gauss–Legendre on each
/// edge's smooth pieces; exact for lines).
pub fn wire_length(body: &Body) -> (f64, V3) {
    let mut length = 0.0;
    let mut moment = V3::zeros();
    for &(ei, _) in body.wires.iter().flat_map(|w| w.edges.iter()) {
        let e = &body.edges[ei];
        if e.degenerate {
            continue;
        }
        let (t0, t1) = (e.t[0].min(e.t[1]), e.t[0].max(e.t[1]));
        let n = if matches!(e.curve, Curve::Line { .. }) {
            1
        } else {
            8
        };
        for w in e.curve.pieces(t0, t1).windows(2) {
            length += integrate(w[0], w[1], n, &GAUSS16, |t| e.curve.tangent(t).norm());
            for k in 0..3 {
                moment[k] += integrate(w[0], w[1], n, &GAUSS16, |t| {
                    e.curve.point(t)[k] * e.curve.tangent(t).norm()
                });
            }
        }
    }
    (length, moment)
}

/// A face's area and area centroid.
pub fn face_area(body: &Body, fi: usize) -> (f64, P3) {
    let (shift, q) = face_integrals_about_center(body, fi);
    let a = q[0];
    let c = if a.abs() > tol::TINY {
        V3::new(q[1], q[2], q[3]) / a + shift
    } else {
        shift
    };
    (a, P3::from(c))
}

/// The middle of a face's box (its vertices and edge midpoints), which its integrals are taken
/// about so they move to any origin to rounding ([`shifted`]).
fn face_center(body: &Body, f: &Face) -> V3 {
    let mut bb = Bbox::empty();
    for l in &f.loops {
        for c in l {
            let e = &body.edges[c.edge];
            bb.add(&body.verts[e.v[0]].p);
            if !e.degenerate {
                bb.add(&e.point(e.mid_t()));
            }
        }
    }
    if bb.is_empty() {
        V3::zeros()
    } else {
        bb.center().coords
    }
}

/// Face `fi`'s integrals (with [`NE`]'s moments) and the point they are about: its
/// [`face_center`], or the centre a face of the same surface and boundary had when they were
/// taken ([`FaceCache`]): a feature leaves most of a body's faces as they were, and each body
/// it makes is weighed and its faces measured.
fn face_integrals_about_center(body: &Body, fi: usize) -> (V3, QE) {
    let f = &body.faces[fi];
    // (Not the GPU's integrals: its kernel takes [`Q`]'s quantities only.)
    let key = (costly(f) && !gpu_active()).then(|| face_key(body, f));
    if let Some(k) = key
        && let Some(hit) = FACE_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(k)
    {
        return hit;
    }
    let center = face_center(body, f);
    let q = face_integrals_of::<true>(body, f, &center);
    if let Some(k) = key {
        FACE_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(k, (center, q));
    }
    (center, q)
}

/// Whether a face's integrals are worth keeping ([`FACE_CACHE`]): a B-spline, swept or
/// revolved surface's or a boundary of B-spline p-curves' (many points each); an analytic face
/// bounded by lines and circles takes fewer points than its key's hash.
fn costly(f: &Face) -> bool {
    !matches!(
        f.surf,
        Surface::Plane { .. }
            | Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
    ) || f
        .loops
        .iter()
        .flatten()
        .any(|c| matches!(c.pc, Curve::Nurbs(_)))
}

/// What a face's integrals depend on, hashed to 128 bits ([`super::fingerprint`]): its
/// surface, its sense and its boundary (each p-curve and its range).
fn face_key(body: &Body, f: &Face) -> u128 {
    use std::hash::Hasher;
    let mut h = super::fingerprint::Key128::new();
    h.surface(&f.surf);
    h.write_u8(f.sense as u8);
    for l in &f.loops {
        h.write_usize(l.len());
        for c in l {
            let e = &body.edges[c.edge];
            h.write_u8(c.fwd as u8);
            h.f64s(&e.t);
            h.curve(&c.pc);
        }
    }
    h.key()
}

/// The faces' integrals last taken, by [`face_key`]: two generations of at most [`Self::KEPT`]
/// each, a hit in the older moved to the newer, so the faces of the bodies a rebuild keeps
/// working on stay.
#[derive(Default)]
struct FaceCache {
    new: rustc_hash::FxHashMap<u128, (V3, QE)>,
    old: rustc_hash::FxHashMap<u128, (V3, QE)>,
}

impl FaceCache {
    /// Faces a generation holds (about 230 bytes each).
    const KEPT: usize = 16_384;

    fn get(&mut self, k: u128) -> Option<(V3, QE)> {
        if let Some(v) = self.new.get(&k) {
            return Some(*v);
        }
        let v = self.old.remove(&k)?;
        self.insert(k, v);
        Some(v)
    }

    fn insert(&mut self, k: u128, v: (V3, QE)) {
        if self.new.len() >= Self::KEPT {
            self.old = std::mem::take(&mut self.new);
        }
        self.new.insert(k, v);
    }
}

static FACE_CACHE: std::sync::LazyLock<std::sync::Mutex<FaceCache>> =
    std::sync::LazyLock::new(Default::default);

#[cfg(test)]
mod tests {
    use super::*;

    /// Each rule integrates the monomials up to degree 2n − 1 over [−1, 1] exactly (to rounding),
    /// and not degree 2n (whose error shrinks fast with n: checked up to 8 points).
    #[test]
    fn gauss_rules_are_exact_to_their_degree() {
        for n in 1..=MAX_GAUSS {
            let g = gauss(n);
            assert_eq!(g.x.len(), n);
            let int = |k: i32| g.x.iter().zip(g.w).map(|(x, w)| w * x.powi(k)).sum::<f64>();
            for k in 0..2 * n as i32 {
                let exact = if k % 2 == 0 {
                    2.0 / (k + 1) as f64
                } else {
                    0.0
                };
                assert!(
                    (int(k) - exact).abs() < 1e-14,
                    "{n} points, x^{k}: {} vs {exact}",
                    int(k)
                );
            }
            let k = 2 * n as i32;
            assert!(
                n > 8 || (int(k) - 2.0 / (k + 1) as f64).abs() > 1e-6,
                "{n} points exact past its degree"
            );
        }
    }

    /// A cone whose face runs through the apex (|N| has a kink there): the ruling's two sides
    /// apart, so its few points stay exact. The cone r = v, 0 ≤ v ≤ 2 (half-angle 45°), over a
    /// quarter turn, holds the area √2·π/4·(2² − 0)/2 on each side of v = 0 over [−2, 2].
    #[test]
    fn rulings_through_a_cone_apex() {
        let s = Surface::Cone {
            f: Frame::world(),
            r: 0.0,
            a: PI / 4.0,
        };
        let mut q = [0.0; NE];
        // ∫ over u ∈ [0, π/2] of G(u, 2) − G(u, −2), each G from v = −2.
        let g = gauss(8);
        for (x, w) in g.x.iter().zip(g.w) {
            let u = PI / 4.0 * (1.0 + x);
            inner_v::<true>(&s, u, -2.0, 2.0, &V3::zeros(), w * PI / 4.0, &mut q);
        }
        let exact = 2.0 * 2.0_f64.sqrt() * PI / 4.0 * 2.0;
        assert!(
            (q[0] - exact).abs() < 1e-12 * exact,
            "area {} vs {exact}",
            q[0]
        );
    }

    /// Integrals taken about one point and moved to another ([`shifted`]) are those taken about
    /// the other, on a box with every edge rounded (B-spline corners, tori, cylinders); and its
    /// mass properties come out the same weighed again ([`FACE_CACHE`]) and as summed about its
    /// own centre.
    #[test]
    fn integrals_moved_to_another_origin() {
        use crate::Kernel;
        use crate::native::kernel::NativeKernel;
        let mut k = NativeKernel::new();
        let crate::native::draw::Shape::Body(cube) = k.box_(10.0, 20.0, 30.0).unwrap() else {
            unreachable!()
        };
        let edges: Vec<crate::EdgeId> = (0..k.edges(cube).unwrap().len() as u64)
            .map(crate::EdgeId)
            .collect();
        let round = k.fillet(cube, &edges, 2.0).unwrap().bodies[0];
        let b = &**k.get(round).unwrap();
        assert!(b.faces.iter().any(costly));
        let (from, to) = (V3::new(1.0, -2.0, 3.0), V3::new(-7.0, 25.0, 40.0));
        // Each quantity's degree in x: its scale is the face's area times a length that often.
        let degree = [0, 1, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 3, 3];
        for f in &b.faces {
            let moved = shifted(&face_integrals_of::<true>(b, f, &from), &from, &to);
            let direct = face_integrals(b, f, &to);
            for (q, d) in degree.iter().enumerate() {
                let tol = 1e-11 * direct[0].abs() * 60f64.powi(*d);
                assert!(
                    (moved[q] - direct[q]).abs() <= tol,
                    "quantity {q}: {} vs {}",
                    moved[q],
                    direct[q]
                );
            }
        }
        let (first, again) = (body_mass(b), body_mass(b));
        assert_eq!(first.volume, again.volume);
        assert_eq!(first.inertia, again.inertia);
        let center = b.bbox().center().coords;
        let summed: f64 = b
            .faces
            .iter()
            .map(|f| face_integrals(b, f, &center)[4])
            .sum();
        assert!(
            (first.volume - summed).abs() < 1e-12 * summed,
            "{} vs {summed}",
            first.volume
        );
    }
}
