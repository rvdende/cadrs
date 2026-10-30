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
                }
            }
            // Linear in u and v with a constant normal: polynomials of degree 3 at most.
            Surface::Plane { .. } => Self {
                knots: Vec::new(),
                cut: 1,
                step: f64::INFINITY,
                n: if ALL { 2 } else { 1 },
            },
            // Round the axis (spheres, tori, surfaces of revolution).
            _ => Self {
                knots: Vec::new(),
                cut: 1,
                step: ANGLE_STEP,
                n: ANGLE_POINTS,
            },
        }
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
) {
    if (u - u0).abs() < tol::TINY {
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
        for (x, gw) in g.x.iter().zip(g.w) {
            let (p, su, sv) = eval(m + r * x);
            accumulate::<ALL>(&p, &su, &sv, shift, acc, w * gw * r);
        }
    }
}

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

/// The 14 face integrals of a face, relative to `shift`, oriented by its sense.
pub fn face_integrals(body: &Body, f: &Face, shift: &V3) -> Q {
    let q = face_integrals_of::<true>(body, f, shift);
    std::array::from_fn(|k| q[k])
}

/// [`face_integrals`] with [`NE`]'s moments, or with `ALL` false only the volume's (`[4]`; the
/// others zero).
fn face_integrals_of<const ALL: bool>(body: &Body, f: &Face, shift: &V3) -> QE {
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
    // B-spline faces gather their inner integrals as jobs (run on the GPU when there are many).
    let mut jobs: Option<Vec<[f64; 4]>> = matches!(f.surf, Surface::Nurbs(_)).then(Vec::new);
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
    for j in jobs {
        inner::<ALL>(s, plan, j[0], j[1], j[2], shift, j[3], acc, scratch);
    }
}

/// [`body_mass`]'s volume (to rounding) for a fraction of the work: only the volume's integrand,
/// on the fewer points it needs.
/// (Booleans and offsets weigh solids by it to drop slivers and check orientation.)
pub fn body_volume(body: &Body) -> f64 {
    if !body.is_solid() {
        return 0.0;
    }
    let bb = body.bbox();
    let shift = if bb.is_empty() {
        V3::zeros()
    } else {
        bb.center().coords
    };
    let faces: Vec<usize> = (0..body.faces.len()).collect();
    let qs: Vec<f64> = faces
        .par_iter()
        .map(|&i| face_integrals_of::<false>(body, &body.faces[i], &shift)[4])
        .collect();
    let mut v = 0.0;
    for q in qs {
        v += q;
    }
    v
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
    let qs: Vec<Q> = faces
        .par_iter()
        .map(|&i| {
            let f = &body.faces[i];
            if gpu_active() || !costly(f) {
                return face_integrals(body, f, &shift);
            }
            let (center, q) = face_integrals_about_center(body, i);
            shifted(&q, &center, &shift)
        })
        .collect();
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
